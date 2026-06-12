# UI Thin-Client Overhaul Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Move all action-building logic from JavaScript into the Rust engine, separate CSS/JS out of serve.html, and add right-click context menus with full action lists.

**Architecture:** Each `CardView` gains an `actions: Vec<ActionItemView>` field computed in `build_player_view`. JS becomes a dumb dispatcher: left-click auto-fires single payable actions, right-click always shows the full popup. `can_*` flags are removed from `CardView` entirely. Basic land types also get their intrinsic mana abilities injected as proper `ActivatedAbility` entries at construction time rather than handled as a UI special case.

**Tech Stack:** Rust/Axum backend, `serde_json::json!()` for action payloads, vanilla JS frontend with no build step.

**Spec:** `docs/superpowers/specs/2026-06-12-ui-thin-client-design.md`

---

## File Map

| File | Change |
|---|---|
| `src/types/card_object.rs` | Add `inject_intrinsic_abilities` fn; call from `new()` |
| `src/serve.rs` | Add `ActionItemView`/`ActionItemKind`; strip `CardView` old fields; add `actions`; add `compute_actions`; add CSS/JS routes; remove dead helpers |
| `src/serve.html` | Skeleton only — `<link>` + `<script>` tags |
| `src/serve.css` | Extracted CSS from `serve.html` + new `.popup-item.disabled` rule |
| `src/serve.js` | Extracted + refactored JS from `serve.html` |

---

### Task 1: Inject intrinsic mana abilities for basic land types

**Files:**
- Modify: `src/types/card_object.rs`

CR 305.6: lands with basic land subtypes (Forest, Island, Mountain, Plains, Swamp) get intrinsic mana abilities. Currently these are a UI fallback in `serve.rs`. This moves them into the engine so they appear as proper `ActivatedAbility` entries.

- [ ] **Step 1: Write failing test**

Add to the `#[cfg(test)]` block in `src/types/card_object.rs`:

```rust
#[test]
fn forest_gets_intrinsic_tap_for_green_mana() {
    use crate::cards::test_helpers::test_db;
    use crate::types::ability::{Ability, CostComponent, OracleSpan};
    use crate::types::effect::EffectStep;

    let db = test_db();
    let forest_def = db.get("Forest").unwrap().clone();
    let obj = CardObject::new(ObjectId(1), forest_def, PlayerId(0), Zone::Hand);

    let mana_abilities: Vec<_> = obj
        .definition
        .abilities
        .iter()
        .filter_map(|span| match span {
            OracleSpan::Parsed(Ability::Activated(a)) => Some(a),
            _ => None,
        })
        .collect();

    assert_eq!(mana_abilities.len(), 1, "Forest should have exactly one activated ability");
    assert!(
        mana_abilities[0].cost.contains(&CostComponent::Tap),
        "cost should contain {{T}}"
    );
    assert!(
        matches!(&mana_abilities[0].effect[0], EffectStep::AddMana(p) if p.green == 1),
        "effect should add one green mana"
    );
}

#[test]
fn island_gets_intrinsic_tap_for_blue_mana() {
    use crate::cards::test_helpers::test_db;
    use crate::types::ability::{Ability, OracleSpan};
    use crate::types::effect::EffectStep;

    let db = test_db();
    let island_def = db.get("Island").unwrap().clone();
    let obj = CardObject::new(ObjectId(1), island_def, PlayerId(0), Zone::Hand);

    let mana_abilities: Vec<_> = obj
        .definition
        .abilities
        .iter()
        .filter_map(|span| match span {
            OracleSpan::Parsed(Ability::Activated(a)) => Some(a),
            _ => None,
        })
        .collect();

    assert_eq!(mana_abilities.len(), 1);
    assert!(
        matches!(&mana_abilities[0].effect[0], EffectStep::AddMana(p) if p.blue == 1),
    );
}

#[test]
fn non_land_gets_no_intrinsic_ability() {
    use crate::cards::test_helpers::test_db;
    use crate::types::ability::{Ability, OracleSpan};

    let db = test_db();
    let obj = CardObject::new(
        ObjectId(1),
        db.get("Grizzly Bears").unwrap().clone(),
        PlayerId(0),
        Zone::Hand,
    );

    let mana_abilities = obj
        .definition
        .abilities
        .iter()
        .filter(|span| matches!(span, OracleSpan::Parsed(Ability::Activated(_))))
        .count();

    assert_eq!(mana_abilities, 0);
}
```

- [ ] **Step 2: Run to confirm the tests fail**

```bash
cargo test -p mecha-oracle types::card_object 2>&1 | grep -E "^test result|FAILED|error\["
```

Expected: `forest_gets_intrinsic_tap_for_green_mana` and `island_gets_intrinsic_tap_for_blue_mana` FAIL.

- [ ] **Step 3: Implement `inject_intrinsic_abilities` and update `new()`**

In `src/types/card_object.rs`, add before `impl CardObject`:

```rust
fn inject_intrinsic_abilities(definition: &mut CardDefinition) {
    use super::ability::{ActivatedAbility, CostComponent};
    use super::effect::EffectStep;
    use super::mana::ManaPool;

    // CR 305.6: each basic land subtype grants a {T}: Add {X} mana ability.
    for subtype in &definition.type_line.subtypes {
        let pool = match subtype.as_str() {
            "Forest"   => ManaPool { green: 1,   ..Default::default() },
            "Island"   => ManaPool { blue: 1,    ..Default::default() },
            "Mountain" => ManaPool { red: 1,     ..Default::default() },
            "Plains"   => ManaPool { white: 1,   ..Default::default() },
            "Swamp"    => ManaPool { black: 1,   ..Default::default() },
            _ => continue,
        };
        definition.abilities.push(OracleSpan::Parsed(Ability::Activated(ActivatedAbility {
            cost: vec![CostComponent::Tap],
            target_requirements: vec![],
            effect: vec![EffectStep::AddMana(pool)],
        })));
    }
}
```

