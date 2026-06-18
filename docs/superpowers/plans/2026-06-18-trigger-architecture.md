# Trigger Architecture Unification Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Replace the two parallel trigger systems (typed `TriggeredAbility` + hardcoded `collect_*` functions) with a single data-driven dispatch model where all triggered abilities are expressed as `TriggeredAbility` oracle spans and collected via one `collect_triggers_for_event` function.

**Architecture:** Extend `TriggerEvent` with a full set of event variants; add `TriggerSubjectFilter` (open-ended filter struct), `TriggerCondition` (closed enum of game-state predicates), and `TriggerTargetMode` (controls how targets are set at dispatch time). A new `GameEvent` enum carries concrete runtime event data; `collect_triggers_for_event` matches all `TriggeredAbility` oracle spans against it. Keywords migrate from `StaticAbility` + hardcoded collectors into `TriggeredAbility` spans in phases.

**Tech Stack:** Rust, Cargo. MTG rules reference at `docs/CR.txt`.

## Global Constraints

- Run `cargo test 2>&1 | grep -E "^test result|FAILED|error\["` after each commit; all tests must pass.
- Run `cargo clippy --all-targets` before finishing any task; fix all warnings.
- CR references in code comments must be verified against `docs/CR.txt` using `grep '^NNN\\.' docs/CR.txt`.
- `Step` is defined in `src/types/game_state.rs` and re-exported via `src/types/mod.rs`.
- `ObjectId`, `PlayerId` are re-exported from `src/types/mod.rs`.
- `ability.rs` must not import from `game_state.rs` (circular dependency); matching logic that requires `GameState` lives in `triggered.rs`.

---

### Task 1: New type definitions in `ability.rs`

Add the new types that the rest of the plan depends on. No existing behaviour changes yet — all new types are additive. The only breaking change is updating `TriggerEvent::EntersTheBattlefield` (replacing `subject_is_self: bool` with `subject: TriggerSubjectFilter`) and adding `condition`/`target_mode` fields to `TriggeredAbility`. All construction sites are updated in this task.

**Files:**
- Modify: `src/types/ability.rs`
- Modify (construction sites): `src/parser/oracle.rs`, `src/engine/triggered.rs` (test helpers)

**Interfaces:**
- Produces: `TurnOwner`, `TriggerSubjectFilter`, `DamageTargetKind`, `TriggerTargetMode`, `TriggerCondition`, `GameEvent` (all pub, exported via `src/types/mod.rs`); updated `TriggerEvent` and `TriggeredAbility`

- [ ] **Step 1: Add new types to `src/types/ability.rs`**

Insert after the existing `TriggerEvent` / `TriggeredAbility` block (around line 51):

```rust
/// CR 603.2: who "you" refers to — resolved relative to the trigger source's controller.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TurnOwner {
    You,
    Opponent,
    Any,
}

/// Open-ended filter describing which objects satisfy a trigger event's subject requirement.
/// All fields are optional; the empty filter (all defaults) matches any object.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct TriggerSubjectFilter {
    /// Some(true) = subject must be the source card itself; Some(false) = must not be self; None = any.
    pub is_self: Option<bool>,
    /// Restrict by controller relative to the trigger source's controller. None = any.
    pub controller: Option<TurnOwner>,
    /// Subject must have at least one of these card types. Empty = no constraint.
    pub card_types: Vec<CardType>,
    /// Subject must have all of these subtypes. Empty = no constraint.
    pub subtypes: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DamageTargetKind { Player, Creature, Any }

/// Controls how targets are populated on the StackObject at trigger dispatch time.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub enum TriggerTargetMode {
    /// No targets — effect resolves without targeting (DrawCard, GainLife, Payment).
    #[default]
    None,
    /// Target the source permanent itself (Prowess, Training, Melee, Bushido).
    Source,
    /// Target the object that triggered the event — the subject (Exalted, Flanking).
    Subject,
    /// Target all current attackers except the source (Battle Cry).
    AllOtherAttackers,
}

/// Closed enum of game-state predicates checked at trigger time.
/// Each variant corresponds to a condition that cannot be expressed by TriggerSubjectFilter alone.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TriggerCondition {
    ExactlyOneAttacker,                         // CR 702.83b Exalted
    AttackingAlongsideGreaterPowerCreature,     // CR 702.149a Training
    EnteringCreatureHasGreaterPower,            // CR 702.100b Evolve (power)
    EnteringCreatureHasGreaterToughness,        // CR 702.100b Evolve (toughness)
    EnteringCreatureHasGreaterPowerOrToughness, // CR 702.100b Evolve (either)
    SubjectLacksKeyword(StaticAbility),         // CR 702.25b Flanking
}

/// Concrete runtime event data fired by the engine at each trigger point.
/// Distinct from TriggerEvent (which carries filter patterns); GameEvent carries IDs and values.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum GameEvent {
    EntersTheBattlefield { subject_id: ObjectId },
    Dies                  { subject_id: ObjectId },
    LeavesBattlefield     { subject_id: ObjectId },
    Attacks               { subject_id: ObjectId },
    Blocks                { subject_id: ObjectId },
    BecomesBlocked        { subject_id: ObjectId },
    DealsCombatDamage     { subject_id: ObjectId, to: DamageTargetKind },
    SpellCast             { caster: PlayerId, spell_id: ObjectId },
    PhaseStep             { step: Step, active_player: PlayerId },
    DrawsCard             { player: PlayerId },
    TargetedBy            { target_id: ObjectId, acting_player: PlayerId },
}
```

`GameEvent` references `ObjectId`, `PlayerId`, and `Step`. Add these imports at the top of `ability.rs`:

```rust
use super::{ObjectId, PlayerId};
use super::game_state::Step;
```

- [ ] **Step 2: Update `TriggerEvent` and `TriggeredAbility`**

Replace the existing `TriggerEvent` enum and `TriggeredAbility` struct:

```rust
#[derive(Debug, Clone, PartialEq, Eq)]
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

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TriggeredAbility {
    pub trigger:     TriggerEvent,
    pub condition:   Option<TriggerCondition>,
    pub target_mode: TriggerTargetMode,
    pub effect:      Effect,
}
```

- [ ] **Step 3: Update `TriggeredAbility` construction sites**

Update every `TriggeredAbility { ... }` literal to include the two new fields.

**In `src/parser/oracle.rs`** (around line 980):
```rust
Some(OracleSpan::Parsed(Ability::Triggered(TriggeredAbility {
    trigger: TriggerEvent::EntersTheBattlefield {
        subject: TriggerSubjectFilter {
            is_self: Some(true),
            ..Default::default()
        },
    },
    condition: None,
    target_mode: TriggerTargetMode::None,
    effect,
})))
```

