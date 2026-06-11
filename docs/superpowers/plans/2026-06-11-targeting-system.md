# Targeting System Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Wire a full targeting pipeline — target declaration at cast/activation time, Shroud/Hexproof enforcement, CR 608.2b fizzle at resolution, and Giant Growth / Lightning Bolt as the two driving test cases.

**Architecture:** EffectTarget moves from an opaque enum to serde struct-variants so it can round-trip through the API; EffectStep::BoostPermanentPT loses its embedded target_id (target lives in StackObject.targets instead); a new engine/targeting.rs module provides is_legal_target / legal_targets / targets_still_legal; cast_spell and activate_ability grow a declared_targets parameter with validation; serve.rs exposes valid_targets in the card view.

**Tech Stack:** Rust, Axum, serde_json; no new crates.

---

## File map

| File | Change |
|------|--------|
| `src/types/effect.rs` | `EffectTarget` struct variants + Serde; `BoostPermanentPT(PTDelta)` tuple; `DealDamage(u32)` |
| `src/types/ids.rs` | Add `Deserialize` to `ObjectId` and `PlayerId` |
| `src/types/ability.rs` | Add `TargetFilter`, `SpellAbility`; `Ability::SpellEffect(SpellAbility)`; `Shroud`/`Hexproof` in `StaticAbility`; `target_requirements` on `ActivatedAbility` |
| `src/types/stack.rs` | `StackObject.targets: Vec<EffectTarget>` |
| `src/engine/mod.rs` | `pub mod targeting`; `WrongNumberOfTargets`, `IllegalTarget` in `EngineError` |
| `src/engine/targeting.rs` | **New.** `is_legal_target`, `legal_targets`, `targets_still_legal` |
| `src/engine/stack.rs` | `execute_effect_steps` gains `targets` param; fizzle check; new arms |
| `src/engine/triggered.rs` | Remove `target_id` from all `BoostPermanentPT` constructions; move to `StackObject.targets` |
| `src/engine/casting.rs` | `cast_spell` gains `declared_targets`; validation |
| `src/engine/activated.rs` | `activate_ability` gains `declared_targets`; validation for non-mana abilities |
| `src/engine/cycling.rs` | Add `targets: vec![]` to the cycling `StackObject` construction |
| `src/parser/oracle.rs` | `parse_instant_or_sorcery` gains `card_name`; `parse_spell_paragraph`; new patterns; Shroud/Hexproof |
| `src/cards/scryfall.rs` | Thread `name` into `parse_instant_or_sorcery` |
| `src/serve.rs` | `TargetView`; `valid_targets` on `CardView` + `ActivatedAbilityView`; `CastSpell.targets`; `ActivateAbility.targets` |
| `tests/fixtures/oracle_cards_test.json` | Add Giant Growth and Lightning Bolt entries |

---

## Task 1: EffectTarget serde + ObjectId/PlayerId Deserialize + StackObject.targets

**Files:**
- Modify: `src/types/effect.rs`
- Modify: `src/types/ids.rs`
- Modify: `src/types/stack.rs`
- Modify: `src/engine/triggered.rs` (add `targets: vec![]`)
- Modify: `src/engine/casting.rs` (add `targets: vec![]`)
- Modify: `src/engine/activated.rs` (add `targets: vec![]`)
- Modify: `src/engine/cycling.rs` (add `targets: vec![]`)
- Modify: `src/engine/stack.rs` (add `targets: vec![]` to test helpers)

- [ ] **Step 1: Write the failing test for EffectTarget serde**

Add to `src/types/effect.rs` in the `#[cfg(test)]` block:

```rust
#[test]
fn effect_target_object_serializes_and_deserializes() {
    let t = EffectTarget::Object { id: ObjectId(42) };
    let json = serde_json::to_string(&t).unwrap();
    assert_eq!(json, r#"{"kind":"object","id":42}"#);
    let round_trip: EffectTarget = serde_json::from_str(&json).unwrap();
    assert_eq!(round_trip, t);
}

#[test]
fn effect_target_player_serializes_and_deserializes() {
    let t = EffectTarget::Player { id: PlayerId(1) };
    let json = serde_json::to_string(&t).unwrap();
    assert_eq!(json, r#"{"kind":"player","id":1}"#);
    let round_trip: EffectTarget = serde_json::from_str(&json).unwrap();
    assert_eq!(round_trip, t);
}
```

- [ ] **Step 2: Run tests to confirm failure**

```bash
cargo test 2>&1 | grep -E "^test result|FAILED|error\["
```

Expected: compile error — `EffectTarget` has no struct variants, no Serialize/Deserialize.

- [ ] **Step 3: Rework EffectTarget in `src/types/effect.rs`**

Replace the current `EffectTarget` enum:

```rust
// Before:
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EffectTarget {
    Player(PlayerId),
    Object(ObjectId),
}
```

With:

```rust
/// A declared target on the stack (CR 115.1).
/// Struct variants for clean Serde round-tripping via the API.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum EffectTarget {
    Player { id: PlayerId },
    Object { id: ObjectId },
}
```

Also add the necessary imports to `effect.rs`:
- The file already imports `ObjectId` and `PlayerId` from `super::ids`.

- [ ] **Step 4: Add `Deserialize` to `ObjectId` and `PlayerId` in `src/types/ids.rs`**

Change the derive attributes:

```rust
// ObjectId — was:
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize)]
// becomes:
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]

// PlayerId — was:
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize)]
// becomes:
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
```

Also add the import at the top of `src/types/ids.rs`:
```rust
use serde::{Deserialize, Serialize};
```
(Currently it only imports `Serialize`.)

- [ ] **Step 5: Add `targets` to `StackObject` in `src/types/stack.rs`**

Change:

```rust
#[derive(Debug, Clone)]
pub struct StackObject {
    pub id: StackId,
    pub payload: StackPayload,
    pub controller: PlayerId,
}
```

To:

```rust
#[derive(Debug, Clone)]
pub struct StackObject {
    pub id: StackId,
    pub payload: StackPayload,
    pub controller: PlayerId,
    pub targets: Vec<super::effect::EffectTarget>,  // declared targets (CR 115.1)
}
```

- [ ] **Step 6: Fix all StackObject constructions (add `targets: vec![]`)**

**`src/engine/triggered.rs`** — 6 places. Add `targets: vec![]` to each:

In `collect_etb_triggers`:
```rust
StackObject {
    id,
    payload: StackPayload::TriggeredAbility { source_id: entering_id, effect, label },
    controller,
    targets: vec![],  // ← add
}
```

In `collect_cast_triggers` (Prowess):
```rust
StackObject {
    id: sid,
    payload: StackPayload::TriggeredAbility { source_id: creature_id, effect: vec![...], label: "Prowess".into() },
    controller,
    targets: vec![],  // ← add
}
```

In `collect_block_triggers` — 3 places (Flanking, Bushido-attacker, Bushido-blocker):
```rust
StackObject {
    id: sid,
    payload: StackPayload::TriggeredAbility { source_id: ..., effect: vec![...], label: "Flanking".into() },
    controller: attacking_player,
    targets: vec![],  // ← add
}
// same pattern for Bushido attacker and Bushido blocker
```

In `collect_attack_triggers` — 2 places (Exalted, Melee):
```rust
StackObject {
    id: sid,
    payload: StackPayload::TriggeredAbility { source_id, effect: vec![...], label: "Exalted".into() },
    controller: attacking_player,
    targets: vec![],  // ← add
}
// same for Melee
```

**`src/engine/casting.rs`**:
```rust
let stack_obj = crate::types::StackObject {
    id: stack_id,
    payload: crate::types::StackPayload::Spell { card_id: object_id },
    controller: player_id,
    targets: vec![],  // ← add
};
```

And in `cast_instant_succeeds_with_nonempty_stack` test (and other tests that construct StackObject inline):
```rust
StackObject {
    id: sid,
    payload: StackPayload::TriggeredAbility { source_id: ObjectId(99), effect: vec![], label: "dummy".into() },
    controller: PlayerId(0),
    targets: vec![],  // ← add
}
```

Same for `cast_flash_creature_with_nonempty_stack_succeeds`, `cast_creature_without_flash_fails_with_nonempty_stack`, `play_land_fails_with_nonempty_stack` tests in casting.rs.

**`src/engine/activated.rs`**:
```rust
let stack_obj = crate::types::StackObject {
    id: stack_id,
    payload: crate::types::StackPayload::ActivatedAbility { source_id: object_id, effect: ability.effect.clone(), label },
    controller: activating_player,
    targets: vec![],  // ← add
};
```

**`src/engine/cycling.rs`**:
```rust
let stack_obj = StackObject {
    id: stack_id,
    payload: StackPayload::ActivatedAbility { source_id: card_id, effect: vec![EffectStep::DrawCard(1)], label: "Cycling".into() },
    controller: player_id,
    targets: vec![],  // ← add
};
```

**`src/engine/stack.rs`** — test helper functions `push_spell` and `push_draw_trigger`, plus inline constructions in tests like `resolve_top_triggered_ability_gains_life`, `resolve_top_triggered_ability_mills`, `boost_permanent_pt_effect_applies_delta`, `boost_permanent_pt_noop_if_not_on_battlefield`:
```rust
let obj = StackObject {
    id: stack_id,
    payload: StackPayload::...,
    controller,
    targets: vec![],  // ← add
};
```

- [ ] **Step 7: Run tests**

```bash
cargo test 2>&1 | grep -E "^test result|FAILED|error\["
```

Expected: all tests pass.

- [ ] **Step 8: Commit**

```bash
git add src/types/effect.rs src/types/ids.rs src/types/stack.rs \
        src/engine/triggered.rs src/engine/casting.rs \
        src/engine/activated.rs src/engine/cycling.rs src/engine/stack.rs
git commit -m "$(cat <<'EOF'
feat: add EffectTarget serde, StackObject.targets, ObjectId/PlayerId Deserialize

EffectTarget gains struct variants and Serialize+Deserialize for API round-tripping.
ObjectId and PlayerId gain Deserialize. StackObject grows a targets field (empty for
all existing constructions) in preparation for the targeting system.

Co-Authored-By: Claude Sonnet 4.6 <noreply@anthropic.com>
EOF
)"
```

---

## Task 2: EffectStep changes + resolution (BoostPermanentPT tuple, DealDamage, targets-based execution)

**Files:**
- Modify: `src/types/effect.rs`
- Modify: `src/engine/stack.rs`
- Modify: `src/engine/triggered.rs`
- Modify: `src/serve.rs`

This task removes `target_id` from `BoostPermanentPT`, adds `DealDamage(u32)`, and changes `execute_effect_steps` to receive `targets` from the stack object. All triggered ability constructions that embed `target_id` in the effect step move the id into `stack_obj.targets` instead.

- [ ] **Step 1: Write failing tests in `src/engine/stack.rs`**

Add these tests to the `#[cfg(test)]` block:

```rust
#[test]
fn deal_damage_to_creature_marks_damage() {
    let mut gs = make_state();
    let def = CardDefinition {
        name: "Target Creature".into(),
        mana_cost: None,
        type_line: TypeLine {
            supertypes: vec![],
            card_types: vec![CardType::Creature],
            subtypes: vec![],
        },
        oracle_text: String::new(),
        abilities: vec![],
        power: Some(2),
        toughness: Some(3),
    };
    let id = gs.alloc_id();
    let obj = CardObject::new(id, def, PlayerId(1), Zone::Battlefield);
    gs.battlefield.insert(id, PermanentState::new(&obj.definition));
    gs.add_object(obj);

    let stack_id = gs.alloc_stack_id();
    use crate::types::effect::EffectTarget;
    let stack_obj = StackObject {
        id: stack_id,
        payload: StackPayload::TriggeredAbility {
            source_id: ObjectId(99),
            effect: vec![EffectStep::DealDamage(3)],
            label: "test damage".into(),
        },
        controller: PlayerId(0),
        targets: vec![EffectTarget::Object { id }],
    };
    gs.stack.push(stack_id);
    gs.stack_objects.insert(stack_id, stack_obj);

    let gs = resolve_top(gs);

    assert_eq!(gs.battlefield[&id].damage_marked, 3);
    assert!(gs.stack.is_empty());
}

#[test]
fn deal_damage_to_player_reduces_life() {
    let mut gs = make_state();
    let before_life = gs.get_player(PlayerId(1)).unwrap().life;

    let stack_id = gs.alloc_stack_id();
    use crate::types::effect::EffectTarget;
    let stack_obj = StackObject {
        id: stack_id,
        payload: StackPayload::TriggeredAbility {
            source_id: ObjectId(99),
            effect: vec![EffectStep::DealDamage(3)],
            label: "test damage".into(),
        },
        controller: PlayerId(0),
        targets: vec![EffectTarget::Player { id: PlayerId(1) }],
    };
    gs.stack.push(stack_id);
    gs.stack_objects.insert(stack_id, stack_obj);

    let gs = resolve_top(gs);

    assert_eq!(gs.get_player(PlayerId(1)).unwrap().life, before_life - 3);
    assert!(gs.stack.is_empty());
}
```

- [ ] **Step 2: Run tests to confirm failure**

```bash
cargo test 2>&1 | grep -E "^test result|FAILED|error\["
```

Expected: compile error — `DealDamage` doesn't exist.

- [ ] **Step 3: Change `BoostPermanentPT` and add `DealDamage` in `src/types/effect.rs`**

```rust
// Before:
BoostPermanentPT { target_id: ObjectId, delta: PTDelta },

// After:
BoostPermanentPT(PTDelta),
DealDamage(u32),
```

Full updated enum:
```rust
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EffectStep {
    AddMana(ManaPool),
    Mill(u32),
    DrawCard(u32),
    GainLife(u32),
    BoostPermanentPT(PTDelta),
    DealDamage(u32),
    Unimplemented(String),
}
```

- [ ] **Step 4: Change `execute_effect_steps` in `src/engine/stack.rs`**

The function signature gains a `targets` parameter and both affected arms are updated:

```rust
// CR 608.2b: execute each effect step for the given controller.
fn execute_effect_steps(
    mut state: GameState,
    controller: PlayerId,
    steps: &[EffectStep],
    targets: &[crate::types::effect::EffectTarget],
) -> GameState {
    use crate::types::effect::EffectTarget;
    for step in steps {
        match step {
            EffectStep::DrawCard(n) => {
                for _ in 0..*n {
                    state = draw_card(state, controller);
                }
            }
            EffectStep::GainLife(n) => {
                if let Some(player) = state.get_player_mut(controller) {
                    player.life += *n as i32;
                }
            }
            EffectStep::Mill(n) => {
                let to_mill =
                    (*n as usize).min(state.libraries.get(&controller).map_or(0, |l| l.len()));
                for _ in 0..to_mill {
                    if let Some(card_id) = state
                        .libraries
                        .get_mut(&controller)
                        .filter(|l| !l.is_empty())
                        .map(|l| l.remove(0))
                    {
                        if let Some(gy) = state.graveyards.get_mut(&controller) {
                            gy.push(card_id);
                        }
                        if let Some(obj) = state.objects.get_mut(&card_id) {
                            obj.zone = Zone::Graveyard;
                        }
                    }
                }
            }
            EffectStep::AddMana(_) => {
                unreachable!("AddMana in stack object");
            }
            EffectStep::BoostPermanentPT(delta) => {
                if let Some(EffectTarget::Object { id }) = targets.first() {
                    if let Some(perm) = state.battlefield.get_mut(id) {
                        perm.pt_boost_until_eot.power += delta.power;
                        perm.pt_boost_until_eot.toughness += delta.toughness;
                    }
                }
            }
            EffectStep::DealDamage(n) => {
                match targets.first() {
                    Some(EffectTarget::Object { id }) => {
                        if let Some(perm) = state.battlefield.get_mut(id) {
                            perm.damage_marked += n;
                        }
                    }
                    Some(EffectTarget::Player { id }) => {
                        if let Some(player) = state.get_player_mut(*id) {
                            player.life -= *n as i32;
                        }
                    }
                    None => {}
                }
            }
            EffectStep::Unimplemented(_) => {}
        }
    }
    state
}
```

- [ ] **Step 5: Update `resolve_top` to extract and pass targets**

In `resolve_top`, after removing `stack_obj` from `stack_objects`, extract `targets` before matching the payload:

```rust
pub fn resolve_top(mut state: GameState) -> GameState {
    let stack_id = match state.stack.last().copied() {
        Some(id) => id,
        None => return state,
    };
    state.stack.pop();
    let stack_obj = match state.stack_objects.remove(&stack_id) {
        Some(obj) => obj,
        None => {
            unreachable!("stack id {stack_id:?} missing from stack_objects; invariant violated")
        }
    };
    let targets = stack_obj.targets.clone();  // ← extract here

    match stack_obj.payload {
        StackPayload::Spell { card_id } => {
            let controller = stack_obj.controller;
            // ... (existing permanent / non-permanent logic) ...

            // Change the execute_effect_steps call:
            state = execute_effect_steps(state, controller, &steps, &targets);  // ← pass targets

            // ... rest unchanged ...
        }
        StackPayload::TriggeredAbility { effect, .. }
        | StackPayload::ActivatedAbility { effect, .. } => {
            let controller = stack_obj.controller;
            state = execute_effect_steps(state, controller, &effect, &targets);  // ← pass targets
            state.consecutive_passes = 0;
            state.priority_player = state.active_player;
            check_and_apply_sbas(state)
        }
    }
}
```

- [ ] **Step 6: Update triggered.rs — move target_id into targets, remove from BoostPermanentPT**

For every `BoostPermanentPT { target_id: X, delta: D }` construction in triggered.rs, change to `BoostPermanentPT(D)` and set `targets: vec![EffectTarget::Object { id: X }]`.

In `collect_cast_triggers` (Prowess):
```rust
use crate::types::effect::EffectTarget;
StackObject {
    id: sid,
    payload: StackPayload::TriggeredAbility {
        source_id: creature_id,
        effect: vec![EffectStep::BoostPermanentPT(PTDelta { power: 1, toughness: 1 })],
        label: "Prowess".into(),
    },
    controller,
    targets: vec![EffectTarget::Object { id: creature_id }],
}
```

In `collect_block_triggers` — Flanking (target is blocker_id):
```rust
use crate::types::effect::EffectTarget;
StackObject {
    id: sid,
    payload: StackPayload::TriggeredAbility {
        source_id: *attacker_id,
        effect: vec![EffectStep::BoostPermanentPT(PTDelta { power: -1, toughness: -1 })],
        label: "Flanking".into(),
    },
    controller: attacking_player,
    targets: vec![EffectTarget::Object { id: blocker_id }],
}
```

Bushido on attacker (target is *attacker_id):
```rust
StackObject {
    id: sid,
    payload: StackPayload::TriggeredAbility {
        source_id: *attacker_id,
        effect: vec![EffectStep::BoostPermanentPT(PTDelta { power: n as i32, toughness: n as i32 })],
        label: format!("Bushido {n}"),
    },
    controller: attacking_player,
    targets: vec![EffectTarget::Object { id: *attacker_id }],
}
```

Bushido on blocker (target is blocker_id):
```rust
StackObject {
    id: sid,
    payload: StackPayload::TriggeredAbility {
        source_id: blocker_id,
        effect: vec![EffectStep::BoostPermanentPT(PTDelta { power: n as i32, toughness: n as i32 })],
        label: format!("Bushido {n}"),
    },
    controller: defending_player,
    targets: vec![EffectTarget::Object { id: blocker_id }],
}
```

In `collect_attack_triggers` — Exalted (target is attacker_id):
```rust
StackObject {
    id: sid,
    payload: StackPayload::TriggeredAbility {
        source_id,
        effect: vec![EffectStep::BoostPermanentPT(PTDelta { power: 1, toughness: 1 })],
        label: "Exalted".into(),
    },
    controller: attacking_player,
    targets: vec![EffectTarget::Object { id: attacker_id }],
}
```

Melee (target is attacker_id):
```rust
StackObject {
    id: sid,
    payload: StackPayload::TriggeredAbility {
        source_id: attacker_id,
        effect: vec![EffectStep::BoostPermanentPT(PTDelta { power: 1, toughness: 1 })],
        label: "Melee".into(),
    },
    controller: attacking_player,
    targets: vec![EffectTarget::Object { id: attacker_id }],
}
```

- [ ] **Step 7: Update `src/serve.rs` format functions**

In `format_activated_ability`, `format_spell_effect`, and `format_triggered_ability`, change the `BoostPermanentPT` match arms:

```rust
// In format_activated_ability:
EffectStep::BoostPermanentPT(delta) => {
    format!("Boost by {}/{}", delta.power, delta.toughness)
}

// In format_spell_effect:
EffectStep::BoostPermanentPT(delta) => {
    format!("Boost by {}/{}", delta.power, delta.toughness)
}

// In format_triggered_ability:
EffectStep::BoostPermanentPT(delta) => {
    format!("boost by {}/{}", delta.power, delta.toughness)
}
```

Also add `DealDamage` arms to all three:

```rust
// format_activated_ability:
EffectStep::DealDamage(n) => format!("Deal {n} damage"),

// format_spell_effect:
EffectStep::DealDamage(n) => format!("Deal {n} damage"),

// format_triggered_ability:
EffectStep::DealDamage(n) => format!("deal {n} damage"),
```

- [ ] **Step 8: Update tests in triggered.rs and stack.rs**

In `triggered.rs`, all tests that assert on `BoostPermanentPT { target_id, delta }` need to change. Examples:

`collect_cast_triggers_prowess_fires_on_noncreature`:
```rust
// Before:
assert_eq!(
    *effect,
    vec![EffectStep::BoostPermanentPT {
        target_id: creature_id,
        delta: PTDelta { power: 1, toughness: 1 },
    }]
);

// After:
use crate::types::effect::EffectTarget;
assert_eq!(
    *effect,
    vec![EffectStep::BoostPermanentPT(PTDelta { power: 1, toughness: 1 })]
);
assert_eq!(triggers[0].targets, vec![EffectTarget::Object { id: creature_id }]);
```

Apply the same pattern to `collect_attack_triggers_exalted_single_attacker` (replace `BoostPermanentPT { target_id: attacker_id, delta }` with tuple + check `triggers[0].targets`), `collect_attack_triggers_melee_in_two_player_gives_one_boost`, `collect_block_triggers_flanking_gives_minus_one_to_non_flanking_blocker`, and `collect_block_triggers_bushido_boosts_attacker_and_blocker`.

In `stack.rs`, update `boost_permanent_pt_effect_applies_delta`:
```rust
// Change the stack_obj construction — remove target_id from effect, add to targets:
use crate::types::effect::EffectTarget;
let stack_obj = StackObject {
    id: stack_id,
    payload: StackPayload::TriggeredAbility {
        source_id: id,
        effect: vec![EffectStep::BoostPermanentPT(PTDelta { power: 1, toughness: 1 })],
        label: "test boost".into(),
    },
    controller: PlayerId(0),
    targets: vec![EffectTarget::Object { id }],
};
```

