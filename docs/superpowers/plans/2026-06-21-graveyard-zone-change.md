# Graveyard Zone-Change Hook + Persist/Undying Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add a generic `MoveZone` effect step and use it to implement Persist (CR 702.79) and Undying (CR 702.93) via runtime shims in the trigger collection path.

**Architecture:** New data types land first (Task 1). The trigger system gains `SubjectLacksCounter` and per-loop shims for `StaticAbility::Persist/Undying` in `collect_triggers_for_event` (Task 2). `execute_effect_steps` gets a `MoveZone` arm that transitions objects between zones and fires ETB triggers inline (Task 3). End-to-end tests drive the full SBA → trigger → resolve pipeline, and `docs/todo.md` is updated (Task 4).

**Tech Stack:** Rust; `cargo clippy --all-targets`; `cargo test`; inline `#[cfg(test)] mod tests` in every source file.

## Global Constraints

- All tests: `cargo test 2>&1 | grep -E "^test result|FAILED|error\\["` must show no failures after every commit.
- Clippy: `cargo clippy --all-targets` must be warning-free before the final commit. Use `cargo clippy --fix --all-targets` as a first pass.
- `Zone` is `Copy`; `ZoneOwner` must also be `#[derive(Debug, Clone, Copy, PartialEq, Eq)]`.
- Graveyard zone: `state.graveyards: HashMap<PlayerId, Vec<ObjectId>>`; keyed by **owner** (`CardObject.owner`), not controller.
- The dying creature is still in `state.battlefield` when `collect_triggers_for_event` is called for a Dies event — the zone change happens *after* trigger collection (CR 603.10a; see comment in `engine/state_based_actions.rs:apply_sbas`).
- Tests use `PlayerId(0)` as the creature owner/controller throughout unless opponent involvement is explicitly needed.
- Spec: `docs/superpowers/specs/2026-06-21-graveyard-zone-change-design.md`.
- Do **not** modify `engine/state_based_actions.rs::move_to_graveyard`; it is correct.
- `TriggerCondition::SubjectLacksCounter` targets the *subject* via `state.battlefield` (the dying permanent, still visible at collection time). It returns `false` if the subject is not found.

---

## File Map

| File | Action | What changes |
|---|---|---|
| `src/types/zone.rs` | Modify | Add `ZoneOwner` enum |
| `src/types/ability.rs` | Modify | Add import `CounterKind`; add `StaticAbility::Persist/Undying`; add `TriggerCondition::SubjectLacksCounter(CounterKind)` |
| `src/types/effect.rs` | Modify | Add import `Zone, ZoneOwner`; add `EffectStep::MoveZone` |
| `src/types/mod.rs` | Modify | `pub use zone::Zone;` → `pub use zone::{Zone, ZoneOwner};` |
| `src/engine/triggered.rs` | Modify | Add `SubjectLacksCounter` arm in `trigger_condition_satisfied`; add Persist/Undying shims inside `collect_triggers_for_event` loop |
| `src/engine/stack.rs` | Modify | Add `MoveZone` arm in `execute_effect_steps` |
| `docs/todo.md` | Modify | Remove Persist/Undying items; add activated-from-other-zones section |

---

### Task 1: New types — `ZoneOwner`, `StaticAbility::Persist/Undying`, `TriggerCondition::SubjectLacksCounter`, `EffectStep::MoveZone`

**Files:**
- Modify: `src/types/zone.rs`
- Modify: `src/types/ability.rs`
- Modify: `src/types/effect.rs`
- Modify: `src/types/mod.rs`

**Interfaces:**
- Produces:
  - `crate::types::ZoneOwner` (`CardOwner | CardController`, `Copy`)
  - `StaticAbility::Persist`, `StaticAbility::Undying` with `display_name()` returning `"Persist"` / `"Undying"`
  - `TriggerCondition::SubjectLacksCounter(CounterKind)`
  - `EffectStep::MoveZone { from: Zone, to: Zone, to_player: ZoneOwner }`

---

