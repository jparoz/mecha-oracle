# Spell Targeting — Counterspells Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Make unconditional counterspells (Counterspell, Negate, Essence Scatter, Dispel) work end-to-end by adding spell-on-stack targeting to the engine.

**Architecture:** Add `EffectTarget::StackObject { id: StackId }` to represent spell targets, `TargetFilter::Spell(SpellFilter)` with an `included/excluded CardType` struct, and `EffectStep::CounterSpell` that delegates to the existing `counter_spell_on_stack`. The targeting, parser, and serve layers are updated to understand these new types.

**Tech Stack:** Rust, Serde (JSON), existing engine modules (`engine/targeting.rs`, `engine/stack.rs`, `engine/casting.rs`, `parser/oracle.rs`, `serve.rs`).

**Spec:** `docs/superpowers/specs/2026-06-15-spell-targeting-design.md`

---

## Files

- **Modify** `src/types/stack.rs` — add `Serialize, Deserialize` derives to `StackId`
- **Modify** `src/types/ability.rs` — add `SpellFilter` struct; add `TargetFilter::Spell(SpellFilter)`; remove `Copy` from `TargetFilter`
- **Modify** `src/types/effect.rs` — add `EffectTarget::StackObject { id: StackId }`; add `EffectStep::CounterSpell`
- **Modify** `src/engine/targeting.rs` — update signatures to `&TargetFilter`; add `StackObject` and `Spell` arms; update all test call sites
- **Modify** `src/engine/stack.rs` — add `CounterSpell` arm to `execute_effect_steps`; fix `DealDamage` match arm
- **Modify** `src/engine/casting.rs` — fix `*filter` → `filter` at `is_legal_target` call site
- **Modify** `src/parser/oracle.rs` — add four counter-spell patterns; update `counterspell_fully_unimplemented` test
- **Modify** `src/serve.rs` — `.copied()` → `.cloned()`; `*filter` → `filter`; add `StackObject` name resolution
- **Modify** `docs/todo.md` — add conditional counter notes

---

## Task 1: Types — new variants, StackId Serde, compile stubs

**Files:**
- Modify: `src/types/stack.rs`
- Modify: `src/types/ability.rs`
- Modify: `src/types/effect.rs`
- Modify: `src/engine/targeting.rs`
- Modify: `src/engine/stack.rs`
- Modify: `src/engine/casting.rs`
- Modify: `src/serve.rs`

- [ ] **Step 1: Add `Serialize`/`Deserialize` to `StackId` in `src/types/stack.rs`**

Find the `StackId` line and add the derives:
```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub struct StackId(pub u64);
```

- [ ] **Step 2: Add `SpellFilter` and update `TargetFilter` in `src/types/ability.rs`**

After the `CastFilter` block (around line 147), add `SpellFilter`:
```rust
/// Describes which spells on the stack can be targeted (CR 115.4).
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct SpellFilter {
    /// Spell must have at least one of these types; empty = no constraint.
    pub included_types: Vec<CardType>,
    /// Spell must have none of these types.
    pub excluded_types: Vec<CardType>,
}

impl SpellFilter {
    pub fn any() -> Self {
        Self::default()
    }

    pub fn noncreature() -> Self {
        Self {
            included_types: vec![],
            excluded_types: vec![CardType::Creature],
        }
    }

    pub fn creature() -> Self {
        Self {
            included_types: vec![CardType::Creature],
            excluded_types: vec![],
        }
    }

    pub fn instant_or_sorcery() -> Self {
        Self {
            included_types: vec![CardType::Instant, CardType::Sorcery],
            excluded_types: vec![],
        }
    }

    pub fn matches(&self, _card_types: &[CardType]) -> bool {
        todo!("implemented in Task 2")
    }
}
```

Find the `TargetFilter` enum. Remove `Copy` from its derive and add the new variant:
```rust
/// Describes what kind of permanent, player, or spell can be targeted (CR 115.4).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TargetFilter {
    Creature,
    Player,
    Any, // CR 115.4: creature, player, planeswalker, battle
    Spell(SpellFilter), // CR 115.4: a spell on the stack
}
```

- [ ] **Step 3: Add `EffectTarget::StackObject` and `EffectStep::CounterSpell` in `src/types/effect.rs`**

Add `use super::stack::StackId;` to the imports at the top of the file.

Add `StackObject` to `EffectTarget`:
```rust
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum EffectTarget {
    Player { id: PlayerId },
    Object { id: ObjectId },
    StackObject { id: StackId },
}
```

Add `CounterSpell` to `EffectStep`:
```rust
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EffectStep {
    AddMana(ManaPool),
    Mill(u32),
    DrawCard(u32),
    GainLife(u32),
    BoostPermanentPT(PTDelta),
    DealDamage(u32),
    CounterSpell, // CR 701.5: counter the target spell on the stack
    Unimplemented(String),
}
```

- [ ] **Step 4: Fix `src/engine/targeting.rs` — signature changes and stub arms**

Change `is_legal_target` signature from `filter: TargetFilter` to `filter: &TargetFilter`.

In the `EffectTarget::Object` arm, add the new `Spell` variant to the `passes_filter` match:
```rust
let passes_filter = match filter {
    TargetFilter::Creature => obj.is_creature(),
    TargetFilter::Player => false,
    TargetFilter::Any => obj.is_creature(),
    TargetFilter::Spell(_) => false, // permanents are not spell targets
};
```

Add a stub `EffectTarget::StackObject` arm (full implementation in Task 3):
```rust
EffectTarget::StackObject { .. } => false, // implemented in Task 3
```