**In `src/engine/triggered.rs`** — every `TriggeredAbility { trigger: TriggerEvent::EntersTheBattlefield { subject_is_self: true }, effect: ... }` in test helpers becomes:
```rust
TriggeredAbility {
    trigger: TriggerEvent::EntersTheBattlefield {
        subject: TriggerSubjectFilter { is_self: Some(true), ..Default::default() },
    },
    condition: None,
    target_mode: TriggerTargetMode::None,
    effect: vec![EffectStep::DrawCard(1)], // (or whatever the effect was)
}
```

Apply the same pattern to every other `TriggeredAbility` construction in the test module (etb_gain_life_def, multi-trigger test, collect_etb_assigns_unique_stack_ids).

- [ ] **Step 4: Export new types from `src/types/mod.rs`**

Add to the re-export block in `src/types/mod.rs`:
```rust
pub use ability::{
    // existing exports ...
    DamageTargetKind, GameEvent, TriggerCondition, TriggerSubjectFilter,
    TriggerTargetMode, TurnOwner,
};
```

- [ ] **Step 5: Write a failing test for `TriggerSubjectFilter` matching**

Add to the `#[cfg(test)]` block in `src/types/ability.rs`:

```rust
#[test]
fn trigger_subject_filter_is_self_matches_same_id() {
    let filter = TriggerSubjectFilter { is_self: Some(true), ..Default::default() };
    // is_self matching is a structural check only — tested via subject_filter_matches in triggered.rs
    assert_eq!(filter.is_self, Some(true));
    assert!(filter.card_types.is_empty());
}

#[test]
fn trigger_subject_filter_default_is_empty() {
    let filter = TriggerSubjectFilter::default();
    assert!(filter.is_self.is_none());
    assert!(filter.controller.is_none());
    assert!(filter.card_types.is_empty());
    assert!(filter.subtypes.is_empty());
}

#[test]
fn trigger_target_mode_default_is_none() {
    assert_eq!(TriggerTargetMode::default(), TriggerTargetMode::None);
}
```

- [ ] **Step 6: Run tests**

```bash
cargo test 2>&1 | grep -E "^test result|FAILED|error\["
```

Expected: `test result: ok` — all existing tests pass, new tests pass.

- [ ] **Step 7: Clippy**

```bash
cargo clippy --all-targets 2>&1 | grep -E "^error|^warning"
```

Fix any issues, then:

- [ ] **Step 8: Commit**

```bash
git add src/types/ability.rs src/parser/oracle.rs src/engine/triggered.rs src/types/mod.rs
git commit -m "feat: add TriggerSubjectFilter, TriggerCondition, TriggerTargetMode, GameEvent; update TriggerEvent and TriggeredAbility"
```

---

### Task 2: `collect_triggers_for_event` + ETB migration

Add the general dispatch function to `triggered.rs`, migrate Evolve into it (since Evolve is already Track 1-adjacent), and replace `collect_etb_triggers` + `collect_evolve_triggers` call sites with `collect_triggers_for_event`.

**Files:**
- Modify: `src/engine/triggered.rs` — add `subject_filter_matches`, `trigger_condition_satisfied`, `collect_triggers_for_event`
- Modify: `src/engine/stack.rs` — replace `collect_etb_triggers` + `collect_evolve_triggers` calls
- Modify: `src/engine/casting.rs` — same replacement

**Interfaces:**
- Consumes: `TriggerSubjectFilter`, `TriggerCondition`, `TriggerTargetMode`, `GameEvent` from Task 1
- Produces: `pub fn collect_triggers_for_event(state: &mut GameState, event: &GameEvent) -> Vec<StackObject>`

- [ ] **Step 1: Write failing tests for `collect_triggers_for_event`**

Add to the `#[cfg(test)]` block in `triggered.rs`:

```rust
#[test]
fn collect_triggers_for_event_etb_fires_draw_trigger() {
    use crate::types::GameEvent;
    let mut gs = two_player_state();
    put_in_library(&mut gs, PlayerId(0));
    let creature_id = place_on_battlefield(&mut gs, etb_draw_def(), PlayerId(0));

    let triggers = collect_triggers_for_event(&mut gs, &GameEvent::EntersTheBattlefield { subject_id: creature_id });

    assert_eq!(triggers.len(), 1);
    use crate::types::stack::StackPayload;
    let StackPayload::TriggeredAbility { source_id, effect, .. } = &triggers[0].payload else {
        panic!("expected TriggeredAbility");
    };
    assert_eq!(*source_id, creature_id);
    assert_eq!(*effect, vec![EffectStep::DrawCard(1)]);
}

#[test]
fn collect_triggers_for_event_etb_does_not_fire_for_other_events() {
    use crate::types::GameEvent;
    let mut gs = two_player_state();
    put_in_library(&mut gs, PlayerId(0));
    let creature_id = place_on_battlefield(&mut gs, etb_draw_def(), PlayerId(0));

    let triggers = collect_triggers_for_event(&mut gs, &GameEvent::Attacks { subject_id: creature_id });

    assert!(triggers.is_empty(), "ETB trigger should not fire on Attacks event");
}

#[test]
fn collect_triggers_for_event_evolve_fires_on_etb_with_greater_power() {
    use crate::types::GameEvent;
    let mut gs = two_player_state();
    let evolve_id = enter_creature_on_battlefield(&mut gs, PlayerId(0), 2, 2, vec![StaticAbility::Evolve]);
    let entering_id = enter_creature_on_battlefield(&mut gs, PlayerId(0), 3, 2, vec![]);

    let triggers = collect_triggers_for_event(&mut gs, &GameEvent::EntersTheBattlefield { subject_id: entering_id });

    assert_eq!(triggers.len(), 1);
    use crate::types::effect::EffectTarget;
    assert!(triggers[0].targets.iter().any(|t| matches!(t, EffectTarget::Object { id } if *id == evolve_id)));
}
```

Run to confirm compile failure (function not yet defined):
```bash
cargo test 2>&1 | grep -E "^test result|FAILED|error\["
```

- [ ] **Step 2: Add `subject_filter_matches` and `trigger_condition_satisfied` helpers**

Add to `src/engine/triggered.rs` (before the existing functions):