- [ ] **Step 1: Write failing tests (types don't exist yet — these will not compile)**

Add to the existing `#[cfg(test)] mod tests` block in `src/types/zone.rs`:

```rust
#[test]
fn zone_owner_is_copy() {
    let a = ZoneOwner::CardOwner;
    let b = a; // Copy
    assert_eq!(a, b);
    assert_ne!(ZoneOwner::CardOwner, ZoneOwner::CardController);
}
```

Add to the existing `#[cfg(test)] mod tests` in `src/types/ability.rs`:

```rust
#[test]
fn display_name_persist_undying() {
    assert_eq!(StaticAbility::Persist.display_name(), "Persist");
    assert_eq!(StaticAbility::Undying.display_name(), "Undying");
}

#[test]
fn subject_lacks_counter_construction() {
    let cond = TriggerCondition::SubjectLacksCounter(crate::types::CounterKind::PtModifier {
        power: -1,
        toughness: -1,
    });
    assert!(matches!(cond, TriggerCondition::SubjectLacksCounter(_)));
}
```

Add to the existing `#[cfg(test)] mod tests` in `src/types/effect.rs`:

```rust
#[test]
fn move_zone_step_construction() {
    use crate::types::zone::{Zone, ZoneOwner};
    let step = EffectStep::MoveZone {
        from: Zone::Graveyard,
        to: Zone::Battlefield,
        to_player: ZoneOwner::CardOwner,
    };
    assert!(matches!(step, EffectStep::MoveZone { .. }));
}
```

Run: `cargo test 2>&1 | grep -E "^test result|FAILED|error\["`
Expected: compile errors (types undefined).

- [ ] **Step 2: Add `ZoneOwner` to `src/types/zone.rs`**

Append after the existing `Zone` enum:

```rust
/// Determines whose player-specific zone is used as the destination in a `MoveZone` step,
/// or who controls a permanent entering the battlefield.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ZoneOwner {
    /// The card's original owner (the player who started the game with it).
    CardOwner,
    /// The card's controller at the time of the zone change.
    CardController,
}
```

- [ ] **Step 3: Add `Persist`, `Undying`, and `SubjectLacksCounter` to `src/types/ability.rs`**

Add import at the top of the file (after the existing `use super::` lines):

```rust
use super::counter::CounterKind;
```

Add two variants to `StaticAbility` after `Training`:

```rust
Persist,  // CR 702.79
Undying,  // CR 702.93
```

Add their display names inside `StaticAbility::display_name()` (before the closing `}`):

```rust
Self::Persist => "Persist".to_string(),
Self::Undying => "Undying".to_string(),
```

Add `SubjectLacksCounter` to `TriggerCondition` after the existing `SubjectLacksKeyword` arm:

```rust
SubjectLacksCounter(CounterKind), // CR 702.79 Persist / CR 702.93 Undying
```

- [ ] **Step 4: Add `EffectStep::MoveZone` to `src/types/effect.rs`**

Add after the existing imports at the top of the file:

```rust
use super::zone::{Zone, ZoneOwner};
```

Add the `MoveZone` variant to `EffectStep` after `AddCounter`:

```rust
/// Move a card object between zones (CR 400.7).
/// `from` is the expected current zone; step is a no-op if the object is not there.
/// `to_player` determines who controls a permanent entering the battlefield, or whose
/// hand/library/graveyard receives the card for player-specific destination zones.
MoveZone {
    from: Zone,
    to: Zone,
    to_player: ZoneOwner,
},
```

- [ ] **Step 5: Update `src/types/mod.rs` to re-export `ZoneOwner`**

Change line 32:

```rust
pub use zone::Zone;
```

to:

```rust
pub use zone::{Zone, ZoneOwner};
```

- [ ] **Step 6: Run tests — expect all to pass**

Run: `cargo test 2>&1 | grep -E "^test result|FAILED|error\["`
Expected: all tests pass (including the three new construction tests).

- [ ] **Step 7: Commit**

```bash
git add src/types/zone.rs src/types/ability.rs src/types/effect.rs src/types/mod.rs
git commit -m "feat: add ZoneOwner, MoveZone effect step, Persist/Undying static abilities, SubjectLacksCounter trigger condition"
```

---

### Task 2: `SubjectLacksCounter` condition + Persist/Undying shims

**Files:**
- Modify: `src/engine/triggered.rs`

**Interfaces:**
- Consumes: `TriggerCondition::SubjectLacksCounter(CounterKind)`, `StaticAbility::Persist`, `StaticAbility::Undying`, `EffectStep::MoveZone`, `ZoneOwner` (all from Task 1)
- Produces:
  - `trigger_condition_satisfied` evaluates `SubjectLacksCounter` correctly
  - `collect_triggers_for_event` emits one Persist trigger on `GameEvent::Dies` when the dying creature has `StaticAbility::Persist` and no `-1/-1` counter; suppressed otherwise
  - Same for `StaticAbility::Undying` / `+1/+1`

---

- [ ] **Step 1: Write four failing tests for `SubjectLacksCounter` condition (via TriggeredAbility dispatch)**

These tests create a creature carrying a `TriggeredAbility` with `condition: Some(TriggerCondition::SubjectLacksCounter(...))` and fire a `Dies` event through `collect_triggers_for_event`. They fail because the condition arm doesn't exist yet (the match is non-exhaustive or panics).

Add to the existing `#[cfg(test)] mod tests` in `src/engine/triggered.rs`:

```rust
#[test]
fn subject_lacks_counter_condition_satisfied_when_no_counter() {
    // TriggerCondition::SubjectLacksCounter fires when the subject has zero of the given counter.
    use crate::types::ability::{TriggerSubjectFilter, TriggerTargetMode};
    use crate::types::effect::EffectStep;
    use crate::types::{CounterKind, GameEvent, RulesText};
    use crate::types::ability::Rule;

    let mut gs = two_player_state();
    let kind = CounterKind::PtModifier { power: -1, toughness: -1 };
    let trigger_span = RulesText::Active(Rule::Triggered(TriggeredAbility {
        trigger: TriggerEvent::Dies {
            subject: TriggerSubjectFilter { is_self: Some(true), ..Default::default() },
        },
        condition: Some(TriggerCondition::SubjectLacksCounter(kind.clone())),
        target_mode: TriggerTargetMode::None,
        effect: vec![EffectStep::DrawCard(1)],
    }));
    let id = triggered_attacker(&mut gs, PlayerId(0), 2, 2, vec![trigger_span]);

    // No counter on the creature → condition satisfied → trigger fires.
    let triggers = collect_triggers_for_event(&mut gs, &GameEvent::Dies { subject_id: id });
    assert_eq!(triggers.len(), 1);
}

#[test]
fn subject_lacks_counter_condition_not_satisfied_when_counter_present() {
    // TriggerCondition::SubjectLacksCounter does NOT fire when the subject has the counter.
    use crate::types::ability::{TriggerSubjectFilter, TriggerTargetMode};
    use crate::types::effect::EffectStep;
    use crate::types::{CounterKind, GameEvent, RulesText};
    use crate::types::ability::Rule;

    let mut gs = two_player_state();
    let kind = CounterKind::PtModifier { power: -1, toughness: -1 };
    let trigger_span = RulesText::Active(Rule::Triggered(TriggeredAbility {
        trigger: TriggerEvent::Dies {
            subject: TriggerSubjectFilter { is_self: Some(true), ..Default::default() },
        },
        condition: Some(TriggerCondition::SubjectLacksCounter(kind.clone())),
        target_mode: TriggerTargetMode::None,
        effect: vec![EffectStep::DrawCard(1)],
    }));
    let id = triggered_attacker(&mut gs, PlayerId(0), 2, 2, vec![trigger_span]);
    gs.battlefield.get_mut(&id).unwrap().add_counters(kind, 1);

    // Has the counter → condition not satisfied → no trigger.
    let triggers = collect_triggers_for_event(&mut gs, &GameEvent::Dies { subject_id: id });
    assert!(triggers.is_empty());
}
```

Run: `cargo test engine::triggered::tests::subject_lacks_counter_condition_satisfied_when_no_counter 2>&1 | grep -E "^test result|FAILED|error\["`
Expected: FAIL (non-exhaustive match on `TriggerCondition`).

- [ ] **Step 2: Write four more failing tests for Persist and Undying shims**

```rust
#[test]
fn persist_trigger_fires_on_death_when_no_minus_counter() {
    // CR 702.79: StaticAbility::Persist shim fires when no -1/-1 counter present.
    use crate::types::effect::EffectStep;
    use crate::types::stack::StackPayload;
    use crate::types::zone::{Zone, ZoneOwner};
    use crate::types::{CounterKind, GameEvent, RulesText};
    use crate::types::ability::{Rule, StaticAbility};

    let mut gs = two_player_state();
    let def = crate::types::card::CardDefinition {
        name: "Young Wolf".into(),
        mana_cost: None,
        type_line: crate::types::card::TypeLine {
            supertypes: vec![],
            card_types: vec![crate::types::card::CardType::Creature],
            subtypes: vec![],
        },
        oracle_text: "Persist".into(),
        rules_text: vec![RulesText::Active(Rule::Static(StaticAbility::Persist))],
        text_annotations: vec![],
        power: Some(2),
        toughness: Some(2),
        colors: vec![],
    };
    let id = place_on_battlefield(&mut gs, def, PlayerId(0));

    let triggers = collect_triggers_for_event(&mut gs, &GameEvent::Dies { subject_id: id });

    assert_eq!(triggers.len(), 1, "exactly one Persist trigger");
    let t = &triggers[0];
    assert_eq!(t.controller, PlayerId(0));
    use crate::types::effect::EffectTarget;
    assert_eq!(t.targets, vec![EffectTarget::Object { id }]);
    let StackPayload::TriggeredAbility { effect, .. } = &t.payload else {
        panic!("expected TriggeredAbility");
    };
    assert_eq!(effect.len(), 2);
    assert!(matches!(
        &effect[0],
        EffectStep::MoveZone {
            from: Zone::Graveyard,
            to: Zone::Battlefield,
            to_player: ZoneOwner::CardOwner,
        }
    ));
    assert!(matches!(
        &effect[1],
        EffectStep::AddCounter {
            kind: CounterKind::PtModifier { power: -1, toughness: -1 },
            count: 1
        }
    ));
}

#[test]
fn persist_trigger_suppressed_when_minus_counter_present() {
    // CR 702.79: Persist does not fire when the dying creature already has a -1/-1 counter.
    use crate::types::{CounterKind, GameEvent, RulesText};
    use crate::types::ability::{Rule, StaticAbility};

    let mut gs = two_player_state();
    let def = crate::types::card::CardDefinition {
        name: "Young Wolf".into(),
        mana_cost: None,
        type_line: crate::types::card::TypeLine {
            supertypes: vec![],
            card_types: vec![crate::types::card::CardType::Creature],
            subtypes: vec![],
        },
        oracle_text: "Persist".into(),
        rules_text: vec![RulesText::Active(Rule::Static(StaticAbility::Persist))],
        text_annotations: vec![],
        power: Some(2),
        toughness: Some(2),
        colors: vec![],
    };
    let id = place_on_battlefield(&mut gs, def, PlayerId(0));
    gs.battlefield.get_mut(&id).unwrap()
        .add_counters(CounterKind::PtModifier { power: -1, toughness: -1 }, 1);

    let triggers = collect_triggers_for_event(&mut gs, &GameEvent::Dies { subject_id: id });
    assert!(triggers.is_empty(), "Persist must not trigger when -1/-1 counter present");
}

#[test]
fn undying_trigger_fires_on_death_when_no_plus_counter() {
    // CR 702.93: StaticAbility::Undying shim fires when no +1/+1 counter present.
    use crate::types::effect::EffectStep;
    use crate::types::stack::StackPayload;
    use crate::types::zone::{Zone, ZoneOwner};
    use crate::types::{CounterKind, GameEvent, RulesText};
    use crate::types::ability::{Rule, StaticAbility};

    let mut gs = two_player_state();
    let def = crate::types::card::CardDefinition {
        name: "Strangleroot Geist".into(),
        mana_cost: None,
        type_line: crate::types::card::TypeLine {
            supertypes: vec![],
            card_types: vec![crate::types::card::CardType::Creature],
            subtypes: vec![],
        },
        oracle_text: "Undying".into(),
        rules_text: vec![RulesText::Active(Rule::Static(StaticAbility::Undying))],
        text_annotations: vec![],
        power: Some(2),
        toughness: Some(1),
        colors: vec![],
    };
    let id = place_on_battlefield(&mut gs, def, PlayerId(0));

    let triggers = collect_triggers_for_event(&mut gs, &GameEvent::Dies { subject_id: id });

    assert_eq!(triggers.len(), 1, "exactly one Undying trigger");
    let t = &triggers[0];
    use crate::types::effect::EffectTarget;
    assert_eq!(t.targets, vec![EffectTarget::Object { id }]);
    let StackPayload::TriggeredAbility { effect, .. } = &t.payload else {
        panic!("expected TriggeredAbility");
    };
    assert_eq!(effect.len(), 2);
    assert!(matches!(
        &effect[0],
        EffectStep::MoveZone {
            from: Zone::Graveyard,
            to: Zone::Battlefield,
            to_player: ZoneOwner::CardOwner,
        }
    ));
    assert!(matches!(
        &effect[1],
        EffectStep::AddCounter {
            kind: CounterKind::PtModifier { power: 1, toughness: 1 },
            count: 1
        }
    ));
}

#[test]
fn undying_trigger_suppressed_when_plus_counter_present() {
    // CR 702.93: Undying does not fire when the dying creature already has a +1/+1 counter.
    use crate::types::{CounterKind, GameEvent, RulesText};
    use crate::types::ability::{Rule, StaticAbility};

    let mut gs = two_player_state();
    let def = crate::types::card::CardDefinition {
        name: "Strangleroot Geist".into(),
        mana_cost: None,
        type_line: crate::types::card::TypeLine {
            supertypes: vec![],
            card_types: vec![crate::types::card::CardType::Creature],
            subtypes: vec![],
        },
        oracle_text: "Undying".into(),
        rules_text: vec![RulesText::Active(Rule::Static(StaticAbility::Undying))],
        text_annotations: vec![],
        power: Some(2),
        toughness: Some(1),
        colors: vec![],
    };
    let id = place_on_battlefield(&mut gs, def, PlayerId(0));
    gs.battlefield.get_mut(&id).unwrap()
        .add_counters(CounterKind::PtModifier { power: 1, toughness: 1 }, 1);

    let triggers = collect_triggers_for_event(&mut gs, &GameEvent::Dies { subject_id: id });
    assert!(triggers.is_empty(), "Undying must not trigger when +1/+1 counter present");
}
```

Run: `cargo test 2>&1 | grep -E "^test result|FAILED|error\["`
Expected: compile error or test failures for the six new tests.

- [ ] **Step 3: Add `SubjectLacksCounter` arm to `trigger_condition_satisfied`**

In `src/engine/triggered.rs`, inside `trigger_condition_satisfied`, add after the `SubjectLacksKeyword` arm:

```rust
TriggerCondition::SubjectLacksCounter(kind) => {
    let sid = match subject_id {
        Some(id) => id,
        None => return false,
    };
    state
        .battlefield
        .get(&sid)
        .map_or(false, |p| p.counter_count(kind) == 0)
}
```

Run: `cargo test engine::triggered::tests::subject_lacks_counter_condition_satisfied_when_no_counter engine::triggered::tests::subject_lacks_counter_condition_not_satisfied_when_counter_present 2>&1 | grep -E "^test result|FAILED|error\["`
Expected: both PASS. The four shim tests still fail.

- [ ] **Step 4: Add Persist and Undying shims inside the `collect_triggers_for_event` loop**

In `src/engine/triggered.rs`, inside the `for source_id in source_ids { ... }` loop body — add the block below **after** the existing Evolve shim (i.e., before the closing `}` of the for loop):

```rust
// TRANSITIONAL SHIM — StaticAbility::Persist / StaticAbility::Undying.
// When the parser emits TriggeredAbility spans for these keywords, remove this block.
// IMPORTANT: If a card carries both StaticAbility::Persist AND a TriggeredAbility Persist
// span, it will double-fire. Remove this shim before that migration begins.
if let GameEvent::Dies { subject_id: dying_id } = event {
    use crate::types::zone::{Zone, ZoneOwner};

    let dying_id = *dying_id;
    if source_id == dying_id {
        let has_persist = rules_text.iter().any(|span| {
            matches!(span, RulesText::Active(Rule::Static(StaticAbility::Persist)))
        });
        let has_undying = rules_text.iter().any(|span| {
            matches!(span, RulesText::Active(Rule::Static(StaticAbility::Undying)))
        });

        if has_persist {
            let minus_key = crate::types::CounterKind::PtModifier { power: -1, toughness: -1 };
            let has_minus = state
                .battlefield
                .get(&dying_id)
                .map(|p| p.counter_count(&minus_key) > 0)
                .unwrap_or(false);
            if !has_minus {
                let sid = state.alloc_stack_id();
                let label = format!(
                    "{}: Persist",
                    state.objects.get(&dying_id).map(|o| o.definition.name.as_str()).unwrap_or("?")
                );
                result.push(StackObject {
                    id: sid,
                    payload: StackPayload::TriggeredAbility {
                        source_id: dying_id,
                        effect: vec![
                            EffectStep::MoveZone {
                                from: Zone::Graveyard,
                                to: Zone::Battlefield,
                                to_player: ZoneOwner::CardOwner,
                            },
                            EffectStep::AddCounter {
                                kind: minus_key,
                                count: 1,
                            },
                        ],
                        label,
                    },
                    controller,
                    targets: vec![EffectTarget::Object { id: dying_id }],
                    x_value: None,
                });
            }
        }

        if has_undying {
            let plus_key = crate::types::CounterKind::PtModifier { power: 1, toughness: 1 };
            let has_plus = state
                .battlefield
                .get(&dying_id)
                .map(|p| p.counter_count(&plus_key) > 0)
                .unwrap_or(false);
            if !has_plus {
                let sid = state.alloc_stack_id();
                let label = format!(
                    "{}: Undying",
                    state.objects.get(&dying_id).map(|o| o.definition.name.as_str()).unwrap_or("?")
                );
                result.push(StackObject {
                    id: sid,
                    payload: StackPayload::TriggeredAbility {
                        source_id: dying_id,
                        effect: vec![
                            EffectStep::MoveZone {
                                from: Zone::Graveyard,
                                to: Zone::Battlefield,
                                to_player: ZoneOwner::CardOwner,
                            },
                            EffectStep::AddCounter {
                                kind: plus_key,
                                count: 1,
                            },
                        ],
                        label,
                    },
                    controller,
                    targets: vec![EffectTarget::Object { id: dying_id }],
                    x_value: None,
                });
            }
        }
    }
}
```

The shim references `StaticAbility` which is already in scope via `use crate::types::ability::{Rule, StaticAbility, TriggerEvent, ...}` at the top of `triggered.rs`. It also references `EffectStep`, `EffectTarget`, `StackObject`, `StackPayload` which are all already imported — confirm the top-of-file imports include them; add any that are missing.

Run: `cargo test 2>&1 | grep -E "^test result|FAILED|error\["`
Expected: all six new tests PASS and no regressions.

- [ ] **Step 5: Commit**

```bash
git add src/engine/triggered.rs
git commit -m "feat: SubjectLacksCounter trigger condition; Persist/Undying shims in collect_triggers_for_event"
```

---

### Task 3: `MoveZone` execution in `execute_effect_steps`

**Files:**
- Modify: `src/engine/stack.rs`

**Interfaces:**
- Consumes: `EffectStep::MoveZone { from: Zone, to: Zone, to_player: ZoneOwner }` (Task 1)
- Produces: `execute_effect_steps` handles `MoveZone` — transitions the targeted object between zones, creates `PermanentState` and fires ETB triggers when destination is `Battlefield`, is a no-op when object is not in `from` zone

---

- [ ] **Step 1: Write three failing tests**

Add to the existing `#[cfg(test)] mod tests` in `src/engine/stack.rs`:

```rust
#[test]
fn move_zone_graveyard_to_battlefield_transitions_object() {
    // MoveZone moves a card from the graveyard to the battlefield.
    use crate::types::effect::{EffectStep, EffectTarget};
    use crate::types::zone::{Zone, ZoneOwner};
    use crate::types::{CardObject, PermanentState};

    let mut gs = make_state();

    let def = CardDefinition {
        name: "Persist Test".into(),
        mana_cost: None,
        type_line: TypeLine { supertypes: vec![], card_types: vec![CardType::Creature], subtypes: vec![] },
        oracle_text: String::new(),
        rules_text: vec![],
        text_annotations: vec![],
        power: Some(2),
        toughness: Some(2),
        colors: vec![],
    };
    let id = gs.alloc_id();
    let obj = CardObject::new(id, def, PlayerId(0), Zone::Graveyard);
    gs.graveyards.get_mut(&PlayerId(0)).unwrap().push(id);
    gs.add_object(obj);

    let targets = vec![EffectTarget::Object { id }];
    let gs = super::execute_effect_steps(
        gs,
        PlayerId(0),
        &[EffectStep::MoveZone { from: Zone::Graveyard, to: Zone::Battlefield, to_player: ZoneOwner::CardOwner }],
        &targets,
        None,
    );

    assert!(gs.battlefield.contains_key(&id), "object should be on battlefield");
    assert_eq!(gs.objects[&id].zone, Zone::Battlefield);
    assert!(!gs.graveyards[&PlayerId(0)].contains(&id), "should not be in graveyard");
    assert_eq!(gs.objects[&id].controller, PlayerId(0));
}

#[test]
fn move_zone_noop_when_object_not_in_from_zone() {
    // MoveZone is a no-op if the object is not in the specified `from` zone.
    use crate::types::effect::{EffectStep, EffectTarget};
    use crate::types::zone::{Zone, ZoneOwner};
    use crate::types::{CardObject, PermanentState};

    let mut gs = make_state();

    // Creature is on the battlefield, but from: Graveyard — should be a no-op.
    let def = CardDefinition {
        name: "Battlefield Creature".into(),
        mana_cost: None,
        type_line: TypeLine { supertypes: vec![], card_types: vec![CardType::Creature], subtypes: vec![] },
        oracle_text: String::new(),
        rules_text: vec![],
        text_annotations: vec![],
        power: Some(2),
        toughness: Some(2),
        colors: vec![],
    };
    let id = gs.alloc_id();
    let obj = CardObject::new(id, def, PlayerId(0), Zone::Battlefield);
    gs.battlefield.insert(id, PermanentState::new(&obj.definition));
    gs.add_object(obj);

    let targets = vec![EffectTarget::Object { id }];
    let gs = super::execute_effect_steps(
        gs,
        PlayerId(0),
        &[EffectStep::MoveZone { from: Zone::Graveyard, to: Zone::Battlefield, to_player: ZoneOwner::CardOwner }],
        &targets,
        None,
    );

    assert!(gs.battlefield.contains_key(&id), "object should still be on battlefield");
    assert_eq!(gs.objects[&id].zone, Zone::Battlefield);
    assert!(gs.graveyards[&PlayerId(0)].is_empty());
}

#[test]
fn move_zone_to_battlefield_fires_etb_triggers() {
    // When MoveZone moves a card to the battlefield, any ETB triggers are pushed onto the stack.
    use crate::types::ability::{Rule, TriggerEvent, TriggerSubjectFilter, TriggerTargetMode, TriggeredAbility};
    use crate::types::effect::{EffectStep, EffectTarget};
    use crate::types::zone::{Zone, ZoneOwner};
    use crate::types::{CardObject, RulesText};

    let mut gs = make_state();
    put_in_library(&mut gs, PlayerId(0));

    // A card in the graveyard with an ETB trigger.
    let def = CardDefinition {
        name: "ETB Creature".into(),
        mana_cost: None,
        type_line: TypeLine { supertypes: vec![], card_types: vec![CardType::Creature], subtypes: vec![] },
        oracle_text: "When this enters, draw a card.".into(),
        rules_text: vec![RulesText::Active(Rule::Triggered(TriggeredAbility {
            trigger: TriggerEvent::EntersTheBattlefield {
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
    let id = gs.alloc_id();
    let obj = CardObject::new(id, def, PlayerId(0), Zone::Graveyard);
    gs.graveyards.get_mut(&PlayerId(0)).unwrap().push(id);
    gs.add_object(obj);

    let targets = vec![EffectTarget::Object { id }];
    let gs = super::execute_effect_steps(
        gs,
        PlayerId(0),
        &[EffectStep::MoveZone { from: Zone::Graveyard, to: Zone::Battlefield, to_player: ZoneOwner::CardOwner }],
        &targets,
        None,
    );

    assert!(gs.battlefield.contains_key(&id));
    assert_eq!(gs.stack.len(), 1, "ETB trigger should be on the stack");
    // Card not yet drawn — trigger hasn't resolved.
    assert!(gs.hands[&PlayerId(0)].is_empty());
}
```

Run: `cargo test engine::stack::tests::move_zone_graveyard_to_battlefield_transitions_object 2>&1 | grep -E "^test result|FAILED|error\["`
Expected: FAIL (`MoveZone` arm not yet implemented — `Unimplemented` arm catches it).

- [ ] **Step 2: Add `MoveZone` arm to `execute_effect_steps` in `src/engine/stack.rs`**

Add the import at the top of `stack.rs` (alongside existing use statements):

```rust
use crate::types::ZoneOwner;
```

Inside `execute_effect_steps`, add before the `EffectStep::Unimplemented(_) => {}` arm:

```rust
EffectStep::MoveZone { from, to, to_player } => {
    for target in targets.iter() {
        if let EffectTarget::Object { id } = target {
            let id = *id;
            // Snapshot before mutation to avoid borrow conflicts.
            let (owner, current_zone, controller_at_move, def) =
                match state.objects.get(&id) {
                    Some(o) => (o.owner, o.zone, o.controller, o.definition.clone()),
                    None => continue,
                };
            // No-op if the object is not in the expected source zone.
            if current_zone != *from {
                continue;
            }
            let new_controller = match to_player {
                ZoneOwner::CardOwner => owner,
                ZoneOwner::CardController => controller_at_move,
            };
            // Remove from source zone.
            match from {
                Zone::Graveyard => {
                    if let Some(gy) = state.graveyards.get_mut(&owner) {
                        gy.retain(|&x| x != id);
                    }
                }
                Zone::Battlefield => {
                    state.battlefield.remove(&id);
                }
                Zone::Exile => {
                    state.exile.retain(|&x| x != id);
                }
                Zone::Hand => {
                    if let Some(hand) = state.hands.get_mut(&owner) {
                        hand.retain(|&x| x != id);
                    }
                }
                Zone::Library => {
                    if let Some(lib) = state.libraries.get_mut(&owner) {
                        lib.retain(|&x| x != id);
                    }
                }
                Zone::Stack | Zone::Command => {}
            }
            // Update object zone and controller.
            if let Some(obj) = state.objects.get_mut(&id) {
                obj.zone = *to;
                if *to == Zone::Battlefield {
                    obj.controller = new_controller;
                }
            }
            // Insert into destination zone.
            match to {
                Zone::Battlefield => {
                    let mut perm = PermanentState::new(&def);
                    perm.controller_since_turn = state.turn_number;
                    state.battlefield.insert(id, perm);
                    // CR 603.2: collect ETB triggers and push them onto the stack immediately.
                    let etb_triggers = crate::engine::triggered::collect_triggers_for_event(
                        &mut state,
                        &crate::types::GameEvent::EntersTheBattlefield { subject_id: id },
                    );
                    for t in etb_triggers {
                        let tid = t.id;
                        state.stack.push(tid);
                        state.stack_objects.insert(tid, t);
                    }
                }
                Zone::Graveyard => {
                    if let Some(gy) = state.graveyards.get_mut(&owner) {
                        gy.push(id);
                    }
                }
                Zone::Exile => {
                    state.exile.push(id);
                }
                Zone::Hand => {
                    if let Some(hand) = state.hands.get_mut(&new_controller) {
                        hand.push(id);
                    }
                }
                Zone::Library => {
                    if let Some(lib) = state.libraries.get_mut(&owner) {
                        lib.push(id);
                    }
                }
                Zone::Stack | Zone::Command => {}
            }
        }
    }
}
```

Run: `cargo test engine::stack::tests::move_zone_graveyard_to_battlefield_transitions_object engine::stack::tests::move_zone_noop_when_object_not_in_from_zone engine::stack::tests::move_zone_to_battlefield_fires_etb_triggers 2>&1 | grep -E "^test result|FAILED|error\["`
Expected: all three PASS.

- [ ] **Step 3: Run all tests, confirm no regressions**

Run: `cargo test 2>&1 | grep -E "^test result|FAILED|error\["`
Expected: all tests pass.

- [ ] **Step 4: Commit**

```bash
git add src/engine/stack.rs
git commit -m "feat: implement MoveZone zone-change effect step in execute_effect_steps"
```

---

### Task 4: End-to-end tests, `docs/todo.md`, and clippy

**Files:**
- Modify: `src/engine/state_based_actions.rs` (add four end-to-end tests)
- Modify: `docs/todo.md`

**Interfaces:**
- Consumes: everything from Tasks 1–3
- Produces: verified end-to-end behaviour for Persist and Undying; cleaned-up `docs/todo.md`

---

- [ ] **Step 1: Write four end-to-end tests**

These tests drive the full pipeline: SBA detects lethal damage → creature dies + trigger is returned → trigger is pushed to the stack → `resolve_top` resolves it → creature back on battlefield with correct counter.

**Important:** Use a **2/2** creature for Persist (so the returned 2/2 with one -1/-1 counter is 1/1, which survives SBAs). Use a **2/1** creature for Undying (returned 2/1 with +1/+1 is 3/2, also survives).

Add to the existing `#[cfg(test)] mod tests` in `src/engine/state_based_actions.rs`:

```rust
#[test]
fn persist_creature_returns_with_minus_counter_after_dying() {
    // CR 702.79: a 2/2 Persist creature (no -1/-1 counters) dies and returns
    // to the battlefield under its owner's control with a -1/-1 counter.
    use crate::types::{CardDefinition, CardType, CounterKind, RulesText, TypeLine, Zone};
    use crate::types::ability::{Rule, StaticAbility};
    use crate::types::mana::ManaCost;

    let mut state = make_state();

    let def = CardDefinition {
        name: "Young Wolf".into(),
        mana_cost: Some(ManaCost { pips: vec![] }),
        type_line: TypeLine {
            supertypes: vec![],
            card_types: vec![CardType::Creature],
            subtypes: vec!["Wolf".into()],
        },
        oracle_text: "Persist".into(),
        rules_text: vec![RulesText::Active(Rule::Static(StaticAbility::Persist))],
        text_annotations: vec![],
        power: Some(2),
        toughness: Some(2),
        colors: vec![],
    };
    let id = add_creature_to_battlefield(&mut state, PlayerId(0), def);
    state.battlefield.get_mut(&id).unwrap().damage_marked = 5;

    // SBA: dies, Persist trigger returned.
    let (mut state, triggers) = check_and_apply_sbas(state);
    assert!(!state.battlefield.contains_key(&id), "creature should have died");
    assert!(state.graveyards[&PlayerId(0)].contains(&id));
    assert_eq!(triggers.len(), 1, "exactly one Persist trigger");

    // Push trigger onto the stack.
    for t in triggers {
        let tid = t.id;
        state.stack.push(tid);
        state.stack_objects.insert(tid, t);
    }

    // Resolve the trigger: MoveZone (gy→bf) + AddCounter (-1/-1).
    let state = crate::engine::stack::resolve_top(state);

    assert!(state.battlefield.contains_key(&id), "creature should be back on battlefield");
    assert_eq!(state.objects[&id].zone, Zone::Battlefield);
    assert!(!state.graveyards[&PlayerId(0)].contains(&id));
    assert_eq!(
        state.battlefield[&id].counter_count(&CounterKind::PtModifier { power: -1, toughness: -1 }),
        1,
        "Persist creature should have exactly one -1/-1 counter"
    );
    assert_eq!(state.objects[&id].controller, PlayerId(0));
}

#[test]
fn persist_does_not_trigger_when_minus_counter_present() {
    // CR 702.79: a Persist creature that already has a -1/-1 counter dies permanently.
    use crate::types::{CardDefinition, CardType, CounterKind, RulesText, TypeLine};
    use crate::types::ability::{Rule, StaticAbility};
    use crate::types::mana::ManaCost;

    let mut state = make_state();

    let def = CardDefinition {
        name: "Young Wolf".into(),
        mana_cost: Some(ManaCost { pips: vec![] }),
        type_line: TypeLine {
            supertypes: vec![],
            card_types: vec![CardType::Creature],
            subtypes: vec![],
        },
        oracle_text: "Persist".into(),
        rules_text: vec![RulesText::Active(Rule::Static(StaticAbility::Persist))],
        text_annotations: vec![],
        power: Some(2),
        toughness: Some(2),
        colors: vec![],
    };
    let id = add_creature_to_battlefield(&mut state, PlayerId(0), def);
    state.battlefield.get_mut(&id).unwrap()
        .add_counters(CounterKind::PtModifier { power: -1, toughness: -1 }, 1);
    state.battlefield.get_mut(&id).unwrap().damage_marked = 5;

    let (state, triggers) = check_and_apply_sbas(state);

    assert!(!state.battlefield.contains_key(&id));
    assert!(state.graveyards[&PlayerId(0)].contains(&id));
    assert!(triggers.is_empty(), "Persist must not trigger when -1/-1 counter present");
}

#[test]
fn undying_creature_returns_with_plus_counter_after_dying() {
    // CR 702.93: a 2/1 Undying creature (no +1/+1 counters) dies and returns
    // under its owner's control with a +1/+1 counter (becomes 3/2).
    use crate::types::{CardDefinition, CardType, CounterKind, RulesText, TypeLine, Zone};
    use crate::types::ability::{Rule, StaticAbility};
    use crate::types::mana::ManaCost;

    let mut state = make_state();

    let def = CardDefinition {
        name: "Strangleroot Geist".into(),
        mana_cost: Some(ManaCost { pips: vec![] }),
        type_line: TypeLine {
            supertypes: vec![],
            card_types: vec![CardType::Creature],
            subtypes: vec!["Spirit".into()],
        },
        oracle_text: "Undying".into(),
        rules_text: vec![RulesText::Active(Rule::Static(StaticAbility::Undying))],
        text_annotations: vec![],
        power: Some(2),
        toughness: Some(1),
        colors: vec![],
    };
    let id = add_creature_to_battlefield(&mut state, PlayerId(0), def);
    state.battlefield.get_mut(&id).unwrap().damage_marked = 5;

    let (mut state, triggers) = check_and_apply_sbas(state);
    assert!(!state.battlefield.contains_key(&id));
    assert!(state.graveyards[&PlayerId(0)].contains(&id));
    assert_eq!(triggers.len(), 1, "exactly one Undying trigger");

    for t in triggers {
        let tid = t.id;
        state.stack.push(tid);
        state.stack_objects.insert(tid, t);
    }

    let state = crate::engine::stack::resolve_top(state);

    assert!(state.battlefield.contains_key(&id), "creature should be back on battlefield");
    assert_eq!(state.objects[&id].zone, Zone::Battlefield);
    assert!(!state.graveyards[&PlayerId(0)].contains(&id));
    assert_eq!(
        state.battlefield[&id].counter_count(&CounterKind::PtModifier { power: 1, toughness: 1 }),
        1,
        "Undying creature should have exactly one +1/+1 counter"
    );
    assert_eq!(state.objects[&id].controller, PlayerId(0));
}

#[test]
fn undying_does_not_trigger_when_plus_counter_present() {
    // CR 702.93: an Undying creature that already has a +1/+1 counter dies permanently.
    use crate::types::{CardDefinition, CardType, CounterKind, RulesText, TypeLine};
    use crate::types::ability::{Rule, StaticAbility};
    use crate::types::mana::ManaCost;

    let mut state = make_state();

    let def = CardDefinition {
        name: "Strangleroot Geist".into(),
        mana_cost: Some(ManaCost { pips: vec![] }),
        type_line: TypeLine {
            supertypes: vec![],
            card_types: vec![CardType::Creature],
            subtypes: vec![],
        },
        oracle_text: "Undying".into(),
        rules_text: vec![RulesText::Active(Rule::Static(StaticAbility::Undying))],
        text_annotations: vec![],
        power: Some(2),
        toughness: Some(1),
        colors: vec![],
    };
    let id = add_creature_to_battlefield(&mut state, PlayerId(0), def);
    state.battlefield.get_mut(&id).unwrap()
        .add_counters(CounterKind::PtModifier { power: 1, toughness: 1 }, 1);
    state.battlefield.get_mut(&id).unwrap().damage_marked = 5;

    let (state, triggers) = check_and_apply_sbas(state);

    assert!(!state.battlefield.contains_key(&id));
    assert!(state.graveyards[&PlayerId(0)].contains(&id));
    assert!(triggers.is_empty(), "Undying must not trigger when +1/+1 counter present");
}
```

Run: `cargo test engine::state_based_actions::tests::persist_creature_returns_with_minus_counter_after_dying 2>&1 | grep -E "^test result|FAILED|error\["`
Expected: PASS (everything is already implemented from Tasks 1–3). If it fails, the pipeline has a bug — investigate before proceeding.

- [ ] **Step 2: Run all four end-to-end tests**

Run: `cargo test 2>&1 | grep -E "^test result|FAILED|error\["`
Expected: all tests pass, zero failures.

- [ ] **Step 3: Run clippy and fix any warnings**

```bash
cargo clippy --fix --all-targets 2>&1 | grep -E "error|warning" | head -40
```

Fix any remaining warnings by hand, then confirm clean:

```bash
cargo clippy --all-targets 2>&1 | grep -E "error|warning"
```

Expected: no output (fully clean).

- [ ] **Step 4: Update `docs/todo.md`**

**Remove** the following bullet from the "Counter system block" section (under the "Still blocked on graveyard zone-change hook:" subtext):

```
- **Persist** (702.79): return from graveyard with -1/-1 counter if no -1/-1 counter.
  (Also needs graveyard zone-change hook — see next section.)
- **Undying** (702.93): return from graveyard with +1/+1 counter if no +1/+1 counter.
  (Also needs graveyard zone-change hook.)
```

**Remove** the following two bullets from the "Graveyard / zone-change block" section:

```
- **Persist** (702.79) — see Counter block above.
- **Undying** (702.93) — see Counter block above.
```

If the "Counter system block" or "Graveyard / zone-change block" sections become empty (or only contain the section header), collapse them or remove them as appropriate.

**Add** a new section after the existing "Alternative casting block":

```markdown
---

## 🔌 Activated abilities from non-battlefield zones

Currently, cycling is implemented as a special case in `engine/cycling.rs`. A general
framework is needed for abilities that activate from zones other than the battlefield.

- **General framework**: `engine/activated.rs` handles only battlefield activations;
  extend to support other source zones.
- **Graveyard activations**:
  - **Scavenge [cost]** (702.97): exile from graveyard, put +1/+1 counters on a creature.
  - **Unearth [cost]** (702.84): return temporarily; exile at EOT or if it would leave.
  - **Escape [cost]** (702.138): cast from graveyard by exiling N other cards.
  - **Flashback [cost]** (702.34): cast from graveyard for the Flashback cost.
  - **Dredge N** (702.52): replace a draw with "mill N, return this card".
  - **Delve** (702.66): exile cards to pay generic mana when casting.
- **Hand activations**:
  - **Foretell [cost]** (702.143): exile face-down during your turn; cast later for reduced cost.
- **Exile activations**:
  - **Cascade** (702.85): exile cards off top until a cheaper one is found, cast it free.
  - **Suspend N—[cost]** (702.62): exile with N time counters; cast when last counter removed.

---
```

- [ ] **Step 5: Final test + commit**

Run: `cargo test 2>&1 | grep -E "^test result|FAILED|error\["`
Expected: all tests pass.

```bash
git add src/engine/state_based_actions.rs docs/todo.md
git commit -m "feat: end-to-end Persist/Undying tests; update todo.md with activated-from-other-zones block"
```
