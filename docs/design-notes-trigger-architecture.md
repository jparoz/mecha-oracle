# Design Notes: Trigger Architecture

## Problem Statement

The engine currently has two parallel, incompatible systems for triggered abilities. They need to be unified before trigger-heavy mechanics become too numerous to retrofit.

---

## Current State

### Track 1 — Typed system (`TriggeredAbility` + `TriggerEvent`)

Defined in `src/types/ability.rs:52-60`.

```rust
pub enum TriggerEvent {
    EntersTheBattlefield { subject_is_self: bool },
}

pub struct TriggeredAbility {
    pub trigger: TriggerEvent,
    pub effect: Effect,
}
```

Lives in `OracleSpan::Parsed(Ability::Triggered(...))`. The parser produces these; the engine reads them. The collecting function `collect_etb_triggers` (triggered.rs:8) iterates a card's oracle spans and finds matching `TriggeredAbility` entries.

**Currently used for:** "When this enters the battlefield, …" (Elvish Visionary, Pelakka Wurm style).

**Key property:** fully data-driven. A new ETB-draw card requires zero engine changes — only parser output.

### Track 2 — Hardcoded system (`StaticAbility` + collector functions)

All other triggers are stored as `StaticAbility` variants, with trigger logic in dedicated imperative functions in `triggered.rs`:

| Keyword    | Stored as         | Collected by                  | Call site                  |
|------------|-------------------|-------------------------------|----------------------------|
| Exalted    | StaticAbility     | collect_attack_triggers()     | declare_attackers()        |
| Melee      | StaticAbility     | collect_attack_triggers()     | declare_attackers()        |
| Battle Cry | StaticAbility     | collect_attack_triggers()     | declare_attackers()        |
| Training   | StaticAbility     | collect_attack_triggers()     | declare_attackers()        |
| Flanking   | StaticAbility     | collect_block_triggers()      | declare_blockers()         |
| Bushido N  | StaticAbility     | collect_block_triggers()      | declare_blockers()         |
| Evolve     | StaticAbility     | collect_evolve_triggers()     | resolve_top()              |
| Prowess    | StaticAbility     | collect_cast_triggers()       | casting.rs                 |
| Ward       | StaticAbility     | collect_ward_triggers()       | targeting layer            |

None of these use `TriggerEvent`. The trigger condition is hardcoded inside the collector function.

---

## Why the Split Exists

`TriggerEvent` can't express the conditions these abilities need:

- **Evolve**: "when *another* creature ETBs *under your control* with *greater power or toughness than this*" — needs subject filter + comparator condition
- **Training**: "when *this* attacks *alongside* a creature with *greater power*" — needs attacker-context condition
- **Prowess**: "whenever *you* cast a *noncreature* spell" — needs cast event + spell type filter
- **Ward**: "whenever *this* becomes the target of a spell or ability *an opponent controls*" — needs targeting event + controller predicate

Track 1's `TriggerEvent` enum has no parameter slots for any of this. So these all fell into Track 2.

---

## The Core Gap: `TriggerEvent` is not extensible

The enum needs variants for every trigger event class that exists in MTG:

- Zone changes (ETB, dies, leaves battlefield, returns to hand, exiled)
- Combat events (attacks, blocks, becomes blocked, deals damage, deals combat damage to player/creature)
- Cast events (whenever a spell is cast)
- Phase/step events (upkeep, end step, beginning of combat)
- State changes (tapped, untapped, gains/loses counter)

Each variant also needs predicate parameters: *whose* creatures, *which* spells, *under what condition*.

---

## Specific Blockers for Planned Mechanics

### Persist (CR 702.79) and Undying (CR 702.93)

Both fire when the creature **dies** — i.e., moves from battlefield to graveyard.

Current `move_to_graveyard` (`state_based_actions.rs:113`) is `fn(GameState, ObjectId) -> GameState`. It has no mechanism to emit triggers. The SBA loop in `apply_sbas` applies `MoveToGraveyard` actions but discards any trigger output.

To implement these today: add `collect_persist_undying_triggers()` and call it inside `apply_sbas` after each `MoveToGraveyard` — but this only deepens Track 2.