```rust
/// Returns true if filter matches the given subject.
/// source_id: the permanent that owns the triggered ability.
/// source_controller: that permanent's controller.
fn subject_filter_matches(
    filter: &TriggerSubjectFilter,
    subject_id: Option<ObjectId>,
    source_id: ObjectId,
    source_controller: PlayerId,
    state: &GameState,
) -> bool {
    let sid = match subject_id {
        Some(id) => id,
        // No subject satisfies any non-empty filter.
        None => return filter == &TriggerSubjectFilter::default(),
    };

    if let Some(is_self) = filter.is_self {
        if is_self != (sid == source_id) {
            return false;
        }
    }

    if let Some(ref required_owner) = filter.controller {
        let subject_controller = state.objects.get(&sid).map(|o| o.controller);
        let ok = match required_owner {
            TurnOwner::You => subject_controller == Some(source_controller),
            TurnOwner::Opponent => subject_controller.map(|c| c != source_controller).unwrap_or(false),
            TurnOwner::Any => true,
        };
        if !ok {
            return false;
        }
    }

    if let Some(obj) = state.objects.get(&sid) {
        if !filter.card_types.is_empty()
            && !filter.card_types.iter().any(|t| obj.definition.type_line.card_types.contains(t))
        {
            return false;
        }
        if !filter.subtypes.is_empty()
            && !filter.subtypes.iter().all(|t| obj.definition.type_line.subtypes.contains(t))
        {
            return false;
        }
    }

    true
}

/// Returns true if the trigger condition is satisfied given the current game state and event subject.
fn trigger_condition_satisfied(
    condition: &TriggerCondition,
    subject_id: Option<ObjectId>,
    source_id: ObjectId,
    state: &GameState,
) -> bool {
    match condition {
        TriggerCondition::ExactlyOneAttacker => state.combat.attackers.len() == 1,

        TriggerCondition::AttackingAlongsideGreaterPowerCreature => {
            let my_power = state.battlefield.get(&source_id)
                .and_then(|p| p.effective_power())
                .unwrap_or(0);
            state.combat.attackers.iter()
                .filter(|&&id| id != source_id)
                .any(|&id| state.battlefield.get(&id).and_then(|p| p.effective_power()).map(|p| p > my_power).unwrap_or(false))
        }

        TriggerCondition::EnteringCreatureHasGreaterPower => {
            let sid = subject_id?;
            let entering_power = state.battlefield.get(&sid).and_then(|p| p.effective_power())?;
            let my_power = state.battlefield.get(&source_id).and_then(|p| p.effective_power()).unwrap_or(0);
            Some(entering_power > my_power)
        }.unwrap_or(false),

        TriggerCondition::EnteringCreatureHasGreaterToughness => {
            let sid = subject_id?;
            let entering_toughness = state.battlefield.get(&sid).and_then(|p| p.effective_toughness())?;
            let my_toughness = state.battlefield.get(&source_id).and_then(|p| p.effective_toughness()).unwrap_or(0);
            Some(entering_toughness > my_toughness)
        }.unwrap_or(false),

        TriggerCondition::EnteringCreatureHasGreaterPowerOrToughness => {
            let sid = match subject_id { Some(id) => id, None => return false };
            let ep = state.battlefield.get(&sid).and_then(|p| p.effective_power());
            let et = state.battlefield.get(&sid).and_then(|p| p.effective_toughness());
            let mp = state.battlefield.get(&source_id).and_then(|p| p.effective_power()).unwrap_or(0);
            let mt = state.battlefield.get(&source_id).and_then(|p| p.effective_toughness()).unwrap_or(0);
            ep.map(|p| p > mp).unwrap_or(false) || et.map(|t| t > mt).unwrap_or(false)
        }

        TriggerCondition::SubjectLacksKeyword(kw) => {
            let sid = match subject_id { Some(id) => id, None => return false };
            !state.battlefield.get(&sid).map(|p| p.has_keyword(kw.clone())).unwrap_or(false)
        }
    }
}
```

Note: `trigger_condition_satisfied` uses `?` inside closures/blocks — this may require wrapping in a helper closure or `Option` block. Use Rust's `(|| { ... })()`  pattern where needed for early returns.

- [ ] **Step 3: Add `collect_triggers_for_event`**

Add after the helpers:

```rust
/// CR 603.2: collect all triggered abilities on the battlefield that fire for the given game event.
pub fn collect_triggers_for_event(state: &mut GameState, event: &GameEvent) -> Vec<StackObject> {
    use crate::types::ability::{Ability, TriggerEvent};
    use crate::types::effect::EffectTarget;

    // Snapshot source IDs to avoid borrow conflicts during iteration.
    let source_ids: Vec<ObjectId> = state.battlefield.keys().copied().collect();
    let mut result = Vec::new();

    for source_id in source_ids {
        let (controller, abilities) = match state.objects.get(&source_id) {
            Some(o) => (o.controller, o.definition.abilities.clone()),
            None => continue,
        };

        for span in &abilities {
            let triggered = match span {
                OracleSpan::Parsed(Ability::Triggered(t)) => t,
                _ => continue,
            };

            // Match event discriminant and subject filter.
            let subject_id: Option<ObjectId> = match (event, &triggered.trigger) {
                (
                    GameEvent::EntersTheBattlefield { subject_id },
                    TriggerEvent::EntersTheBattlefield { subject },
                ) if subject_filter_matches(subject, Some(*subject_id), source_id, controller, state) => {
                    Some(*subject_id)
                }
                (GameEvent::Dies { subject_id }, TriggerEvent::Dies { subject })
                    if subject_filter_matches(subject, Some(*subject_id), source_id, controller, state) =>
                {
                    Some(*subject_id)
                }
                (GameEvent::Attacks { subject_id }, TriggerEvent::Attacks { subject })
                    if subject_filter_matches(subject, Some(*subject_id), source_id, controller, state) =>
                {
                    Some(*subject_id)
                }
                (GameEvent::Blocks { subject_id }, TriggerEvent::Blocks { subject })
                    if subject_filter_matches(subject, Some(*subject_id), source_id, controller, state) =>
                {
                    Some(*subject_id)
                }
                (GameEvent::BecomesBlocked { subject_id }, TriggerEvent::BecomesBlocked { subject })
                    if subject_filter_matches(subject, Some(*subject_id), source_id, controller, state) =>
                {
                    Some(*subject_id)
                }
                (
                    GameEvent::SpellCast { caster, spell_id },
                    TriggerEvent::SpellCast { caster: required_caster, filter },
                ) => {
                    let caster_ok = match required_caster {
                        TurnOwner::You => *caster == controller,
                        TurnOwner::Opponent => *caster != controller,
                        TurnOwner::Any => true,
                    };
                    if !caster_ok {
                        continue;
                    }
                    let spell_ok = state.objects.get(spell_id).map(|o| {
                        filter.matches(
                            &o.definition.type_line.card_types,
                            o.definition.mana_cost.as_ref().map(|c| c.mana_value()).unwrap_or(0),
                            &o.definition.colors,
                        )
                    }).unwrap_or(false);
                    if !spell_ok { continue; }
                    None
                }
                (GameEvent::TargetedBy { target_id, acting_player }, TriggerEvent::TargetedBy { controller: required }) => {
                    if *target_id != source_id { continue; }
                    let ok = match required {
                        TurnOwner::Opponent => *acting_player != controller,
                        TurnOwner::You => *acting_player == controller,
                        TurnOwner::Any => true,
                    };
                    if !ok { continue; }
                    None
                }
                _ => continue,
            };

            // Check condition.
            if let Some(cond) = &triggered.condition {
                if !trigger_condition_satisfied(cond, subject_id, source_id, state) {
                    continue;
                }
            }

            // Resolve targets.
            let targets: Vec<EffectTarget> = match &triggered.target_mode {
                TriggerTargetMode::None => vec![],
                TriggerTargetMode::Source => vec![EffectTarget::Object { id: source_id }],
                TriggerTargetMode::Subject => match subject_id {
                    Some(sid) => vec![EffectTarget::Object { id: sid }],
                    None => vec![],
                },
                TriggerTargetMode::AllOtherAttackers => state
                    .combat
                    .attackers
                    .iter()
                    .filter(|&&id| id != source_id)
                    .map(|&id| EffectTarget::Object { id })
                    .collect(),
            };

            let effect = inject_source_flags(triggered.effect.clone(), &abilities);
            let sid = state.alloc_stack_id();
            result.push(StackObject {
                id: sid,
                payload: StackPayload::TriggeredAbility {
                    source_id,
                    effect,
                    label: format!("{}: trigger", state.objects.get(&source_id).map(|o| o.definition.name.as_str()).unwrap_or("?")),
                },
                controller,
                targets,
                x_value: None,
            });
        }
    }

    result
}
```