Update `boost_permanent_pt_noop_if_not_on_battlefield`:
```rust
use crate::types::effect::EffectTarget;
let stack_obj = StackObject {
    id: stack_id,
    payload: StackPayload::TriggeredAbility {
        source_id: nonexistent_id,
        effect: vec![EffectStep::BoostPermanentPT(PTDelta { power: 5, toughness: 5 })],
        label: "noop boost".into(),
    },
    controller: PlayerId(0),
    targets: vec![EffectTarget::Object { id: nonexistent_id }],
};
```

- [ ] **Step 9: Run tests**

```bash
cargo test 2>&1 | grep -E "^test result|FAILED|error\["
```

Expected: all tests pass.

- [ ] **Step 10: Commit**

```bash
git add src/types/effect.rs src/engine/stack.rs src/engine/triggered.rs src/serve.rs
git commit -m "$(cat <<'EOF'
feat: BoostPermanentPT tuple + DealDamage + targets-based execution

EffectStep::BoostPermanentPT loses embedded target_id; target lives in
StackObject.targets. DealDamage(u32) added. execute_effect_steps gains a
targets parameter and reads targets[0] for both new arms. All triggered
ability constructions (Prowess, Flanking, Bushido, Exalted, Melee) move
their target id from inside the step into the stack object's targets vec.

Co-Authored-By: Claude Sonnet 4.6 <noreply@anthropic.com>
EOF
)"
```

---

## Task 3: SpellAbility, TargetFilter, Shroud/Hexproof, ActivatedAbility.target_requirements

**Files:**
- Modify: `src/types/ability.rs`
- Modify: `src/parser/oracle.rs` (keyword changes only, not the new patterns)
- Modify: `src/engine/stack.rs` (update test helpers using `Ability::SpellEffect`)
- Modify: `src/engine/casting.rs` (update test helpers)
- Modify: `src/serve.rs` (update `Ability::SpellEffect` match arm)

- [ ] **Step 1: Write failing tests for Shroud/Hexproof parsing**

Add to the parser tests in `src/parser/oracle.rs`:

```rust
#[test]
fn parse_shroud_keyword() {
    use crate::types::ability::StaticAbility;
    assert_eq!(
        parse_permanent("Shroud", ""),
        vec![parsed(StaticAbility::Shroud)]
    );
}

#[test]
fn parse_hexproof_keyword() {
    use crate::types::ability::StaticAbility;
    assert_eq!(
        parse_permanent("Hexproof", ""),
        vec![parsed(StaticAbility::Hexproof)]
    );
}
```

- [ ] **Step 2: Run tests to confirm failure**

```bash
cargo test 2>&1 | grep -E "^test result|FAILED|error\["
```

Expected: compile errors or test failures — `Shroud`/`Hexproof` don't exist in `StaticAbility`.

- [ ] **Step 3: Add `TargetFilter`, `SpellAbility`, `Shroud`/`Hexproof` to `src/types/ability.rs`**

Add after the `CastFilter` block:

```rust
/// Describes what kind of permanent or player can be targeted (CR 115.4).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TargetFilter {
    Creature,
    Player,
    Any,  // CR 115.4: creature, player, planeswalker, battle
}

/// A spell ability — the text of an instant or sorcery that takes effect when it resolves.
/// Wraps effect steps and any targeting requirements.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SpellAbility {
    pub target_requirements: Vec<TargetFilter>,  // empty for untargeted spells
    pub steps: Effect,
}
```

Add `Shroud` and `Hexproof` to `StaticAbility`:

```rust
pub enum StaticAbility {
    Flying,
    Reach,
    Trample,
    FirstStrike,
    DoubleStrike,
    Vigilance,
    Haste,
    Lifelink,
    Deathtouch,
    Menace,
    Indestructible,
    Defender,
    Shadow,
    Horsemanship,
    Skulk,
    Decayed,
    Flash,
    Exalted,
    Flanking,
    BushidoN(u32),
    Melee,
    Prowess,
    Shroud,    // CR 702.18
    Hexproof,  // CR 702.11
}
```

Add to `display_name`:
```rust
Self::Shroud => "Shroud".to_string(),
Self::Hexproof => "Hexproof".to_string(),
```

Add `target_requirements` to `ActivatedAbility`:
```rust
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ActivatedAbility {
    pub cost: ActivationCost,
    pub target_requirements: Vec<TargetFilter>,
    pub effect: Effect,
}
```

Change `Ability::SpellEffect`:
```rust
pub enum Ability {
    Static(StaticAbility),
    Triggered(TriggeredAbility),
    Activated(ActivatedAbility),
    SpellEffect(SpellAbility),  // was: SpellEffect(Effect)
    Cycling(ManaCost),
}
```

Add the `SpellAbility` import to `effect.rs` (it uses `EffectStep`):
The import chain is: `ability.rs` imports `effect::Effect`, and `SpellAbility` contains `Vec<EffectStep>`, so add `use super::effect::EffectStep;` if not already present. Actually, `Effect = Vec<EffectStep>` is already imported. Change `steps: Vec<EffectStep>` to use the type alias or the concrete type — use `Effect` to be consistent:

```rust
pub struct SpellAbility {
    pub target_requirements: Vec<TargetFilter>,
    pub steps: Effect,  // Vec<EffectStep> via type alias
}
```

- [ ] **Step 4: Update `match_keyword` in `src/parser/oracle.rs`**

In the fully-implemented keywords section of `match_keyword`, add before the `_ => {}` fallthrough:

```rust
"shroud" => return OracleSpan::Parsed(Ability::Static(StaticAbility::Shroud)),
"hexproof" => return OracleSpan::Parsed(Ability::Static(StaticAbility::Hexproof)),
```

Remove `"hexproof"` and `"shroud"` from the `is_cr702_keyword` match (lines ~373 and ~377). They no longer need to be there since they're handled above.

Also update the existing test that checks `"Hexproof"` → `ParsedUnimplemented`:

```rust
// Find and change this test:
fn parse_hexproof_is_unimplemented() { ... }
// Delete it entirely — it's superseded by parse_hexproof_keyword above.
```

Look for the test around line 1030:
```rust
// Before (delete this test block):
#[test]
fn parse_hexproof_keyword() {
    assert_eq!(
        parse_permanent("Hexproof", ""),
        vec![unimplemented("Hexproof")]
    );
}
// (This test conflicts with our new parse_hexproof_keyword test; remove it.)
```

Wait — actually the old test was probably named differently. Check `oracle.rs` around line 1030 for a test asserting `unimplemented("Hexproof")`. Remove it.

- [ ] **Step 5: Update all `Ability::SpellEffect` usages**

**`src/engine/stack.rs`** — test helper `make_instant_obj` and `make_sorcery_obj` and any inline SpellEffect constructions:

```rust
// Before:
abilities: vec![OracleSpan::Parsed(Ability::SpellEffect(steps))],

// After:
use crate::types::ability::SpellAbility;
abilities: vec![OracleSpan::Parsed(Ability::SpellEffect(SpellAbility {
    target_requirements: vec![],
    steps,
}))],
```

The spell resolution code in `resolve_top` that extracts steps also needs updating:
```rust
// Before:
OracleSpan::Parsed(crate::types::Ability::SpellEffect(steps)) => Some(steps.clone()),

// After:
OracleSpan::Parsed(crate::types::Ability::SpellEffect(spell_ability)) => Some(spell_ability.steps.clone()),
```

**`src/engine/casting.rs`** — test helpers `make_instant_def` and `make_flash_creature_def`:
```rust
// Before:
abilities: vec![OracleSpan::Parsed(Ability::SpellEffect(vec![EffectStep::DrawCard(1)]))],

// After:
use crate::types::ability::SpellAbility;
abilities: vec![OracleSpan::Parsed(Ability::SpellEffect(SpellAbility {
    target_requirements: vec![],
    steps: vec![EffectStep::DrawCard(1)],
}))],
```

**`src/serve.rs`** — oracle_text match arm:
```rust
// Before:
OracleSpan::Parsed(Ability::SpellEffect(steps)) => OracleSpanView {
    kind: SpanKind::Parsed,
    text: format_spell_effect(steps),
    ignored_kind: None,
},

// After:
OracleSpan::Parsed(Ability::SpellEffect(spell_ability)) => OracleSpanView {
    kind: SpanKind::Parsed,
    text: format_spell_effect(&spell_ability.steps),
    ignored_kind: None,
},
```

- [ ] **Step 6: Update `parse_instant_or_sorcery` to produce `SpellAbility`**