Change `legal_targets` signature from `filter: TargetFilter` to `filter: &TargetFilter`.

Add a stub `Spell` branch (full implementation in Task 4):
```rust
if matches!(filter, TargetFilter::Spell(_)) {
    // implemented in Task 4
}
```

In the `targets_still_legal` function, add a stub `StackObject` arm:
```rust
EffectTarget::StackObject { .. } => false, // implemented in Task 4
```

In the existing tests, update all call sites to pass `&TargetFilter::X` instead of `TargetFilter::X`, and `&TargetFilter::X` to `legal_targets`. For example:
```rust
// was: is_legal_target(&gs, &target, TargetFilter::Creature, PlayerId(0), &[],)
is_legal_target(&gs, &target, &TargetFilter::Creature, PlayerId(0), &[])
// was: legal_targets(&gs, TargetFilter::Any, PlayerId(0), &[])
legal_targets(&gs, &TargetFilter::Any, PlayerId(0), &[])
```
There are ~10 call sites in the test module; update all of them.

- [ ] **Step 5: Fix `src/engine/stack.rs` — stub `CounterSpell` arm, fix `DealDamage` match**

In `execute_effect_steps`, add a `CounterSpell` stub before `Unimplemented`:
```rust
EffectStep::CounterSpell => {} // implemented in Task 5
```

The `DealDamage` arm's inner match doesn't cover `EffectTarget::StackObject`. Change `None => {}` to a wildcard:
```rust
EffectStep::DealDamage(n) => match targets.first() {
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
    _ => {}
},
```

- [ ] **Step 6: Fix `src/engine/casting.rs` — remove `*filter` dereference**