Change `new()` to take `mut definition`:

```rust
pub fn new(id: ObjectId, mut definition: CardDefinition, owner: PlayerId, zone: Zone) -> Self {
    inject_intrinsic_abilities(&mut definition);
    Self {
        id,
        definition,
        controller: owner,
        owner,
        zone,
    }
}
```

- [ ] **Step 4: Run tests**

```bash
cargo test -p mecha-oracle types::card_object 2>&1 | grep -E "^test result|FAILED|error\["
```

Expected: all pass.

- [ ] **Step 5: Run full test suite to confirm nothing broke**

```bash
cargo test 2>&1 | grep -E "^test result|FAILED|error\["
```

Expected: all pass (the `dryad_arbor_appears_in_both_lands_and_creatures` test in serve.rs should still pass since Dryad Arbor has the Forest subtype and will now have an injected ability, which means `getBattlefieldLandActions` — wait, we haven't changed that yet — the existing `tap_land` fallback still fires. The test checks the game view lands/creatures, not actions, so it should still pass).

- [ ] **Step 6: Commit**

```bash
git add src/types/card_object.rs
git commit -m "feat: inject intrinsic mana abilities for basic land types (CR 305.6)"
```

---

### Task 2: Define `ActionItemView` and `ActionItemKind` types

**Files:**
- Modify: `src/serve.rs`

Just adds the new types. No behavior change. Compile check only.

- [ ] **Step 1: Add types to `serve.rs`**

After the `StackItemView` struct definition, add:

```rust
#[derive(Serialize)]
struct ActionItemView {
    label: String,
    can_pay_cost: bool,
    #[serde(flatten)]
    kind: ActionItemKind,
}

#[derive(Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
enum ActionItemKind {
    /// Pre-built JSON payload posted verbatim to /action
    Server { action: serde_json::Value },
    /// Toggle this creature in/out of the client-side attacker-staging list
    ToggleAttacker { object_id: u64 },
    /// Assign this creature as a blocker for the given attacker (client-side staging)
    AssignBlocker { blocker_id: u64, attacker_id: u64 },
}
```

- [ ] **Step 2: Confirm it compiles**

```bash
cargo build 2>&1 | grep -E "^error"
```

Expected: no errors.

- [ ] **Step 3: Commit**

```bash
git add src/serve.rs
git commit -m "feat: define ActionItemView and ActionItemKind types in serve.rs"
```

---

### Task 3: Strip old `CardView` fields and rewrite tests

**Files:**
- Modify: `src/serve.rs`

Removes `can_cast`, `can_attack`, `can_block`, `can_cycle`, `cycling_cost`, `activated_abilities`, `valid_targets` from `CardView`. Removes `ActivatedAbilityView` and `TargetView` types. Adds `actions: Vec<ActionItemView>` (starts as `vec![]`). Rewrites affected tests to compile. After this task tests for cast/attack/block will FAIL — that is correct and expected.

- [ ] **Step 1: Remove `ActivatedAbilityView` and `TargetView` structs**

Delete these two struct definitions from `serve.rs`:

```rust
// DELETE THIS:
#[derive(Serialize)]
struct TargetView {
    kind: String, // "permanent" | "player"
    id: u64,
    name: String,
}

// DELETE THIS:
#[derive(Serialize)]
struct ActivatedAbilityView {
    index: usize,
    label: String,
    can_activate: bool,
    valid_targets: Vec<TargetView>,
}
```

- [ ] **Step 2: Replace old `CardView` fields with `actions`**

Replace the `CardView` struct definition with:

```rust
#[derive(Serialize)]
struct CardView {
    id: ObjectId,
    name: String,
    type_line: String,
    oracle_text: Vec<OracleSpanView>,
    mana_cost: Option<String>,
    power: Option<i32>,
    toughness: Option<i32>,
    tapped: bool,
    summoning_sick: bool,
    damage_marked: u32,
    is_attacking: bool,
    is_blocking: bool,
    actions: Vec<ActionItemView>,
}
```

- [ ] **Step 3: Remove `compute_can_cast` and `build_target_views` helper functions**

Delete these two free functions — they are replaced by the `compute_actions` function in Task 4:

```rust
// DELETE compute_can_cast(...)
// DELETE build_target_views(...)
```

- [ ] **Step 4: Update `to_card_view` closure in `build_player_view`**

Replace the entire `to_card_view` closure with the version below. Note: `actions` is `vec![]` for now — the computation is added in Tasks 4 and 5.

```rust
let to_card_view = |obj: &mecha_oracle::types::CardObject| {
    let perm = state.battlefield.get(&obj.id);
    CardView {
        id: obj.id,
        name: obj.definition.name.clone(),
        type_line: format_type_line(&obj.definition.type_line),
        oracle_text: {
            obj.definition
                .abilities
                .iter()
                .map(|span| match span {
                    OracleSpan::Parsed(Ability::Static(kw)) => OracleSpanView {
                        kind: SpanKind::Parsed,
                        text: kw.display_name(),
                        ignored_kind: None,
                    },
                    OracleSpan::Parsed(Ability::Activated(a)) => OracleSpanView {
                        kind: SpanKind::Parsed,
                        text: format_activated_ability(a),
                        ignored_kind: None,
                    },
                    OracleSpan::Parsed(Ability::Triggered(t)) => OracleSpanView {
                        kind: SpanKind::Parsed,
                        text: format_triggered_ability(t),
                        ignored_kind: None,
                    },
                    OracleSpan::Parsed(Ability::SpellEffect(spell_ability)) => OracleSpanView {
                        kind: SpanKind::Parsed,
                        text: format_spell_effect(&spell_ability.steps),
                        ignored_kind: None,
                    },
                    OracleSpan::Parsed(Ability::Cycling(cost)) => OracleSpanView {
                        kind: SpanKind::Parsed,
                        text: format!("Cycling {}", format_mana_cost(cost)),
                        ignored_kind: None,
                    },
                    OracleSpan::Ignored(kind, t) => OracleSpanView {
                        kind: SpanKind::Ignored,
                        text: t.clone(),
                        ignored_kind: Some(kind.clone()),
                    },
                    OracleSpan::ParsedUnimplemented(t) => OracleSpanView {
                        kind: SpanKind::ParsedUnimplemented,
                        text: t.clone(),
                        ignored_kind: None,
                    },
                    OracleSpan::Unparsed(t) => OracleSpanView {
                        kind: SpanKind::Unparsed,
                        text: t.clone(),
                        ignored_kind: None,
                    },
                })
                .collect()
        },
        mana_cost: obj.definition.mana_cost.as_ref().map(format_mana_cost),
        power: perm.and_then(|p| p.effective_power()),
        toughness: perm.and_then(|p| p.effective_toughness()),
        tapped: perm.map(|p| p.tapped).unwrap_or(false),
        summoning_sick: perm.map(|p| p.summoning_sick).unwrap_or(false),
        damage_marked: perm.map(|p| p.damage_marked).unwrap_or(0),
        is_attacking: state.combat.attackers.contains(&obj.id),
        is_blocking: all_blockers.contains(&obj.id),
        actions: vec![], // populated in Tasks 4 and 5
    }
};
```

Also update the `StackPayload::Spell` arm in `build_game_view` — replace its inline `CardView` construction with:

```rust
card: card.map(|c| CardView {
    id: c.id,
    name: c.definition.name.clone(),
    type_line: format_type_line(&c.definition.type_line),
    oracle_text: vec![],
    mana_cost: c.definition.mana_cost.as_ref().map(format_mana_cost),
    power: c.definition.power,
    toughness: c.definition.toughness,
    tapped: false,
    summoning_sick: false,
    damage_marked: 0,
    is_attacking: false,
    is_blocking: false,
    actions: vec![],
}),
```

- [ ] **Step 5: Add test helper functions and rewrite tests**

Add these helpers immediately before the `#[cfg(test)]` `mod tests` block in `serve.rs`:

```rust
#[cfg(test)]
fn has_payable_server_action(card: &CardView) -> bool {
    card.actions
        .iter()
        .any(|a| a.can_pay_cost && matches!(a.kind, ActionItemKind::Server { .. }))
}

#[cfg(test)]
fn has_toggle_attacker(card: &CardView) -> bool {
    card.actions
        .iter()
        .any(|a| matches!(a.kind, ActionItemKind::ToggleAttacker { .. }))
}

#[cfg(test)]
fn has_assign_blocker(card: &CardView) -> bool {
    card.actions
        .iter()
        .any(|a| matches!(a.kind, ActionItemKind::AssignBlocker { .. }))
}
```

Rewrite the four affected tests inside `mod tests`:

```rust
#[test]
fn can_cast_true_for_instant_in_hand_with_mana_and_priority() {
    use mecha_oracle::types::card::{CardDefinition, CardType, TypeLine};
    use mecha_oracle::types::effect::EffectStep;
    use mecha_oracle::types::mana::{ManaCost, ManaPip};
    use mecha_oracle::types::{Ability, OracleSpan};
    use mecha_oracle::types::{CardObject, Zone};

    let config = vec![
        (0..10).map(|_| "Forest".to_string()).collect(),
        (0..10).map(|_| "Forest".to_string()).collect(),
    ];
    let db = test_db();
    let mut gs = build_game_state(config, &db, false).unwrap();
    use mecha_oracle::types::ability::SpellAbility;
    let def = CardDefinition {
        name: "Cheap Instant".into(),
        mana_cost: Some(ManaCost {
            pips: vec![ManaPip::Generic(1)],
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
        power: None,
        toughness: None,
    };
    let id = gs.alloc_id();
    let obj = CardObject::new(id, def, mecha_oracle::types::PlayerId(0), Zone::Hand);
    gs.hands
        .get_mut(&mecha_oracle::types::PlayerId(0))
        .unwrap()
        .push(id);
    gs.add_object(obj);
    gs.get_player_mut(mecha_oracle::types::PlayerId(0))
        .unwrap()
        .mana_pool
        .colorless = 1;

    let view = build_game_view(&gs);
    let card = view
        .p1
        .hand
        .iter()
        .find(|c| c.name == "Cheap Instant")
        .unwrap();
    assert!(
        has_payable_server_action(card),
        "instant with mana in hand with priority should have a payable server action"
    );
}

#[test]
fn can_cast_false_for_creature_when_not_active_player() {
    use mecha_oracle::types::{CardObject, Zone};
    let db = test_db();
    let config = vec![
        (0..10).map(|_| "Forest".to_string()).collect(),
        (0..10).map(|_| "Forest".to_string()).collect(),
    ];
    let mut gs = build_game_state(config, &db, false).unwrap();
    gs.active_player = mecha_oracle::types::PlayerId(1);
    gs.priority_player = mecha_oracle::types::PlayerId(0);

    let id = gs.alloc_id();
    let obj = CardObject::new(
        id,
        db.get("Grizzly Bears").unwrap().clone(),
        mecha_oracle::types::PlayerId(0),
        Zone::Hand,
    );
    gs.hands
        .get_mut(&mecha_oracle::types::PlayerId(0))
        .unwrap()
        .push(id);
    gs.add_object(obj);
    gs.get_player_mut(mecha_oracle::types::PlayerId(0))
        .unwrap()
        .mana_pool
        .green = 2;

    let view = build_game_view(&gs);
    let card = view
        .p1
        .hand
        .iter()
        .find(|c| c.name == "Grizzly Bears")
        .unwrap();
    assert!(
        card.actions.is_empty(),
        "creature cannot be cast when player is not active player — should have no actions"
    );
}

#[test]
fn can_attack_true_only_for_active_player_at_declare_attackers() {
    use mecha_oracle::types::{CardObject, Step, Zone};
    let db = test_db();
    let config = vec![
        (0..10).map(|_| "Forest".to_string()).collect(),
        (0..10).map(|_| "Forest".to_string()).collect(),
    ];
    let mut gs = build_game_state(config, &db, false).unwrap();

    let p1_id = {
        let id = gs.alloc_id();
        let obj = CardObject::new(
            id,
            db.get("Grizzly Bears").unwrap().clone(),
            PlayerId(0),
            Zone::Battlefield,
        );
        let mut perm = PermanentState::new(&obj.definition);
        perm.summoning_sick = false;
        gs.battlefield.insert(id, perm);
        gs.add_object(obj);
        id.0
    };
    let p2_id = {
        let id = gs.alloc_id();
        let obj = CardObject::new(
            id,
            db.get("Grizzly Bears").unwrap().clone(),
            PlayerId(1),
            Zone::Battlefield,
        );
        let mut perm = PermanentState::new(&obj.definition);
        perm.summoning_sick = false;
        gs.battlefield.insert(id, perm);
        gs.add_object(obj);
        id.0
    };

    for _ in 0..4 {
        gs = dispatch_action(gs, ActionRequest::AdvanceStep).unwrap();
    }
    assert_eq!(gs.step(), Step::DeclareAttackers);

    let view = build_game_view(&gs);
    let p1_c = view
        .p1
        .creatures
        .iter()
        .find(|c| c.id == ObjectId(p1_id))
        .unwrap();
    let p2_c = view
        .p2
        .creatures
        .iter()
        .find(|c| c.id == ObjectId(p2_id))
        .unwrap();

    assert!(
        has_toggle_attacker(p1_c),
        "active player's creature should have toggle_attacker action"
    );
    assert!(
        !has_toggle_attacker(p2_c),
        "defending player's creature should not have toggle_attacker action"
    );
    assert!(
        !has_assign_blocker(p1_c),
        "assign_blocker is false outside DeclareBlockers"
    );
    assert!(
        !has_assign_blocker(p2_c),
        "assign_blocker is false outside DeclareBlockers"
    );
}

#[test]
fn can_block_true_only_for_defending_player_at_declare_blockers() {
    use mecha_oracle::types::{CardObject, Step, Zone};
    let db = test_db();
    let config = vec![
        (0..10).map(|_| "Forest".to_string()).collect(),
        (0..10).map(|_| "Forest".to_string()).collect(),
    ];
    let mut gs = build_game_state(config, &db, false).unwrap();

    let p1_id = {
        let id = gs.alloc_id();
        let obj = CardObject::new(
            id,
            db.get("Grizzly Bears").unwrap().clone(),
            PlayerId(0),
            Zone::Battlefield,
        );
        let mut perm = PermanentState::new(&obj.definition);
        perm.summoning_sick = false;
        gs.battlefield.insert(id, perm);
        gs.add_object(obj);
        id.0
    };
    let p2_id = {
        let id = gs.alloc_id();
        let obj = CardObject::new(
            id,
            db.get("Grizzly Bears").unwrap().clone(),
            PlayerId(1),
            Zone::Battlefield,
        );
        let mut perm = PermanentState::new(&obj.definition);
        perm.summoning_sick = false;
        gs.battlefield.insert(id, perm);
        gs.add_object(obj);
        id.0
    };

    for _ in 0..4 {
        gs = dispatch_action(gs, ActionRequest::AdvanceStep).unwrap();
    }
    let gs = dispatch_action(
        gs,
        ActionRequest::DeclareAttackers {
            attacker_ids: vec![p1_id],
        },
    )
    .unwrap();
    let gs = dispatch_action(gs, ActionRequest::AdvanceStep).unwrap();
    let gs = dispatch_action(gs, ActionRequest::AdvanceStep).unwrap();
    assert_eq!(gs.step(), Step::DeclareBlockers);

    let view = build_game_view(&gs);
    let p1_c = view
        .p1
        .creatures
        .iter()
        .find(|c| c.id == ObjectId(p1_id))
        .unwrap();
    let p2_c = view
        .p2
        .creatures
        .iter()
        .find(|c| c.id == ObjectId(p2_id))
        .unwrap();

    assert!(
        !has_assign_blocker(p1_c),
        "active player's creature should not have assign_blocker action"
    );
    assert!(
        has_assign_blocker(p2_c),
        "defending player's creature should have assign_blocker action"
    );
    assert!(
        !has_toggle_attacker(p1_c),
        "declared attacker (tapped) should not have toggle_attacker action"
    );
}
```

- [ ] **Step 6: Confirm it compiles (tests are expected to fail)**

```bash
cargo test 2>&1 | grep -E "^test result|FAILED|error\["
```

Expected: compilation succeeds; the four rewritten tests FAIL because `actions` is always `vec![]`. That is correct — the implementation comes in Tasks 4 and 5.

- [ ] **Step 7: Commit**

```bash
git add src/serve.rs
git commit -m "refactor: strip can_* flags from CardView, add actions field, rewrite tests"
```

---

### Task 4: Implement hand card action computation

**Files:**
- Modify: `src/serve.rs`

Adds `compute_actions`, `can_cast_structural`, and the hand-zone action logic. After this task the `can_cast` tests pass.

- [ ] **Step 1: Add imports and `can_cast_structural` helper**

Add at the top of `serve.rs` with the existing imports:

```rust
use mecha_oracle::engine::targeting::legal_targets;
use mecha_oracle::types::effect::EffectTarget;
```

Add this free function (after the `format_*` helpers, before `build_player_view`):

```rust
fn can_cast_structural(state: &GameState, pid: PlayerId, obj: &CardObject) -> bool {
    use mecha_oracle::types::card::CardType;
    if obj.zone != Zone::Hand {
        return false;
    }
    if state.priority_player != pid {
        return false;
    }
    if obj.definition.mana_cost.is_none() {
        return false;
    }
    let is_instant_speed = obj
        .definition
        .type_line
        .card_types
        .contains(&CardType::Instant)
        || obj.has_keyword(StaticAbility::Flash);
    if is_instant_speed {
        return true;
    }
    state.active_player == pid
        && matches!(state.step(), Step::PreCombatMain | Step::PostCombatMain)
        && state.stack.is_empty()
}
```

- [ ] **Step 2: Add `compute_actions` with hand zone logic**

Add this free function immediately after `can_cast_structural`:

```rust
fn compute_actions(state: &GameState, pid: PlayerId, obj: &CardObject) -> Vec<ActionItemView> {
    match obj.zone {
        Zone::Hand => compute_hand_actions(state, pid, obj),
        Zone::Battlefield => compute_battlefield_actions(state, pid, obj),
        _ => vec![],
    }
}

fn compute_hand_actions(
    state: &GameState,
    pid: PlayerId,
    obj: &CardObject,
) -> Vec<ActionItemView> {
    let mut actions = Vec::new();

    // Play land (no mana cost — always can_pay_cost: true when structurally valid)
    if obj.definition.type_line.is_land() {
        let can_play = state.active_player == pid
            && state.priority_player == pid
            && state.lands_played_this_turn == 0
            && matches!(state.step(), Step::PreCombatMain | Step::PostCombatMain)
            && state.stack.is_empty();
        if can_play {
            actions.push(ActionItemView {
                label: "Play land".to_string(),
                can_pay_cost: true,
                kind: ActionItemKind::Server {
                    action: serde_json::json!({
                        "type": "play_land",
                        "object_id": obj.id.0
                    }),
                },
            });
        }
        return actions;
    }

    // Cast spell
    if let Some(cost) = &obj.definition.mana_cost {
        if can_cast_structural(state, pid, obj) {
            let player = state.get_player(pid).unwrap();
            let mana_ok =
                greedy_payment_plan(cost, &player.mana_pool, player.life).is_some();

            // Collect target requirements from all SpellEffect abilities
            let target_filters: Vec<_> = obj
                .definition
                .abilities
                .iter()
                .filter_map(|span| match span {
                    OracleSpan::Parsed(Ability::SpellEffect(sa))
                        if !sa.target_requirements.is_empty() =>
                    {
                        Some(sa.target_requirements.as_slice())
                    }
                    _ => None,
                })
                .flatten()
                .copied()
                .collect();

            if target_filters.is_empty() {
                // Untargeted spell
                actions.push(ActionItemView {
                    label: format!("Cast {}", obj.definition.name),
                    can_pay_cost: mana_ok,
                    kind: ActionItemKind::Server {
                        action: serde_json::json!({
                            "type": "cast_spell",
                            "object_id": obj.id.0
                        }),
                    },
                });
            } else {
                // Targeted spell: one action per legal target
                let mut seen = std::collections::HashSet::new();
                for filter in &target_filters {
                    for target in legal_targets(state, *filter, pid) {
                        let key = match &target {
                            EffectTarget::Object { id } => format!("o{}", id.0),
                            EffectTarget::Player { id } => format!("p{}", id.0),
                        };
                        if !seen.insert(key) {
                            continue;
                        }
                        let target_name = match &target {
                            EffectTarget::Object { id } => state
                                .objects
                                .get(id)
                                .map(|o| o.definition.name.clone())
                                .unwrap_or_default(),
                            EffectTarget::Player { id } => state
                                .get_player(*id)
                                .map(|p| p.name.clone())
                                .unwrap_or_default(),
                        };
                        let target_val = serde_json::to_value(&target).unwrap();
                        actions.push(ActionItemView {
                            label: format!(
                                "Cast {} → {}",
                                obj.definition.name, target_name
                            ),
                            can_pay_cost: mana_ok,
                            kind: ActionItemKind::Server {
                                action: serde_json::json!({
                                    "type": "cast_spell",
                                    "object_id": obj.id.0,
                                    "targets": [target_val]
                                }),
                            },
                        });
                    }
                }
                // If no legal targets were found, no action is emitted (structural failure).
            }
        }
    }

    // Cycling
    for span in &obj.definition.abilities {
        if let OracleSpan::Parsed(Ability::Cycling(cost)) = span {
            if state.priority_player == pid {
                let player = state.get_player(pid).unwrap();
                let mana_ok = can_pay_mana(cost, &player.mana_pool, player.life);
                actions.push(ActionItemView {
                    label: format!("Cycle ({})", format_mana_cost(cost)),
                    can_pay_cost: mana_ok,
                    kind: ActionItemKind::Server {
                        action: serde_json::json!({
                            "type": "cycle_card",
                            "object_id": obj.id.0
                        }),
                    },
                });
            }
        }
    }

    actions
}

fn compute_battlefield_actions(
    _state: &GameState,
    _pid: PlayerId,
    _obj: &CardObject,
) -> Vec<ActionItemView> {
    vec![] // implemented in Task 5
}
```

- [ ] **Step 3: Wire `compute_actions` into `to_card_view`**

In `build_player_view`, change the `actions: vec![]` line in `to_card_view` to:

```rust
actions: compute_actions(state, pid, obj),
```

- [ ] **Step 4: Run tests**

```bash
cargo test 2>&1 | grep -E "^test result|FAILED|error\["
```

Expected:
- `can_cast_true_for_instant_in_hand_with_mana_and_priority` → PASS
- `can_cast_false_for_creature_when_not_active_player` → PASS
- `can_attack_true_only_for_active_player_at_declare_attackers` → still FAIL (battlefield not done)
- `can_block_true_only_for_defending_player_at_declare_blockers` → still FAIL

- [ ] **Step 5: Commit**

```bash
git add src/serve.rs
git commit -m "feat: compute actions for hand cards (cast, play land, cycle)"
```

---

### Task 5: Implement battlefield action computation

**Files:**
- Modify: `src/serve.rs`

Replaces the stub `compute_battlefield_actions` with the real implementation. After this task all rewritten tests should pass.

- [ ] **Step 1: Replace `compute_battlefield_actions` stub**

Replace the stub with:

```rust
fn compute_battlefield_actions(
    state: &GameState,
    pid: PlayerId,
    obj: &CardObject,
) -> Vec<ActionItemView> {
    let mut actions = Vec::new();

    // Attacker toggle (no cost — can_pay_cost always true)
    if state.step() == Step::DeclareAttackers && pid == state.active_player {
        let can_atk = state
            .battlefield
            .get(&obj.id)
            .map(|p| p.can_attack())
            .unwrap_or(false);
        if can_atk {
            actions.push(ActionItemView {
                label: "Declare as attacker".to_string(),
                can_pay_cost: true,
                kind: ActionItemKind::ToggleAttacker { object_id: obj.id.0 },
            });
        }
    }

    // Blocker assignment (no cost — can_pay_cost always true)
    if state.step() == Step::DeclareBlockers && pid != state.active_player {
        let can_blk = state
            .battlefield
            .get(&obj.id)
            .map(|p| p.can_block())
            .unwrap_or(false);
        if can_blk {
            for &atk_id in &state.combat.attackers {
                let atk_name = state
                    .objects
                    .get(&atk_id)
                    .map(|o| o.definition.name.as_str())
                    .unwrap_or("Unknown");
                actions.push(ActionItemView {
                    label: format!("Block {atk_name}"),
                    can_pay_cost: true,
                    kind: ActionItemKind::AssignBlocker {
                        blocker_id: obj.id.0,
                        attacker_id: atk_id.0,
                    },
                });
            }
        }
    }

    // Activated abilities
    let abilities: Vec<_> = obj
        .definition
        .abilities
        .iter()
        .filter_map(|span| match span {
            OracleSpan::Parsed(Ability::Activated(a)) => Some(a),
            _ => None,
        })
        .enumerate()
        .collect();

    for (i, ability) in &abilities {
        let produces_mana = ability
            .effect
            .iter()
            .any(|e| matches!(e, EffectStep::AddMana(_)));
        // Mana abilities don't need priority; non-mana abilities do (CR 117.1b)
        let structural_ok = produces_mana || state.priority_player == pid;
        if structural_ok {
            let cost_ok = can_pay_cost(state, obj.id, ability, pid);
            actions.push(ActionItemView {
                label: format_activated_ability(ability),
                can_pay_cost: cost_ok,
                kind: ActionItemKind::Server {
                    action: serde_json::json!({
                        "type": "activate_ability",
                        "object_id": obj.id.0,
                        "ability_index": i
                    }),
                },
            });
        }
    }

    actions
}
```

Note: `EffectStep` needs to be in scope here. Add to the top of `serve.rs` if not already present:

```rust
use mecha_oracle::types::effect::EffectStep;
```

- [ ] **Step 2: Run tests**

```bash
cargo test 2>&1 | grep -E "^test result|FAILED|error\["
```

Expected: all tests pass.

- [ ] **Step 3: Run clippy**

```bash
cargo clippy --all-targets 2>&1 | grep -E "^error|warning\[" | head -30
```

Fix any warnings (use `cargo clippy --fix` for mechanical ones).

- [ ] **Step 4: Commit**

```bash
git add src/serve.rs
git commit -m "feat: compute actions for battlefield cards (attack, block, activated abilities)"
```

---

### Task 6: Extract CSS and JS into separate files

**Files:**
- Modify: `src/serve.html` → skeleton only
- Create: `src/serve.css`
- Create: `src/serve.js`
- Modify: `src/serve.rs` → new routes, new `include_str!` constants

- [ ] **Step 1: Create `src/serve.css`**

Create `src/serve.css` and move all content from between `<style>` and `</style>` in `serve.html` into it verbatim (the entire block from `* { box-sizing: border-box; margin: 0; padding: 0; }` through `.popup-item.active { ... }`). Do not include the `<style>` tags themselves.

- [ ] **Step 2: Create `src/serve.js`**

Create `src/serve.js` and move all content from between `<script>` and `</script>` in `serve.html` into it verbatim. Do not include the `<script>` tags themselves.

- [ ] **Step 3: Replace `serve.html` with a skeleton**

Replace `src/serve.html` with:

```html
<!DOCTYPE html>
<html lang="en">
<head>
<meta charset="UTF-8">
<title>Mecha-Oracle</title>
<link rel="stylesheet" href="/static/app.css">
</head>
<body>
<div id="root">
  <div id="board">
    <div class="hand-row p2" id="p2-hand"></div>
    <div class="player-section p2" id="p2-section">
      <div class="player-header">
        <span class="player-name p2">Player 2</span>
        <span class="life p2" id="p2-life">♥ 20</span>
        <div class="mana-pool" id="p2-mana"></div>
        <div class="zone-info">
          <div class="library-pile" id="p2-lib"><span class="pile-count">0</span><span class="pile-label">Library</span></div>
          <div class="gy-wrap" id="p2-gy-wrap" onclick="openGY(2)">
            <div class="gy-card"></div><div class="gy-card"></div>
            <div class="gy-card" id="p2-gy-top"></div>
            <span class="gy-label" id="p2-gy-label">GY (0)</span>
          </div>
        </div>
      </div>
      <div class="zone-row"><span class="zone-label">Lands</span><div id="p2-lands" style="display:flex;gap:6px;flex-wrap:wrap"></div></div>
      <div class="zone-row"><span class="zone-label">Creatures</span><div id="p2-creatures" style="display:flex;gap:6px;flex-wrap:wrap"></div></div>
    </div>
    <div id="action-bar"></div>
    <div class="player-section p1" id="p1-section">
      <div class="zone-row"><span class="zone-label">Creatures</span><div id="p1-creatures" style="display:flex;gap:6px;flex-wrap:wrap"></div></div>
      <div class="zone-row"><span class="zone-label">Lands</span><div id="p1-lands" style="display:flex;gap:6px;flex-wrap:wrap"></div></div>
      <div class="player-header">
        <span class="player-name p1">Player 1</span>
        <span class="life p1" id="p1-life">♥ 20</span>
        <div class="mana-pool" id="p1-mana"></div>
        <div class="zone-info">
          <div class="library-pile" id="p1-lib"><span class="pile-count">0</span><span class="pile-label">Library</span></div>
          <div class="gy-wrap" id="p1-gy-wrap" onclick="openGY(1)">
            <div class="gy-card"></div><div class="gy-card"></div>
            <div class="gy-card" id="p1-gy-top"></div>
            <span class="gy-label" id="p1-gy-label">GY (0)</span>
          </div>
        </div>
      </div>
    </div>
    <div class="hand-row p1" id="p1-hand"></div>
  </div>
  <div id="stack-col">
    <div class="stack-edge-label">top</div>
    <div id="stack-items">
      <div id="stack-empty">Stack</div>
    </div>
    <div class="stack-edge-label">bottom</div>
  </div>
  <div id="log-drawer">
    <div id="log-drawer-header">
      <span style="font-size:10px;text-transform:uppercase;letter-spacing:1px;color:#888">Game Log</span>
      <button onclick="toggleLog()" style="background:none;border:none;color:#555;cursor:pointer;font-size:14px;line-height:1">✕</button>
    </div>
    <div id="log-entries"></div>
  </div>
</div>

<!-- GY Modal -->
<div id="gy-modal">
  <div class="gy-modal-box">
    <h3 id="gy-modal-title">Graveyard</h3>
    <div id="gy-modal-cards"></div>
    <button class="gy-modal-close" onclick="closeGY()">Close</button>
  </div>
</div>

<!-- Server disconnect overlay -->
<div id="server-overlay">
  <div class="server-overlay-box">
    <div class="overlay-title">Server Disconnected</div>
    <div class="overlay-sub">Attempting to reconnect…</div>
  </div>
</div>

<!-- Toast notification -->
<div id="toast"></div>

<!-- Popup disambiguation menu -->
<div id="popup"></div>

<script src="/static/app.js"></script>
</body>
</html>
```

- [ ] **Step 4: Add CSS/JS routes to `serve.rs`**

Add these constants near the top of `serve.rs` alongside `INDEX_HTML`:

```rust
const INDEX_HTML: &str = include_str!("serve.html");
const STYLE_CSS: &str  = include_str!("serve.css");
const APP_JS: &str     = include_str!("serve.js");
```

Add handler functions:

```rust
async fn css_handler() -> impl IntoResponse {
    ([(axum::http::header::CONTENT_TYPE, "text/css")], STYLE_CSS)
}

async fn js_handler() -> impl IntoResponse {
    (
        [(axum::http::header::CONTENT_TYPE, "application/javascript")],
        APP_JS,
    )
}
```

Add routes to the router in `run()`:

```rust
let router = Router::new()
    .route("/", get(index_handler))
    .route("/static/app.css", get(css_handler))
    .route("/static/app.js", get(js_handler))
    .route("/state", get(state_handler))
    .route("/action", post(action_handler))
    .with_state(app_state);
```

- [ ] **Step 5: Confirm it compiles and tests still pass**

```bash
cargo test 2>&1 | grep -E "^test result|FAILED|error\["
```

Expected: all pass.

- [ ] **Step 6: Commit**

```bash
git add src/serve.html src/serve.css src/serve.js src/serve.rs
git commit -m "refactor: extract CSS and JS from serve.html into separate embedded files"
```

---

### Task 7: Refactor JS — new action dispatch and combined click handler

**Files:**
- Modify: `src/serve.js`

Removes `getBattlefieldCreatureActions`, `getBattlefieldLandActions`, `getHandActions`, `isCardActionable`. Adds `findCard`, `dispatchAction`, `buildPopupItems`. Replaces `handleCardClick` with a combined version accepting `autoDispatchIfSingle`. Updates `cardHTML` visual state and `oncontextmenu` attribute.

- [ ] **Step 1: Remove old action-building functions and `isCardActionable`**

Delete these functions from `serve.js`:
- `isCardActionable`
- `getBattlefieldCreatureActions`
- `getBattlefieldLandActions`
- `getHandActions`

- [ ] **Step 2: Add `findCard`, `dispatchAction`, `buildPopupItems`**

Add these functions in the `// ── Card click dispatch` section:

```js
function findCard(cardId, pid) {
    const p = pid === 0 ? currentState.p1 : currentState.p2;
    return p.hand.find(c => c.id === cardId)
        || p.lands.find(c => c.id === cardId)
        || p.creatures.find(c => c.id === cardId);
}

function dispatchAction(item) {
    if (item.kind === 'server') {
        sendAction(item.action);
    } else if (item.kind === 'toggle_attacker') {
        const idx = attackersSelected.indexOf(item.object_id);
        if (idx >= 0) attackersSelected.splice(idx, 1);
        else attackersSelected.push(item.object_id);
        render(currentState);
    } else if (item.kind === 'assign_blocker') {
        if (blockersAssignment[item.blocker_id] === item.attacker_id)
            delete blockersAssignment[item.blocker_id];
        else
            blockersAssignment[item.blocker_id] = item.attacker_id;
        render(currentState);
    }
}

function buildPopupItems(actions) {
    return actions.map(a => ({
        label: a.label,
        disabled: !a.can_pay_cost,
        onClick: a.can_pay_cost ? () => dispatchAction(a) : () => {},
    }));
}
```

- [ ] **Step 3: Replace `handleCardClick` with combined version**

Replace the existing `handleCardClick` function with:

```js
// autoDispatchIfSingle=true for left-click; false for right-click (always show popup)
function handleCardClick(cardId, pid, event, autoDispatchIfSingle) {
    if (!autoDispatchIfSingle) event.preventDefault();
    if (!currentState) return;
    closePopup();
    const card = pid >= 0 ? findCard(cardId, pid) : null;
    const actions = card ? card.actions : [];

    if (autoDispatchIfSingle) {
        if (actions.length === 1 && actions[0].can_pay_cost) {
            dispatchAction(actions[0]); return;
        }
        if (actions.length === 0) return;
    }

    const items = actions.length > 0
        ? buildPopupItems(actions)
        : [{ label: 'No valid actions', onClick: () => {} }];
    openPopup(items, event.target, 'Actions');
}
```

- [ ] **Step 4: Update `cardHTML` visual state and wire right-click**

In `cardHTML`, replace the `isCardActionable` call with an `actions`-based check:

```js
// Replace this block:
if (!isSelected && !card.is_attacking && !card.is_blocking) {
    if (s && pid !== undefined && isCardActionable(card, s, pid, zone)) {
        classes += ' actionable';
    } else {
        classes += ' dim';
    }
}

// With this:
if (!isSelected && !card.is_attacking && !card.is_blocking) {
    if (card.actions && card.actions.some(a => a.can_pay_cost)) {
        classes += ' actionable';
    } else {
        classes += ' dim';
    }
}
```

In `cardHTML`, change the click attribute line from:

```js
const clickAttr = `onclick="handleCardClick(${card.id}, ${pid !== undefined ? pid : -1}, event)"`;
```

to:

```js
const pid_ = pid !== undefined ? pid : -1;
const clickAttr = `onclick="handleCardClick(${card.id}, ${pid_}, event, true)" oncontextmenu="handleCardClick(${card.id}, ${pid_}, event, false)"`;
```

- [ ] **Step 5: Update `openPopup` to handle `disabled` items**

In `openPopup`, change the button construction from:

```js
items.map((item, i) =>
    `<button class="popup-item${item.active ? ' active' : ''}" data-idx="${i}">${esc(item.label)}</button>`
).join('');
```

to:

```js
items.map((item, i) =>
    `<button class="popup-item${item.active ? ' active' : ''}${item.disabled ? ' disabled' : ''}" data-idx="${i}">${esc(item.label)}</button>`
).join('');
```

And change the click wiring to skip disabled items:

```js
popup.querySelectorAll('.popup-item').forEach((btn, i) => {
    btn.addEventListener('click', e => {
        e.stopPropagation();
        if (items[i].disabled) return;
        closePopup();
        items[i].onClick();
    });
});
```

- [ ] **Step 6: Verify the server runs and basic interactions work**

```bash
cargo build 2>&1 | grep "^error"
```

Start the server in a separate terminal:
```bash
cargo run -- --deck decks/default.json
```

Open http://localhost:3000. Verify:
- Cards render with dim/actionable styling
- Left-click on a land taps it for mana (single payable action, fires directly)
- Right-click on any card opens the popup even with only one item
- Right-click on a tapped land shows the ability greyed out
- Right-click on a card with no actions shows "No valid actions"

- [ ] **Step 7: Commit**

```bash
git add src/serve.js
git commit -m "refactor: replace JS action-building with Rust-provided actions dispatch"
```

---

### Task 8: Add CSS for disabled popup items

**Files:**
- Modify: `src/serve.css`

- [ ] **Step 1: Add disabled popup item styles**

Append to the popup section in `serve.css` (after `.popup-item.active { ... }`):

```css
.popup-item.disabled { color: #555; border-color: #2a3a4a; cursor: default; }
.popup-item.disabled:hover { background: #1c2a3a; border-color: #2a4a6a; }
```

- [ ] **Step 2: Verify greyed-out items look correct**

Restart the server, right-click a tapped land — the mana ability should appear greyed. Right-click a card with insufficient mana for a spell — the cast action should appear greyed.

- [ ] **Step 3: Commit**

```bash
git add src/serve.css
git commit -m "style: disabled popup item appearance for unaffordable actions"
```

---

### Task 9: Cleanup — remove dead code, run clippy, update todo.md

**Files:**
- Modify: `src/serve.rs`
- Modify: `docs/todo.md`

- [ ] **Step 1: Run clippy and fix warnings**

```bash
cargo clippy --fix --all-targets 2>&1 | grep -E "^error|warning\["
cargo clippy --all-targets 2>&1 | grep -E "^error|warning\["
```

Expected: clean output. Common things to fix:
- Dead `compute_can_cast` function if still present (delete it)
- Any unused import warnings from the removed `TargetView`/`ActivatedAbilityView` types

- [ ] **Step 2: Run full test suite**

```bash
cargo test 2>&1 | grep -E "^test result|FAILED|error\["
```

Expected: all tests pass.

- [ ] **Step 3: Remove the thin UI bullet from `docs/todo.md`**

Delete this line from `docs/todo.md`:

```
- Make the UI layer in Javascript as thin as possible; have it query the server for a list of allowed actions, and populate the UI with the provided actions. Similar for valid targets, mana costs, etc. The web UI should be literally just a bunch of buttons which are connected to server API calls; there should be no validation done in JS, only in Rust.
```

- [ ] **Step 4: Commit**

```bash
git add src/serve.rs docs/todo.md
git commit -m "chore: remove dead code, clean clippy warnings, close thin-UI todo item"
```
