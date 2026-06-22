# Attachment System Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Implement a full attachment system for Auras and Equipment including cast-time targeting, equip activated ability, continuous-effect grants to the host, and SBAs for illegal attachments.

**Architecture:** Add `attached_to: Option<ObjectId>` to `PermanentState` as the single source of truth for attachment. Add `Rule::Aura` and `Rule::Equip` rule variants that bundle the attachment constraint with the grants effect. Extend `continuous_pt_bonus` to scan for attached sources, add three SBA cases, create a new `engine/equip.rs` for the equip action, extend `engine/casting.rs` and `engine/stack.rs` for aura cast flow, and extend `serve.rs` for the API.

**Tech Stack:** Rust, existing `mecha_oracle` crate, no new dependencies.

## Global Constraints

- All CR references must be verified against `docs/CR.txt` before adding to code comments: `grep '^NNN\.MM' docs/CR.txt`
- Run `cargo test 2>&1 | grep -E "^test result|FAILED|error\["` after every task
- Run `cargo clippy --all-targets` before each commit; run `cargo clippy --fix` first if needed
- Never use `unwrap()` on `Option` without a comment explaining why it is safe
- Keep `PermanentState::new` initialising all new fields to their zero/None/empty default

---

### Task 1: Core type additions

**Files:**
- Modify: `src/types/permanent.rs`
- Modify: `src/types/ability.rs`
- Modify: `src/types/effect.rs`

**Interfaces:**
- Produces:
  - `PermanentState.attached_to: Option<ObjectId>` — initialized to `None` in `new()`
  - `PermanentFilter.object_ids: Vec<ObjectId>` — initialized to `vec![]` in `Default`
  - `Rule::Aura { enchant: TargetFilter, grants: ContinuousEffect }` — new rule variant
  - `Rule::Equip { cost: Cost, grants: ContinuousEffect }` — new rule variant
  - `EffectStep::Attach { source_id: ObjectId }` — new effect step

- [ ] **Step 1: Add `attached_to` to `PermanentState`**

In `src/types/permanent.rs`, add the import and field:

```rust
// At the top, `ObjectId` is already imported via `super::ids::ObjectId` — verify it is.
// In PermanentState struct, after `pub counters: HashMap<CounterKind, u32>,` add:
/// CR 303.4b / 301.5b: which permanent this is currently attached to (aura or equipment).
pub attached_to: Option<ObjectId>,
```

In `PermanentState::new`, add to the initialiser body (after `counters: HashMap::new(),`):
```rust
attached_to: None,
```

- [ ] **Step 2: Write a test for the new field**

In the `#[cfg(test)]` block at the bottom of `src/types/permanent.rs`, add:

```rust
#[test]
fn attached_to_initialises_to_none() {
    let perm = grizzly_bears_perm();
    assert!(perm.attached_to.is_none());
}
```

- [ ] **Step 3: Run the test to verify it passes**

```bash
cargo test -p mecha-oracle permanent::tests::attached_to_initialises_to_none 2>&1 | grep -E "^test result|FAILED|error\["
```
Expected: `test result: ok. 1 passed`

- [ ] **Step 4: Add `object_ids` to `PermanentFilter`**

In `src/types/ability.rs`, in the `PermanentFilter` struct, add after `pub colors: Vec<ManaColor>,`:

```rust
/// If non-empty, only match permanents whose ObjectId is in this list.
/// Used for effects targeting a specific object. Empty = no ID constraint.
pub object_ids: Vec<ObjectId>,
```

In `PermanentFilter`'s `Default` impl, add:
```rust
object_ids: vec![],
```

Note: `ObjectId` must be in scope. `ability.rs` already imports from `super::ids::ObjectId` — verify at the top of the file.

- [ ] **Step 5: Write a test for `object_ids`**

In the `#[cfg(test)]` block in `src/types/ability.rs`, add:

```rust
#[test]
fn permanent_filter_default_has_empty_object_ids() {
    let f = PermanentFilter::default();
    assert!(f.object_ids.is_empty());
}
```

- [ ] **Step 6: Run the test to verify it passes**

```bash
cargo test -p mecha-oracle ability::tests::permanent_filter_default_has_empty_object_ids 2>&1 | grep -E "^test result|FAILED|error\["
```
Expected: `test result: ok. 1 passed`

- [ ] **Step 7: Add `Rule::Aura` and `Rule::Equip` variants**

In `src/types/ability.rs`, in the `Rule` enum, add after `Continuous(ContinuousEffect),`:

```rust
// CR 303.4: an Aura enchants the object matching `enchant`.
// `enchant` is the target requirement at cast time and for SBA legality checks.
// `grants` is applied to the attached permanent while on the battlefield.
Aura {
    enchant: TargetFilter,
    grants: ContinuousEffect,
},
// CR 301.5: an Equipment with an Equip activated ability.
// `cost` is paid at sorcery speed to attach/re-attach.
// `grants` is applied to the equipped creature.
Equip {
    cost: Cost,
    grants: ContinuousEffect,
},
```

- [ ] **Step 8: Write tests for the new Rule variants**

In the `#[cfg(test)]` block in `src/types/ability.rs`, add:

```rust
#[test]
fn rule_aura_construction() {
    use crate::types::card::CardType;
    use crate::types::permanent::PTDelta;
    let rule = Rule::Aura {
        enchant: TargetFilter::Creature,
        grants: ContinuousEffect {
            subject_filter: PermanentFilter::default(),
            pt_modification: Some(PTDelta { power: 2, toughness: 1 }),
        },
    };
    assert!(matches!(rule, Rule::Aura { .. }));
}

#[test]
fn rule_equip_construction() {
    use crate::types::mana::{ManaCost, ManaPip};
    use crate::types::permanent::PTDelta;
    let rule = Rule::Equip {
        cost: vec![CostComponent::Mana(ManaCost { pips: vec![ManaPip::Generic(1)] })],
        grants: ContinuousEffect {
            subject_filter: PermanentFilter::default(),
            pt_modification: Some(PTDelta { power: 2, toughness: 0 }),
        },
    };
    assert!(matches!(rule, Rule::Equip { .. }));
}
```

- [ ] **Step 9: Add `EffectStep::Attach`**

In `src/types/effect.rs`, add `ObjectId` to the imports if not already there (check: `use super::ids::{ObjectId, PlayerId};`).

In the `EffectStep` enum, add after `Unimplemented(String),`:

```rust
// CR 301.5d: attaches the source equipment to the first target of the stack object.
// `source_id` is the equipment's ObjectId, captured at activation time.
Attach { source_id: ObjectId },
```

- [ ] **Step 10: Write a test for `EffectStep::Attach`**

In the `#[cfg(test)]` block in `src/types/effect.rs`, add:

```rust
#[test]
fn attach_step_construction() {
    use crate::types::ids::ObjectId;
    let step = EffectStep::Attach { source_id: ObjectId(5) };
    assert!(matches!(step, EffectStep::Attach { source_id: ObjectId(5) }));
}
```