In `cast_spell`, find the loop:
```rust
for (filter, target) in target_requirements.iter().zip(declared_targets.iter()) {
    if !is_legal_target(&state, target, *filter, player_id, &spell_colors) {
```
Change `*filter` to `filter` (it's already `&TargetFilter` from the iterator):
```rust
    if !is_legal_target(&state, target, filter, player_id, &spell_colors) {
```

- [ ] **Step 7: Fix `src/serve.rs` — remove `*filter`, `.copied()` → `.cloned()`, stub StackObject name**

In `compute_hand_actions`, change `.copied()` to `.cloned()`:
```rust
        .flatten()
        .cloned()
        .collect();
```

Change `*filter` to `filter` in the `legal_targets` call:
```rust
for target in legal_targets(state, filter, pid, &spell_colors) {
```

Add a stub `StackObject` arm to the `target_name` match (full implementation in Task 7):
```rust
EffectTarget::StackObject { .. } => String::new(),
```

- [ ] **Step 8: Verify compilation and existing tests still pass**

```bash
cargo test 2>&1 | grep -E "^test result|FAILED|error\["
```
Expected: all existing tests pass. The `counterspell_fully_unimplemented` parser test will fail — that's expected (fixed in Task 6). If any other test fails, fix before committing.

- [ ] **Step 9: Commit**

```bash
git add src/types/stack.rs src/types/ability.rs src/types/effect.rs \
        src/engine/targeting.rs src/engine/stack.rs src/engine/casting.rs \
        src/serve.rs
git commit -m "feat: add SpellFilter, TargetFilter::Spell, EffectTarget::StackObject, EffectStep::CounterSpell — types and compile stubs"
```

---

## Task 2: `SpellFilter::matches` — TDD

**Files:**
- Modify: `src/types/ability.rs`

- [ ] **Step 1: Write failing tests**

In the `#[cfg(test)]` block at the bottom of `src/types/ability.rs`, add:
```rust
#[test]
fn spell_filter_any_matches_all_types() {
    use super::super::card::CardType;
    let f = SpellFilter::any();
    assert!(f.matches(&[CardType::Creature]));
    assert!(f.matches(&[CardType::Instant]));
    assert!(f.matches(&[CardType::Sorcery]));
    assert!(f.matches(&[]));
}

#[test]
fn spell_filter_noncreature_excludes_creature_spells() {
    use super::super::card::CardType;
    let f = SpellFilter::noncreature();
    assert!(!f.matches(&[CardType::Creature]));
    assert!(f.matches(&[CardType::Instant]));
    assert!(f.matches(&[CardType::Sorcery]));
    // A multitype card with Creature is still excluded
    assert!(!f.matches(&[CardType::Creature, CardType::Artifact]));
}

#[test]
fn spell_filter_creature_includes_creature_only() {
    use super::super::card::CardType;
    let f = SpellFilter::creature();
    assert!(f.matches(&[CardType::Creature]));
    assert!(!f.matches(&[CardType::Instant]));
    assert!(!f.matches(&[CardType::Sorcery]));
}

#[test]
fn spell_filter_instant_or_sorcery_matches_either() {
    use super::super::card::CardType;
    let f = SpellFilter::instant_or_sorcery();
    assert!(f.matches(&[CardType::Instant]));
    assert!(f.matches(&[CardType::Sorcery]));
    assert!(!f.matches(&[CardType::Creature]));
    assert!(!f.matches(&[]));
}
```

- [ ] **Step 2: Run tests to confirm they fail**

```bash
cargo test spell_filter 2>&1 | grep -E "^test result|FAILED|error\["
```
Expected: tests panic with "not yet implemented".

- [ ] **Step 3: Implement `SpellFilter::matches`**

Replace the `todo!()` stub in `SpellFilter::matches`:
```rust
pub fn matches(&self, card_types: &[CardType]) -> bool {
    let included_ok = self.included_types.is_empty()
        || self.included_types.iter().any(|t| card_types.contains(t));
    let excluded_ok = self
        .excluded_types
        .iter()
        .all(|t| !card_types.contains(t));
    included_ok && excluded_ok
}
```

- [ ] **Step 4: Run tests to confirm they pass**

```bash
cargo test spell_filter 2>&1 | grep -E "^test result|FAILED|error\["
```
Expected: 4 tests pass.

- [ ] **Step 5: Commit**

```bash
git add src/types/ability.rs
git commit -m "feat: implement SpellFilter::matches — included/excluded CardType logic"
```

---

## Task 3: `targeting.rs` — `is_legal_target` for `StackObject`

**Files:**
- Modify: `src/engine/targeting.rs`

- [ ] **Step 1: Write failing tests**

In the `#[cfg(test)]` block of `src/engine/targeting.rs`, add a helper and tests. The helper needs access to `StackObject` and `StackPayload`:

```rust
use crate::types::stack::{StackId, StackObject, StackPayload};

fn push_instant_on_stack(
    state: &mut GameState,
    owner: PlayerId,
    card_types: Vec<crate::types::card::CardType>,
) -> (crate::types::ObjectId, StackId) {
    use crate::types::card::{CardDefinition, TypeLine};
    use crate::types::{CardObject, Zone};
    let def = CardDefinition {
        name: "Stack Spell".into(),
        mana_cost: None,
        type_line: TypeLine {
            supertypes: vec![],
            card_types,
            subtypes: vec![],
        },
        oracle_text: String::new(),
        abilities: vec![],
        text_annotations: vec![],
        power: None,
        toughness: None,
        colors: vec![],
    };
    let card_id = state.alloc_id();
    let obj = CardObject::new(card_id, def, owner, Zone::Stack);
    state.add_object(obj);
    let stack_id = state.alloc_stack_id();
    let sobj = StackObject {
        id: stack_id,
        payload: StackPayload::Spell { card_id },
        controller: owner,
        targets: vec![],
    };
    state.stack.push(stack_id);
    state.stack_objects.insert(stack_id, sobj);
    (card_id, stack_id)
}

#[test]
fn spell_on_stack_is_legal_for_spell_any() {
    use crate::types::ability::SpellFilter;
    use crate::types::card::CardType;
    let mut gs = two_player_state();
    let (_, sid) = push_instant_on_stack(&mut gs, PlayerId(0), vec![CardType::Creature]);
    let target = EffectTarget::StackObject { id: sid };
    assert!(is_legal_target(
        &gs,
        &target,
        &TargetFilter::Spell(SpellFilter::any()),
        PlayerId(1),
        &[],
    ));
}

#[test]
fn creature_spell_not_legal_for_noncreature_filter() {
    use crate::types::ability::SpellFilter;
    use crate::types::card::CardType;
    let mut gs = two_player_state();
    let (_, sid) = push_instant_on_stack(&mut gs, PlayerId(0), vec![CardType::Creature]);
    let target = EffectTarget::StackObject { id: sid };
    assert!(!is_legal_target(
        &gs,
        &target,
        &TargetFilter::Spell(SpellFilter::noncreature()),
        PlayerId(1),
        &[],
    ));
}

#[test]
fn instant_spell_legal_for_noncreature_filter() {
    use crate::types::ability::SpellFilter;
    use crate::types::card::CardType;
    let mut gs = two_player_state();
    let (_, sid) = push_instant_on_stack(&mut gs, PlayerId(0), vec![CardType::Instant]);
    let target = EffectTarget::StackObject { id: sid };
    assert!(is_legal_target(
        &gs,
        &target,
        &TargetFilter::Spell(SpellFilter::noncreature()),
        PlayerId(1),
        &[],
    ));
}

#[test]
fn triggered_ability_not_legal_spell_target() {
    use crate::types::ability::SpellFilter;
    use crate::types::ObjectId;
    let mut gs = two_player_state();
    let sid = gs.alloc_stack_id();
    gs.stack.push(sid);
    gs.stack_objects.insert(
        sid,
        StackObject {
            id: sid,
            payload: StackPayload::TriggeredAbility {
                source_id: ObjectId(99),
                effect: vec![],
                label: "test".into(),
            },
            controller: PlayerId(0),
            targets: vec![],
        },
    );
    let target = EffectTarget::StackObject { id: sid };
    assert!(!is_legal_target(
        &gs,
        &target,
        &TargetFilter::Spell(SpellFilter::any()),
        PlayerId(1),
        &[],
    ));
}

#[test]
fn battlefield_object_not_legal_for_spell_filter() {
    use crate::types::ability::SpellFilter;
    let mut gs = two_player_state();
    let id = place_creature(&mut gs, PlayerId(1), vec![]);
    let target = EffectTarget::Object { id };
    assert!(!is_legal_target(
        &gs,
        &target,
        &TargetFilter::Spell(SpellFilter::any()),
        PlayerId(0),
        &[],
    ));
}
```

- [ ] **Step 2: Run tests to confirm they fail**

```bash
cargo test "spell_on_stack_is_legal\|creature_spell_not_legal\|instant_spell_legal\|triggered_ability_not_legal\|battlefield_object_not_legal" 2>&1 | grep -E "^test result|FAILED|error\["
```
Expected: 5 tests fail (currently returns `false` from the stub).

- [ ] **Step 3: Implement `EffectTarget::StackObject` arm in `is_legal_target`**

Replace the stub arm:
```rust
EffectTarget::StackObject { id } => {
    // CR 115.4: a spell on the stack is a legal target for TargetFilter::Spell
    // if it exists and its card types satisfy the spell filter.
    // CR 702.11a/702.18a: shroud/hexproof protect permanents, not spells on the stack.
    if let TargetFilter::Spell(spell_filter) = filter {
        let Some(sobj) = state.stack_objects.get(id) else {
            return false;
        };
        let StackPayload::Spell { card_id } = &sobj.payload else {
            return false; // triggered/activated abilities are not spells
        };
        let card_types = state
            .objects
            .get(card_id)
            .map(|o| o.definition.type_line.card_types.as_slice())
            .unwrap_or(&[]);
        spell_filter.matches(card_types)
    } else {
        false
    }
}
```

- [ ] **Step 4: Run tests to confirm they pass**

```bash
cargo test "spell_on_stack_is_legal\|creature_spell_not_legal\|instant_spell_legal\|triggered_ability_not_legal\|battlefield_object_not_legal" 2>&1 | grep -E "^test result|FAILED|error\["
```
Expected: all 5 pass.

- [ ] **Step 5: Run full suite to confirm no regressions**

```bash
cargo test 2>&1 | grep -E "^test result|FAILED|error\["
```

- [ ] **Step 6: Commit**

```bash
git add src/engine/targeting.rs
git commit -m "feat: is_legal_target handles EffectTarget::StackObject for SpellFilter"
```

---

## Task 4: `targeting.rs` — `legal_targets` and `targets_still_legal` for stack objects

**Files:**
- Modify: `src/engine/targeting.rs`

- [ ] **Step 1: Write failing tests**

Add to the `#[cfg(test)]` block of `src/engine/targeting.rs` (after Task 3 tests):

```rust
#[test]
fn legal_targets_spell_any_returns_matching_stack_spells() {
    use crate::types::ability::SpellFilter;
    use crate::types::card::CardType;
    let mut gs = two_player_state();
    let (_, sid) = push_instant_on_stack(&mut gs, PlayerId(0), vec![CardType::Creature]);
    let targets = legal_targets(&gs, &TargetFilter::Spell(SpellFilter::any()), PlayerId(1), &[]);
    assert_eq!(targets, vec![EffectTarget::StackObject { id: sid }]);
}

#[test]
fn legal_targets_spell_filter_excludes_noncreature() {
    use crate::types::ability::SpellFilter;
    use crate::types::card::CardType;
    let mut gs = two_player_state();
    let (_, _sid) = push_instant_on_stack(&mut gs, PlayerId(0), vec![CardType::Creature]);
    let targets = legal_targets(
        &gs,
        &TargetFilter::Spell(SpellFilter::noncreature()),
        PlayerId(1),
        &[],
    );
    assert!(targets.is_empty());
}

#[test]
fn legal_targets_spell_filter_excludes_triggered_abilities() {
    use crate::types::ability::SpellFilter;
    use crate::types::ObjectId;
    let mut gs = two_player_state();
    let sid = gs.alloc_stack_id();
    gs.stack.push(sid);
    gs.stack_objects.insert(
        sid,
        StackObject {
            id: sid,
            payload: StackPayload::TriggeredAbility {
                source_id: ObjectId(99),
                effect: vec![],
                label: "test".into(),
            },
            controller: PlayerId(0),
            targets: vec![],
        },
    );
    let targets = legal_targets(&gs, &TargetFilter::Spell(SpellFilter::any()), PlayerId(1), &[]);
    assert!(targets.is_empty());
}

#[test]
fn targets_still_legal_true_for_spell_on_stack() {
    use crate::types::card::CardType;
    let mut gs = two_player_state();
    let (_, sid) = push_instant_on_stack(&mut gs, PlayerId(0), vec![CardType::Instant]);
    let targets = vec![EffectTarget::StackObject { id: sid }];
    assert!(targets_still_legal(&gs, &targets));
}

#[test]
fn targets_still_legal_false_after_spell_countered() {
    use crate::engine::stack::counter_spell_on_stack;
    use crate::types::card::CardType;
    let mut gs = two_player_state();
    let (_, sid) = push_instant_on_stack(&mut gs, PlayerId(0), vec![CardType::Instant]);
    let targets = vec![EffectTarget::StackObject { id: sid }];
    counter_spell_on_stack(&mut gs, sid);
    assert!(!targets_still_legal(&gs, &targets));
}
```

- [ ] **Step 2: Run tests to confirm they fail**

```bash
cargo test "legal_targets_spell\|targets_still_legal_true_for_spell\|targets_still_legal_false_after" 2>&1 | grep -E "^test result|FAILED|error\["
```
Expected: all 5 fail.

- [ ] **Step 3: Implement `legal_targets` Spell branch**

Replace the stub Spell branch in `legal_targets`:
```rust
if matches!(filter, TargetFilter::Spell(_)) {
    for &id in &state.stack {
        let t = EffectTarget::StackObject { id };
        if is_legal_target(state, &t, filter, caster, source_colors) {
            result.push(t);
        }
    }
}
```

- [ ] **Step 4: Implement `targets_still_legal` StackObject arm**

Replace the stub arm:
```rust
EffectTarget::StackObject { id } => state
    .stack_objects
    .get(id)
    .map(|o| matches!(o.payload, StackPayload::Spell { .. }))
    .unwrap_or(false),
```

- [ ] **Step 5: Run tests to confirm they pass**

```bash
cargo test "legal_targets_spell\|targets_still_legal_true_for_spell\|targets_still_legal_false_after" 2>&1 | grep -E "^test result|FAILED|error\["
```

- [ ] **Step 6: Run full suite**

```bash
cargo test 2>&1 | grep -E "^test result|FAILED|error\["
```

- [ ] **Step 7: Commit**

```bash
git add src/engine/targeting.rs
git commit -m "feat: legal_targets and targets_still_legal handle Spell filter and StackObject"
```

---

## Task 5: `stack.rs` — `CounterSpell` effect step

**Files:**
- Modify: `src/engine/stack.rs`

- [ ] **Step 1: Write failing test**

In the `#[cfg(test)]` block of `src/engine/stack.rs`, add:

```rust
#[test]
fn counter_spell_step_counters_targeted_stack_spell() {
    use crate::types::ability::{SpellAbility, SpellFilter, TargetFilter};
    use crate::types::card::CardType;
    use crate::types::effect::{EffectStep, EffectTarget};
    use crate::types::mana::ManaColor;

    let mut gs = make_state();

    // Put a target creature spell on the stack (player 1's Bears).
    let target_def = CardDefinition {
        name: "Bears".into(),
        mana_cost: None,
        type_line: TypeLine {
            supertypes: vec![],
            card_types: vec![CardType::Creature],
            subtypes: vec![],
        },
        oracle_text: String::new(),
        abilities: vec![],
        text_annotations: vec![],
        power: Some(2),
        toughness: Some(2),
        colors: vec![],
    };
    let bears_card_id = gs.alloc_id();
    let bears_obj = CardObject::new(bears_card_id, target_def, PlayerId(1), Zone::Stack);
    gs.add_object(bears_obj);
    let bears_sid = gs.alloc_stack_id();
    gs.stack.push(bears_sid);
    gs.stack_objects.insert(
        bears_sid,
        StackObject {
            id: bears_sid,
            payload: StackPayload::Spell { card_id: bears_card_id },
            controller: PlayerId(1),
            targets: vec![],
        },
    );

    // Put a Counterspell on the stack above Bears, targeting Bears.
    let counter_def = CardDefinition {
        name: "Counterspell".into(),
        mana_cost: Some(ManaCost { pips: vec![ManaPip::Blue, ManaPip::Blue] }),
        type_line: TypeLine {
            supertypes: vec![],
            card_types: vec![CardType::Instant],
            subtypes: vec![],
        },
        oracle_text: "Counter target spell.".into(),
        abilities: vec![OracleSpan::Parsed(Ability::SpellEffect(SpellAbility {
            target_requirements: vec![TargetFilter::Spell(SpellFilter::any())],
            steps: vec![EffectStep::CounterSpell],
        }))],
        text_annotations: vec![],
        power: None,
        toughness: None,
        colors: vec![ManaColor::Blue],
    };
    let counter_card_id = gs.alloc_id();
    let counter_obj = CardObject::new(counter_card_id, counter_def, PlayerId(0), Zone::Stack);
    gs.add_object(counter_obj);
    let counter_sid = gs.alloc_stack_id();
    gs.stack.push(counter_sid);
    gs.stack_objects.insert(
        counter_sid,
        StackObject {
            id: counter_sid,
            payload: StackPayload::Spell { card_id: counter_card_id },
            controller: PlayerId(0),
            targets: vec![EffectTarget::StackObject { id: bears_sid }],
        },
    );

    // Resolve Counterspell (top of stack).
    let gs = resolve_top(gs);

    // Bears countered: removed from stack, card in player 1's graveyard.
    assert!(!gs.stack.contains(&bears_sid));
    assert!(!gs.stack_objects.contains_key(&bears_sid));
    assert_eq!(gs.objects[&bears_card_id].zone, Zone::Graveyard);
    assert!(gs.graveyards[&PlayerId(1)].contains(&bears_card_id));
    // Counterspell itself resolved to player 0's graveyard.
    assert_eq!(gs.objects[&counter_card_id].zone, Zone::Graveyard);
    assert!(gs.graveyards[&PlayerId(0)].contains(&counter_card_id));
    assert!(gs.stack.is_empty());
}
```

- [ ] **Step 2: Run test to confirm it fails**

```bash
cargo test counter_spell_step_counters_targeted_stack_spell 2>&1 | grep -E "^test result|FAILED|error\["
```
Expected: FAILED (stub does nothing).

- [ ] **Step 3: Implement `CounterSpell` arm in `execute_effect_steps`**

Replace the stub in `execute_effect_steps`:
```rust
// CR 701.5: move the targeted stack object to the graveyard (if a spell)
// or simply remove it (if an ability). counter_spell_on_stack handles both.
EffectStep::CounterSpell => {
    if let Some(EffectTarget::StackObject { id }) = targets.first() {
        counter_spell_on_stack(&mut state, *id);
    }
}
```

- [ ] **Step 4: Run test to confirm it passes**

```bash
cargo test counter_spell_step_counters_targeted_stack_spell 2>&1 | grep -E "^test result|FAILED|error\["
```

- [ ] **Step 5: Run full suite**

```bash
cargo test 2>&1 | grep -E "^test result|FAILED|error\["
```

- [ ] **Step 6: Commit**

```bash
git add src/engine/stack.rs
git commit -m "feat: CounterSpell effect step delegates to counter_spell_on_stack (CR 701.5)"
```

---

## Task 6: Parser — counter-spell patterns

**Files:**
- Modify: `src/parser/oracle.rs`

- [ ] **Step 1: Update the existing failing test and add new ones**

Find `counterspell_fully_unimplemented` in the `#[cfg(test)]` block and rename + rewrite it:
```rust
#[test]
fn counterspell_parses_to_counter_any_spell() {
    use crate::types::ability::{SpellFilter, TargetFilter};
    use crate::types::effect::EffectStep;
    let result = parse_spell("Counter target spell.", "");
    assert_eq!(result.len(), 1);
    let OracleSpan::Parsed(Ability::SpellEffect(sa)) = &result[0] else {
        panic!("expected SpellEffect, got {:?}", result[0]);
    };
    assert_eq!(
        sa.target_requirements,
        vec![TargetFilter::Spell(SpellFilter::any())]
    );
    assert_eq!(sa.steps, vec![EffectStep::CounterSpell]);
}

#[test]
fn negate_parses_to_counter_noncreature_spell() {
    use crate::types::ability::{SpellFilter, TargetFilter};
    use crate::types::effect::EffectStep;
    let result = parse_spell("Counter target noncreature spell.", "");
    let OracleSpan::Parsed(Ability::SpellEffect(sa)) = &result[0] else {
        panic!("expected SpellEffect");
    };
    assert_eq!(
        sa.target_requirements,
        vec![TargetFilter::Spell(SpellFilter::noncreature())]
    );
    assert_eq!(sa.steps, vec![EffectStep::CounterSpell]);
}

#[test]
fn essence_scatter_parses_to_counter_creature_spell() {
    use crate::types::ability::{SpellFilter, TargetFilter};
    use crate::types::effect::EffectStep;
    let result = parse_spell("Counter target creature spell.", "");
    let OracleSpan::Parsed(Ability::SpellEffect(sa)) = &result[0] else {
        panic!("expected SpellEffect");
    };
    assert_eq!(
        sa.target_requirements,
        vec![TargetFilter::Spell(SpellFilter::creature())]
    );
    assert_eq!(sa.steps, vec![EffectStep::CounterSpell]);
}

#[test]
fn dispel_parses_to_counter_instant_or_sorcery_spell() {
    use crate::types::ability::{SpellFilter, TargetFilter};
    use crate::types::effect::EffectStep;
    let result = parse_spell("Counter target instant or sorcery spell.", "");
    let OracleSpan::Parsed(Ability::SpellEffect(sa)) = &result[0] else {
        panic!("expected SpellEffect");
    };
    assert_eq!(
        sa.target_requirements,
        vec![TargetFilter::Spell(SpellFilter::instant_or_sorcery())]
    );
    assert_eq!(sa.steps, vec![EffectStep::CounterSpell]);
}
```

- [ ] **Step 2: Run tests to confirm they fail**

```bash
cargo test "counterspell_parses\|negate_parses\|essence_scatter_parses\|dispel_parses" 2>&1 | grep -E "^test result|FAILED|error\["
```
Expected: all 4 fail.

- [ ] **Step 3: Implement counter patterns in `parse_spell_paragraph`**

In `parse_spell_paragraph` in `src/parser/oracle.rs`, add the following block **before** the "no targeting pattern found" return at the end of the function. Insert after the last existing pattern block (after the `// Pattern B` block):

```rust
// Counter patterns — "counter target [type] spell."
// These are complete sentences; no suffix needs stripping.
{
    use crate::types::ability::SpellFilter;
    use crate::types::effect::EffectStep;
    // lc has trailing periods stripped (see trim_end_matches above), so
    // patterns here are written without a trailing period.
    let counter_patterns: &[(&str, SpellFilter)] = &[
        ("counter target instant or sorcery spell", SpellFilter::instant_or_sorcery()),
        ("counter target noncreature spell", SpellFilter::noncreature()),
        ("counter target creature spell", SpellFilter::creature()),
        ("counter target spell", SpellFilter::any()),
    ];
    for (pattern, filter) in counter_patterns {
        if lc == *pattern {
            return SpellAbility {
                target_requirements: vec![TargetFilter::Spell(filter.clone())],
                steps: vec![EffectStep::CounterSpell],
            };
        }
    }
}
```

Note: "instant or sorcery" must come before the plain "spell" entry since both end in "spell." — the more specific patterns are checked first.

- [ ] **Step 4: Run tests to confirm they pass**

```bash
cargo test "counterspell_parses\|negate_parses\|essence_scatter_parses\|dispel_parses" 2>&1 | grep -E "^test result|FAILED|error\["
```

- [ ] **Step 5: Run full suite**

```bash
cargo test 2>&1 | grep -E "^test result|FAILED|error\["
```

- [ ] **Step 6: Commit**

```bash
git add src/parser/oracle.rs
git commit -m "feat: parser recognises counter-target-spell patterns (CR 701.5)"
```

---

## Task 7: `serve.rs` — `StackObject` name resolution

**Files:**
- Modify: `src/serve.rs`

- [ ] **Step 1: Implement `StackObject` arm in `target_name` match**

In `compute_hand_actions`, find the `target_name` match block (the stub `EffectTarget::StackObject { .. } => String::new()` added in Task 1) and replace it:

```rust
EffectTarget::StackObject { id } => state
    .stack_objects
    .get(id)
    .and_then(|obj| {
        if let StackPayload::Spell { card_id } = &obj.payload {
            state
                .objects
                .get(card_id)
                .map(|c| c.definition.name.clone())
        } else {
            None
        }
    })
    .unwrap_or_default(),
```

- [ ] **Step 2: Verify compilation and run full suite**

```bash
cargo test 2>&1 | grep -E "^test result|FAILED|error\["
```
Expected: all tests pass.

- [ ] **Step 3: Commit**

```bash
git add src/serve.rs
git commit -m "feat: serve.rs resolves EffectTarget::StackObject to spell name for action labels"
```

---

## Task 8: Integration tests — full cast → counter flow

**Files:**
- Modify: `src/engine/casting.rs` (test module)

- [ ] **Step 1: Add helper definitions and four integration tests**

At the bottom of the `#[cfg(test)]` block in `src/engine/casting.rs`, add:

```rust
fn make_counterspell_def() -> CardDefinition {
    use crate::types::ability::{SpellAbility, SpellFilter};
    use crate::types::mana::ManaColor;
    CardDefinition {
        name: "Counterspell".into(),
        mana_cost: Some(ManaCost {
            pips: vec![ManaPip::Blue, ManaPip::Blue],
        }),
        type_line: TypeLine {
            supertypes: vec![],
            card_types: vec![CardType::Instant],
            subtypes: vec![],
        },
        oracle_text: "Counter target spell.".into(),
        abilities: vec![OracleSpan::Parsed(Ability::SpellEffect(SpellAbility {
            target_requirements: vec![TargetFilter::Spell(SpellFilter::any())],
            steps: vec![EffectStep::CounterSpell],
        }))],
        text_annotations: vec![],
        power: None,
        toughness: None,
        colors: vec![ManaColor::Blue],
    }
}

fn make_negate_def() -> CardDefinition {
    use crate::types::ability::{SpellAbility, SpellFilter};
    use crate::types::mana::ManaColor;
    CardDefinition {
        name: "Negate".into(),
        mana_cost: Some(ManaCost {
            pips: vec![ManaPip::Generic(1), ManaPip::Blue],
        }),
        type_line: TypeLine {
            supertypes: vec![],
            card_types: vec![CardType::Instant],
            subtypes: vec![],
        },
        oracle_text: "Counter target noncreature spell.".into(),
        abilities: vec![OracleSpan::Parsed(Ability::SpellEffect(SpellAbility {
            target_requirements: vec![TargetFilter::Spell(SpellFilter::noncreature())],
            steps: vec![EffectStep::CounterSpell],
        }))],
        text_annotations: vec![],
        power: None,
        toughness: None,
        colors: vec![ManaColor::Blue],
    }
}

/// P0 casts Grizzly Bears; P0 passes; P1 counters with Counterspell; both pass → Bears countered.
#[test]
fn counterspell_counters_opponent_creature_spell() {
    use crate::engine::stack::pass_priority;
    use crate::types::effect::EffectTarget;

    let db = test_db();
    let mut gs = make_state();

    gs.get_player_mut(PlayerId(0)).unwrap().mana_pool.green += 2;
    let bears_id = put_in_hand(&mut gs, PlayerId(0), db.get("Grizzly Bears").unwrap().clone());
    let gs = cast_spell(gs, PlayerId(0), bears_id, vec![]).unwrap();
    let bears_sid = gs.stack[0];

    let mut gs = pass_priority(gs, PlayerId(0)).unwrap();

    gs.get_player_mut(PlayerId(1)).unwrap().mana_pool.blue += 2;
    let counter_id = put_in_hand(&mut gs, PlayerId(1), make_counterspell_def());
    let gs = cast_spell(
        gs,
        PlayerId(1),
        counter_id,
        vec![EffectTarget::StackObject { id: bears_sid }],
    )
    .unwrap();

    // P1 retains priority; P1 passes, P0 passes → resolve Counterspell.
    let gs = pass_priority(gs, PlayerId(1)).unwrap();
    let gs = pass_priority(gs, PlayerId(0)).unwrap();

    assert!(!gs.battlefield.contains_key(&bears_id));
    assert_eq!(gs.objects[&bears_id].zone, Zone::Graveyard);
    assert!(gs.graveyards[&PlayerId(0)].contains(&bears_id));
    assert_eq!(gs.objects[&counter_id].zone, Zone::Graveyard);
    assert!(gs.stack.is_empty());
}

/// P0 casts an instant (noncreature); P0 passes; P1 counters with Negate; both pass → instant countered.
#[test]
fn negate_counters_opponent_noncreature_spell() {
    use crate::engine::stack::pass_priority;
    use crate::types::ability::SpellAbility;
    use crate::types::effect::{EffectStep, EffectTarget};
    use crate::types::mana::ManaColor;

    let mut gs = make_state();

    let draw_def = CardDefinition {
        name: "Opt".into(),
        mana_cost: Some(ManaCost {
            pips: vec![ManaPip::Blue],
        }),
        type_line: TypeLine {
            supertypes: vec![],
            card_types: vec![CardType::Instant],
            subtypes: vec![],
        },
        oracle_text: "Draw a card.".into(),
        abilities: vec![OracleSpan::Parsed(Ability::SpellEffect(SpellAbility {
            target_requirements: vec![],
            steps: vec![EffectStep::DrawCard(1)],
        }))],
        text_annotations: vec![],
        power: None,
        toughness: None,
        colors: vec![ManaColor::Blue],
    };
    gs.get_player_mut(PlayerId(0)).unwrap().mana_pool.blue += 1;
    let draw_id = put_in_hand(&mut gs, PlayerId(0), draw_def);
    let gs = cast_spell(gs, PlayerId(0), draw_id, vec![]).unwrap();
    let draw_sid = gs.stack[0];

    let mut gs = pass_priority(gs, PlayerId(0)).unwrap();

    gs.get_player_mut(PlayerId(1)).unwrap().mana_pool.blue += 2;
    let negate_id = put_in_hand(&mut gs, PlayerId(1), make_negate_def());
    let gs = cast_spell(
        gs,
        PlayerId(1),
        negate_id,
        vec![EffectTarget::StackObject { id: draw_sid }],
    )
    .unwrap();

    let gs = pass_priority(gs, PlayerId(1)).unwrap();
    let gs = pass_priority(gs, PlayerId(0)).unwrap();

    assert_eq!(gs.objects[&draw_id].zone, Zone::Graveyard);
    assert!(gs.graveyards[&PlayerId(0)].contains(&draw_id));
    assert!(gs.stack.is_empty());
}

/// P1 tries to Negate P0's Grizzly Bears (a creature spell) → IllegalTarget.
#[test]
fn negate_cannot_target_creature_spell() {
    use crate::engine::stack::pass_priority;
    use crate::types::effect::EffectTarget;

    let db = test_db();
    let mut gs = make_state();

    gs.get_player_mut(PlayerId(0)).unwrap().mana_pool.green += 2;
    let bears_id = put_in_hand(&mut gs, PlayerId(0), db.get("Grizzly Bears").unwrap().clone());
    let gs = cast_spell(gs, PlayerId(0), bears_id, vec![]).unwrap();
    let bears_sid = gs.stack[0];

    let mut gs = pass_priority(gs, PlayerId(0)).unwrap();

    gs.get_player_mut(PlayerId(1)).unwrap().mana_pool.blue += 2;
    let negate_id = put_in_hand(&mut gs, PlayerId(1), make_negate_def());
    let result = cast_spell(
        gs,
        PlayerId(1),
        negate_id,
        vec![EffectTarget::StackObject { id: bears_sid }],
    );

    assert!(matches!(result, Err(EngineError::IllegalTarget)));
}

/// P0's Counterspell fizzles when its Bears target is already countered before resolution.
#[test]
fn counterspell_fizzles_when_target_already_gone() {
    use crate::engine::stack::{counter_spell_on_stack, pass_priority};
    use crate::types::effect::EffectTarget;

    let db = test_db();
    let mut gs = make_state();

    gs.get_player_mut(PlayerId(0)).unwrap().mana_pool.green += 2;
    let bears_id = put_in_hand(&mut gs, PlayerId(0), db.get("Grizzly Bears").unwrap().clone());
    let gs = cast_spell(gs, PlayerId(0), bears_id, vec![]).unwrap();
    let bears_sid = gs.stack[0];

    let mut gs = pass_priority(gs, PlayerId(0)).unwrap();

    gs.get_player_mut(PlayerId(1)).unwrap().mana_pool.blue += 2;
    let counter_id = put_in_hand(&mut gs, PlayerId(1), make_counterspell_def());
    let gs = cast_spell(
        gs,
        PlayerId(1),
        counter_id,
        vec![EffectTarget::StackObject { id: bears_sid }],
    )
    .unwrap();

    // P1 passes, then Bears are removed from the stack before resolution.
    let mut gs = pass_priority(gs, PlayerId(1)).unwrap();
    counter_spell_on_stack(&mut gs, bears_sid);

    // P0 passes → Counterspell resolves but fizzles (target gone).
    let gs = pass_priority(gs, PlayerId(0)).unwrap();

    // Counterspell fizzled to graveyard without effect.
    assert_eq!(gs.objects[&counter_id].zone, Zone::Graveyard);
    assert!(gs.graveyards[&PlayerId(1)].contains(&counter_id));
    // Bears already countered.
    assert_eq!(gs.objects[&bears_id].zone, Zone::Graveyard);
    assert!(gs.stack.is_empty());
}
```

- [ ] **Step 2: Run the four integration tests**

```bash
cargo test "counterspell_counters_opponent\|negate_counters_opponent\|negate_cannot_target\|counterspell_fizzles" 2>&1 | grep -E "^test result|FAILED|error\["
```
Expected: all 4 pass.

- [ ] **Step 3: Run full suite**

```bash
cargo test 2>&1 | grep -E "^test result|FAILED|error\["
```

- [ ] **Step 4: Commit**

```bash
git add src/engine/casting.rs
git commit -m "test: integration tests for counterspell, negate, fizzle (CR 701.5, CR 608.2b)"
```

---

## Task 9: `docs/todo.md` notes + lint pass

**Files:**
- Modify: `docs/todo.md`
- Possibly modify: any file flagged by clippy

- [ ] **Step 1: Add conditional counter notes to `docs/todo.md`**

Add the following section after the `## ✅ Unblocked — implementable now` section (or at the end of the Gameplay issues block):

```markdown
## Conditional counter spells

Cards like Mana Leak ({1}{U}), Quench ({U}{U}), Syncopate ({X}{U}), and Condescend require
"counter target spell unless its controller pays {N}" semantics. This requires:

- A payment obligation directed at the targeted spell's controller (not the counterspell caster) — similar to Ward, but triggered at resolution rather than at targeting time.
- Extend `StackPayload` with a `ConditionalCounter` variant (analogous to `WardTrigger`) that sits on the stack when the countered spell's controller still has a window to pay.
- Or model as a two-step resolution: the counterspell resolves into a triggered ability ("unless paid, counter that spell") using the existing cost/payment infrastructure in `engine/costs.rs`.
- See CR 116.2b (players may take actions during cost-payment windows) and the Ward implementation in `engine/triggered.rs` and `engine/costs.rs` for the pattern to follow.
```

- [ ] **Step 2: Run clippy auto-fix**

```bash
cargo clippy --fix --all-targets 2>&1 | grep -E "^error|^warning"
```

- [ ] **Step 3: Run clippy to check for remaining warnings**

```bash
cargo clippy --all-targets 2>&1 | grep -E "^error|^warning"
```
Expected: clean output.

- [ ] **Step 4: Run full test suite one final time**

```bash
cargo test 2>&1 | grep -E "^test result|FAILED|error\["
```
Expected: all tests pass.

- [ ] **Step 5: Commit**

```bash
git add docs/todo.md
# Add any clippy-fixed files too
git commit -m "docs: conditional counter spell notes in todo.md; clippy clean"
```
