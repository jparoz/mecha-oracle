# Trigger Architecture Unification

**Date:** 2026-06-18  
**Status:** Approved

---

## Problem

The engine has two incompatible trigger systems running in parallel:

**Track 1 — typed system** (`TriggeredAbility` + `TriggerEvent`): Data-driven. The parser emits `OracleSpan::Parsed(Ability::Triggered(...))` and the engine reads it. Currently handles only `EntersTheBattlefield { subject_is_self: bool }`.

**Track 2 — hardcoded system** (`StaticAbility` + per-keyword collector functions): All other triggers stored as `StaticAbility` variants with logic hardcoded in `collect_attack_triggers`, `collect_block_triggers`, `collect_cast_triggers`, `collect_evolve_triggers`, `collect_ward_triggers`. Adding a new keyword means touching both the collector functions and their call sites.

The split exists because `TriggerEvent` is too narrow to express the conditions these abilities need (subject filters, P/T comparisons, cast-type filters). This spec defines the endgame model: all triggered abilities expressed as data, dispatched through a single function.

---

## Data Model

### `TriggerSubjectFilter`

Describes which objects an event must involve for the trigger to fire. All fields are optional — the empty filter matches any object.

```rust
pub struct TriggerSubjectFilter {
    /// Some(true) = must be the source card itself; Some(false) = must not be self; None = any.
    pub is_self: Option<bool>,
    /// Restrict to objects under a particular controller. None = any.
    pub controller: Option<TurnOwner>,
    /// Object must have at least one of these card types. Empty = no constraint.
    pub card_types: Vec<CardType>,
    /// Object must have all of these subtypes. Empty = no constraint.
    pub subtypes: Vec<String>,
}
```

`TurnOwner` resolves relative to the controller of the triggered ability's source — "You" means the ability's controller, not the active player.

```rust
pub enum TurnOwner { You, Opponent, Any }
```

Representative examples:

| Oracle text fragment | Filter |
|---|---|
| "when *this* enters" | `{ is_self: Some(true), .. }` |
| "when *another creature you control* enters" | `{ is_self: Some(false), controller: Some(You), card_types: [Creature], .. }` |
| "when *a creature* dies" | `{ card_types: [Creature], .. }` |
| "when *a creature an opponent controls* dies" | `{ controller: Some(Opponent), card_types: [Creature], .. }` |
| "whenever *any permanent* enters" | `{}` (all fields default) |

---

### `TriggerEvent`

```rust
pub enum TriggerEvent {
    // Zone changes
    EntersTheBattlefield { subject: TriggerSubjectFilter },
    Dies                  { subject: TriggerSubjectFilter },
    LeavesBattlefield     { subject: TriggerSubjectFilter },

    // Combat
    Attacks           { subject: TriggerSubjectFilter },
    Blocks            { subject: TriggerSubjectFilter },
    BecomesBlocked    { subject: TriggerSubjectFilter },
    DealsCombatDamage { subject: TriggerSubjectFilter, to: DamageTargetKind },

    // Cast
    SpellCast { caster: TurnOwner, filter: SpellFilter },

    // Phase/step
    PhaseStep { step: Step, whose_turn: TurnOwner },

    // Draw
    DrawsCard { who: TurnOwner },

    // Targeting (CR 702.21: Ward)
    TargetedBy { controller: TurnOwner },
}

pub enum DamageTargetKind { Player, Creature, Any }
```

---

### `TriggerCondition`

A closed enum of game-state predicates evaluated at trigger time. These express conditions that require reading live game state beyond what the subject filter can express (combat lineups, relative P/T, keyword presence).