- [ ] **Step 11: Run all tests to verify nothing broke**

```bash
cargo test 2>&1 | grep -E "^test result|FAILED|error\["
```
Expected: all test results `ok`, no `FAILED`.

- [ ] **Step 12: Clippy and commit**

```bash
cargo clippy --fix --all-targets 2>&1 | grep -E "^error|^warning"
cargo clippy --all-targets 2>&1 | grep -E "^error|^warning"
git add src/types/permanent.rs src/types/ability.rs src/types/effect.rs
git commit -m "feat: add attached_to, PermanentFilter.object_ids, Rule::Aura/Equip, EffectStep::Attach"
```

---

### Task 2: Continuous effect evaluation for attachments

**Files:**
- Modify: `src/engine/mod.rs`

**Interfaces:**
- Consumes: `PermanentState.attached_to` (Task 1), `Rule::Aura { grants }`, `Rule::Equip { grants }` (Task 1)
- Produces: `continuous_pt_bonus` correctly sums grants from attached sources; `PermanentFilter.object_ids` respected in existing broadcast loop

- [ ] **Step 1: Write failing tests**

In `src/engine/mod.rs`, in the existing `#[cfg(test)]` block, add:

```rust
fn make_attachment_def(
    card_type: CardType,
    rule: Rule,
    owner: PlayerId,
) -> CardDefinition {
    CardDefinition {
        name: "Test Attachment".into(),
        mana_cost: None,
        type_line: TypeLine {
            supertypes: vec![],
            card_types: vec![card_type],
            subtypes: vec![],
        },
        oracle_text: String::new(),
        rules_text: vec![RulesText::Active(rule)],
        text_annotations: vec![],
        power: None,
        toughness: None,
        colors: vec![],
    }
}

#[test]
fn equipment_grants_pt_to_attached_creature() {
    // Bonesplitter: +2/+0 to attached creature.
    use crate::types::{ContinuousEffect, PermanentFilter, Rule, RulesText,
        ability::{CostComponent, TargetFilter}, mana::{ManaCost, ManaPip}};

    let mut gs = two_player_state();

    // Target creature: a 2/2 Bear
    let bear_id = add_permanent(&mut gs, PlayerId(0), test_db().get("Grizzly Bears").unwrap().clone(), Zone::Battlefield);

    // Equipment: +2/+0
    let equip_def = make_attachment_def(
        CardType::Artifact,
        Rule::Equip {
            cost: vec![CostComponent::Mana(ManaCost { pips: vec![ManaPip::Generic(1)] })],
            grants: ContinuousEffect {
                subject_filter: PermanentFilter::default(),
                pt_modification: Some(PTDelta { power: 2, toughness: 0 }),
            },
        },
        PlayerId(0),
    );
    let equip_id = add_permanent(&mut gs, PlayerId(0), equip_def, Zone::Battlefield);

    // Attach
    gs.battlefield.get_mut(&equip_id).unwrap().attached_to = Some(bear_id);

    let bonus = continuous_pt_bonus(&gs, bear_id);
    assert_eq!(bonus.power, 2);
    assert_eq!(bonus.toughness, 0);
}

#[test]
fn aura_grants_pt_to_enchanted_creature() {
    // Unholy Strength: +2/+1 to enchanted creature.
    use crate::types::{ContinuousEffect, PermanentFilter, Rule, RulesText,
        ability::TargetFilter};

    let mut gs = two_player_state();

    let bear_id = add_permanent(&mut gs, PlayerId(0), test_db().get("Grizzly Bears").unwrap().clone(), Zone::Battlefield);

    let aura_def = make_attachment_def(
        CardType::Enchantment,
        Rule::Aura {
            enchant: TargetFilter::Creature,
            grants: ContinuousEffect {
                subject_filter: PermanentFilter::default(),
                pt_modification: Some(PTDelta { power: 2, toughness: 1 }),
            },
        },
        PlayerId(0),
    );
    let aura_id = add_permanent(&mut gs, PlayerId(0), aura_def, Zone::Battlefield);
    gs.battlefield.get_mut(&aura_id).unwrap().attached_to = Some(bear_id);

    let bonus = continuous_pt_bonus(&gs, bear_id);
    assert_eq!(bonus.power, 2);
    assert_eq!(bonus.toughness, 1);
}

#[test]
fn detached_equipment_does_not_grant_pt() {
    use crate::types::{ContinuousEffect, PermanentFilter, Rule, RulesText,
        ability::{CostComponent, TargetFilter}, mana::{ManaCost, ManaPip}};

    let mut gs = two_player_state();
    let bear_id = add_permanent(&mut gs, PlayerId(0), test_db().get("Grizzly Bears").unwrap().clone(), Zone::Battlefield);
    let equip_def = make_attachment_def(
        CardType::Artifact,
        Rule::Equip {
            cost: vec![CostComponent::Mana(ManaCost { pips: vec![ManaPip::Generic(1)] })],
            grants: ContinuousEffect {
                subject_filter: PermanentFilter::default(),
                pt_modification: Some(PTDelta { power: 2, toughness: 0 }),
            },
        },
        PlayerId(0),
    );
    let equip_id = add_permanent(&mut gs, PlayerId(0), equip_def, Zone::Battlefield);
    // Not attached — attached_to remains None

    let bonus = continuous_pt_bonus(&gs, bear_id);
    assert_eq!(bonus.power, 0);
    assert_eq!(bonus.toughness, 0);
}

#[test]
fn object_ids_filter_restricts_to_specific_id() {
    // A Rule::Continuous with object_ids = [bear_id] should only apply to bear_id,
    // not to another creature on the battlefield.
    use crate::types::{ContinuousEffect, ControllerFilter, PermanentFilter, Rule, RulesText};

    let mut gs = two_player_state();
    let bear_id = add_permanent(&mut gs, PlayerId(0), test_db().get("Grizzly Bears").unwrap().clone(), Zone::Battlefield);
    let giant_id = add_permanent(&mut gs, PlayerId(0), test_db().get("Hill Giant").unwrap().clone(), Zone::Battlefield);

    // A "continuous" source on the battlefield whose filter targets only bear_id.
    let source_def = CardDefinition {
        name: "Targeted Boost".into(),
        mana_cost: None,
        type_line: TypeLine {
            supertypes: vec![],
            card_types: vec![CardType::Enchantment],
            subtypes: vec![],
        },
        oracle_text: String::new(),
        rules_text: vec![RulesText::Active(Rule::Continuous(ContinuousEffect {
            subject_filter: PermanentFilter {
                object_ids: vec![bear_id],
                ..Default::default()
            },
            pt_modification: Some(PTDelta { power: 3, toughness: 0 }),
        }))],
        text_annotations: vec![],
        power: None,
        toughness: None,
        colors: vec![],
    };
    add_permanent(&mut gs, PlayerId(0), source_def, Zone::Battlefield);

    // Bear gets +3/+0; Hill Giant gets nothing.
    assert_eq!(continuous_pt_bonus(&gs, bear_id).power, 3);
    assert_eq!(continuous_pt_bonus(&gs, giant_id).power, 0);
}
```