Add required imports at the top of `triggered.rs`:
```rust
use crate::types::{GameEvent, TriggerCondition, TriggerSubjectFilter, TriggerTargetMode, TurnOwner};
```

- [ ] **Step 4: Update `stack.rs` ETB + Evolve call sites**

In `src/engine/stack.rs`, replace the two separate collect calls (around lines 319–333):

```rust
// CR 603.2 / CR 702.100b: collect ETB triggers and Evolve triggers via unified dispatch.
let etb_triggers = crate::engine::triggered::collect_triggers_for_event(
    &mut state,
    &crate::types::GameEvent::EntersTheBattlefield { subject_id: card_id },
);
for trigger in etb_triggers {
    let id = trigger.id;
    state.stack.push(id);
    state.stack_objects.insert(id, trigger);
}
```

Remove the now-unused `use crate::engine::triggered::collect_etb_triggers;` import.

- [ ] **Step 5: Update `casting.rs` ETB + Evolve call sites**

In `src/engine/casting.rs`, replace the two `collect_etb_triggers` / `collect_evolve_triggers` calls (around lines 63–75) with:

```rust
let etb_triggers = crate::engine::triggered::collect_triggers_for_event(
    &mut state,
    &crate::types::GameEvent::EntersTheBattlefield { subject_id: object_id },
);
for t in etb_triggers {
    let id = t.id;
    state.stack.push(id);
    state.stack_objects.insert(id, t);
}
```

Remove the now-unused `collect_cast_triggers` import if Prowess hasn't migrated yet (it hasn't — leave the `collect_cast_triggers` call as-is for now, just remove the unused import warning by keeping it used).

- [ ] **Step 6: Run tests**

```bash
cargo test 2>&1 | grep -E "^test result|FAILED|error\["
```

Expected: `test result: ok`.

- [ ] **Step 7: Clippy, then commit**

```bash
cargo clippy --all-targets 2>&1 | grep -E "^error|^warning"
git add src/engine/triggered.rs src/engine/stack.rs src/engine/casting.rs
git commit -m "feat: add collect_triggers_for_event; route ETB and Evolve through general dispatch"
```

---

### Task 3: Migrate attack triggers (Exalted, Melee, Battle Cry, Training)

Replace `collect_attack_triggers` hardcoded logic with `TriggeredAbility` oracle spans + general dispatch. The parser still emits `StaticAbility` for these keywords — the migration moves the oracle spans in the **test card construction helpers** only. The parser update is a follow-on once keywords are stable.

For this task, the strategy is:
1. Add card helper that builds a `TriggeredAbility` span for each attack keyword.
2. Wire `Attacks` event emission in `declare_attackers`.
3. Delete `collect_attack_triggers`.
4. Update all test card constructions to use the new helpers.

**Files:**
- Modify: `src/engine/triggered.rs` — delete `collect_attack_triggers`, add triggered-ability helpers
- Modify: `src/engine/combat.rs` — replace `collect_attack_triggers` call with event emission

**Interfaces:**
- Consumes: `collect_triggers_for_event`, `GameEvent::Attacks` from Task 2

- [ ] **Step 1: Add TriggeredAbility constructors for attack keywords**

Add to `src/engine/triggered.rs` (above the test module) as public functions so test card-builders can use them:

```rust
use crate::types::ability::{TriggerCondition, TriggerEvent, TriggerSubjectFilter, TriggerTargetMode, TriggeredAbility, TurnOwner};
use crate::types::effect::EffectStep;
use crate::types::{CounterKind, PTDelta};

/// CR 702.83b: Exalted — when exactly one creature attacks, it gets +1/+1.
pub fn exalted_triggered_ability() -> TriggeredAbility {
    TriggeredAbility {
        trigger: TriggerEvent::Attacks {
            subject: TriggerSubjectFilter {
                controller: Some(TurnOwner::You),
                ..Default::default()
            },
        },
        condition: Some(TriggerCondition::ExactlyOneAttacker),
        target_mode: TriggerTargetMode::Subject,
        effect: vec![EffectStep::BoostPermanentPT(PTDelta { power: 1, toughness: 1 })],
    }
}

/// CR 702.121b: Melee — when this attacks, it gets +1/+1 (2-player = 1 opponent always).
pub fn melee_triggered_ability() -> TriggeredAbility {
    TriggeredAbility {
        trigger: TriggerEvent::Attacks {
            subject: TriggerSubjectFilter { is_self: Some(true), ..Default::default() },
        },
        condition: None,
        target_mode: TriggerTargetMode::Source,
        effect: vec![EffectStep::BoostPermanentPT(PTDelta { power: 1, toughness: 1 })],
    }
}

/// CR 702.91b: Battle Cry — when this attacks, each other attacking creature gets +1/+0.
pub fn battle_cry_triggered_ability() -> TriggeredAbility {
    TriggeredAbility {
        trigger: TriggerEvent::Attacks {
            subject: TriggerSubjectFilter { is_self: Some(true), ..Default::default() },
        },
        condition: None,
        target_mode: TriggerTargetMode::AllOtherAttackers,
        effect: vec![EffectStep::BoostPermanentPT(PTDelta { power: 1, toughness: 0 })],
    }
}

/// CR 702.149a: Training — when this attacks alongside a creature with greater power, +1/+1 counter.
pub fn training_triggered_ability() -> TriggeredAbility {
    TriggeredAbility {
        trigger: TriggerEvent::Attacks {
            subject: TriggerSubjectFilter { is_self: Some(true), ..Default::default() },
        },
        condition: Some(TriggerCondition::AttackingAlongsideGreaterPowerCreature),
        target_mode: TriggerTargetMode::Source,
        effect: vec![EffectStep::AddCounter {
            kind: CounterKind::PtModifier { power: 1, toughness: 1 },
            count: 1,
        }],
    }
}
```