```rust
pub enum TriggerCondition {
    ExactlyOneAttacker,                         // CR 702.83b Exalted
    AttackingAlongsideGreaterPowerCreature,     // CR 702.149a Training
    EnteringCreatureHasGreaterPower,            // CR 702.100b Evolve (power)
    EnteringCreatureHasGreaterToughness,        // CR 702.100b Evolve (toughness)
    EnteringCreatureHasGreaterPowerOrToughness, // CR 702.100b Evolve (either)
    SubjectLacksKeyword(StaticAbility),         // CR 702.25b Flanking — blocker must not have Flanking
}
```

New variants are added as new keywords require them.

---

### `TriggerTargetMode`

Specifies how `targets` is populated on the `StackObject` at dispatch time. The dispatch function uses this to set targets before pushing onto the stack.

```rust
pub enum TriggerTargetMode {
    /// Effect uses no targets (DrawCard, GainLife, Payment). targets = [].
    None,
    /// Effect targets the source permanent itself (Prowess, Training, Melee, Bushido).
    Source,
    /// Effect targets the object that triggered the event — the subject_id (Exalted, Flanking).
    Subject,
    /// Effect targets all objects currently matching a filter (Battle Cry: all other attackers).
    /// Produces a single StackObject with multiple targets; stack resolution iterates all of them.
    AllMatching(TriggerSubjectFilter),
}
```

---

### Updated `TriggeredAbility`

```rust
pub struct TriggeredAbility {
    pub trigger:     TriggerEvent,
    pub condition:   Option<TriggerCondition>,
    pub target_mode: TriggerTargetMode,
    pub effect:      Effect,
}
```

`condition: None` and `target_mode: TriggerTargetMode::None` are the common defaults. Evolve uses `condition: Some(EnteringCreatureHasGreaterPowerOrToughness)` and `target_mode: Source`.

---

## General Dispatch Function

```rust
// CR 603.2: collect all triggered abilities that fire for the given event.
pub fn collect_triggers_for_event(
    state: &GameState,
    event: &TriggerEvent,
    subject_id: Option<ObjectId>,
) -> Vec<StackObject>
```

`subject_id` is the `ObjectId` of the object that "did the thing" (the permanent that entered, died, attacked, etc.). It is `None` for phase/step and draw events, and for `TargetedBy` where the subject is implicit in the targeting context.

The function iterates every permanent on the battlefield, finds those whose oracle spans include `OracleSpan::Parsed(Ability::Triggered(t))` where:

1. `t.trigger` matches the event — event variants match by discriminant; struct-variant fields are checked:
   - `subject` filter: `TriggerSubjectFilter::matches(subject_id, source_id, state)` — resolves `is_self` by comparing `subject_id == source_id`, resolves `controller` via `TurnOwner` relative to the source's controller, checks `card_types` and `subtypes` against the subject's card definition.
   - other per-variant fields (`caster`, `filter`, `whose_turn`, `who`, `to`, `controller`) matched against event data.
2. `t.condition` is satisfied (if `Some`) — conditions are evaluated against game state at trigger time.

Inject source flags (lifelink, deathtouch, etc.) into any `DealDamage` steps in the effect before constructing the `StackObject`, using the existing `inject_source_flags` helper.

The individual `collect_*_triggers` functions are deleted as each keyword migrates into this path.

---

## SBA Return Type

```rust
// CR 704.3: check and apply state-based actions until none remain.
// Returns the updated state and any triggered abilities that fired.
pub fn check_and_apply_sbas(state: GameState) -> (GameState, Vec<StackObject>)
```

Inside `apply_sbas`, after each `MoveToGraveyard` action, `collect_triggers_for_event(state, &TriggerEvent::Dies { subject: TriggerSubjectFilter::default() }, Some(id))` is called and results accumulated. The `Vec<StackObject>` is returned to all callers and pushed onto the stack.

Current callers of `check_and_apply_sbas`: `stack.rs:resolve_top`, `combat.rs:declare_attackers`, `combat.rs:declare_blockers`. All three update to destructure `(state, triggers)` and push triggers.

---

## Ward Migration