In `src/parser/oracle.rs`, change the function body (not the signature — that's Task 5):

```rust
pub fn parse_instant_or_sorcery(text: &str) -> Vec<OracleSpan> {
    use crate::types::ability::{Ability, SpellAbility};
    let mut spans = Vec::new();
    for paragraph in text.split('\n') {
        let paragraph = paragraph.trim();
        if paragraph.is_empty() {
            continue;
        }
        let steps = parse_spell_effect(paragraph);
        spans.push(OracleSpan::Parsed(Ability::SpellEffect(SpellAbility {
            target_requirements: vec![],
            steps,
        })));
    }
    spans
}
```

- [ ] **Step 7: Update `ActivatedAbility` constructions in `src/parser/oracle.rs`**

In `parse_permanent`, wherever `ActivatedAbility { cost, effect }` is constructed (the colon-separated path), add `target_requirements: vec![]`:

```rust
ActivatedAbility {
    cost: parse_activation_cost(cost_str),
    target_requirements: vec![],
    effect: parse_ability_effect(effect_str)
        .unwrap_or_else(|| vec![EffectStep::Unimplemented(effect_str.to_string())]),
}
```

Also update the `activated_ability_construction` test in `ability.rs`:
```rust
let ability = ActivatedAbility {
    cost: vec![CostComponent::Tap],
    target_requirements: vec![],
    effect: vec![EffectStep::AddMana(ManaPool { green: 1, ..Default::default() })],
};
```

- [ ] **Step 8: Run tests**

```bash
cargo test 2>&1 | grep -E "^test result|FAILED|error\["
```

Expected: all tests pass. The `parse_hexproof_keyword` test now passes with `Parsed(Static(Hexproof))`.

- [ ] **Step 9: Commit**

```bash
git add src/types/ability.rs src/parser/oracle.rs \
        src/engine/stack.rs src/engine/casting.rs src/serve.rs
git commit -m "$(cat <<'EOF'
feat: SpellAbility, TargetFilter, Shroud/Hexproof, ActivatedAbility.target_requirements

Ability::SpellEffect now carries SpellAbility{target_requirements, steps} instead
of a bare Effect. TargetFilter enum added. StaticAbility gains Shroud (CR 702.18)
and Hexproof (CR 702.11), both now fully parsed by match_keyword. ActivatedAbility
gains target_requirements (empty for all current abilities).

Co-Authored-By: Claude Sonnet 4.6 <noreply@anthropic.com>
EOF
)"
```

---

## Task 4: EngineError additions + engine/targeting.rs

**Files:**
- Modify: `src/engine/mod.rs`
- Create: `src/engine/targeting.rs`

- [ ] **Step 1: Write failing tests**

Create `src/engine/targeting.rs` with test module first (the tests will fail because the functions don't exist):

```rust
use crate::types::ability::{StaticAbility, TargetFilter};
use crate::types::effect::EffectTarget;
use crate::types::{GameState, ObjectId, PlayerId};

// CR 115.4: a target is legal if it exists in the targeted zone, satisfies the filter,
// and is not protected from the source by Shroud (CR 702.18) or Hexproof (CR 702.11).
pub fn is_legal_target(
    state: &GameState,
    target: &EffectTarget,
    filter: TargetFilter,
    caster: PlayerId,
) -> bool {
    todo!()
}

/// Returns all legal targets for `filter` from `caster`'s point of view.
pub fn legal_targets(
    state: &GameState,
    filter: TargetFilter,
    caster: PlayerId,
) -> Vec<EffectTarget> {
    todo!()
}

/// CR 608.2b: a spell or ability with targets only resolves if at least one target is
/// still legal. Used at resolution to check whether to apply effects.
pub fn targets_still_legal(state: &GameState, targets: &[EffectTarget]) -> bool {
    todo!()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::ability::Ability;
    use crate::types::card::{CardDefinition, CardType, TypeLine};
    use crate::types::{CardObject, OracleSpan, PermanentState, Player, Zone};

    fn two_player_state() -> GameState {
        GameState::new(vec![
            Player::new(PlayerId(0), "Alice"),
            Player::new(PlayerId(1), "Bob"),
        ])
    }

    fn place_creature(
        state: &mut GameState,
        owner: PlayerId,
        abilities: Vec<OracleSpan>,
    ) -> ObjectId {
        let def = CardDefinition {
            name: "Test Creature".into(),
            mana_cost: None,
            type_line: TypeLine {
                supertypes: vec![],
                card_types: vec![CardType::Creature],
                subtypes: vec![],
            },
            oracle_text: String::new(),
            abilities,
            power: Some(2),
            toughness: Some(2),
        };
        let id = state.alloc_id();
        let obj = CardObject::new(id, def, owner, Zone::Battlefield);
        state.battlefield.insert(id, PermanentState::new(&obj.definition));
        state.add_object(obj);
        id
    }

    #[test]
    fn creature_on_battlefield_is_legal_target_for_creature_filter() {
        let mut gs = two_player_state();
        let id = place_creature(&mut gs, PlayerId(1), vec![]);
        let target = EffectTarget::Object { id };
        assert!(is_legal_target(&gs, &target, TargetFilter::Creature, PlayerId(0)));
    }

    #[test]
    fn nonexistent_object_is_not_legal_target() {
        let gs = two_player_state();
        let target = EffectTarget::Object { id: ObjectId(999) };
        assert!(!is_legal_target(&gs, &target, TargetFilter::Creature, PlayerId(0)));
    }

    #[test]
    fn object_not_on_battlefield_is_not_legal_target() {
        let mut gs = two_player_state();
        let def = CardDefinition {
            name: "Hand Card".into(),
            mana_cost: None,
            type_line: TypeLine {
                supertypes: vec![],
                card_types: vec![CardType::Creature],
                subtypes: vec![],
            },
            oracle_text: String::new(),
            abilities: vec![],
            power: Some(1),
            toughness: Some(1),
        };
        let id = gs.alloc_id();
        let obj = CardObject::new(id, def, PlayerId(1), Zone::Hand);
        gs.add_object(obj);
        let target = EffectTarget::Object { id };
        assert!(!is_legal_target(&gs, &target, TargetFilter::Creature, PlayerId(0)));
    }

    #[test]
    fn creature_with_shroud_is_not_legal_target_for_anyone() {
        let mut gs = two_player_state();
        let id = place_creature(
            &mut gs,
            PlayerId(1),
            vec![OracleSpan::Parsed(Ability::Static(StaticAbility::Shroud))],
        );
        let target = EffectTarget::Object { id };
        // Shroud stops targeting from anyone — even the controller
        assert!(!is_legal_target(&gs, &target, TargetFilter::Creature, PlayerId(0)));
        assert!(!is_legal_target(&gs, &target, TargetFilter::Creature, PlayerId(1)));
    }

    #[test]
    fn creature_with_hexproof_is_not_legal_target_for_opponent() {
        let mut gs = two_player_state();
        let id = place_creature(
            &mut gs,
            PlayerId(1),
            vec![OracleSpan::Parsed(Ability::Static(StaticAbility::Hexproof))],
        );
        let target = EffectTarget::Object { id };
        // Hexproof stops targeting from opponents
        assert!(!is_legal_target(&gs, &target, TargetFilter::Creature, PlayerId(0)));
    }

    #[test]
    fn creature_with_hexproof_is_legal_target_for_controller() {
        let mut gs = two_player_state();
        let id = place_creature(
            &mut gs,
            PlayerId(1),
            vec![OracleSpan::Parsed(Ability::Static(StaticAbility::Hexproof))],
        );
        let target = EffectTarget::Object { id };
        // Controller can still target their own Hexproof creature
        assert!(is_legal_target(&gs, &target, TargetFilter::Creature, PlayerId(1)));
    }

    #[test]
    fn active_player_is_legal_player_target() {
        let gs = two_player_state();
        let target = EffectTarget::Player { id: PlayerId(0) };
        assert!(is_legal_target(&gs, &target, TargetFilter::Player, PlayerId(1)));
    }

    #[test]
    fn any_filter_includes_creatures_and_players() {
        let mut gs = two_player_state();
        let creature_id = place_creature(&mut gs, PlayerId(1), vec![]);
        let targets = legal_targets(&gs, TargetFilter::Any, PlayerId(0));
        assert!(targets.contains(&EffectTarget::Object { id: creature_id }));
        assert!(targets.contains(&EffectTarget::Player { id: PlayerId(0) }));
        assert!(targets.contains(&EffectTarget::Player { id: PlayerId(1) }));
    }

    #[test]
    fn creature_filter_excludes_players() {
        let mut gs = two_player_state();
        let creature_id = place_creature(&mut gs, PlayerId(1), vec![]);
        let targets = legal_targets(&gs, TargetFilter::Creature, PlayerId(0));
        assert!(targets.contains(&EffectTarget::Object { id: creature_id }));
        assert!(!targets.contains(&EffectTarget::Player { id: PlayerId(0) }));
    }

    #[test]
    fn player_filter_excludes_creatures() {
        let mut gs = two_player_state();
        let creature_id = place_creature(&mut gs, PlayerId(1), vec![]);
        let targets = legal_targets(&gs, TargetFilter::Player, PlayerId(0));
        assert!(!targets.contains(&EffectTarget::Object { id: creature_id }));
        assert!(targets.contains(&EffectTarget::Player { id: PlayerId(0) }));
        assert!(targets.contains(&EffectTarget::Player { id: PlayerId(1) }));
    }

    #[test]
    fn targets_still_legal_true_when_creature_on_battlefield() {
        let mut gs = two_player_state();
        let id = place_creature(&mut gs, PlayerId(1), vec![]);
        let targets = vec![EffectTarget::Object { id }];
        assert!(targets_still_legal(&gs, &targets));
    }

    #[test]
    fn targets_still_legal_false_when_creature_off_battlefield() {
        let mut gs = two_player_state();
        // Don't place it on battlefield
        let id = gs.alloc_id();
        let targets = vec![EffectTarget::Object { id }];
        assert!(!targets_still_legal(&gs, &targets));
    }

    #[test]
    fn targets_still_legal_true_for_player_alive() {
        let gs = two_player_state();
        let targets = vec![EffectTarget::Player { id: PlayerId(0) }];
        assert!(targets_still_legal(&gs, &targets));
    }

    #[test]
    fn targets_still_legal_false_for_player_who_lost() {
        let mut gs = two_player_state();
        gs.get_player_mut(PlayerId(1)).unwrap().has_lost = true;
        let targets = vec![EffectTarget::Player { id: PlayerId(1) }];
        assert!(!targets_still_legal(&gs, &targets));
    }

    #[test]
    fn targets_still_legal_true_for_empty_slice() {
        let gs = two_player_state();
        assert!(targets_still_legal(&gs, &[]));
    }
}
```

- [ ] **Step 2: Run tests to confirm failure**

```bash
cargo test 2>&1 | grep -E "^test result|FAILED|error\["
```

Expected: compile error — module doesn't exist.

- [ ] **Step 3: Add `pub mod targeting` and new `EngineError` variants to `src/engine/mod.rs`**

```rust
pub mod activated;
pub mod casting;
pub mod combat;
pub mod cycling;
pub mod mana;
pub mod stack;
pub mod state_based_actions;
pub mod targeting;  // ← add
pub mod triggered;
pub mod turn;

#[derive(Debug, Clone, PartialEq)]
pub enum EngineError {
    CardNotFound,
    CardNotInHand,
    CardNotOnBattlefield,
    AlreadyTapped,
    InsufficientMana,
    CannotCastNow,
    LandLimitReached,
    NotALand,
    NotACreature,
    NotYourCard,
    SummoningSick,
    CreatureTapped,
    InvalidBlocker,
    MenaceRequiresTwoBlockers,
    NoManaCheckpoint,
    AbilityIndexOutOfRange,
    InvalidPaymentPlan,
    NotYourPriority,
    WrongNumberOfTargets,  // ← add
    IllegalTarget,         // ← add
}
```

- [ ] **Step 4: Implement the three targeting functions**

Replace the `todo!()` stubs in `src/engine/targeting.rs`:

```rust
use crate::types::ability::{StaticAbility, TargetFilter};
use crate::types::effect::EffectTarget;
use crate::types::{GameState, PlayerId, Zone};

// CR 115.4: a target is legal if it exists in the targeted zone, satisfies the
// filter, and is not protected by Shroud (CR 702.18) or Hexproof (CR 702.11).
pub fn is_legal_target(
    state: &GameState,
    target: &EffectTarget,
    filter: TargetFilter,
    caster: PlayerId,
) -> bool {
    match target {
        EffectTarget::Object { id } => {
            let obj = match state.objects.get(id) {
                Some(o) => o,
                None => return false,
            };
            if obj.zone != Zone::Battlefield {
                return false;
            }
            // Filter check
            let passes_filter = match filter {
                TargetFilter::Creature => obj.is_creature(),
                TargetFilter::Player => false,
                TargetFilter::Any => obj.is_creature(), // planeswalkers/battles: future
            };
            if !passes_filter {
                return false;
            }
            // CR 702.18: Shroud prevents targeting by anyone
            if obj.has_keyword(StaticAbility::Shroud) {
                return false;
            }
            // CR 702.11: Hexproof prevents targeting by opponents
            if obj.has_keyword(StaticAbility::Hexproof) && obj.controller != caster {
                return false;
            }
            true
        }
        EffectTarget::Player { id } => {
            let player = match state.get_player(*id) {
                Some(p) => p,
                None => return false,
            };
            if player.has_lost {
                return false;
            }
            matches!(filter, TargetFilter::Player | TargetFilter::Any)
        }
    }
}

pub fn legal_targets(
    state: &GameState,
    filter: TargetFilter,
    caster: PlayerId,
) -> Vec<EffectTarget> {
    let mut result = Vec::new();
    if matches!(filter, TargetFilter::Creature | TargetFilter::Any) {
        for &id in state.battlefield.keys() {
            let t = EffectTarget::Object { id };
            if is_legal_target(state, &t, filter, caster) {
                result.push(t);
            }
        }
    }
    if matches!(filter, TargetFilter::Player | TargetFilter::Any) {
        for player in &state.players {
            let t = EffectTarget::Player { id: player.id };
            if is_legal_target(state, &t, filter, caster) {
                result.push(t);
            }
        }
    }
    result
}

// CR 608.2b: targets are still legal if at least the object/player still exists
// in the required zone. (Does not re-check Shroud/Hexproof — those apply at
// declaration time, not at resolution time.)
pub fn targets_still_legal(state: &GameState, targets: &[EffectTarget]) -> bool {
    if targets.is_empty() {
        return true;
    }
    targets.iter().all(|t| match t {
        EffectTarget::Object { id } => {
            state
                .objects
                .get(id)
                .map(|o| o.zone == Zone::Battlefield)
                .unwrap_or(false)
        }
        EffectTarget::Player { id } => {
            state
                .get_player(*id)
                .map(|p| !p.has_lost)
                .unwrap_or(false)
        }
    })
}

#[cfg(test)]
mod tests {
    // (tests written in Step 1 — no changes needed here)
}
```

- [ ] **Step 5: Run tests**

```bash
cargo test 2>&1 | grep -E "^test result|FAILED|error\["
```

Expected: all tests pass.

- [ ] **Step 6: Commit**

```bash
git add src/engine/mod.rs src/engine/targeting.rs
git commit -m "$(cat <<'EOF'
feat: add engine/targeting.rs with is_legal_target, legal_targets, targets_still_legal

CR 115.4 is_legal_target checks zone, type filter, Shroud (CR 702.18), and
Hexproof (CR 702.11). targets_still_legal implements the CR 608.2b fizzle
predicate (zone check only — protection applies at declaration time). New
EngineError variants WrongNumberOfTargets and IllegalTarget.

Co-Authored-By: Claude Sonnet 4.6 <noreply@anthropic.com>
EOF
)"
```

---

## Task 5: Parser changes (parse_instant_or_sorcery new patterns)

**Files:**
- Modify: `src/parser/oracle.rs`
- Modify: `src/cards/scryfall.rs`

- [ ] **Step 1: Write failing tests for Giant Growth and Lightning Bolt parsing**

Add to the parser test section in `src/parser/oracle.rs`:

```rust
// ── Targeted spell parsing ─────────────────────────────────────────────────

#[test]
fn parse_giant_growth_effect() {
    use crate::types::ability::{SpellAbility, TargetFilter};
    use crate::types::effect::EffectStep;
    use crate::types::PTDelta;
    let result = parse_instant_or_sorcery(
        "Target creature gets +3/+3 until end of turn.",
        "Giant Growth",
    );
    assert_eq!(result.len(), 1);
    let OracleSpan::Parsed(Ability::SpellEffect(sa)) = &result[0] else {
        panic!("expected SpellEffect, got {:?}", result[0]);
    };
    assert_eq!(sa.target_requirements, vec![TargetFilter::Creature]);
    assert_eq!(
        sa.steps,
        vec![EffectStep::BoostPermanentPT(PTDelta { power: 3, toughness: 3 })]
    );
}

#[test]
fn parse_lightning_bolt_effect() {
    use crate::types::ability::{SpellAbility, TargetFilter};
    use crate::types::effect::EffectStep;
    let result = parse_instant_or_sorcery(
        "Lightning Bolt deals 3 damage to any target.",
        "Lightning Bolt",
    );
    assert_eq!(result.len(), 1);
    let OracleSpan::Parsed(Ability::SpellEffect(sa)) = &result[0] else {
        panic!("expected SpellEffect, got {:?}", result[0]);
    };
    assert_eq!(sa.target_requirements, vec![TargetFilter::Any]);
    assert_eq!(sa.steps, vec![EffectStep::DealDamage(3)]);
}

#[test]
fn parse_draw_a_card_spell_is_untargeted() {
    use crate::types::ability::SpellAbility;
    let result = parse_instant_or_sorcery("Draw a card.", "Opt");
    assert_eq!(result.len(), 1);
    let OracleSpan::Parsed(Ability::SpellEffect(sa)) = &result[0] else {
        panic!("expected SpellEffect");
    };
    assert!(sa.target_requirements.is_empty());
}
```

Note: `parse_instant_or_sorcery` tests already in the file (around line 1568) use the old one-argument form. Those need to be updated too (see Step 4).

- [ ] **Step 2: Run tests to confirm failure**

```bash
cargo test 2>&1 | grep -E "^test result|FAILED|error\["
```

Expected: compile error — `parse_instant_or_sorcery` takes 1 argument, tests pass 2.

- [ ] **Step 3: Add new `try_parse_effect_step` patterns**

In `src/parser/oracle.rs`, find `fn try_parse_effect_step(s: &str) -> Option<EffectStep>`. Add two new match arms before the final `None` return:

```rust
// "gets +N/+M until end of turn"
if let Some(rest) = s.strip_prefix("gets ")
    && let Some(boost_str) = rest.strip_suffix(" until end of turn")
{
    // Parse "+N/+M" or "+N/+M" patterns
    let boost_str = boost_str.trim();
    if boost_str.starts_with('+') || boost_str.starts_with('-') {
        let parts: Vec<&str> = boost_str.splitn(2, '/').collect();
        if parts.len() == 2 {
            let power_s = parts[0].trim_start_matches('+');
            let toughness_s = parts[1].trim_start_matches('+');
            if let (Ok(p), Ok(t)) = (power_s.parse::<i32>(), toughness_s.parse::<i32>()) {
                use crate::types::PTDelta;
                return Some(EffectStep::BoostPermanentPT(PTDelta { power: p, toughness: t }));
            }
        }
    }
}

// "deals N damage"
if let Some(rest) = s.strip_prefix("deals ")
    && let Some(damage_str) = rest.strip_suffix(" damage")
{
    if let Ok(n) = damage_str.trim().parse::<u32>() {
        return Some(EffectStep::DealDamage(n));
    }
}
```

- [ ] **Step 4: Add `parse_spell_paragraph` and update `parse_instant_or_sorcery`**

Replace the existing `parse_instant_or_sorcery` function and add `parse_spell_paragraph`:

```rust
/// Detects targeting patterns in a spell paragraph and returns a SpellAbility.
///
/// Pattern A (target at front): "Target creature ..." → Creature filter; strip prefix.
/// Pattern B (card name damage): "CardName deals N damage to any target" → Any filter; strip suffix.
///
/// All prefix/suffix lengths are computed on the lowercase form then applied at the
/// same byte offset on the original because every prefix/suffix is pure ASCII.
fn parse_spell_paragraph(paragraph: &str, card_name: &str) -> crate::types::ability::SpellAbility {
    use crate::types::ability::{SpellAbility, TargetFilter};
    let lc = paragraph.trim_end_matches('.').to_lowercase();

    // Pattern A — "target creature " prefix
    {
        const PREFIX: &str = "target creature ";
        if lc.starts_with(PREFIX) {
            let effective = paragraph[PREFIX.len()..].trim_end_matches('.');
            let steps = parse_spell_effect(effective);
            return SpellAbility { target_requirements: vec![TargetFilter::Creature], steps };
        }
    }
    // Pattern A — "target player " prefix
    {
        const PREFIX: &str = "target player ";
        if lc.starts_with(PREFIX) {
            let effective = paragraph[PREFIX.len()..].trim_end_matches('.');
            let steps = parse_spell_effect(effective);
            return SpellAbility { target_requirements: vec![TargetFilter::Player], steps };
        }
    }
    // Pattern B — "<CardName> deals N damage to any target"
    {
        let card_lower = card_name.to_lowercase();
        let prefix = format!("{} ", card_lower);
        if lc.starts_with(prefix.as_str()) {
            let rest_lc = &lc[prefix.len()..];
            if let Some(damage_part) = rest_lc.strip_suffix(" to any target") {
                // damage_part is from lc, but it's all lowercase ascii so safe to use as step input
                let steps = parse_spell_effect(damage_part);
                return SpellAbility { target_requirements: vec![TargetFilter::Any], steps };
            }
            if let Some(damage_part) = rest_lc.strip_suffix(" to target creature") {
                let steps = parse_spell_effect(damage_part);
                return SpellAbility { target_requirements: vec![TargetFilter::Creature], steps };
            }
        }
    }
    // No targeting pattern found — untargeted spell
    SpellAbility {
        target_requirements: vec![],
        steps: parse_spell_effect(paragraph),
    }
}

pub fn parse_instant_or_sorcery(text: &str, card_name: &str) -> Vec<OracleSpan> {
    use crate::types::ability::Ability;
    let mut spans = Vec::new();
    for paragraph in text.split('\n') {
        let paragraph = paragraph.trim();
        if paragraph.is_empty() {
            continue;
        }
        let spell_ability = parse_spell_paragraph(paragraph, card_name);
        spans.push(OracleSpan::Parsed(Ability::SpellEffect(spell_ability)));
    }
    spans
}
```

- [ ] **Step 5: Update existing tests that call `parse_instant_or_sorcery` with one argument**

Find all call sites in the test block (around line 1568) and add `""` as the second argument (card_name doesn't affect untargeted spells):

```rust
// Around line 1568 — update the first few tests:
let result = parse_instant_or_sorcery("Draw a card.", "");
let result = parse_instant_or_sorcery("Draw a card.\nDraw a card.", "");
let result = parse_instant_or_sorcery("Scry 1. Draw a card.", "");
let result = parse_instant_or_sorcery("Draw a card, then scry 2.", "");
let result = parse_instant_or_sorcery("Counter target spell.", "");
let result = parse_instant_or_sorcery("Draw two cards.\nGain 3 life.", "");
let result = parse_instant_or_sorcery("", "");
```

Also update the SpellEffect assertions in those tests — after the change, the result is `SpellEffect(SpellAbility { target_requirements: vec![], steps: ... })`.

For example, the `parse_draw_a_card_sorcery` test (line ~1568):
```rust
#[test]
fn parse_draw_a_card_sorcery() {
    use crate::types::ability::SpellAbility;
    let result = parse_instant_or_sorcery("Draw a card.", "");
    assert_eq!(result.len(), 1);
    let OracleSpan::Parsed(Ability::SpellEffect(sa)) = &result[0] else {
        panic!("expected SpellEffect");
    };
    assert!(sa.target_requirements.is_empty());
    assert_eq!(sa.steps, vec![EffectStep::DrawCard(1)]);
}
```

- [ ] **Step 6: Update `src/cards/scryfall.rs` to pass `name`**

Change:
```rust
// Before:
parse_instant_or_sorcery(&oracle_text)

// After:
parse_instant_or_sorcery(&oracle_text, &name)
```

- [ ] **Step 7: Run tests**

```bash
cargo test 2>&1 | grep -E "^test result|FAILED|error\["
```

Expected: all tests pass, including the new Giant Growth and Lightning Bolt tests.

- [ ] **Step 8: Commit**

```bash
git add src/parser/oracle.rs src/cards/scryfall.rs
git commit -m "$(cat <<'EOF'
feat: parse_instant_or_sorcery gains targeting patterns (Giant Growth, Lightning Bolt)

parse_instant_or_sorcery gains a card_name parameter. parse_spell_paragraph
detects "target creature" prefix and "<CardName> deals N damage to any target"
suffix patterns, setting target_requirements on the resulting SpellAbility.
New try_parse_effect_step patterns: "gets +N/+M until end of turn" and
"deals N damage".

Co-Authored-By: Claude Sonnet 4.6 <noreply@anthropic.com>
EOF
)"
```

---

## Task 6: cast_spell target validation

**Files:**
- Modify: `src/engine/casting.rs`
- Modify: `src/serve.rs`

- [ ] **Step 1: Write failing tests**

Add to `src/engine/casting.rs` tests:

```rust
#[test]
fn cast_targeted_spell_without_target_returns_wrong_number() {
    use crate::types::ability::{SpellAbility, TargetFilter};
    use crate::types::effect::EffectTarget;
    let mut gs = make_state();
    gs.get_player_mut(PlayerId(0)).unwrap().mana_pool.green += 1;
    let targeted_instant_def = CardDefinition {
        name: "Giant Growth".into(),
        mana_cost: Some(ManaCost { pips: vec![ManaPip::Green] }),
        type_line: TypeLine {
            supertypes: vec![],
            card_types: vec![CardType::Instant],
            subtypes: vec![],
        },
        oracle_text: "Target creature gets +3/+3 until end of turn.".into(),
        abilities: vec![OracleSpan::Parsed(Ability::SpellEffect(SpellAbility {
            target_requirements: vec![TargetFilter::Creature],
            steps: vec![],
        }))],
        power: None,
        toughness: None,
    };
    let id = put_in_hand(&mut gs, PlayerId(0), targeted_instant_def);
    let result = cast_spell(gs, PlayerId(0), id, vec![]);
    assert!(matches!(result, Err(EngineError::WrongNumberOfTargets)));
}

#[test]
fn cast_targeted_spell_with_illegal_target_returns_illegal_target() {
    use crate::types::ability::{SpellAbility, TargetFilter};
    use crate::types::effect::EffectTarget;
    let mut gs = make_state();
    gs.get_player_mut(PlayerId(0)).unwrap().mana_pool.green += 1;
    let targeted_instant_def = CardDefinition {
        name: "Giant Growth".into(),
        mana_cost: Some(ManaCost { pips: vec![ManaPip::Green] }),
        type_line: TypeLine {
            supertypes: vec![],
            card_types: vec![CardType::Instant],
            subtypes: vec![],
        },
        oracle_text: "Target creature gets +3/+3 until end of turn.".into(),
        abilities: vec![OracleSpan::Parsed(Ability::SpellEffect(SpellAbility {
            target_requirements: vec![TargetFilter::Creature],
            steps: vec![],
        }))],
        power: None,
        toughness: None,
    };
    let id = put_in_hand(&mut gs, PlayerId(0), targeted_instant_def);
    // ObjectId(999) doesn't exist
    let result = cast_spell(gs, PlayerId(0), id, vec![EffectTarget::Object { id: ObjectId(999) }]);
    assert!(matches!(result, Err(EngineError::IllegalTarget)));
}

#[test]
fn cast_targeted_spell_with_valid_target_succeeds() {
    use crate::types::ability::{SpellAbility, TargetFilter};
    use crate::types::effect::EffectTarget;
    use crate::types::{PermanentState, Zone as Z};
    let mut gs = make_state();
    gs.get_player_mut(PlayerId(0)).unwrap().mana_pool.green += 1;
    // Place a creature on the battlefield
    let creature_def = CardDefinition {
        name: "Bear".into(),
        mana_cost: None,
        type_line: TypeLine {
            supertypes: vec![],
            card_types: vec![CardType::Creature],
            subtypes: vec![],
        },
        oracle_text: String::new(),
        abilities: vec![],
        power: Some(2),
        toughness: Some(2),
    };
    let creature_id = gs.alloc_id();
    let obj = CardObject::new(creature_id, creature_def, PlayerId(1), Zone::Battlefield);
    gs.battlefield.insert(creature_id, PermanentState::new(&obj.definition));
    gs.add_object(obj);

    let targeted_instant_def = CardDefinition {
        name: "Giant Growth".into(),
        mana_cost: Some(ManaCost { pips: vec![ManaPip::Green] }),
        type_line: TypeLine {
            supertypes: vec![],
            card_types: vec![CardType::Instant],
            subtypes: vec![],
        },
        oracle_text: "Target creature gets +3/+3 until end of turn.".into(),
        abilities: vec![OracleSpan::Parsed(Ability::SpellEffect(SpellAbility {
            target_requirements: vec![TargetFilter::Creature],
            steps: vec![],
        }))],
        power: None,
        toughness: None,
    };
    let id = put_in_hand(&mut gs, PlayerId(0), targeted_instant_def);
    let gs = cast_spell(gs, PlayerId(0), id, vec![EffectTarget::Object { id: creature_id }]).unwrap();
    assert_eq!(gs.objects[&id].zone, Zone::Stack);
    // targets stored on stack object
    let stack_id = gs.stack[0];
    assert_eq!(gs.stack_objects[&stack_id].targets, vec![EffectTarget::Object { id: creature_id }]);
}

#[test]
fn cast_untargeted_spell_with_no_targets_still_works() {
    use crate::types::effect::EffectTarget;
    let mut gs = make_state();
    gs.get_player_mut(PlayerId(0)).unwrap().mana_pool.blue += 1;
    let id = put_in_hand(&mut gs, PlayerId(0), make_instant_def("Opt", vec![ManaPip::Blue]));
    let gs = cast_spell(gs, PlayerId(0), id, vec![]).unwrap();
    assert_eq!(gs.objects[&id].zone, Zone::Stack);
}
```

- [ ] **Step 2: Run tests to confirm failure**

```bash
cargo test 2>&1 | grep -E "^test result|FAILED|error\["
```

Expected: compile error — `cast_spell` takes 3 arguments, tests pass 4.

- [ ] **Step 3: Update `cast_spell` signature and add target validation**

In `src/engine/casting.rs`, change the function signature and add validation after the timing/mana checks:

```rust
pub fn cast_spell(
    mut state: GameState,
    player_id: PlayerId,
    object_id: ObjectId,
    declared_targets: Vec<crate::types::effect::EffectTarget>,
) -> Result<GameState, EngineError> {
    // ... existing priority check ...
    // ... existing hand/timing/mana-cost checks ...

    // Target validation (CR 601.2c: targets declared when spell is cast)
    {
        use crate::engine::targeting::{is_legal_target, legal_targets as _};
        use crate::types::ability::{Ability, SpellAbility};
        use crate::types::OracleSpan;
        let obj = state.objects.get(&object_id).unwrap();
        let target_requirements: Vec<crate::types::ability::TargetFilter> = obj
            .definition
            .abilities
            .iter()
            .filter_map(|span| match span {
                OracleSpan::Parsed(Ability::SpellEffect(sa)) => Some(sa.target_requirements.clone()),
                _ => None,
            })
            .flatten()
            .collect();
        if target_requirements.len() != declared_targets.len() {
            return Err(EngineError::WrongNumberOfTargets);
        }
        for (filter, target) in target_requirements.iter().zip(declared_targets.iter()) {
            if !is_legal_target(&state, target, *filter, player_id) {
                return Err(EngineError::IllegalTarget);
            }
        }
    }

    // ... existing plan/pay_mana_cost/move-from-hand/zone-to-Stack code ...

    let stack_id = state.alloc_stack_id();
    let stack_obj = crate::types::StackObject {
        id: stack_id,
        payload: crate::types::StackPayload::Spell { card_id: object_id },
        controller: player_id,
        targets: declared_targets,  // ← use declared targets
    };
    // ... rest unchanged ...
}
```

- [ ] **Step 4: Update all `cast_spell` callers**

In existing tests in `casting.rs`, add `vec![]` as the fourth argument to all `cast_spell(gs, player_id, id)` calls:

```rust
// All existing calls like:
cast_spell(gs, PlayerId(0), id)
// become:
cast_spell(gs, PlayerId(0), id, vec![])
```

There are about 15 such calls in the test section. Use search-and-replace in the file.

In `src/serve.rs`, update the dispatch:

```rust
// Before:
ActionRequest::CastSpell { object_id } => {
    let player = state.priority_player;
    cast_spell(state, player, ObjectId(object_id)).map_err(|e| format!("{e:?}"))
}

// After:
ActionRequest::CastSpell { object_id, targets } => {
    let player = state.priority_player;
    cast_spell(state, player, ObjectId(object_id), targets).map_err(|e| format!("{e:?}"))
}
```

And add `targets` to the `CastSpell` variant in `ActionRequest`:
```rust
CastSpell {
    object_id: u64,
    #[serde(default)]
    targets: Vec<mecha_oracle::types::effect::EffectTarget>,
},
```

- [ ] **Step 5: Run tests**

```bash
cargo test 2>&1 | grep -E "^test result|FAILED|error\["
```

Expected: all tests pass.

- [ ] **Step 6: Commit**

```bash
git add src/engine/casting.rs src/serve.rs
git commit -m "$(cat <<'EOF'
feat: cast_spell validates declared targets (CR 601.2c)

cast_spell gains declared_targets parameter. Validates target count against
spell_ability.target_requirements and legality via is_legal_target before
paying mana. Targets stored in StackObject.targets. WrongNumberOfTargets and
IllegalTarget errors returned on violations. CastSpell action gains targets
field (#[serde(default)] keeps existing non-targeted requests working).

Co-Authored-By: Claude Sonnet 4.6 <noreply@anthropic.com>
EOF
)"
```

---

## Task 7: activate_ability target validation

**Files:**
- Modify: `src/engine/activated.rs`
- Modify: `src/serve.rs`

No targeted activated abilities exist yet, so this task just wires the plumbing.

- [ ] **Step 1: Update `activate_ability` signature**

In `src/engine/activated.rs`, change:

```rust
pub fn activate_ability(
    mut state: GameState,
    object_id: ObjectId,
    ability_index: usize,
    activating_player: PlayerId,
    x_value: Option<u32>,
    payment_plan: Option<PaymentPlan>,
    declared_targets: Vec<crate::types::effect::EffectTarget>,
) -> Result<GameState, EngineError> {
```

After getting the `ability` (after the `ok_or(EngineError::AbilityIndexOutOfRange)?`), add target validation before the cost checks:

```rust
// Target validation for non-mana abilities (CR 601.2c analog)
let produces_mana_early_check = ability.effect.iter().any(|e| matches!(e, EffectStep::AddMana(_)));
if !produces_mana_early_check {
    use crate::engine::targeting::is_legal_target;
    if ability.target_requirements.len() != declared_targets.len() {
        return Err(EngineError::WrongNumberOfTargets);
    }
    for (filter, target) in ability.target_requirements.iter().zip(declared_targets.iter()) {
        if !is_legal_target(&state, target, *filter, activating_player) {
            return Err(EngineError::IllegalTarget);
        }
    }
}
```

In the non-mana ability branch (where the StackObject is created), change `targets: vec![]` to `targets: declared_targets`:

```rust
let stack_obj = crate::types::StackObject {
    id: stack_id,
    payload: crate::types::StackPayload::ActivatedAbility { source_id: object_id, effect: ability.effect.clone(), label },
    controller: activating_player,
    targets: declared_targets,  // ← was vec![]
};
```

- [ ] **Step 2: Update all `activate_ability` call sites**

In `src/serve.rs`, update the dispatch to pass `targets`:

```rust
// In ActionRequest:
ActivateAbility {
    object_id: u64,
    ability_index: usize,
    #[serde(default)]
    x_value: Option<u32>,
    #[serde(default)]
    payment_plan: Option<mecha_oracle::types::mana::PaymentPlan>,
    #[serde(default)]
    targets: Vec<mecha_oracle::types::effect::EffectTarget>,
},

// In dispatch:
ActionRequest::ActivateAbility { object_id, ability_index, x_value, payment_plan, targets } => {
    let player = state.priority_player;
    activate_ability(
        state,
        ObjectId(object_id),
        ability_index,
        player,
        x_value,
        payment_plan,
        targets,
    )
    .map_err(|e| format!("{e:?}"))
}
```

- [ ] **Step 3: Update test calls in `activated.rs`**

All `activate_ability(state, id, index, player, x_value, plan)` calls need `vec![]` as the last argument. Search and update them all.

Also update the `cycle_card` function in `cycling.rs` if it calls `activate_ability` directly — check: it does not; it has its own StackObject construction. No change needed there.

- [ ] **Step 4: Run tests**

```bash
cargo test 2>&1 | grep -E "^test result|FAILED|error\["
```

Expected: all tests pass.

- [ ] **Step 5: Commit**

```bash
git add src/engine/activated.rs src/serve.rs
git commit -m "$(cat <<'EOF'
feat: activate_ability gains declared_targets parameter

activate_ability validates target count and legality for non-mana abilities.
Mana abilities bypass targeting (they can't be targeted). ActivateAbility
action gains targets field (#[serde(default)] keeps existing requests working).

Co-Authored-By: Claude Sonnet 4.6 <noreply@anthropic.com>
EOF
)"
```

---

## Task 8: Fizzle check in resolution (CR 608.2b)

**Files:**
- Modify: `src/engine/stack.rs`

- [ ] **Step 1: Write failing test**

Add to `src/engine/stack.rs` tests:

```rust
#[test]
fn targeted_spell_fizzles_when_target_leaves_before_resolution() {
    use crate::types::ability::{SpellAbility, TargetFilter};
    use crate::types::effect::EffectTarget;
    use crate::types::PTDelta;

    let mut gs = make_state();

    // Put a creature on battlefield as target
    let def = CardDefinition {
        name: "Target".into(),
        mana_cost: None,
        type_line: TypeLine {
            supertypes: vec![],
            card_types: vec![CardType::Creature],
            subtypes: vec![],
        },
        oracle_text: String::new(),
        abilities: vec![],
        power: Some(1),
        toughness: Some(1),
    };
    let creature_id = gs.alloc_id();
    let creature_obj = CardObject::new(creature_id, def, PlayerId(1), Zone::Battlefield);
    gs.battlefield.insert(creature_id, PermanentState::new(&creature_obj.definition));
    gs.add_object(creature_obj);

    // Put a Giant Growth targeting that creature on the stack
    let gg_def = CardDefinition {
        name: "Giant Growth".into(),
        mana_cost: Some(ManaCost { pips: vec![ManaPip::Green] }),
        type_line: TypeLine {
            supertypes: vec![],
            card_types: vec![CardType::Instant],
            subtypes: vec![],
        },
        oracle_text: "Target creature gets +3/+3 until end of turn.".into(),
        abilities: vec![OracleSpan::Parsed(Ability::SpellEffect(SpellAbility {
            target_requirements: vec![TargetFilter::Creature],
            steps: vec![EffectStep::BoostPermanentPT(PTDelta { power: 3, toughness: 3 })],
        }))],
        power: None,
        toughness: None,
    };
    let gg_id = gs.alloc_id();
    let gg_obj = CardObject::new(gg_id, gg_def, PlayerId(0), Zone::Stack);
    gs.add_object(gg_obj);
    push_spell(&mut gs, gg_id);
    // Set the target on the stack object
    let stack_id = *gs.stack.last().unwrap();
    gs.stack_objects.get_mut(&stack_id).unwrap().targets = vec![EffectTarget::Object { id: creature_id }];

    // Remove creature from battlefield BEFORE resolution
    gs.battlefield.remove(&creature_id);
    gs.objects.get_mut(&creature_id).unwrap().zone = Zone::Graveyard;

    let gs = resolve_top(gs);

    // Spell fizzled: no boost applied (creature is gone), spell moved to graveyard
    assert!(gs.stack.is_empty());
    assert!(!gs.battlefield.contains_key(&creature_id));
    assert!(gs.graveyards[&PlayerId(0)].contains(&gg_id));
    // No crash; game continues normally
}
```

- [ ] **Step 2: Run test to confirm failure**

```bash
cargo test 2>&1 | grep -E "^test result|FAILED|error\["
```

Expected: test fails — no fizzle check exists, resolve continues and potentially panics or applies step to gone creature.

- [ ] **Step 3: Add fizzle check in `resolve_top`**

In the `StackPayload::Spell { card_id }` branch, for non-permanent spells, add the fizzle check before `execute_effect_steps`:

```rust
// CR 608.2b: if all targets are illegal at resolution, the spell is countered
// by the rules (instant/sorcery still moves to graveyard, effects not applied).
if !targets.is_empty() && !crate::engine::targeting::targets_still_legal(&state, &targets) {
    if let Some(obj) = state.objects.get_mut(&card_id) {
        obj.zone = Zone::Graveyard;
    }
    if let Some(gy) = state.graveyards.get_mut(&controller) {
        gy.push(card_id);
    }
    state.consecutive_passes = 0;
    state.priority_player = state.active_player;
    return check_and_apply_sbas(state);
}

state = execute_effect_steps(state, controller, &steps, &targets);
```

Also add for `StackPayload::ActivatedAbility`:

```rust
StackPayload::TriggeredAbility { effect, .. }
| StackPayload::ActivatedAbility { effect, .. } => {
    let controller = stack_obj.controller;
    // CR 608.2b: non-mana activated abilities with all-illegal targets fizzle.
    // Triggered abilities don't fizzle — they just have no effect on gone targets.
    if matches!(stack_obj.payload, StackPayload::ActivatedAbility { .. })
        && !targets.is_empty()
        && !crate::engine::targeting::targets_still_legal(&state, &targets)
    {
        state.consecutive_passes = 0;
        state.priority_player = state.active_player;
        return check_and_apply_sbas(state);
    }
    state = execute_effect_steps(state, controller, &effect, &targets);
    state.consecutive_passes = 0;
    state.priority_player = state.active_player;
    check_and_apply_sbas(state)
}
```

Wait — the match arm currently destructures `stack_obj.payload` and moves it. After extracting `targets = stack_obj.targets.clone()`, we can't access `stack_obj.payload` again via `matches!`. Fix by checking the payload kind before moving it:

```rust
let is_activated = matches!(stack_obj.payload, StackPayload::ActivatedAbility { .. });
let targets = stack_obj.targets.clone();

match stack_obj.payload {
    // ...
    StackPayload::TriggeredAbility { effect, .. }
    | StackPayload::ActivatedAbility { effect, .. } => {
        let controller = stack_obj.controller;
        if is_activated
            && !targets.is_empty()
            && !crate::engine::targeting::targets_still_legal(&state, &targets)
        {
            state.consecutive_passes = 0;
            state.priority_player = state.active_player;
            return check_and_apply_sbas(state);
        }
        state = execute_effect_steps(state, controller, &effect, &targets);
        // ...
    }
}
```

- [ ] **Step 4: Run tests**

```bash
cargo test 2>&1 | grep -E "^test result|FAILED|error\["
```

Expected: all tests pass, including `targeted_spell_fizzles_when_target_leaves_before_resolution`.

- [ ] **Step 5: Commit**

```bash
git add src/engine/stack.rs
git commit -m "$(cat <<'EOF'
feat: CR 608.2b fizzle check at resolution

resolve_top checks targets_still_legal before executing effect steps for
instant/sorcery spells and non-mana activated abilities. Fizzled spells still
move to the graveyard; fizzled activated abilities simply don't apply. Triggered
abilities don't fizzle (they just have no effect on gone targets via get_mut).

Co-Authored-By: Claude Sonnet 4.6 <noreply@anthropic.com>
EOF
)"
```

---

## Task 9: serve.rs UI additions

**Files:**
- Modify: `src/serve.rs`

- [ ] **Step 1: Add `TargetView` and `valid_targets` to the view model**

Add after `ActivatedAbilityView`:

```rust
#[derive(Serialize)]
struct TargetView {
    kind: String,  // "permanent" | "player"
    id: u64,
    name: String,
}
```

Update `ActivatedAbilityView`:
```rust
#[derive(Serialize)]
struct ActivatedAbilityView {
    index: usize,
    label: String,
    can_activate: bool,
    valid_targets: Vec<TargetView>,
}
```

Update `CardView`:
```rust
#[derive(Serialize)]
struct CardView {
    // ... existing fields ...
    valid_targets: Vec<TargetView>,  // populated for castable targeted spells
}
```

- [ ] **Step 2: Add a helper to build `TargetView` items**

Add near the other format functions:

```rust
fn build_target_views(
    state: &GameState,
    targets: &[mecha_oracle::types::effect::EffectTarget],
) -> Vec<TargetView> {
    use mecha_oracle::types::effect::EffectTarget;
    targets
        .iter()
        .filter_map(|t| match t {
            EffectTarget::Object { id } => {
                let obj = state.objects.get(id)?;
                Some(TargetView {
                    kind: "permanent".into(),
                    id: id.0,
                    name: obj.definition.name.clone(),
                })
            }
            EffectTarget::Player { id } => {
                let player = state.get_player(*id)?;
                Some(TargetView {
                    kind: "player".into(),
                    id: id.0 as u64,
                    name: player.name.clone(),
                })
            }
        })
        .collect()
}
```

Note: `player.name` — check `Player` has a `name` field. If not (it might just have `id` and `life`), use `format!("Player {}", id.0)`.

Check `src/types/player.rs`:
- Look for `pub name` — if absent, use `format!("Player {}", id.0)`.

- [ ] **Step 3: Populate `valid_targets` in `to_card_view`**

In `build_player_view`, inside `to_card_view`, add `valid_targets`:

```rust
valid_targets: {
    use mecha_oracle::engine::targeting::legal_targets;
    use mecha_oracle::types::ability::{Ability, OracleSpan};
    if compute_can_cast(state, pid, obj) {
        let filters: Vec<_> = obj
            .definition
            .abilities
            .iter()
            .filter_map(|span| match span {
                OracleSpan::Parsed(Ability::SpellEffect(sa)) => {
                    Some(sa.target_requirements.clone())
                }
                _ => None,
            })
            .flatten()
            .collect();
        if filters.is_empty() {
            vec![]
        } else {
            // For multi-target spells: expose union of legal targets across all slots.
            // Per-slot structure is a future concern.
            let mut seen = std::collections::HashSet::new();
            let mut all_targets = vec![];
            for filter in &filters {
                for t in legal_targets(state, *filter, pid) {
                    use mecha_oracle::types::effect::EffectTarget;
                    let key = match &t {
                        EffectTarget::Object { id } => format!("o{}", id.0),
                        EffectTarget::Player { id } => format!("p{}", id.0),
                    };
                    if seen.insert(key) {
                        all_targets.push(t);
                    }
                }
            }
            build_target_views(state, &all_targets)
        }
    } else {
        vec![]
    }
},
```

- [ ] **Step 4: Populate `valid_targets` on `ActivatedAbilityView`**

In the `activated_abilities` field of `to_card_view`:

```rust
activated_abilities: obj
    .definition
    .abilities
    .iter()
    .filter_map(|span| match span {
        OracleSpan::Parsed(Ability::Activated(a)) => Some(a),
        _ => None,
    })
    .enumerate()
    .map(|(i, ability)| ActivatedAbilityView {
        index: i,
        label: format_activated_ability(ability),
        can_activate: can_pay_cost(state, obj.id, ability, pid),
        valid_targets: vec![],  // no targeted activated abilities yet
    })
    .collect(),
```

- [ ] **Step 5: Run tests**

```bash
cargo test 2>&1 | grep -E "^test result|FAILED|error\["
```

Expected: all tests pass.

- [ ] **Step 6: Commit**

```bash
git add src/serve.rs
git commit -m "$(cat <<'EOF'
feat: serve.rs exposes valid_targets per hand card and activated ability

CardView gains valid_targets (populated when can_cast and the spell has
target_requirements). ActivatedAbilityView gains valid_targets (empty for now).
TargetView carries kind ("permanent"|"player"), id, and name. CastSpell and
ActivateAbility actions already accept targets from the previous task.

Co-Authored-By: Claude Sonnet 4.6 <noreply@anthropic.com>
EOF
)"
```

---

## Task 10: Integration tests, fixture cards, lint

**Files:**
- Modify: `tests/fixtures/oracle_cards_test.json`
- Create or modify: `tests/integration_targeting.rs` (or add to an existing integration test file)

- [ ] **Step 1: Add Giant Growth and Lightning Bolt to the fixture**

In `tests/fixtures/oracle_cards_test.json`, append two new card objects to the JSON array. Use this minimal Scryfall-shaped JSON:

```json
{
  "object": "card",
  "id": "giant-growth-test-id",
  "name": "Giant Growth",
  "lang": "en",
  "layout": "normal",
  "mana_cost": "{G}",
  "cmc": 1.0,
  "type_line": "Instant",
  "oracle_text": "Target creature gets +3/+3 until end of turn.",
  "colors": ["G"],
  "color_identity": ["G"],
  "keywords": [],
  "legalities": {},
  "set": "lea",
  "collector_number": "93"
},
{
  "object": "card",
  "id": "lightning-bolt-test-id",
  "name": "Lightning Bolt",
  "lang": "en",
  "layout": "normal",
  "mana_cost": "{R}",
  "cmc": 1.0,
  "type_line": "Instant",
  "oracle_text": "Lightning Bolt deals 3 damage to any target.",
  "colors": ["R"],
  "color_identity": ["R"],
  "keywords": [],
  "legalities": {},
  "set": "lea",
  "collector_number": "161"
}
```

- [ ] **Step 2: Write integration tests**

Find where integration tests live (look for existing test files in `tests/`). Add the following tests, either to an existing file or to a new `tests/targeting.rs`:

```rust
use mecha_oracle::cards::test_helpers::test_db;
use mecha_oracle::engine::casting::cast_spell;
use mecha_oracle::engine::stack::{pass_priority, resolve_top};
use mecha_oracle::types::ability::StaticAbility;
use mecha_oracle::types::card::{CardDefinition, CardType, TypeLine};
use mecha_oracle::types::effect::EffectTarget;
use mecha_oracle::types::{CardObject, GameState, ObjectId, PermanentState, Player, PlayerId, Step, Zone};

fn make_state() -> GameState {
    let mut gs = GameState::new(vec![
        Player::new(PlayerId(0), "Alice"),
        Player::new(PlayerId(1), "Bob"),
    ]);
    gs.step = Step::PreCombatMain;
    gs
}

fn put_in_hand(state: &mut GameState, owner: PlayerId, def: CardDefinition) -> ObjectId {
    let id = state.alloc_id();
    let obj = CardObject::new(id, def, owner, Zone::Hand);
    state.hands.get_mut(&owner).unwrap().push(id);
    state.add_object(obj);
    id
}

fn place_on_battlefield(state: &mut GameState, def: CardDefinition, owner: PlayerId) -> ObjectId {
    let id = state.alloc_id();
    let obj = CardObject::new(id, def, owner, Zone::Battlefield);
    state.battlefield.insert(id, PermanentState::new(&obj.definition));
    state.add_object(obj);
    id
}

#[test]
fn giant_growth_cast_and_resolve_boosts_creature() {
    let db = test_db();
    let mut gs = make_state();

    // Give player 1 green mana
    gs.get_player_mut(PlayerId(0)).unwrap().mana_pool.green += 1;

    // Grizzly Bears on the battlefield (target)
    let bear_def = db.get("Grizzly Bears").unwrap().clone();
    let bear_id = place_on_battlefield(&mut gs, bear_def, PlayerId(0));

    // Giant Growth in hand
    let gg_def = db.get("Giant Growth").unwrap().clone();
    let gg_id = put_in_hand(&mut gs, PlayerId(0), gg_def);

    // Cast Giant Growth targeting the Bears
    let gs = cast_spell(gs, PlayerId(0), gg_id, vec![EffectTarget::Object { id: bear_id }]).unwrap();
    assert_eq!(gs.objects[&gg_id].zone, Zone::Stack);

    // Both players pass priority → resolves
    let gs = pass_priority(gs, PlayerId(0)).unwrap();
    let gs = pass_priority(gs, PlayerId(1)).unwrap();

    // Bears now 5/5
    assert_eq!(gs.battlefield[&bear_id].effective_power(), Some(5));
    assert_eq!(gs.battlefield[&bear_id].effective_toughness(), Some(5));
    assert_eq!(gs.objects[&gg_id].zone, Zone::Graveyard);
    assert!(gs.stack.is_empty());
}

#[test]
fn lightning_bolt_cast_and_resolve_kills_creature() {
    let db = test_db();
    let mut gs = make_state();
    gs.get_player_mut(PlayerId(0)).unwrap().mana_pool.red += 1;

    // Typhoid Rats (1/1 deathtouch) as the target — killed by 3 damage
    let rats_def = db.get("Typhoid Rats").unwrap().clone();
    let rats_id = place_on_battlefield(&mut gs, rats_def, PlayerId(1));

    let bolt_def = db.get("Lightning Bolt").unwrap().clone();
    let bolt_id = put_in_hand(&mut gs, PlayerId(0), bolt_def);

    let gs = cast_spell(gs, PlayerId(0), bolt_id, vec![EffectTarget::Object { id: rats_id }]).unwrap();

    let gs = pass_priority(gs, PlayerId(0)).unwrap();
    let gs = pass_priority(gs, PlayerId(1)).unwrap();

    // Rats dead (3 damage >= toughness 1 → SBA kills it)
    assert!(!gs.battlefield.contains_key(&rats_id));
    assert!(gs.graveyards[&PlayerId(1)].contains(&rats_id));
    assert!(gs.stack.is_empty());
}

#[test]
fn lightning_bolt_cast_and_resolve_damages_player() {
    let db = test_db();
    let mut gs = make_state();
    gs.get_player_mut(PlayerId(0)).unwrap().mana_pool.red += 1;
    let before_life = gs.get_player(PlayerId(1)).unwrap().life;

    let bolt_def = db.get("Lightning Bolt").unwrap().clone();
    let bolt_id = put_in_hand(&mut gs, PlayerId(0), bolt_def);

    let gs = cast_spell(
        gs,
        PlayerId(0),
        bolt_id,
        vec![EffectTarget::Player { id: PlayerId(1) }],
    )
    .unwrap();

    let gs = pass_priority(gs, PlayerId(0)).unwrap();
    let gs = pass_priority(gs, PlayerId(1)).unwrap();

    assert_eq!(gs.get_player(PlayerId(1)).unwrap().life, before_life - 3);
    assert_eq!(gs.objects[&bolt_id].zone, Zone::Graveyard);
}

#[test]
fn giant_growth_fizzles_if_target_dies_before_resolution() {
    let db = test_db();
    let mut gs = make_state();
    gs.get_player_mut(PlayerId(0)).unwrap().mana_pool.green += 1;

    let bear_def = db.get("Grizzly Bears").unwrap().clone();
    let bear_id = place_on_battlefield(&mut gs, bear_def, PlayerId(0));

    let gg_def = db.get("Giant Growth").unwrap().clone();
    let gg_id = put_in_hand(&mut gs, PlayerId(0), gg_def);

    let gs = cast_spell(gs, PlayerId(0), gg_id, vec![EffectTarget::Object { id: bear_id }]).unwrap();

    // Remove bear from battlefield before resolution
    let mut gs = gs;
    gs.battlefield.remove(&bear_id);
    gs.objects.get_mut(&bear_id).unwrap().zone = Zone::Graveyard;

    let gs = pass_priority(gs, PlayerId(0)).unwrap();
    let gs = pass_priority(gs, PlayerId(1)).unwrap();

    // Spell fizzled: Giant Growth in graveyard but no boost applied
    assert!(!gs.battlefield.contains_key(&bear_id));
    assert_eq!(gs.objects[&gg_id].zone, Zone::Graveyard);
    assert!(gs.stack.is_empty());
}

#[test]
fn cant_cast_giant_growth_targeting_shroud_creature() {
    use mecha_oracle::engine::EngineError;
    use mecha_oracle::types::ability::Ability;
    use mecha_oracle::types::OracleSpan;
    let db = test_db();
    let mut gs = make_state();
    gs.get_player_mut(PlayerId(0)).unwrap().mana_pool.green += 1;

    let mut bear_def = db.get("Grizzly Bears").unwrap().clone();
    bear_def.abilities = vec![OracleSpan::Parsed(Ability::Static(StaticAbility::Shroud))];
    let bear_id = place_on_battlefield(&mut gs, bear_def, PlayerId(0));

    let gg_def = db.get("Giant Growth").unwrap().clone();
    let gg_id = put_in_hand(&mut gs, PlayerId(0), gg_def);

    let result = cast_spell(gs, PlayerId(0), gg_id, vec![EffectTarget::Object { id: bear_id }]);
    assert!(matches!(result, Err(EngineError::IllegalTarget)));
}
```

- [ ] **Step 3: Check if `Player` has a `name` field**

```bash
grep -n "pub name\|pub struct Player" src/types/player.rs | head -5
```

If absent, update the `build_target_views` function in `serve.rs` to use `format!("Player {}", id.0)` instead of `player.name.clone()`.

- [ ] **Step 4: Run tests**

```bash
cargo test 2>&1 | grep -E "^test result|FAILED|error\["
```

Expected: all tests pass.

- [ ] **Step 5: Run clippy and fix any issues**

```bash
cargo clippy --fix --all-targets 2>&1 | grep -E "error\[|warning\[" | head -20
cargo clippy --all-targets 2>&1 | grep -E "^error"
```

Fix any remaining warnings that clippy couldn't auto-fix.

- [ ] **Step 6: Final commit**

```bash
git add tests/fixtures/oracle_cards_test.json tests/ src/
git commit -m "$(cat <<'EOF'
feat: end-to-end targeting tests for Giant Growth and Lightning Bolt

Adds Giant Growth and Lightning Bolt to the test fixture. Integration tests
cover: boost resolves correctly, damage kills creature via SBA, damage reduces
player life, fizzle when target dies before resolution, and Shroud prevents
targeting at cast time.

Co-Authored-By: Claude Sonnet 4.6 <noreply@anthropic.com>
EOF
)"
```

---

## Self-review

### Spec coverage check

| Spec section | Covered by task |
|---|---|
| `EffectStep::BoostPermanentPT` tuple change | Task 2 |
| `StackObject.targets` | Task 1 |
| `EffectTarget` struct variants + serde | Task 1 |
| `TargetFilter` enum | Task 3 |
| `SpellAbility` struct | Task 3 |
| `Ability::SpellEffect(SpellAbility)` | Task 3 |
| `ActivatedAbility.target_requirements` | Task 3 |
| `EffectStep::DealDamage(u32)` | Task 2 |
| `ObjectId`/`PlayerId` Deserialize | Task 1 |
| `engine/targeting.rs` — `is_legal_target` | Task 4 |
| `engine/targeting.rs` — `legal_targets` | Task 4 |
| `engine/targeting.rs` — `targets_still_legal` | Task 4 |
| Shroud/Hexproof enforcement in targeting | Task 4 |
| Shroud/Hexproof parsed as `Parsed(Static(...))` | Task 3 |
| `cast_spell` gains `declared_targets` | Task 6 |
| `activate_ability` gains `declared_targets` | Task 7 |
| CR 608.2b fizzle check | Task 8 |
| `execute_effect_steps` gains `targets` param | Task 2 |
| `parse_instant_or_sorcery` gains `card_name` | Task 5 |
| `parse_spell_paragraph` with target patterns | Task 5 |
| `EngineError::WrongNumberOfTargets` / `IllegalTarget` | Task 4 |
| `TargetView` + `valid_targets` in serve.rs | Task 9 |
| `CastSpell` action gains `targets` field | Task 6 |
| `ActivateAbility` action gains `targets` field | Task 7 |
| Giant Growth end-to-end test | Task 10 |
| Lightning Bolt end-to-end test | Task 10 |
| Ward remains `ParsedUnimplemented` | ✓ (not changed) |

### Type consistency check

- `EffectTarget::Object { id: ObjectId }` and `EffectTarget::Player { id: PlayerId }` — consistent throughout.
- `BoostPermanentPT(PTDelta)` — tuple variant used in triggered.rs constructions, test assertions, and the resolution arm.
- `SpellAbility { target_requirements: Vec<TargetFilter>, steps: Effect }` — `Effect` is the `Vec<EffectStep>` type alias.
- `parse_instant_or_sorcery(text: &str, card_name: &str)` — two-arg form used everywhere after Task 5.
- `cast_spell(..., declared_targets: Vec<EffectTarget>)` — `vec![]` passed from all existing call sites.
- `activate_ability(..., declared_targets: Vec<EffectTarget>)` — same pattern.

### Placeholder scan

No TBDs, todos, or "similar to task N" phrases found.