- [ ] **Step 2: Update test helpers to use TriggeredAbility spans**

In `triggered.rs`'s `#[cfg(test)]` block, the `keyword_attacker` helper currently takes `Vec<StaticAbility>`. Update all test calls that use `StaticAbility::Exalted`, `StaticAbility::Melee`, `StaticAbility::BattleCry`, `StaticAbility::Training` to instead pass `OracleSpan::Parsed(Ability::Triggered(...))` spans.

The easiest way: change `keyword_attacker` to accept `Vec<OracleSpan>` and update callers. For Exalted tests:

```rust
// Old:
keyword_attacker(&mut gs, PlayerId(0), 1, 1, vec![StaticAbility::Exalted])
// New:
let exalted_def = CardDefinition {
    name: "Exalted Permanent".into(),
    mana_cost: None,
    type_line: TypeLine { supertypes: vec![], card_types: vec![CardType::Creature], subtypes: vec![] },
    oracle_text: String::new(),
    abilities: vec![OracleSpan::Parsed(Ability::Triggered(exalted_triggered_ability()))],
    text_annotations: vec![],
    power: Some(1),
    toughness: Some(1),
    colors: vec![],
};
place_on_battlefield(&mut gs, exalted_def, PlayerId(0))
```

Apply the same pattern for Melee, Battle Cry, and Training tests — replace `StaticAbility::Melee/BattleCry/Training` with the corresponding `*_triggered_ability()` span.

- [ ] **Step 3: Wire `Attacks` event emission in `combat.rs`**

In `src/engine/combat.rs`, replace the `collect_attack_triggers` call (around line 54) with:

```rust
// CR 603.2: fire Attacks event for each attacker; collect triggered abilities.
let mut attack_triggers = Vec::new();
for &attacker_id in &state.combat.attackers.clone() {
    let mut t = crate::engine::triggered::collect_triggers_for_event(
        &mut state,
        &crate::types::GameEvent::Attacks { subject_id: attacker_id },
    );
    attack_triggers.append(&mut t);
}
for trigger in attack_triggers {
    let id = trigger.id;
    state.stack.push(id);
    state.stack_objects.insert(id, trigger);
}
```

Remove the `use crate::engine::triggered::{collect_attack_triggers, ...}` import and replace with the remaining needed imports.

- [ ] **Step 4: Delete `collect_attack_triggers`**

Remove the entire `collect_attack_triggers` function from `triggered.rs`.

- [ ] **Step 5: Run tests**

```bash
cargo test 2>&1 | grep -E "^test result|FAILED|error\["
```

Expected: `test result: ok`.

- [ ] **Step 6: Clippy, then commit**

```bash
cargo clippy --all-targets 2>&1 | grep -E "^error|^warning"
git add src/engine/triggered.rs src/engine/combat.rs
git commit -m "feat: migrate Exalted, Melee, Battle Cry, Training to TriggeredAbility + general dispatch"
```

---

### Task 4: Migrate block triggers (Flanking, Bushido)

Same pattern as Task 3. Replace `collect_block_triggers` with `TriggeredAbility` spans + `Blocks`/`BecomesBlocked` event emission in `declare_blockers`.

**Files:**
- Modify: `src/engine/triggered.rs`
- Modify: `src/engine/combat.rs`

- [ ] **Step 1: Add TriggeredAbility constructors for block keywords**

```rust
/// CR 702.25b: Flanking — when a non-Flanking creature blocks this, it gets -1/-1.
pub fn flanking_triggered_ability() -> TriggeredAbility {
    TriggeredAbility {
        trigger: TriggerEvent::Blocks {
            subject: TriggerSubjectFilter {
                controller: Some(TurnOwner::Opponent),
                ..Default::default()
            },
        },
        condition: Some(TriggerCondition::SubjectLacksKeyword(StaticAbility::Flanking)),
        target_mode: TriggerTargetMode::Subject,
        effect: vec![EffectStep::BoostPermanentPT(PTDelta { power: -1, toughness: -1 })],
    }
}

/// CR 702.45b: Bushido N (attacker) — fires when this becomes blocked.
pub fn bushido_attacker_triggered_ability(n: u32) -> TriggeredAbility {
    TriggeredAbility {
        trigger: TriggerEvent::BecomesBlocked {
            subject: TriggerSubjectFilter { is_self: Some(true), ..Default::default() },
        },
        condition: None,
        target_mode: TriggerTargetMode::Source,
        effect: vec![EffectStep::BoostPermanentPT(PTDelta { power: n as i32, toughness: n as i32 })],
    }
}

/// CR 702.45b: Bushido N (blocker) — fires when this blocks.
pub fn bushido_blocker_triggered_ability(n: u32) -> TriggeredAbility {
    TriggeredAbility {
        trigger: TriggerEvent::Blocks {
            subject: TriggerSubjectFilter { is_self: Some(true), ..Default::default() },
        },
        condition: None,
        target_mode: TriggerTargetMode::Source,
        effect: vec![EffectStep::BoostPermanentPT(PTDelta { power: n as i32, toughness: n as i32 })],
    }
}
```

Note: `Bushido N` needs two triggered abilities (one for attacker, one for blocker). A card with Bushido N carries both spans. But `n` is a runtime value — the constructor takes it as a parameter; the card definition stores the concrete spans.

- [ ] **Step 2: Update test helpers for Flanking and Bushido**

Replace `StaticAbility::Flanking` and `StaticAbility::BushidoN(n)` card constructions in the test module with `TriggeredAbility` spans using the constructors above.