`StaticAbility::Ward(Vec<CostComponent>)` is retired. The parser emits a `TriggeredAbility` instead:

```rust
TriggeredAbility {
    trigger: TriggerEvent::TargetedBy { controller: TurnOwner::Opponent },
    condition: None,
    effect: vec![EffectStep::Payment {
        cost: components,
        on_paid: vec![],
        on_declined: vec![EffectStep::CounterSpell],
    }],
}
```

The `TargetedBy` event is emitted from `targeting.rs` after targeting resolves, replacing the `collect_ward_triggers` call site. `collect_ward_triggers` is deleted. The `StaticAbility::Ward` variant and its display name branch are removed.

---

## Event Emission Sites

| Event | File | Where | `subject_id` |
|---|---|---|---|
| `EntersTheBattlefield` | `stack.rs` | `resolve_top` ETB path | entering permanent |
| `Dies` | `state_based_actions.rs` | after each `MoveToGraveyard` in `apply_sbas` | dying permanent |
| `Attacks` | `combat.rs` | `declare_attackers` (one call per attacker) | attacker |
| `Blocks` | `combat.rs` | `declare_blockers` (one call per blocker) | blocker |
| `BecomesBlocked` | `combat.rs` | `declare_blockers` (one call per newly blocked attacker) | attacker |
| `DealsCombatDamage` | `combat.rs` | `deal_combat_damage` (one call per dealing permanent) | dealing permanent |
| `SpellCast` | `casting.rs` | after spell hits stack | spell `ObjectId` |
| `PhaseStep` | `turn.rs` | `apply_step_start` | `None` |
| `DrawsCard` | `turn.rs` | `draw_card` | `None` |
| `TargetedBy` | `targeting.rs` | after targeting resolves | `None` |

---

## Keyword Migration Map

Each Track 2 keyword and its destination in the new model:

| Keyword | Current | New `TriggerEvent` | `TriggerCondition` |
|---|---|---|---|
| ETB draw/life (Elvish Visionary, Pelakka Wurm) | Track 1 (update subject field only) | `EntersTheBattlefield { subject: { is_self: Some(true), .. } }` | — |
| Evolve | `StaticAbility::Evolve` + `collect_evolve_triggers` | `EntersTheBattlefield { subject: { is_self: Some(false), controller: Some(You), card_types: [Creature], .. } }` | `EnteringCreatureHasGreaterPowerOrToughness` |
| Exalted | `StaticAbility::Exalted` + `collect_attack_triggers` | `Attacks { subject: { controller: Some(You), .. } }` | `ExactlyOneAttacker` |
| Melee | `StaticAbility::Melee` + `collect_attack_triggers` | `Attacks { subject: { is_self: Some(true), .. } }` | — |
| Battle Cry | `StaticAbility::BattleCry` + `collect_attack_triggers` | `Attacks { subject: { is_self: Some(true), .. } }` | — (target_mode: `AllMatching { is_self: Some(false), controller: Some(You), .. }` among current attackers) |
| Training | `StaticAbility::Training` + `collect_attack_triggers` | `Attacks { subject: { is_self: Some(true), .. } }` | `AttackingAlongsideGreaterPowerCreature` |
| Flanking | `StaticAbility::Flanking` + `collect_block_triggers` | `Blocks { subject: { controller: Some(Opponent), .. } }` | `SubjectLacksKeyword(Flanking)` |
| Bushido N | `StaticAbility::BushidoN` + `collect_block_triggers` | `Blocks { subject: { is_self: Some(true), .. } }` (blocker) + `BecomesBlocked { subject: { is_self: Some(true), .. } }` (attacker) | — |
| Prowess | `StaticAbility::Prowess` + `collect_cast_triggers` | `SpellCast { caster: You, filter: SpellFilter::noncreature() }` | — |
| Ward | `StaticAbility::Ward` + `collect_ward_triggers` | `TargetedBy { controller: Opponent }` | — |