### "Whenever you draw a card" (Rhystic Study, Alhammarret's Archive)

No `TriggerEvent::Drew` variant. No draw-event hook in `draw_card()` (turn.rs:116). Would require a new collector and a call site inside draw_card.

### "At the beginning of your upkeep" (Cumulative Upkeep, Sphere of the Suns)

No `TriggerEvent::BeginningOfStep` variant. No trigger collection in `advance_step()` or `apply_step_start()` for Upkeep.

### "Whenever a creature you control deals combat damage to a player" (Coastal Piracy, Ohran Viper)

No `TriggerEvent::DealsCombatDamageToPlayer` variant. No trigger hook in `deal_combat_damage()`.

---

## Proposed Direction

### Extend `TriggerEvent`

Expand the enum to cover the main event classes. Use struct variants to carry predicate parameters as data rather than hardcoding them in functions:

```rust
pub enum TriggerEvent {
    // Zone change
    EntersTheBattlefield { subject: TriggerSubject },
    Dies { subject: TriggerSubject },
    LeavesBattlefield { subject: TriggerSubject },

    // Combat
    Attacks { subject: TriggerSubject },
    Blocks { subject: TriggerSubject },
    DealsCombatDamageTo { subject: TriggerSubject, target: DamageTarget },

    // Cast
    SpellCast { caster: TriggerCaster, filter: SpellFilter },

    // Phase/step
    PhaseStep { step: Step, whose_turn: TurnOwner },
}

pub enum TriggerSubject {
    This,                            // the card itself
    ControlledBy(TurnOwner),         // any permanent under the trigger's controller
    Any,
}

pub enum TurnOwner { You, Opponent, Any }
pub enum DamageTarget { Player, Creature, Any }
```

### Add a trigger condition field

Some triggers only fire under a condition checked at trigger time (Evolve: entering creature must have greater P or T). Add an optional predicate:

```rust
pub struct TriggeredAbility {
    pub trigger: TriggerEvent,
    pub condition: Option<TriggerCondition>,
    pub effect: Effect,
}

pub enum TriggerCondition {
    EnteringCreatureHasGreaterPower,
    EnteringCreatureHasGreaterToughness,
    EnteringCreatureHasGreaterPowerOrToughness,
    AttackingAlongsideCreatureWithGreaterPower,
    // ...
}
```

### Replace collect_*_triggers() with a general dispatch loop

Once `TriggerEvent` covers the relevant event types, the engine can have a single function:

```rust
fn collect_triggers_for_event(state: &GameState, event: &TriggerEvent) -> Vec<StackObject>
```

This iterates all permanents on the battlefield, finds those with `OracleSpan::Parsed(Ability::Triggered(t))` where `t.trigger` matches the event, evaluates `t.condition` if present, and constructs `StackObject`s. The individual `collect_*_triggers()` functions disappear.

Call sites become uniform: wherever an event fires, call `collect_triggers_for_event(state, &event)` and push results onto the stack.

### Migration path

The existing Track 2 keywords can be migrated incrementally. Each one needs:
1. A new `TriggerEvent` variant (or reuse of an existing one with the right subject)
2. A `TriggerCondition` variant if needed
3. The `StaticAbility` variant replaced with (or accompanied by) the parser generating a `TriggeredAbility` oracle span

Prowess and Evolve are good first candidates because their trigger conditions are well-defined and testable. Exalted is simple (no condition beyond "exactly one attacker"). Training and Battle Cry come next.

---

## Files to Change

- `src/types/ability.rs` — extend `TriggerEvent`, add `TriggerCondition`, update `TriggeredAbility`
- `src/engine/triggered.rs` — replace individual collectors with general dispatch
- `src/engine/stack.rs` — update `resolve_top` to use general dispatch for ETB/Evolve
- `src/engine/combat.rs` — add trigger dispatch at declare_attackers / declare_blockers
- `src/engine/turn.rs` — add trigger dispatch at apply_step_start for upkeep/end-step
- `src/parser/oracle.rs` — update parser to emit `TriggeredAbility` spans for affected keywords