For Bushido cards, add BOTH attacker and blocker `TriggeredAbility` spans to the abilities vec:
```rust
abilities: vec![
    OracleSpan::Parsed(Ability::Triggered(bushido_attacker_triggered_ability(2))),
    OracleSpan::Parsed(Ability::Triggered(bushido_blocker_triggered_ability(2))),
],
```

- [ ] **Step 3: Wire Blocks + BecomesBlocked events in `combat.rs`**

In `declare_blockers` (around line 144), replace `collect_block_triggers` with:

```rust
let blocking_map_snapshot: Vec<(ObjectId, Vec<ObjectId>)> = state
    .combat.blocking_map.iter()
    .map(|(&a, bs)| (a, bs.clone()))
    .collect();

let mut block_triggers = Vec::new();

// Fire Blocks event for each blocker.
for (_, blockers) in &blocking_map_snapshot {
    for &blocker_id in blockers {
        let mut t = crate::engine::triggered::collect_triggers_for_event(
            &mut state,
            &crate::types::GameEvent::Blocks { subject_id: blocker_id },
        );
        block_triggers.append(&mut t);
    }
}

// Fire BecomesBlocked event for each attacker that has at least one blocker.
for (attacker_id, blockers) in &blocking_map_snapshot {
    if !blockers.is_empty() {
        let mut t = crate::engine::triggered::collect_triggers_for_event(
            &mut state,
            &crate::types::GameEvent::BecomesBlocked { subject_id: *attacker_id },
        );
        block_triggers.append(&mut t);
    }
}

for trigger in block_triggers {
    let id = trigger.id;
    state.stack.push(id);
    state.stack_objects.insert(id, trigger);
}
```

- [ ] **Step 4: Delete `collect_block_triggers`**

Remove the function from `triggered.rs`.

- [ ] **Step 5: Run tests, clippy, commit**

```bash
cargo test 2>&1 | grep -E "^test result|FAILED|error\["
cargo clippy --all-targets 2>&1 | grep -E "^error|^warning"
git add src/engine/triggered.rs src/engine/combat.rs
git commit -m "feat: migrate Flanking and Bushido to TriggeredAbility + general dispatch"
```

---

### Task 5: Migrate Prowess (cast triggers)

Replace `collect_cast_triggers` + `StaticAbility::Prowess` with a `TriggeredAbility` span + `SpellCast` event emission in `casting.rs`.

**Files:**
- Modify: `src/engine/triggered.rs`
- Modify: `src/engine/casting.rs`

- [ ] **Step 1: Add Prowess TriggeredAbility constructor**

```rust
/// CR 702.108b: Prowess — whenever you cast a noncreature spell, this creature gets +1/+1 until EOT.
pub fn prowess_triggered_ability() -> TriggeredAbility {
    TriggeredAbility {
        trigger: TriggerEvent::SpellCast {
            caster: TurnOwner::You,
            filter: SpellFilter::noncreature(),
        },
        condition: None,
        target_mode: TriggerTargetMode::Source,
        effect: vec![EffectStep::BoostPermanentPT(PTDelta { power: 1, toughness: 1 })],
    }
}
```

- [ ] **Step 2: Update Prowess test cards**

In the test for `collect_cast_triggers_prowess_fires_on_noncreature` and `..._silent_on_creature_spell`, replace `StaticAbility::Prowess` with `Ability::Triggered(prowess_triggered_ability())`.

- [ ] **Step 3: Wire `SpellCast` event in `casting.rs`**

In `src/engine/casting.rs`, after the spell is pushed onto the stack (around line 206 where `collect_cast_triggers` is called), replace with:

```rust
// CR 603.2: fire SpellCast event for triggered abilities.
let spell_cast_triggers = crate::engine::triggered::collect_triggers_for_event(
    &mut state,
    &crate::types::GameEvent::SpellCast { caster: player_id, spell_id: object_id },
);
for t in spell_cast_triggers {
    let id = t.id;
    state.stack.push(id);
    state.stack_objects.insert(id, t);
}
```

Remove the `use crate::engine::triggered::collect_cast_triggers;` import.

- [ ] **Step 4: Delete `collect_cast_triggers`**

- [ ] **Step 5: Run tests, clippy, commit**

```bash
cargo test 2>&1 | grep -E "^test result|FAILED|error\["
cargo clippy --all-targets 2>&1 | grep -E "^error|^warning"
git add src/engine/triggered.rs src/engine/casting.rs
git commit -m "feat: migrate Prowess to TriggeredAbility + general dispatch (SpellCast event)"
```

---

### Task 6: Migrate Ward

Retire `StaticAbility::Ward` and `collect_ward_triggers`. Ward becomes a `TriggeredAbility { trigger: TargetedBy { controller: Opponent }, effect: [Payment { ... }] }` emitted by the parser.

**Files:**
- Modify: `src/types/ability.rs` — remove `StaticAbility::Ward`
- Modify: `src/parser/oracle.rs` — emit `TriggeredAbility` instead of `StaticAbility::Ward`
- Modify: `src/engine/targeting.rs` — replace `collect_ward_triggers` with `TargetedBy` event emission
- Modify: `src/engine/triggered.rs` — delete `collect_ward_triggers`

- [ ] **Step 1: Find all `StaticAbility::Ward` construction sites**

```bash
grep -rn "StaticAbility::Ward\|Ward(" src/ | grep -v "test\|//\|display_name"
```

Note each site — they'll all need updating.

- [ ] **Step 2: Update the parser to emit TriggeredAbility for Ward**

In `src/parser/oracle.rs`, find the Ward keyword parsing (search for `"ward"` case-insensitively). Change the emitted span from:

```rust
OracleSpan::Parsed(Ability::Static(StaticAbility::Ward(cost_components)))
```

to:

```rust
OracleSpan::Parsed(Ability::Triggered(TriggeredAbility {
    trigger: TriggerEvent::TargetedBy { controller: TurnOwner::Opponent },
    condition: None,
    target_mode: TriggerTargetMode::None,
    effect: vec![EffectStep::Payment {
        cost: cost_components,
        on_paid: vec![],
        on_declined: vec![EffectStep::CounterSpell],
    }],
}))
```

Add the needed imports to the parser function.

- [ ] **Step 3: Update targeting.rs to emit TargetedBy event**

In `src/engine/targeting.rs`, find the `collect_ward_triggers` call site. Replace with:

```rust
// CR 702.21a: fire TargetedBy for each opponent-controlled targeted permanent.
let mut ward_triggers = Vec::new();
for target in &spell_targets {
    if let crate::types::effect::EffectTarget::Object { id: target_id } = target {
        if state.objects.get(target_id).map(|o| o.controller != acting_player).unwrap_or(false)
            && state.battlefield.contains_key(target_id)
        {
            let mut t = crate::engine::triggered::collect_triggers_for_event(
                &mut state,
                &crate::types::GameEvent::TargetedBy {
                    target_id: *target_id,
                    acting_player,
                },
            );
            ward_triggers.append(&mut t);
        }
    }
}
// Push Ward triggers above the spell (CR 603.3a: trigger controlled by Ward permanent's controller).
for trigger in ward_triggers.into_iter().rev() {
    let id = trigger.id;
    state.stack.push(id);
    state.stack_objects.insert(id, trigger);
}
```

Note: `collect_triggers_for_event` checks `target_id == source_id` via the `TargetedBy` match arm already written in Task 2. The `acting_player != controller` check happens inside the dispatch.

- [ ] **Step 4: Update test cards to use TriggeredAbility for Ward**

In the Ward tests in `triggered.rs`, replace `StaticAbility::Ward(ward_cost.clone())` with:

```rust
OracleSpan::Parsed(Ability::Triggered(TriggeredAbility {
    trigger: TriggerEvent::TargetedBy { controller: TurnOwner::Opponent },
    condition: None,
    target_mode: TriggerTargetMode::None,
    effect: vec![EffectStep::Payment {
        cost: ward_cost.clone(),
        on_paid: vec![],
        on_declined: vec![EffectStep::CounterSpell],
    }],
}))
```

The Ward test (`collect_ward_triggers_emits_triggered_ability_with_payment`) should now test the `collect_triggers_for_event` path with a `TargetedBy` event.

- [ ] **Step 5: Remove `StaticAbility::Ward` and `collect_ward_triggers`**

- Remove `Ward(Vec<CostComponent>)` from the `StaticAbility` enum.
- Remove its `display_name` branch.
- Delete `collect_ward_triggers` from `triggered.rs`.
- Remove the `collect_ward_triggers` call in `casting.rs`.

- [ ] **Step 6: Run tests, clippy, commit**

```bash
cargo test 2>&1 | grep -E "^test result|FAILED|error\["
cargo clippy --all-targets 2>&1 | grep -E "^error|^warning"
git add src/types/ability.rs src/parser/oracle.rs src/engine/targeting.rs src/engine/triggered.rs src/engine/casting.rs
git commit -m "feat: migrate Ward to TriggeredAbility TargetedBy; delete collect_ward_triggers"
```

---

### Task 7: `Dies` triggers + SBA return type

Change `check_and_apply_sbas` to return `(GameState, Vec<StackObject>)` and emit `Dies` triggers after each `MoveToGraveyard`. This unblocks Persist and Undying.

**Files:**
- Modify: `src/engine/state_based_actions.rs`
- Modify: `src/engine/stack.rs` (3 call sites)
- Modify: `src/engine/combat.rs` (1 call site)
- Modify: `src/engine/casting.rs` (1 call site)

- [ ] **Step 1: Write a failing test for Dies trigger emission**

Add to `src/engine/state_based_actions.rs` tests:

```rust
#[test]
fn check_and_apply_sbas_returns_dies_trigger_when_creature_dies() {
    use crate::engine::triggered::collect_triggers_for_event;
    use crate::types::ability::{Ability, TriggerEvent, TriggerSubjectFilter, TriggerTargetMode, TriggeredAbility};
    use crate::types::{CardObject, OracleSpan, PermanentState, Player, PlayerId, Zone};
    use crate::types::card::{CardDefinition, CardType, TypeLine};
    use crate::types::effect::EffectStep;

    let mut state = make_state();

    // A permanent with "when this dies, draw a card"
    let watcher_def = CardDefinition {
        name: "Doomed Watcher".into(),
        mana_cost: None,
        type_line: TypeLine {
            supertypes: vec![],
            card_types: vec![CardType::Creature],
            subtypes: vec![],
        },
        oracle_text: String::new(),
        abilities: vec![OracleSpan::Parsed(Ability::Triggered(TriggeredAbility {
            trigger: TriggerEvent::Dies {
                subject: TriggerSubjectFilter { is_self: Some(true), ..Default::default() },
            },
            condition: None,
            target_mode: TriggerTargetMode::None,
            effect: vec![EffectStep::DrawCard(1)],
        }))],
        text_annotations: vec![],
        power: Some(1),
        toughness: Some(1),
        colors: vec![],
    };
    let watcher_id = add_creature_to_battlefield(&mut state, PlayerId(0), watcher_def);
    // Mark it as having lethal damage so SBA kills it.
    state.battlefield.get_mut(&watcher_id).unwrap().damage_marked = 99;

    let (new_state, triggers) = check_and_apply_sbas(state);

    assert!(!new_state.battlefield.contains_key(&watcher_id), "creature should be dead");
    assert_eq!(triggers.len(), 1, "should have one Dies trigger");
}
```

Run to confirm failure:
```bash
cargo test check_and_apply_sbas_returns_dies_trigger 2>&1 | grep -E "^test result|FAILED|error\["
```

- [ ] **Step 2: Update `check_and_apply_sbas` signature and implementation**

In `src/engine/state_based_actions.rs`:

```rust
pub fn check_and_apply_sbas(state: GameState) -> (GameState, Vec<crate::types::stack::StackObject>) {
    let mut state = state;
    let mut all_triggers: Vec<crate::types::stack::StackObject> = Vec::new();
    loop {
        let sbas = find_sbas(&state);
        if sbas.is_empty() {
            break;
        }
        let (new_state, triggers) = apply_sbas(state, sbas);
        state = new_state;
        all_triggers.extend(triggers);
    }
    (state, all_triggers)
}
```

Update `apply_sbas` to return `(GameState, Vec<StackObject>)` and emit Dies triggers:

```rust
fn apply_sbas(mut state: GameState, sbas: Vec<Sba>) -> (GameState, Vec<crate::types::stack::StackObject>) {
    let mut triggers = Vec::new();
    for sba in sbas {
        match sba {
            Sba::PlayerLoses(pid) => {
                if let Some(p) = state.get_player_mut(pid) { p.has_lost = true; }
                state.game_over = true;
            }
            Sba::MoveToGraveyard(id) => {
                state = move_to_graveyard(state, id);
                // CR 603.2: collect Dies triggers after the zone change.
                let mut t = crate::engine::triggered::collect_triggers_for_event(
                    &mut state,
                    &crate::types::GameEvent::Dies { subject_id: id },
                );
                triggers.append(&mut t);
            }
            Sba::CancelCounters(id, n) => {
                if let Some(perm) = state.battlefield.get_mut(&id) {
                    perm.remove_counters(&crate::types::CounterKind::PtModifier { power: 1, toughness: 1 }, n);
                    perm.remove_counters(&crate::types::CounterKind::PtModifier { power: -1, toughness: -1 }, n);
                }
            }
        }
    }
    (state, triggers)
}
```