Persist (CR 702.79) and Undying (CR 702.93) are unblocked once Phase 7 (Dies + SBA return type) lands. They are not migrated in this spec — their parser support is a follow-on.

---

## Migration Phases

Each phase ships independently with tests passing throughout.

**Phase 1 — Foundation**  
Define `TriggerSubjectFilter`, `TurnOwner`, `DamageTargetKind`, expanded `TriggerEvent`, `TriggerCondition`, updated `TriggeredAbility`. Add `collect_triggers_for_event` in `triggered.rs`. No keywords migrate; existing `collect_*` functions remain. Tests unchanged.

**Phase 2 — ETB triggers**  
Update `EntersTheBattlefield` to use `TriggerSubjectFilter` (replace `subject_is_self: bool`). Route existing ETB `TriggeredAbility` oracle spans through general dispatch. Verify Elvish Visionary / Pelakka Wurm still work.

**Phase 3 — Combat attack triggers**  
Migrate Exalted, Melee, Battle Cry, Training: add `TriggeredAbility` oracle spans, wire `Attacks` event emission in `declare_attackers`, delete `collect_attack_triggers`.

**Phase 4 — Combat block triggers**  
Migrate Flanking, Bushido: add `TriggeredAbility` oracle spans, wire `Blocks` / `BecomesBlocked` emission in `declare_blockers`, delete `collect_block_triggers`.

**Phase 5 — Cast triggers**  
Migrate Prowess: add `TriggeredAbility` oracle span, wire `SpellCast` emission in `casting.rs`, delete `collect_cast_triggers`.

**Phase 6 — Ward**  
Migrate Ward: update parser to emit `TriggeredAbility { trigger: TargetedBy }`, wire `TargetedBy` emission in `targeting.rs`, delete `collect_ward_triggers`, remove `StaticAbility::Ward`.

**Phase 7 — Dies + SBA return type**  
Change `check_and_apply_sbas` signature to `-> (GameState, Vec<StackObject>)`. Emit `Dies` triggers from SBA loop. Update three call sites. Unblocks Persist, Undying.

**Phase 8 — Step/draw triggers**  
Wire `PhaseStep` emission in `turn.rs:apply_step_start` and `DrawsCard` emission in `turn.rs:draw_card`. Unblocks Cumulative Upkeep, Rhystic Study, Alhammarret's Archive.

**Phase 9 — Combat damage triggers**  
Wire `DealsCombatDamage` emission in `combat.rs:deal_combat_damage`. Unblocks Coastal Piracy, Ohran Viper, first-strike interaction triggers.

---

## Files Changed

| File | Changes |
|---|---|
| `src/types/ability.rs` | Add `TriggerSubjectFilter`, `TurnOwner`, `DamageTargetKind`; expand `TriggerEvent`; add `TriggerCondition`; update `TriggeredAbility`; remove `StaticAbility::Ward` (Phase 6) |
| `src/engine/triggered.rs` | Add `collect_triggers_for_event`; delete `collect_*` functions as phases complete |
| `src/engine/state_based_actions.rs` | Change `check_and_apply_sbas` return type; emit `Dies` triggers (Phase 7) |
| `src/engine/stack.rs` | Update ETB trigger collection; update SBA call site (Phase 7) |
| `src/engine/combat.rs` | Add attack/block/damage event emission; update SBA call sites (Phase 7) |
| `src/engine/casting.rs` | Add `SpellCast` event emission (Phase 5) |
| `src/engine/turn.rs` | Add `PhaseStep` and `DrawsCard` emission (Phase 8) |
| `src/engine/targeting.rs` | Replace `collect_ward_triggers` with `TargetedBy` dispatch (Phase 6) |
| `src/parser/oracle.rs` | Update Ward to emit `TriggeredAbility` (Phase 6); update attack/block/cast keywords to emit `TriggeredAbility` (Phases 3–5) |