- [ ] **Step 2: Run tests to verify they fail**

```bash
cargo test -p mecha-oracle engine::tests::equipment_grants_pt_to_attached_creature 2>&1 | grep -E "^test result|FAILED|error\["
```
Expected: `FAILED` (the new loop hasn't been written yet)

- [ ] **Step 3: Extend `continuous_pt_bonus` in `src/engine/mod.rs`**

In the existing broadcast loop (after checking `controller_ok`, `type_ok`, `subtype_ok`, `color_ok`), add an `object_ids` check. Replace the four-check block:

```rust
let controller_ok = match filter.controller {
    ControllerFilter::Any => true,
    ControllerFilter::You => src_obj.controller == target_controller,
    ControllerFilter::Opponent => src_obj.controller != target_controller,
};
if !controller_ok {
    continue;
}

let type_ok = filter.card_types.is_empty()
    || filter.card_types.iter().any(|t| target_types.contains(t));
if !type_ok {
    continue;
}

let subtype_ok = filter.subtypes.is_empty()
    || filter.subtypes.iter().all(|s| target_subtypes.contains(s));
if !subtype_ok {
    continue;
}

let color_ok = filter.colors.is_empty()
    || filter.colors.iter().any(|c| target_colors.contains(c));
if !color_ok {
    continue;
}
```

with:

```rust
// If object_ids is non-empty, only match if the target is one of those IDs.
if !filter.object_ids.is_empty() && !filter.object_ids.contains(&target_id) {
    continue;
}

let controller_ok = match filter.controller {
    ControllerFilter::Any => true,
    ControllerFilter::You => src_obj.controller == target_controller,
    ControllerFilter::Opponent => src_obj.controller != target_controller,
};
if !controller_ok {
    continue;
}

let type_ok = filter.card_types.is_empty()
    || filter.card_types.iter().any(|t| target_types.contains(t));
if !type_ok {
    continue;
}

let subtype_ok = filter.subtypes.is_empty()
    || filter.subtypes.iter().all(|s| target_subtypes.contains(s));
if !subtype_ok {
    continue;
}

let color_ok = filter.colors.is_empty()
    || filter.colors.iter().any(|c| target_colors.contains(c));
if !color_ok {
    continue;
}
```

Then, after the closing `}` of the existing `for (&src_id, src_perm) in &state.battlefield` loop (just before `bonus`), add the attachment pass:

```rust
// Second pass: attached sources (Rule::Aura and Rule::Equip).
// CR 611.1: a continuous effect from an attached permanent applies to its host.
for (&src_id, src_perm) in &state.battlefield {
    if src_perm.attached_to != Some(target_id) {
        continue;
    }
    for span in &src_perm.definition.rules_text {
        let grants = match span {
            RulesText::Active(Rule::Aura { grants, .. }) => grants,
            RulesText::Active(Rule::Equip { grants, .. }) => grants,
            _ => continue,
        };
        if let Some(delta) = grants.pt_modification {
            bonus.power += delta.power;
            bonus.toughness += delta.toughness;
        }
    }
}
```

- [ ] **Step 4: Run all tests to verify they pass**

```bash
cargo test 2>&1 | grep -E "^test result|FAILED|error\["
```
Expected: all `ok`, no `FAILED`.

- [ ] **Step 5: Clippy and commit**

```bash
cargo clippy --fix --all-targets 2>&1 | grep -E "^error|^warning"
cargo clippy --all-targets 2>&1 | grep -E "^error|^warning"
git add src/engine/mod.rs
git commit -m "feat: extend continuous_pt_bonus to evaluate attachment grants and object_ids filter"
```

---

### Task 3: Attachment SBAs

**Files:**
- Modify: `src/engine/state_based_actions.rs`

**Interfaces:**
- Consumes: `PermanentState.attached_to` (Task 1), `Rule::Aura { enchant }`, `Rule::Equip` (Task 1), `targeting::is_legal_target` (already exists)
- Produces:
  - `Sba::AuraToGraveyard(ObjectId)` — fired when an Aura has no legal attachment
  - `Sba::DetachEquipment(ObjectId)` — fired when Equipment is attached to a non-creature

- [ ] **Step 1: Write failing tests**

In the `#[cfg(test)]` block in `src/engine/state_based_actions.rs`, add:

```rust
fn make_aura(
    enchant: crate::types::ability::TargetFilter,
) -> crate::types::CardDefinition {
    use crate::types::{
        CardDefinition, CardType, ContinuousEffect, PermanentFilter, Rule, RulesText, TypeLine,
        ability::TargetFilter,
    };
    use crate::types::permanent::PTDelta;
    CardDefinition {
        name: "Test Aura".into(),
        mana_cost: None,
        type_line: TypeLine {
            supertypes: vec![],
            card_types: vec![CardType::Enchantment],
            subtypes: vec!["Aura".into()],
        },
        oracle_text: String::new(),
        rules_text: vec![RulesText::Active(Rule::Aura {
            enchant,
            grants: ContinuousEffect {
                subject_filter: PermanentFilter::default(),
                pt_modification: Some(PTDelta { power: 1, toughness: 0 }),
            },
        })],
        text_annotations: vec![],
        power: None,
        toughness: None,
        colors: vec![],
    }
}

fn make_equipment() -> crate::types::CardDefinition {
    use crate::types::{
        CardDefinition, CardType, ContinuousEffect, PermanentFilter, Rule, RulesText, TypeLine,
    };
    use crate::types::ability::CostComponent;
    use crate::types::mana::{ManaCost, ManaPip};
    use crate::types::permanent::PTDelta;
    CardDefinition {
        name: "Test Equipment".into(),
        mana_cost: None,
        type_line: TypeLine {
            supertypes: vec![],
            card_types: vec![CardType::Artifact],
            subtypes: vec!["Equipment".into()],
        },
        oracle_text: String::new(),
        rules_text: vec![RulesText::Active(Rule::Equip {
            cost: vec![CostComponent::Mana(ManaCost { pips: vec![ManaPip::Generic(1)] })],
            grants: ContinuousEffect {
                subject_filter: PermanentFilter::default(),
                pt_modification: Some(PTDelta { power: 2, toughness: 0 }),
            },
        })],
        text_annotations: vec![],
        power: None,
        toughness: None,
        colors: vec![],
    }
}

#[test]
fn aura_not_attached_to_anything_goes_to_graveyard() {
    // CR 704.5m: Aura on battlefield with attached_to = None → graveyard.
    use crate::types::ability::TargetFilter;
    let mut gs = make_state();
    let aura_id = add_creature_to_battlefield(&mut gs, PlayerId(0), make_aura(TargetFilter::Creature));
    // attached_to is None by default (not attached to anything)
    assert!(gs.battlefield[&aura_id].attached_to.is_none());

    let (gs, _) = check_and_apply_sbas(gs);

    assert!(!gs.battlefield.contains_key(&aura_id), "aura should have gone to graveyard");
    assert!(gs.graveyards[&PlayerId(0)].contains(&aura_id));
}

#[test]
fn aura_attached_to_nonexistent_permanent_goes_to_graveyard() {
    // CR 704.5n: host has left the battlefield.
    use crate::types::ability::TargetFilter;
    let mut gs = make_state();
    let aura_id = add_creature_to_battlefield(&mut gs, PlayerId(0), make_aura(TargetFilter::Creature));
    let phantom_id = ObjectId(999); // does not exist on the battlefield
    gs.battlefield.get_mut(&aura_id).unwrap().attached_to = Some(phantom_id);

    let (gs, _) = check_and_apply_sbas(gs);

    assert!(!gs.battlefield.contains_key(&aura_id));
    assert!(gs.graveyards[&PlayerId(0)].contains(&aura_id));
}

#[test]
fn aura_attached_to_valid_creature_survives() {
    use crate::types::ability::TargetFilter;
    let db = test_db();
    let mut gs = make_state();
    let bear_id = add_creature_to_battlefield(&mut gs, PlayerId(0), db.get("Grizzly Bears").unwrap().clone());
    let aura_id = add_creature_to_battlefield(&mut gs, PlayerId(0), make_aura(TargetFilter::Creature));
    gs.battlefield.get_mut(&aura_id).unwrap().attached_to = Some(bear_id);

    let (gs, _) = check_and_apply_sbas(gs);

    assert!(gs.battlefield.contains_key(&aura_id), "aura on a legal creature should survive");
}

#[test]
fn equipment_attached_to_non_creature_becomes_detached() {
    // CR 704.5r: Equipment attached to something that's no longer a creature → unattach, stays on bf.
    let db = test_db();
    let mut gs = make_state();
    let land_id = {
        let id = gs.alloc_id();
        let obj = crate::types::CardObject::new(id, db.get("Forest").unwrap().clone(), PlayerId(0), crate::types::Zone::Battlefield);
        let perm = PermanentState::new(&obj.definition);
        gs.battlefield.insert(id, perm);
        gs.add_object(obj);
        id
    };
    let equip_id = {
        let id = gs.alloc_id();
        let obj = crate::types::CardObject::new(id, make_equipment(), PlayerId(0), crate::types::Zone::Battlefield);
        let perm = PermanentState::new(&obj.definition);
        gs.battlefield.insert(id, perm);
        gs.add_object(obj);
        id
    };
    gs.battlefield.get_mut(&equip_id).unwrap().attached_to = Some(land_id);

    let (gs, _) = check_and_apply_sbas(gs);

    assert!(gs.battlefield.contains_key(&equip_id), "equipment stays on battlefield");
    assert!(
        gs.battlefield[&equip_id].attached_to.is_none(),
        "equipment is detached"
    );
}

#[test]
fn equipment_attached_to_creature_stays_attached() {
    let db = test_db();
    let mut gs = make_state();
    let bear_id = add_creature_to_battlefield(&mut gs, PlayerId(0), db.get("Grizzly Bears").unwrap().clone());
    let equip_id = {
        let id = gs.alloc_id();
        let obj = crate::types::CardObject::new(id, make_equipment(), PlayerId(0), crate::types::Zone::Battlefield);
        let perm = PermanentState::new(&obj.definition);
        gs.battlefield.insert(id, perm);
        gs.add_object(obj);
        id
    };
    gs.battlefield.get_mut(&equip_id).unwrap().attached_to = Some(bear_id);

    let (gs, _) = check_and_apply_sbas(gs);

    assert_eq!(
        gs.battlefield[&equip_id].attached_to,
        Some(bear_id),
        "equipment should stay attached to a creature"
    );
}
```

- [ ] **Step 2: Run tests to verify they fail**

```bash
cargo test -p mecha-oracle state_based_actions::tests::aura_not_attached_to_anything_goes_to_graveyard 2>&1 | grep -E "^test result|FAILED|error\["
```
Expected: `FAILED`

- [ ] **Step 3: Add new SBA variants and implement `find_sbas` / `apply_sbas`**

In `src/engine/state_based_actions.rs`, extend the `Sba` enum:

```rust
#[derive(Debug, Clone)]
enum Sba {
    PlayerLoses(PlayerId),
    MoveToGraveyard(ObjectId),
    CancelCounters(ObjectId, u32),
    AuraToGraveyard(ObjectId),  // CR 704.5m / 704.5n
    DetachEquipment(ObjectId),  // CR 704.5r
}
```

In `find_sbas`, after the existing counter-cancellation block, add:

```rust
use crate::types::ability::{Rule, RulesText};

// CR 704.5m: Aura on battlefield not attached to anything → graveyard.
// CR 704.5n: Aura attached to something that no longer satisfies the enchant restriction → graveyard.
for (&id, perm) in &state.battlefield {
    let enchant = perm.definition.rules_text.iter().find_map(|span| {
        if let RulesText::Active(Rule::Aura { enchant, .. }) = span {
            Some(enchant.clone())
        } else {
            None
        }
    });
    if let Some(enchant) = enchant {
        let should_die = match perm.attached_to {
            None => true, // 704.5m: not attached
            Some(host_id) => {
                // 704.5n: host no longer satisfies enchant restriction
                let target =
                    crate::types::effect::EffectTarget::Object { id: host_id };
                let controller = state
                    .objects
                    .get(&id)
                    .map(|o| o.controller)
                    .unwrap_or(PlayerId(0));
                let colors = state
                    .objects
                    .get(&id)
                    .map(|o| o.definition.colors.clone())
                    .unwrap_or_default();
                !crate::engine::targeting::is_legal_target(
                    state, &target, &enchant, controller, &colors,
                )
            }
        };
        if should_die {
            sbas.push(Sba::AuraToGraveyard(id));
        }
    }
}

// CR 704.5r: Equipment attached to a permanent that isn't a creature → detach.
for (&id, perm) in &state.battlefield {
    let has_equip = perm
        .definition
        .rules_text
        .iter()
        .any(|span| matches!(span, RulesText::Active(Rule::Equip { .. })));
    if has_equip {
        if let Some(host_id) = perm.attached_to {
            let host_is_creature = state
                .objects
                .get(&host_id)
                .map(|o| o.definition.type_line.is_creature())
                .unwrap_or(false);
            let host_on_battlefield = state.battlefield.contains_key(&host_id);
            if !host_on_battlefield || !host_is_creature {
                sbas.push(Sba::DetachEquipment(id));
            }
        }
    }
}
```

In `apply_sbas`, add the two new arms:

```rust
Sba::AuraToGraveyard(id) => {
    state = move_to_graveyard(state, id);
}
Sba::DetachEquipment(id) => {
    if let Some(perm) = state.battlefield.get_mut(&id) {
        perm.attached_to = None;
    }
}
```

- [ ] **Step 4: Run all tests to verify they pass**

```bash
cargo test 2>&1 | grep -E "^test result|FAILED|error\["
```
Expected: all `ok`, no `FAILED`.

- [ ] **Step 5: Clippy and commit**

```bash
cargo clippy --fix --all-targets 2>&1 | grep -E "^error|^warning"
cargo clippy --all-targets 2>&1 | grep -E "^error|^warning"
git add src/engine/state_based_actions.rs
git commit -m "feat: add attachment SBAs — aura-to-graveyard (704.5m/n) and detach-equipment (704.5r)"
```

---

### Task 4: Equip engine

**Files:**
- Create: `src/engine/equip.rs`
- Modify: `src/engine/mod.rs` (add `pub mod equip;` and `EffectStep::Attach` handler in `execute_effect_steps`)

**Interfaces:**
- Consumes: `PermanentState.attached_to` (Task 1), `Rule::Equip { cost }` (Task 1), `EffectStep::Attach { source_id }` (Task 1), `can_pay_cost_components`, `pay_cost_components` (both already exist in `engine/costs.rs`)
- Produces:
  - `engine::equip::activate_equip(state, equipment_id, target_creature_id, player_id) -> Result<GameState, EngineError>`
  - `EffectStep::Attach` handled in `execute_effect_steps` in `src/engine/stack.rs`

- [ ] **Step 1: Write failing tests for `activate_equip`**

Create `src/engine/equip.rs` with just the test module first (implementation will follow):

```rust
use super::EngineError;
use crate::engine::costs::{can_pay_cost_components, pay_cost_components};
use crate::types::ability::{Cost, Rule, RulesText};
use crate::types::effect::{EffectStep, EffectTarget};
use crate::types::stack::{StackObject, StackPayload};
use crate::types::{GameState, ObjectId, PlayerId, Step, Zone};

pub fn activate_equip(
    _state: GameState,
    _equipment_id: ObjectId,
    _target_creature_id: ObjectId,
    _player_id: PlayerId,
) -> Result<GameState, EngineError> {
    unimplemented!()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cards::test_helpers::test_db;
    use crate::types::{
        CardDefinition, CardObject, CardType, ContinuousEffect, PermanentFilter, PermanentState,
        Player, Rule, RulesText, TypeLine, Zone,
    };
    use crate::types::ability::{CostComponent, TargetFilter};
    use crate::types::mana::{ManaCost, ManaPip};
    use crate::types::permanent::PTDelta;

    fn two_player_state() -> GameState {
        GameState::new(vec![
            Player::new(PlayerId(0), "Alice"),
            Player::new(PlayerId(1), "Bob"),
        ])
    }

    fn make_bonesplitter() -> CardDefinition {
        CardDefinition {
            name: "Bonesplitter".into(),
            mana_cost: Some(ManaCost { pips: vec![ManaPip::Generic(1)] }),
            type_line: TypeLine {
                supertypes: vec![],
                card_types: vec![CardType::Artifact],
                subtypes: vec!["Equipment".into()],
            },
            oracle_text: "Equipped creature gets +2/+0. Equip {1}".into(),
            rules_text: vec![crate::types::RulesText::Active(Rule::Equip {
                cost: vec![CostComponent::Mana(ManaCost { pips: vec![ManaPip::Generic(1)] })],
                grants: ContinuousEffect {
                    subject_filter: PermanentFilter::default(),
                    pt_modification: Some(PTDelta { power: 2, toughness: 0 }),
                },
            })],
            text_annotations: vec![],
            power: None,
            toughness: None,
            colors: vec![],
        }
    }

    fn place_on_battlefield(state: &mut GameState, def: CardDefinition, owner: PlayerId) -> ObjectId {
        let id = state.alloc_id();
        let obj = CardObject::new(id, def, owner, Zone::Battlefield);
        let mut perm = PermanentState::new(&obj.definition);
        perm.controller_since_turn = 0;
        state.battlefield.insert(id, perm);
        state.add_object(obj);
        id
    }

    fn setup_equip_state() -> (GameState, ObjectId, ObjectId) {
        let mut gs = two_player_state();
        gs.step = Step::PreCombatMain;
        let db = test_db();
        let bear_id = place_on_battlefield(&mut gs, db.get("Grizzly Bears").unwrap().clone(), PlayerId(0));
        let equip_id = place_on_battlefield(&mut gs, make_bonesplitter(), PlayerId(0));
        gs.get_player_mut(PlayerId(0)).unwrap().mana_pool.colorless = 1;
        (gs, equip_id, bear_id)
    }

    #[test]
    fn equip_puts_ability_on_stack() {
        let (gs, equip_id, bear_id) = setup_equip_state();
        let gs = activate_equip(gs, equip_id, bear_id, PlayerId(0)).unwrap();
        assert_eq!(gs.stack.len(), 1);
    }

    #[test]
    fn equip_resolution_sets_attached_to() {
        use crate::engine::stack::resolve_top;
        let (gs, equip_id, bear_id) = setup_equip_state();
        let gs = activate_equip(gs, equip_id, bear_id, PlayerId(0)).unwrap();
        let gs = resolve_top(gs);
        assert_eq!(gs.battlefield[&equip_id].attached_to, Some(bear_id));
    }

    #[test]
    fn equip_deducts_mana_cost() {
        let (gs, equip_id, bear_id) = setup_equip_state();
        let gs = activate_equip(gs, equip_id, bear_id, PlayerId(0)).unwrap();
        assert_eq!(gs.get_player(PlayerId(0)).unwrap().mana_pool.colorless, 0);
    }

    #[test]
    fn equip_fails_not_your_priority() {
        let (mut gs, equip_id, bear_id) = setup_equip_state();
        gs.priority_player = PlayerId(1);
        assert!(matches!(
            activate_equip(gs, equip_id, bear_id, PlayerId(0)),
            Err(EngineError::NotYourPriority)
        ));
    }

    #[test]
    fn equip_fails_not_main_phase() {
        let (mut gs, equip_id, bear_id) = setup_equip_state();
        gs.step = Step::Combat;
        assert!(matches!(
            activate_equip(gs, equip_id, bear_id, PlayerId(0)),
            Err(EngineError::CannotCastNow)
        ));
    }

    #[test]
    fn equip_fails_stack_not_empty() {
        use crate::types::stack::{StackId, StackObject, StackPayload};
        let (mut gs, equip_id, bear_id) = setup_equip_state();
        // Push a dummy stack object
        let sid = gs.alloc_stack_id();
        gs.stack.push(sid);
        gs.stack_objects.insert(sid, StackObject {
            id: sid,
            payload: StackPayload::ActivatedAbility {
                source_id: equip_id,
                effect: vec![],
                label: "dummy".into(),
            },
            controller: PlayerId(0),
            targets: vec![],
            x_value: None,
        });
        assert!(matches!(
            activate_equip(gs, equip_id, bear_id, PlayerId(0)),
            Err(EngineError::CannotCastNow)
        ));
    }

    #[test]
    fn equip_fails_target_is_not_creature() {
        let mut gs = two_player_state();
        gs.step = Step::PreCombatMain;
        let db = test_db();
        let land_id = {
            let id = gs.alloc_id();
            let obj = CardObject::new(id, db.get("Forest").unwrap().clone(), PlayerId(0), Zone::Battlefield);
            let perm = PermanentState::new(&obj.definition);
            gs.battlefield.insert(id, perm);
            gs.add_object(obj);
            id
        };
        let equip_id = place_on_battlefield(&mut gs, make_bonesplitter(), PlayerId(0));
        gs.get_player_mut(PlayerId(0)).unwrap().mana_pool.colorless = 1;
        assert!(matches!(
            activate_equip(gs, equip_id, land_id, PlayerId(0)),
            Err(EngineError::NotACreature)
        ));
    }

    #[test]
    fn equip_reattach_moves_attachment() {
        // Equip to bear A, then equip to bear B — attached_to should update.
        use crate::engine::stack::resolve_top;
        let mut gs = two_player_state();
        gs.step = Step::PreCombatMain;
        let db = test_db();
        let bear_a = place_on_battlefield(&mut gs, db.get("Grizzly Bears").unwrap().clone(), PlayerId(0));
        let bear_b = place_on_battlefield(&mut gs, db.get("Grizzly Bears").unwrap().clone(), PlayerId(0));
        let equip_id = place_on_battlefield(&mut gs, make_bonesplitter(), PlayerId(0));

        // First equip to bear_a
        gs.battlefield.get_mut(&equip_id).unwrap().attached_to = Some(bear_a);

        // Re-equip to bear_b
        gs.get_player_mut(PlayerId(0)).unwrap().mana_pool.colorless = 1;
        let gs = activate_equip(gs, equip_id, bear_b, PlayerId(0)).unwrap();
        let gs = resolve_top(gs);
        assert_eq!(gs.battlefield[&equip_id].attached_to, Some(bear_b));
    }
}
```

- [ ] **Step 2: Run tests to verify they fail (compilation error expected since `unimplemented!()`)**

First add the module declaration. In `src/engine/mod.rs`, add:
```rust
pub mod equip;
```

Then run:
```bash
cargo test -p mecha-oracle engine::equip::tests::equip_puts_ability_on_stack 2>&1 | grep -E "^test result|FAILED|error\[|panicked"
```
Expected: `panicked at 'not yet implemented'`

- [ ] **Step 3: Implement `activate_equip`**

Replace the `unimplemented!()` stub in `src/engine/equip.rs` with the full implementation:

```rust
pub fn activate_equip(
    mut state: GameState,
    equipment_id: ObjectId,
    target_creature_id: ObjectId,
    player_id: PlayerId,
) -> Result<GameState, EngineError> {
    if state.priority_player != player_id {
        return Err(EngineError::NotYourPriority);
    }
    // CR 301.5d: equip only as a sorcery
    if state.active_player != player_id {
        return Err(EngineError::CannotCastNow);
    }
    if !matches!(state.step(), Step::PreCombatMain | Step::PostCombatMain) {
        return Err(EngineError::CannotCastNow);
    }
    if !state.stack.is_empty() {
        return Err(EngineError::CannotCastNow);
    }
    {
        let obj = state.objects.get(&equipment_id).ok_or(EngineError::CardNotFound)?;
        if obj.zone != Zone::Battlefield {
            return Err(EngineError::CardNotOnBattlefield);
        }
        if obj.controller != player_id {
            return Err(EngineError::NotYourCard);
        }
    }
    let cost: Cost = state
        .objects
        .get(&equipment_id)
        .and_then(|obj| {
            obj.definition.rules_text.iter().find_map(|span| {
                if let RulesText::Active(Rule::Equip { cost, .. }) = span {
                    Some(cost.clone())
                } else {
                    None
                }
            })
        })
        .ok_or(EngineError::AbilityIndexOutOfRange)?;
    {
        let target_obj = state
            .objects
            .get(&target_creature_id)
            .ok_or(EngineError::CardNotFound)?;
        if target_obj.zone != Zone::Battlefield {
            return Err(EngineError::CardNotOnBattlefield);
        }
        if target_obj.controller != player_id {
            return Err(EngineError::NotYourCard);
        }
        if !target_obj.definition.type_line.is_creature() {
            return Err(EngineError::NotACreature);
        }
    }
    if !can_pay_cost_components(&state, player_id, Some(equipment_id), &cost) {
        return Err(EngineError::InsufficientMana);
    }
    state = pay_cost_components(state, player_id, &cost, None)?;
    let stack_id = state.alloc_stack_id();
    let label = state
        .objects
        .get(&equipment_id)
        .map(|o| format!("Equip — {}", o.definition.name))
        .unwrap_or_else(|| "Equip".into());
    let stack_obj = StackObject {
        id: stack_id,
        payload: StackPayload::ActivatedAbility {
            source_id: equipment_id,
            effect: vec![EffectStep::Attach { source_id: equipment_id }],
            label,
        },
        controller: player_id,
        targets: vec![EffectTarget::Object { id: target_creature_id }],
        x_value: None,
    };
    state.stack.push(stack_id);
    state.stack_objects.insert(stack_id, stack_obj);
    state.consecutive_passes = 0;
    state.priority_player = player_id;
    Ok(state)
}
```

- [ ] **Step 4: Add `EffectStep::Attach` handler in `src/engine/stack.rs`**

In `execute_effect_steps`, add a new arm to the match statement, after `EffectStep::Unimplemented(_) => {}`:

```rust
EffectStep::Attach { source_id } => {
    // CR 301.5d: attach the equipment (source_id) to the first target.
    // Both source and target must still be on the battlefield (LKI — CR 608.2b).
    if let Some(EffectTarget::Object { id: target_id }) = targets.first() {
        let target_id = *target_id;
        if state.battlefield.contains_key(source_id)
            && state.battlefield.contains_key(&target_id)
        {
            if let Some(perm) = state.battlefield.get_mut(source_id) {
                perm.attached_to = Some(target_id);
            }
        }
    }
}
```

- [ ] **Step 5: Run all tests to verify they pass**

```bash
cargo test 2>&1 | grep -E "^test result|FAILED|error\["
```
Expected: all `ok`, no `FAILED`.

- [ ] **Step 6: Clippy and commit**

```bash
cargo clippy --fix --all-targets 2>&1 | grep -E "^error|^warning"
cargo clippy --all-targets 2>&1 | grep -E "^error|^warning"
git add src/engine/equip.rs src/engine/mod.rs src/engine/stack.rs
git commit -m "feat: add equip engine — activate_equip, EffectStep::Attach resolution"
```

---

### Task 5: Aura casting flow

**Files:**
- Modify: `src/engine/casting.rs`
- Modify: `src/engine/stack.rs`

**Interfaces:**
- Consumes: `Rule::Aura { enchant, .. }` (Task 1), `PermanentState.attached_to` (Task 1), `targeting::is_legal_target` and `targets_still_legal` (already exist)
- Produces:
  - `cast_spell` validates aura target using `Rule::Aura.enchant` (not `Rule::SpellAbility`)
  - `resolve_top` sets `attached_to` after a permanent aura enters the battlefield

- [ ] **Step 1: Write failing tests**

In `src/engine/casting.rs`, in the existing `#[cfg(test)]` block, add:

```rust
fn make_unholy_strength() -> CardDefinition {
    use crate::types::{
        ContinuousEffect, PermanentFilter, Rule, RulesText, ability::TargetFilter,
    };
    use crate::types::mana::ManaPip;
    use crate::types::permanent::PTDelta;
    CardDefinition {
        name: "Unholy Strength".into(),
        mana_cost: Some(ManaCost { pips: vec![ManaPip::Black] }),
        type_line: TypeLine {
            supertypes: vec![],
            card_types: vec![CardType::Enchantment],
            subtypes: vec!["Aura".into()],
        },
        oracle_text: "Enchant creature\nEnchanted creature gets +2/+1.".into(),
        rules_text: vec![RulesText::Active(Rule::Aura {
            enchant: TargetFilter::Creature,
            grants: ContinuousEffect {
                subject_filter: PermanentFilter::default(),
                pt_modification: Some(PTDelta { power: 2, toughness: 1 }),
            },
        })],
        text_annotations: vec![],
        power: None,
        toughness: None,
        colors: vec![crate::types::mana::ManaColor::Black],
    }
}

#[test]
fn cast_aura_without_target_fails() {
    let db = test_db();
    let mut gs = two_player_state();
    gs.step = crate::types::Step::PreCombatMain;
    let aura_id = put_in_hand(&mut gs, PlayerId(0), make_unholy_strength());
    gs.get_player_mut(PlayerId(0)).unwrap().mana_pool.black = 1;

    // No target declared → wrong number of targets
    assert!(matches!(
        cast_spell(gs, PlayerId(0), aura_id, vec![], None),
        Err(crate::engine::EngineError::WrongNumberOfTargets)
    ));
}

#[test]
fn cast_aura_on_non_creature_fails() {
    let db = test_db();
    let mut gs = two_player_state();
    gs.step = crate::types::Step::PreCombatMain;
    let aura_id = put_in_hand(&mut gs, PlayerId(0), make_unholy_strength());
    let land_id = {
        let id = gs.alloc_id();
        let obj = CardObject::new(id, db.get("Forest").unwrap().clone(), PlayerId(0), Zone::Battlefield);
        let perm = PermanentState::new(&obj.definition);
        gs.battlefield.insert(id, perm);
        gs.add_object(obj);
        id
    };
    gs.get_player_mut(PlayerId(0)).unwrap().mana_pool.black = 1;

    assert!(matches!(
        cast_spell(gs, PlayerId(0), aura_id, vec![crate::types::effect::EffectTarget::Object { id: land_id }], None),
        Err(crate::engine::EngineError::IllegalTarget)
    ));
}

#[test]
fn cast_aura_on_creature_succeeds_and_enters_attached() {
    use crate::engine::stack::resolve_top;
    let db = test_db();
    let mut gs = two_player_state();
    gs.step = crate::types::Step::PreCombatMain;
    let bear_id = place_on_battlefield(&mut gs, db.get("Grizzly Bears").unwrap().clone(), PlayerId(0));
    let aura_id = put_in_hand(&mut gs, PlayerId(0), make_unholy_strength());
    gs.get_player_mut(PlayerId(0)).unwrap().mana_pool.black = 1;

    let gs = cast_spell(
        gs,
        PlayerId(0),
        aura_id,
        vec![crate::types::effect::EffectTarget::Object { id: bear_id }],
        None,
    )
    .unwrap();
    assert_eq!(gs.stack.len(), 1);

    let gs = resolve_top(gs);

    assert!(gs.battlefield.contains_key(&aura_id), "aura should be on battlefield");
    assert_eq!(
        gs.battlefield[&aura_id].attached_to,
        Some(bear_id),
        "aura should be attached to the bear"
    );
}
```

You also need a `put_in_hand` helper. Add it if it doesn't already exist in the test module:

```rust
fn put_in_hand(state: &mut GameState, owner: PlayerId, def: CardDefinition) -> ObjectId {
    let id = state.alloc_id();
    let obj = CardObject::new(id, def, owner, Zone::Hand);
    state.hands.get_mut(&owner).unwrap().push(id);
    state.add_object(obj);
    id
}
```

- [ ] **Step 2: Run tests to verify they fail**

```bash
cargo test -p mecha-oracle casting::tests::cast_aura_without_target_fails 2>&1 | grep -E "^test result|FAILED|error\["
```
Expected: `FAILED` (target requirements not yet extracted from Rule::Aura)

- [ ] **Step 3: Extend `cast_spell` in `src/engine/casting.rs` to extract aura targets**

In `cast_spell`, find the `target_requirements` extraction block:

```rust
let target_requirements: Vec<crate::types::ability::TargetFilter> = state
    .objects
    .get(&object_id)
    .map(|obj| {
        obj.definition
            .rules_text
            .iter()
            .filter_map(|span| match span {
                RulesText::Active(Rule::SpellAbility(sa)) => {
                    Some(sa.target_requirements.clone())
                }
                _ => None,
            })
            .flatten()
            .collect()
    })
    .unwrap_or_default();
```

Replace with:

```rust
let target_requirements: Vec<crate::types::ability::TargetFilter> = state
    .objects
    .get(&object_id)
    .map(|obj| {
        obj.definition
            .rules_text
            .iter()
            .filter_map(|span| match span {
                RulesText::Active(Rule::SpellAbility(sa)) => {
                    Some(sa.target_requirements.clone())
                }
                // CR 303.4a: an Aura's enchant restriction is its target requirement at cast time.
                RulesText::Active(Rule::Aura { enchant, .. }) => Some(vec![enchant.clone()]),
                _ => None,
            })
            .flatten()
            .collect()
    })
    .unwrap_or_default();
```

- [ ] **Step 4: Extend `resolve_top` in `src/engine/stack.rs` to attach auras on ETB**

In `resolve_top`, in the `StackPayload::Spell { card_id }` branch, inside the `if is_permanent` block, after the ETB triggers loop (after `for trigger in etb_triggers { ... }`), add:

```rust
// CR 303.4c: if this permanent is an Aura, attach it to its declared target.
let has_aura_rule = state
    .objects
    .get(&card_id)
    .map(|o| {
        o.definition
            .rules_text
            .iter()
            .any(|span| matches!(span, crate::types::RulesText::Active(crate::types::Rule::Aura { .. })))
    })
    .unwrap_or(false);
if has_aura_rule {
    if let Some(crate::types::effect::EffectTarget::Object { id: host_id }) = targets.first() {
        let host_id = *host_id;
        if state.battlefield.contains_key(&host_id) {
            if let Some(perm) = state.battlefield.get_mut(&card_id) {
                perm.attached_to = Some(host_id);
            }
        }
        // If host_id is not on battlefield: aura enters unattached; 704.5m SBA
        // fires immediately after and moves it to the graveyard.
    }
}
```

- [ ] **Step 5: Run all tests to verify they pass**

```bash
cargo test 2>&1 | grep -E "^test result|FAILED|error\["
```
Expected: all `ok`, no `FAILED`.

- [ ] **Step 6: Clippy and commit**

```bash
cargo clippy --fix --all-targets 2>&1 | grep -E "^error|^warning"
cargo clippy --all-targets 2>&1 | grep -E "^error|^warning"
git add src/engine/casting.rs src/engine/stack.rs
git commit -m "feat: aura cast-time targeting and post-ETB attachment via resolve_top"
```

---

### Task 6: API changes

**Files:**
- Modify: `src/serve.rs`

**Interfaces:**
- Consumes: `engine::equip::activate_equip` (Task 4), `Rule::Aura { enchant }` (Task 1), `Rule::Equip { cost }` (Task 1), `targeting::legal_targets` (already exists), `can_pay_cost_components` (already exists)
- Produces:
  - `ActionRequest::ActivateEquip { equipment_id, target_id }` action variant
  - `CardView.attached_to: Option<u64>` field exposed to frontend
  - Aura cast actions generated in `compute_hand_actions`
  - Equip actions generated in `compute_battlefield_actions`

- [ ] **Step 1: Add `attached_to` to `CardView`**

In `src/serve.rs`, in the `CardView` struct, add after `counters: Vec<CounterView>,`:

```rust
#[serde(skip_serializing_if = "Option::is_none")]
attached_to: Option<u64>,
```

In the `to_card_view` closure (around line 710), add to the `CardView { ... }` initialiser after `counters: perm.map(...)`:

```rust
attached_to: perm.and_then(|p| p.attached_to.map(|id| id.0)),
```

- [ ] **Step 2: Add `ActionRequest::ActivateEquip`**

In `src/serve.rs`, in the `ActionRequest` enum, add after `CycleCard { ... },`:

```rust
ActivateEquip {
    equipment_id: u64,
    target_id: u64,
},
```

In the `action_handler` match (where other `ActionRequest` variants are handled), add:

```rust
ActionRequest::ActivateEquip { equipment_id, target_id } => {
    mecha_oracle::engine::equip::activate_equip(
        state,
        mecha_oracle::types::ObjectId(equipment_id),
        mecha_oracle::types::ObjectId(target_id),
        player,
    )
    .map_err(|e| format!("{e:?}"))
}
```

- [ ] **Step 3: Add aura actions to `compute_hand_actions`**

In `compute_hand_actions`, after the existing targeted-spell block (the `for filter in &target_filters` loop that generates `cast_spell` actions), add:

```rust
// Aura spells: use Rule::Aura.enchant as the target requirement (CR 303.4a).
use mecha_oracle::types::ability::{Rule, RulesText};
let aura_enchant_filter = obj.definition.rules_text.iter().find_map(|span| {
    if let RulesText::Active(Rule::Aura { enchant, .. }) = span {
        Some(enchant.clone())
    } else {
        None
    }
});
if let Some(enchant_filter) = aura_enchant_filter {
    let spell_colors = obj.definition.colors.clone();
    for target in legal_targets(state, &enchant_filter, pid, &spell_colors) {
        let EffectTarget::Object { id: target_obj_id } = target else { continue };
        let target_name = target_display_name(state, &EffectTarget::Object { id: target_obj_id });
        let target_val = serde_json::to_value(&EffectTarget::Object { id: target_obj_id }).unwrap();
        actions.push(ActionItemView {
            label: format!("Cast {} → {}", obj.definition.name, target_name),
            kind: ActionItemKind::Server {
                action: serde_json::json!({
                    "type": "cast_spell",
                    "object_id": obj.id.0,
                    "targets": [target_val],
                }),
                key: format!("cast-{}-o{}", obj.id.0, target_obj_id.0),
            },
        });
    }
}
```

- [ ] **Step 4: Add equip actions to `compute_battlefield_actions`**

In `compute_battlefield_actions`, after the existing activated-abilities loop, add:

```rust
// Equip actions: CR 301.5d — sorcery speed, main phase, empty stack, controller only.
use mecha_oracle::types::ability::{Rule, RulesText};
use mecha_oracle::types::Step;
if state.priority_player == pid
    && state.active_player == pid
    && matches!(state.step(), Step::PreCombatMain | Step::PostCombatMain)
    && state.stack.is_empty()
{
    let equip_cost = obj.definition.rules_text.iter().find_map(|span| {
        if let RulesText::Active(Rule::Equip { cost, .. }) = span {
            Some(cost.clone())
        } else {
            None
        }
    });
    if let Some(cost) = equip_cost {
        if can_pay_cost_components(state, pid, Some(obj.id), &cost) {
            // One action per creature the player controls (excluding the equipment itself).
            let creature_targets: Vec<_> = state
                .battlefield
                .keys()
                .filter(|&&cid| {
                    cid != obj.id
                        && state
                            .objects
                            .get(&cid)
                            .map(|o| {
                                o.controller == pid
                                    && o.definition.type_line.is_creature()
                            })
                            .unwrap_or(false)
                })
                .copied()
                .collect();
            for creature_id in creature_targets {
                let creature_name = state
                    .objects
                    .get(&creature_id)
                    .map(|o| o.definition.name.as_str())
                    .unwrap_or("creature");
                actions.push(ActionItemView {
                    label: format!("Equip {creature_name}"),
                    kind: ActionItemKind::Server {
                        action: serde_json::json!({
                            "type": "activate_equip",
                            "equipment_id": obj.id.0,
                            "target_id": creature_id.0,
                        }),
                        key: format!("equip-{}-{}", obj.id.0, creature_id.0),
                    },
                });
            }
        }
    }
}
```

- [ ] **Step 5: Run all tests to verify they pass**

```bash
cargo test 2>&1 | grep -E "^test result|FAILED|error\["
```
Expected: all `ok`, no `FAILED`.

- [ ] **Step 6: Clippy and commit**

```bash
cargo clippy --fix --all-targets 2>&1 | grep -E "^error|^warning"
cargo clippy --all-targets 2>&1 | grep -E "^error|^warning"
git add src/serve.rs
git commit -m "feat: serve.rs — ActivateEquip action, aura/equip UI actions, attached_to in CardView"
```

---

## Self-Review Notes

**Spec coverage:**
- Section 1 (data model) → Task 1 ✓
- Section 2 (Rule variants) → Task 1 ✓
- Section 3 (EffectStep::Attach) → Task 1 + Task 4 ✓
- Section 4 (continuous effect evaluation) → Task 2 ✓
- Section 5a (aura casting flow) → Task 5 ✓
- Section 5b (equip action) → Task 4 ✓
- Section 5c (SBAs) → Task 3 ✓
- Section 6 (API) → Task 6 ✓
- Section 7 (test data JSON) — deferred: JSON test decks rely on the card database (Scryfall oracle parse). Since parser integration for `Rule::Aura` / `Rule::Equip` is explicitly out of scope, Bonesplitter and Unholy Strength are defined inline as `CardDefinition` helpers inside each task's test code rather than added to the deck JSON. No deck JSON task needed.