- [ ] **Step 3: Update all call sites**

There are 6+ call sites. Each becomes a destructure:

**`src/engine/stack.rs`** — every `check_and_apply_sbas(state)` becomes:
```rust
let (state, sba_triggers) = check_and_apply_sbas(state);
for t in sba_triggers {
    let id = t.id;
    state.stack.push(id);
    state.stack_objects.insert(id, t);
}
```
(Where `state` was previously returned or assigned — adapt to local control flow. Use `let mut state = state;` as needed.)

Do this for all call sites in `stack.rs`, `combat.rs`, and `casting.rs`.

- [ ] **Step 4: Run tests, clippy, commit**

```bash
cargo test 2>&1 | grep -E "^test result|FAILED|error\["
cargo clippy --all-targets 2>&1 | grep -E "^error|^warning"
git add src/engine/state_based_actions.rs src/engine/stack.rs src/engine/combat.rs src/engine/casting.rs
git commit -m "feat: check_and_apply_sbas returns Dies triggers; enables Persist/Undying"
```

---

### Task 8: Phase/step and draw triggers

Wire `PhaseStep` and `DrawsCard` event emission. No keyword migrations in this task — it simply adds the emission hooks that future abilities (Cumulative Upkeep, Rhystic Study) will use.

**Files:**
- Modify: `src/engine/turn.rs`

- [ ] **Step 1: Wire `PhaseStep` in `apply_step_start` / `advance_step`**

Find where each step starts in `src/engine/turn.rs`. In the function that runs step-start logic (search for `Step::Upkeep` handling), after any existing step logic, add:

```rust
// CR 603.2: fire PhaseStep event for step-triggered abilities.
let step_triggers = crate::engine::triggered::collect_triggers_for_event(
    &mut state,
    &crate::types::GameEvent::PhaseStep {
        step: state.step,
        active_player: state.active_player,
    },
);
for t in step_triggers {
    let id = t.id;
    state.stack.push(id);
    state.stack_objects.insert(id, t);
}
```

- [ ] **Step 2: Wire `DrawsCard` in `draw_card`**

In `src/engine/turn.rs`'s `draw_card` function, after the card is drawn, add:

```rust
let draw_triggers = crate::engine::triggered::collect_triggers_for_event(
    &mut state,
    &crate::types::GameEvent::DrawsCard { player: player_id },
);
for t in draw_triggers {
    let id = t.id;
    state.stack.push(id);
    state.stack_objects.insert(id, t);
}
```

- [ ] **Step 3: Add tests confirming no spurious triggers fire**

```rust
#[test]
fn draw_event_does_not_fire_non_draw_triggers() {
    // Sanity check: an ETB trigger on a creature in play does not fire during draw.
    // (No positive test yet — no DrawsCard keyword implemented — but ensure no panic/crash.)
    // This is a smoke test; remove when a real DrawsCard keyword is added.
}
```

- [ ] **Step 4: Run tests, clippy, commit**

```bash
cargo test 2>&1 | grep -E "^test result|FAILED|error\["
cargo clippy --all-targets 2>&1 | grep -E "^error|^warning"
git add src/engine/turn.rs
git commit -m "feat: wire PhaseStep and DrawsCard event emission (unblocks Cumulative Upkeep, Rhystic Study)"
```

---

### Task 9: Combat damage triggers

Wire `DealsCombatDamage` event emission in `deal_combat_damage`. No keyword migrations — adds the hook that Coastal Piracy, Ohran Viper, etc. will use.

**Files:**
- Modify: `src/engine/combat.rs`

- [ ] **Step 1: Wire `DealsCombatDamage` emission**

In `src/engine/combat.rs`'s `deal_combat_damage` function, after damage is applied and before the function returns, add:

```rust
// CR 603.2: fire DealsCombatDamage for each permanent that dealt combat damage to a player.
// Only player damage is tracked here; creature-damage triggers are a future extension.
for (&attacker_id, &dmg) in &damage_to_players {
    if dmg > 0 {
        // dmg key is PlayerId; we need to find which attacker dealt it.
        // Iterate attackers and check if they contributed to this player's damage.
        // For now emit once per attacker that attacked (simplified; refine when a keyword needs it).
        let _ = crate::engine::triggered::collect_triggers_for_event(
            &mut state,
            &crate::types::GameEvent::DealsCombatDamage {
                subject_id: attacker_id,
                to: crate::types::DamageTargetKind::Player,
            },
        );
        // Triggers are not pushed yet — no keyword uses this event. Add push when needed.
    }
}
```

Note: This wires the hook without pushing triggers yet (no keyword uses `DealsCombatDamage` at this point). The `collect_triggers_for_event` call will return `vec![]` until a relevant `TriggeredAbility` exists. Remove the `let _ =` and add the push loop once a keyword migration needs it.

- [ ] **Step 2: Run tests, clippy, commit**

```bash
cargo test 2>&1 | grep -E "^test result|FAILED|error\["
cargo clippy --all-targets 2>&1 | grep -E "^error|^warning"
git add src/engine/combat.rs
git commit -m "feat: wire DealsCombatDamage event emission (unblocks Coastal Piracy, Ohran Viper)"
```

---

## Self-Review Notes

- **`trigger_condition_satisfied`**: The `EnteringCreatureHasGreaterPower/Toughness` arms use `?` in a non-`Option`-returning function body. Wrap each arm in `(|| -> Option<bool> { ... })().unwrap_or(false)` or restructure as explicit `match`.
- **Evolve migration (Task 2)**: `collect_evolve_triggers` is deleted when ETB general dispatch covers it. Verify the `Evolve` keyword's `StaticAbility::Evolve` variant is also removed from the enum (or kept if any other code still reads it — check with `grep -rn "StaticAbility::Evolve" src/`).
- **StaticAbility cleanup**: After Tasks 3–6, `Exalted`, `Melee`, `BattleCry`, `Training`, `Flanking`, `BushidoN`, `Prowess` are removed from `StaticAbility`. Run `grep -rn "StaticAbility::" src/` and remove all dead branches and display_name arms.
- **Battle Cry multi-target**: `TriggerTargetMode::AllOtherAttackers` produces one `StackObject` with multiple targets. Verify `execute_effect_steps` in `stack.rs` iterates all targets (not just `targets.first()`) for `BoostPermanentPT`. If it doesn't, add a loop.
- **`mana_value()` on `ManaCost`**: Used in Task 2's `SpellCast` matching. Verify `ManaCost::mana_value()` exists; if not, add it to `src/types/mana.rs`.
