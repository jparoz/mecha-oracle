# Combat Declaration Auto-Skip Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Auto-skip Declare Attackers / Declare Blockers when no legal declarations exist, and filter the blocker-assignment UI to only show evasion-legal pairings.

**Architecture:** Extract a `pub fn can_block_attacker` from `declare_blockers`; use it to filter UI actions in `compute_battlefield_actions` and to drive `has_valid_attackers` / `has_valid_blockers` predicates that auto-skip in `apply_step_start_loop`.

**Tech Stack:** Rust, mecha-oracle engine (`src/engine/combat.rs`) and serve layer (`src/serve.rs`).

---

## File Map

- Modify: `src/engine/combat.rs` — add `pub fn can_block_attacker`; refactor `declare_blockers` to call it
- Modify: `src/serve.rs` — import `can_block_attacker`; update `compute_battlefield_actions`; add `has_valid_attackers` / `has_valid_blockers`; update `apply_step_start_loop`; fix four broken existing tests

---

## Task 1: Extract `can_block_attacker` from `declare_blockers`

**Files:**
- Modify: `src/engine/combat.rs`

- [ ] **Step 1: Write failing tests for `can_block_attacker`**

Add these tests inside the `#[cfg(test)]` block at the bottom of `src/engine/combat.rs`, after the last existing test:

```rust
#[test]
fn can_block_attacker_vanilla_vs_vanilla() {
    let mut gs = make_combat_state();
    let attacker = keyword_creature(&mut gs, PlayerId(0), 2, 2, vec![]);
    let blocker = keyword_creature(&mut gs, PlayerId(1), 2, 2, vec![]);
    gs = declare_attackers(gs, PlayerId(0), &[attacker]).unwrap();
    assert!(can_block_attacker(&gs, blocker, attacker));
}

#[test]
fn can_block_attacker_ground_cannot_block_flier() {
    let mut gs = make_combat_state();
    let attacker = keyword_creature(&mut gs, PlayerId(0), 2, 2, vec![StaticAbility::Flying]);
    let blocker = keyword_creature(&mut gs, PlayerId(1), 2, 2, vec![]);
    gs = declare_attackers(gs, PlayerId(0), &[attacker]).unwrap();
    assert!(!can_block_attacker(&gs, blocker, attacker));
}

#[test]
fn can_block_attacker_reach_can_block_flier() {
    let mut gs = make_combat_state();
    let attacker = keyword_creature(&mut gs, PlayerId(0), 2, 2, vec![StaticAbility::Flying]);
    let blocker = keyword_creature(&mut gs, PlayerId(1), 2, 2, vec![StaticAbility::Reach]);
    gs = declare_attackers(gs, PlayerId(0), &[attacker]).unwrap();
    assert!(can_block_attacker(&gs, blocker, attacker));
}

#[test]
fn can_block_attacker_non_shadow_cannot_block_shadow() {
    let mut gs = make_combat_state();
    let attacker = keyword_creature(&mut gs, PlayerId(0), 2, 2, vec![StaticAbility::Shadow]);
    let blocker = keyword_creature(&mut gs, PlayerId(1), 2, 2, vec![]);
    gs = declare_attackers(gs, PlayerId(0), &[attacker]).unwrap();
    assert!(!can_block_attacker(&gs, blocker, attacker));
}

#[test]
fn can_block_attacker_shadow_cannot_block_non_shadow() {
    let mut gs = make_combat_state();
    let attacker = keyword_creature(&mut gs, PlayerId(0), 2, 2, vec![]);
    let blocker = keyword_creature(&mut gs, PlayerId(1), 2, 2, vec![StaticAbility::Shadow]);
    gs = declare_attackers(gs, PlayerId(0), &[attacker]).unwrap();
    assert!(!can_block_attacker(&gs, blocker, attacker));
}

#[test]
fn can_block_attacker_shadow_can_block_shadow() {
    let mut gs = make_combat_state();
    let attacker = keyword_creature(&mut gs, PlayerId(0), 2, 2, vec![StaticAbility::Shadow]);
    let blocker = keyword_creature(&mut gs, PlayerId(1), 2, 2, vec![StaticAbility::Shadow]);
    gs = declare_attackers(gs, PlayerId(0), &[attacker]).unwrap();
    assert!(can_block_attacker(&gs, blocker, attacker));
}

#[test]
fn can_block_attacker_non_horsemanship_cannot_block_horsemanship() {
    let mut gs = make_combat_state();
    let attacker =
        keyword_creature(&mut gs, PlayerId(0), 2, 2, vec![StaticAbility::Horsemanship]);
    let blocker = keyword_creature(&mut gs, PlayerId(1), 2, 2, vec![]);
    gs = declare_attackers(gs, PlayerId(0), &[attacker]).unwrap();
    assert!(!can_block_attacker(&gs, blocker, attacker));
}

#[test]
fn can_block_attacker_skulk_not_blockable_by_greater_power() {
    let mut gs = make_combat_state();
    let attacker = keyword_creature(&mut gs, PlayerId(0), 1, 1, vec![StaticAbility::Skulk]);
    let blocker = keyword_creature(&mut gs, PlayerId(1), 2, 2, vec![]);
    gs = declare_attackers(gs, PlayerId(0), &[attacker]).unwrap();
    assert!(!can_block_attacker(&gs, blocker, attacker));
}

#[test]
fn can_block_attacker_skulk_blockable_by_equal_power() {
    let mut gs = make_combat_state();
    let attacker = keyword_creature(&mut gs, PlayerId(0), 2, 2, vec![StaticAbility::Skulk]);
    let blocker = keyword_creature(&mut gs, PlayerId(1), 2, 2, vec![]);
    gs = declare_attackers(gs, PlayerId(0), &[attacker]).unwrap();
    assert!(can_block_attacker(&gs, blocker, attacker));
}

#[test]
fn can_block_attacker_decayed_cannot_block() {
    let mut gs = make_combat_state();
    let attacker = keyword_creature(&mut gs, PlayerId(0), 2, 2, vec![]);
    let blocker = keyword_creature(&mut gs, PlayerId(1), 2, 2, vec![StaticAbility::Decayed]);
    gs = declare_attackers(gs, PlayerId(0), &[attacker]).unwrap();
    assert!(!can_block_attacker(&gs, blocker, attacker));
}
```

- [ ] **Step 2: Confirm tests fail to compile**

```bash
cargo test can_block_attacker 2>&1 | grep -E "^test result|FAILED|error\["
```

Expected: compile error — `can_block_attacker` not found.

- [ ] **Step 3: Implement `can_block_attacker`**

Add this function to `src/engine/combat.rs` immediately after `declare_blockers` ends (before `/// Deal combat damage`):

```rust
/// CR 509.1: returns true if `blocker_id` can legally block `attacker_id`.
/// Checks per-pair evasion rules. Does not check menace (a whole-declaration constraint).
pub fn can_block_attacker(state: &GameState, blocker_id: ObjectId, attacker_id: ObjectId) -> bool {
    let Some(blocker_perm) = state.battlefield.get(&blocker_id) else {
        return false;
    };
    let Some(blocker_obj) = state.objects.get(&blocker_id) else {
        return false;
    };
    let Some(attacker_obj) = state.objects.get(&attacker_id) else {
        return false;
    };
    if !blocker_perm.can_block() {
        return false;
    }
    // CR 702.9b: flying
    if attacker_obj.has_keyword(StaticAbility::Flying)
        && !blocker_obj.has_keyword(StaticAbility::Flying)
        && !blocker_obj.has_keyword(StaticAbility::Reach)
    {
        return false;
    }
    // CR 702.28b: shadow
    if attacker_obj.has_keyword(StaticAbility::Shadow)
        != blocker_obj.has_keyword(StaticAbility::Shadow)
    {
        return false;
    }
    // CR 702.31b: horsemanship
    if attacker_obj.has_keyword(StaticAbility::Horsemanship)
        && !blocker_obj.has_keyword(StaticAbility::Horsemanship)
    {
        return false;
    }
    // CR 702.118b: skulk
    if attacker_obj.has_keyword(StaticAbility::Skulk) {
        let attacker_power = state
            .battlefield
            .get(&attacker_id)
            .and_then(|p| p.effective_power())
            .unwrap_or(0);
        let blocker_power = state
            .battlefield
            .get(&blocker_id)
            .and_then(|p| p.effective_power())
            .unwrap_or(0);
        if blocker_power > attacker_power {
            return false;
        }
    }
    true
}
```

- [ ] **Step 4: Refactor `declare_blockers` to use `can_block_attacker`**

Inside the `for &(blocker_id, attacker_id) in blocks {` loop in `declare_blockers`, replace the entire block of inline evasion checks (the flying/shadow/decayed/horsemanship/skulk blocks, roughly lines 101–157) with a single call. The loop body should become:

```rust
    for &(blocker_id, attacker_id) in blocks {
        let obj = state
            .objects
            .get(&blocker_id)
            .ok_or(EngineError::CardNotFound)?;
        if obj.controller != player_id {
            return Err(EngineError::NotYourCard);
        }
        if !obj.is_creature() {
            return Err(EngineError::NotACreature);
        }
        let perm = state
            .battlefield
            .get(&blocker_id)
            .ok_or(EngineError::CardNotFound)?;
        if perm.tapped {
            return Err(EngineError::CreatureTapped);
        }
        if !state.combat.attackers.contains(&attacker_id) {
            return Err(EngineError::CannotCastNow);
        }
        if !can_block_attacker(&state, blocker_id, attacker_id) {
            return Err(EngineError::InvalidBlocker);
        }
    }
```

The `perm.tapped` check is kept separate to preserve the `CreatureTapped` error variant. `can_block_attacker` handles all evasion rules (including `Decayed` via `can_block()`).

- [ ] **Step 5: Run all combat tests**

```bash
cargo test --lib engine::combat 2>&1 | grep -E "^test result|FAILED|error\["
```

Expected: `test result: ok.` with all tests passing (10 new + all prior).

- [ ] **Step 6: Commit**

```bash
git add src/engine/combat.rs
git commit -m "refactor: extract can_block_attacker from declare_blockers"
```

---

## Task 2: Filter blocker UI actions by evasion

**Files:**
- Modify: `src/serve.rs`

The test uses two P0 attackers (one flying, one ground) and one P1 ground blocker. This ensures `has_valid_blockers` returns true after Task 3 lands (blocking the ground attacker is legal), so this test won't be broken by the auto-skip added in Task 3.

- [ ] **Step 1: Write a failing test**

Add inside the `#[cfg(test)]` mod in `src/serve.rs`:

```rust
#[test]
fn blocker_ui_only_shows_valid_pairings() {
    use mecha_oracle::types::ability::StaticAbility;
    use mecha_oracle::types::card::{CardDefinition, CardType, TypeLine};
    use mecha_oracle::types::{Ability, CardObject, OracleSpan, Zone};

    let db = test_db();
    let config = vec![
        (0..10).map(|_| "Forest".to_string()).collect(),
        (0..10).map(|_| "Forest".to_string()).collect(),
    ];
    let mut gs = build_game_state(config, &db, false).unwrap();

    // P0: a flying attacker
    let flying_atk = {
        let id = gs.alloc_id();
        let def = CardDefinition {
            name: "Flying Attacker".into(),
            mana_cost: None,
            type_line: TypeLine {
                supertypes: vec![],
                card_types: vec![CardType::Creature],
                subtypes: vec![],
            },
            oracle_text: String::new(),
            abilities: vec![OracleSpan::Parsed(Ability::Static(StaticAbility::Flying))],
            power: Some(2),
            toughness: Some(2),
        };
        let obj = CardObject::new(id, def, PlayerId(0), Zone::Battlefield);
        let mut perm = PermanentState::new(&obj.definition);
        perm.controller_since_turn = 0;
        gs.battlefield.insert(id, perm);
        gs.add_object(obj);
        id
    };
    // P0: a ground attacker
    let ground_atk = {
        let id = gs.alloc_id();
        let obj = CardObject::new(
            id,
            db.get("Grizzly Bears").unwrap().clone(),
            PlayerId(0),
            Zone::Battlefield,
        );
        let mut perm = PermanentState::new(&obj.definition);
        perm.controller_since_turn = 0;
        gs.battlefield.insert(id, perm);
        gs.add_object(obj);
        id
    };
    // P1: a ground blocker (can block ground_atk but not flying_atk)
    let ground_blk = {
        let id = gs.alloc_id();
        let obj = CardObject::new(
            id,
            db.get("Grizzly Bears").unwrap().clone(),
            PlayerId(1),
            Zone::Battlefield,
        );
        let mut perm = PermanentState::new(&obj.definition);
        perm.controller_since_turn = 0;
        gs.battlefield.insert(id, perm);
        gs.add_object(obj);
        id
    };

    // Navigate to DeclareAttackers and declare both P0 creatures
    for _ in 0..4 {
        gs = dispatch_action(gs, ActionRequest::AdvanceStep).unwrap();
    }
    assert_eq!(gs.step(), Step::DeclareAttackers);
    gs = dispatch_action(
        gs,
        ActionRequest::DeclareAttackers {
            attacker_ids: vec![flying_atk.0, ground_atk.0],
        },
    )
    .unwrap();
    for _ in 0..2 {
        gs = dispatch_action(gs, ActionRequest::AdvanceStep).unwrap();
    }
    assert_eq!(gs.step(), Step::DeclareBlockers);

    let view = build_game_view(&gs);
    let blk_card = view
        .p2
        .creatures
        .iter()
        .find(|c| c.id == ground_blk)
        .unwrap();

    let blocker_targets: Vec<u64> = blk_card
        .actions
        .iter()
        .filter_map(|a| {
            if let ActionItemKind::AssignBlocker { attacker_id, .. } = a.kind {
                Some(attacker_id)
            } else {
                None
            }
        })
        .collect();

    assert!(
        blocker_targets.contains(&ground_atk.0),
        "ground blocker should be offered as blocker for ground attacker"
    );
    assert!(
        !blocker_targets.contains(&flying_atk.0),
        "ground blocker must not be offered as blocker for flying attacker"
    );
}
```

- [ ] **Step 2: Confirm the test fails**

```bash
cargo test blocker_ui_only_shows_valid_pairings 2>&1 | grep -E "^test result|FAILED|error\["
```

Expected: FAILED (the ground blocker currently shows actions for both attackers).

- [ ] **Step 3: Import `can_block_attacker` in `src/serve.rs`**

Find line 10:
```rust
use mecha_oracle::engine::combat::{declare_attackers, declare_blockers};
```
Replace with:
```rust
use mecha_oracle::engine::combat::{can_block_attacker, declare_attackers, declare_blockers};
```

- [ ] **Step 4: Update `compute_battlefield_actions` blocker block**

Find the blocker-assignment block (the `if state.step() == Step::DeclareBlockers && pid != state.active_player {` block, roughly lines 571–595):

```rust
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
```

Replace with:

```rust
    // Blocker assignment (no cost — can_pay_cost always true)
    if state.step() == Step::DeclareBlockers && pid != state.active_player {
        for &atk_id in &state.combat.attackers {
            if !can_block_attacker(state, obj.id, atk_id) {
                continue;
            }
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
```

`can_block_attacker` already calls `perm.can_block()` internally, so the outer `can_blk` guard is no longer needed.

- [ ] **Step 5: Confirm new test passes and no regression**

```bash
cargo test blocker_ui_only_shows_valid_pairings 2>&1 | grep -E "^test result|FAILED|error\["
cargo test 2>&1 | grep -E "^test result|FAILED|error\["
```

Both expected: `test result: ok.`

- [ ] **Step 6: Commit**

```bash
git add src/serve.rs
git commit -m "feat: filter blocker UI actions by evasion compatibility"
```

---

## Task 3: Auto-skip and fix existing tests

**Files:**
- Modify: `src/serve.rs`

- [ ] **Step 1: Write failing auto-skip tests**

Add inside the `#[cfg(test)]` mod in `src/serve.rs`:

```rust
#[test]
fn autoskips_declare_attackers_when_no_valid_attackers() {
    // All-Forest deck: no creatures → auto-skip both DA and DB
    let config = vec![
        (0..10).map(|_| "Forest".to_string()).collect(),
        (0..10).map(|_| "Forest".to_string()).collect(),
    ];
    let db = test_db();
    let mut gs = build_game_state(config, &db, false).unwrap();
    // 2 passes → BOC; 2 more passes → DA auto-skip → DB auto-skip → CombatDamage
    for _ in 0..4 {
        gs = dispatch_action(gs, ActionRequest::AdvanceStep).unwrap();
    }
    assert_eq!(
        gs.step(),
        Step::CombatDamage,
        "should have auto-skipped DA and DB to reach CombatDamage"
    );
    assert!(gs.combat.attackers_declared);
    assert!(gs.combat.blockers_declared);
}

#[test]
fn no_autoskip_declare_attackers_when_valid_attacker_exists() {
    use mecha_oracle::types::{CardObject, Zone};
    let db = test_db();
    let config = vec![
        (0..10).map(|_| "Forest".to_string()).collect(),
        (0..10).map(|_| "Forest".to_string()).collect(),
    ];
    let mut gs = build_game_state(config, &db, false).unwrap();
    // Add an untapped, non-sick creature for P0
    let id = gs.alloc_id();
    let obj = CardObject::new(
        id,
        db.get("Grizzly Bears").unwrap().clone(),
        PlayerId(0),
        Zone::Battlefield,
    );
    let mut perm = PermanentState::new(&obj.definition);
    perm.controller_since_turn = 0;
    gs.battlefield.insert(id, perm);
    gs.add_object(obj);

    for _ in 0..4 {
        gs = dispatch_action(gs, ActionRequest::AdvanceStep).unwrap();
    }
    assert_eq!(
        gs.step(),
        Step::DeclareAttackers,
        "should stop at DA when a valid attacker exists"
    );
    assert!(!gs.combat.attackers_declared);
}

#[test]
fn autoskips_declare_blockers_when_no_valid_blocker_for_any_attacker() {
    // P0 has a flying attacker; P1 has only a ground creature — no valid blockers.
    use mecha_oracle::types::ability::StaticAbility;
    use mecha_oracle::types::card::{CardDefinition, CardType, TypeLine};
    use mecha_oracle::types::{Ability, CardObject, OracleSpan, Zone};
    let db = test_db();
    let config = vec![
        (0..10).map(|_| "Forest".to_string()).collect(),
        (0..10).map(|_| "Forest".to_string()).collect(),
    ];
    let mut gs = build_game_state(config, &db, false).unwrap();

    let flying_id = {
        let id = gs.alloc_id();
        let def = CardDefinition {
            name: "Flying Attacker".into(),
            mana_cost: None,
            type_line: TypeLine {
                supertypes: vec![],
                card_types: vec![CardType::Creature],
                subtypes: vec![],
            },
            oracle_text: String::new(),
            abilities: vec![OracleSpan::Parsed(Ability::Static(StaticAbility::Flying))],
            power: Some(2),
            toughness: Some(2),
        };
        let obj = CardObject::new(id, def, PlayerId(0), Zone::Battlefield);
        let mut perm = PermanentState::new(&obj.definition);
        perm.controller_since_turn = 0;
        gs.battlefield.insert(id, perm);
        gs.add_object(obj);
        id
    };
    {
        let id = gs.alloc_id();
        let obj = CardObject::new(
            id,
            db.get("Grizzly Bears").unwrap().clone(),
            PlayerId(1),
            Zone::Battlefield,
        );
        let mut perm = PermanentState::new(&obj.definition);
        perm.controller_since_turn = 0;
        gs.battlefield.insert(id, perm);
        gs.add_object(obj);
    }

    for _ in 0..4 {
        gs = dispatch_action(gs, ActionRequest::AdvanceStep).unwrap();
    }
    assert_eq!(gs.step(), Step::DeclareAttackers);
    gs = dispatch_action(
        gs,
        ActionRequest::DeclareAttackers {
            attacker_ids: vec![flying_id.0],
        },
    )
    .unwrap();
    // 2 passes → transition to DB triggers auto-skip → CombatDamage
    for _ in 0..2 {
        gs = dispatch_action(gs, ActionRequest::AdvanceStep).unwrap();
    }
    assert_eq!(
        gs.step(),
        Step::CombatDamage,
        "should have auto-skipped DB since ground creature cannot block flying attacker"
    );
    assert!(gs.combat.blockers_declared);
}

#[test]
fn no_autoskip_declare_blockers_when_valid_blocker_exists() {
    use mecha_oracle::types::{CardObject, Zone};
    let db = test_db();
    let config = vec![
        (0..10).map(|_| "Forest".to_string()).collect(),
        (0..10).map(|_| "Forest".to_string()).collect(),
    ];
    let mut gs = build_game_state(config, &db, false).unwrap();

    let p0_id = {
        let id = gs.alloc_id();
        let obj = CardObject::new(
            id,
            db.get("Grizzly Bears").unwrap().clone(),
            PlayerId(0),
            Zone::Battlefield,
        );
        let mut perm = PermanentState::new(&obj.definition);
        perm.controller_since_turn = 0;
        gs.battlefield.insert(id, perm);
        gs.add_object(obj);
        id
    };
    {
        let id = gs.alloc_id();
        let obj = CardObject::new(
            id,
            db.get("Grizzly Bears").unwrap().clone(),
            PlayerId(1),
            Zone::Battlefield,
        );
        let mut perm = PermanentState::new(&obj.definition);
        perm.controller_since_turn = 0;
        gs.battlefield.insert(id, perm);
        gs.add_object(obj);
    }

    for _ in 0..4 {
        gs = dispatch_action(gs, ActionRequest::AdvanceStep).unwrap();
    }
    assert_eq!(gs.step(), Step::DeclareAttackers);
    gs = dispatch_action(
        gs,
        ActionRequest::DeclareAttackers {
            attacker_ids: vec![p0_id.0],
        },
    )
    .unwrap();
    for _ in 0..2 {
        gs = dispatch_action(gs, ActionRequest::AdvanceStep).unwrap();
    }
    assert_eq!(
        gs.step(),
        Step::DeclareBlockers,
        "should stop at DB when P1 has a valid blocker"
    );
    assert!(!gs.combat.blockers_declared);
}
```

- [ ] **Step 2: Confirm all four new tests fail**

```bash
cargo test "autoskips_declare_attackers|no_autoskip_declare_attackers|autoskips_declare_blockers|no_autoskip_declare_blockers" 2>&1 | grep -E "^test result|FAILED|error\["
```

Expected: all 4 FAILED.

- [ ] **Step 3: Add `has_valid_attackers` and `has_valid_blockers` to `src/serve.rs`**

Add these two functions immediately before the `fn apply_step_start_loop` function:

```rust
fn has_valid_attackers(state: &GameState) -> bool {
    let cmt = state.controllers_most_recent_turn(state.active_player);
    state.battlefield.iter().any(|(&id, perm)| {
        state
            .objects
            .get(&id)
            .map(|o| o.controller == state.active_player)
            .unwrap_or(false)
            && perm.can_attack(cmt)
    })
}

fn has_valid_blockers(state: &GameState) -> bool {
    let defender = state.opponent_of(state.active_player);
    state.combat.attackers.iter().any(|&atk_id| {
        state.battlefield.keys().any(|&blk_id| {
            state
                .objects
                .get(&blk_id)
                .map(|o| o.controller == defender)
                .unwrap_or(false)
                && can_block_attacker(state, blk_id, atk_id)
        })
    })
}
```

- [ ] **Step 4: Replace `apply_step_start_loop` in `src/serve.rs`**

Find:
```rust
fn apply_step_start_loop(mut state: GameState) -> GameState {
    loop {
        state = apply_step_start(state);
        if !matches!(state.step(), Step::Untap | Step::Cleanup) || state.is_game_over() {
            break;
        }
        state = advance_step(state);
    }
    state
}
```

Replace with:
```rust
fn apply_step_start_loop(mut state: GameState) -> GameState {
    loop {
        state = apply_step_start(state);
        if state.is_game_over() {
            break;
        }
        let step = state.step();
        if step == Step::DeclareAttackers && !has_valid_attackers(&state) {
            let active = state.active_player;
            state = declare_attackers(state, active, &[])
                .expect("auto-declare empty attackers cannot fail");
        } else if step == Step::DeclareBlockers && !has_valid_blockers(&state) {
            let defender = state.opponent_of(state.active_player);
            state = declare_blockers(state, defender, &[])
                .expect("auto-declare empty blockers cannot fail");
        } else if !matches!(step, Step::Untap | Step::Cleanup) {
            break;
        }
        state = advance_step(state);
    }
    state
}
```

- [ ] **Step 5: Confirm the four new tests now pass**

```bash
cargo test "autoskips_declare_attackers|no_autoskip_declare_attackers|autoskips_declare_blockers|no_autoskip_declare_blockers" 2>&1 | grep -E "^test result|FAILED|error\["
```

Expected: all 4 pass.

- [ ] **Step 6: Run full suite to identify newly broken tests**

```bash
cargo test 2>&1 | grep -E "^test result|FAILED|error\["
```

Expected: 4 existing tests fail:
- `advance_step_blocked_before_attackers_declared`
- `advance_step_blocked_before_blockers_declared`
- `game_view_includes_combat_declared_flags`
- `advancing_from_end_step_auto_advances_to_next_upkeep`

- [ ] **Step 7: Fix `advance_step_blocked_before_attackers_declared`**

This test previously relied on a no-creature state to get stuck at DA. Now DA auto-skips without creatures. Add a P0 creature to prevent auto-skip while still verifying the block:

```rust
#[test]
fn advance_step_blocked_before_attackers_declared() {
    use mecha_oracle::types::{CardObject, Zone};
    let config = vec![
        (0..10).map(|_| "Forest".to_string()).collect(),
        (0..10).map(|_| "Forest".to_string()).collect(),
    ];
    let db = test_db();
    let mut gs = build_game_state(config, &db, false).unwrap();
    // Add an untapped creature for P0 so DA is not auto-skipped
    let id = gs.alloc_id();
    let obj = CardObject::new(
        id,
        db.get("Grizzly Bears").unwrap().clone(),
        PlayerId(0),
        Zone::Battlefield,
    );
    let mut perm = PermanentState::new(&obj.definition);
    perm.controller_since_turn = 0;
    gs.battlefield.insert(id, perm);
    gs.add_object(obj);

    for _ in 0..4 {
        gs = dispatch_action(gs, ActionRequest::AdvanceStep).unwrap();
    }
    assert_eq!(gs.step(), Step::DeclareAttackers);
    assert!(!gs.combat.attackers_declared);

    assert!(dispatch_action(gs, ActionRequest::AdvanceStep).is_err());
}
```

- [ ] **Step 8: Fix `advance_step_blocked_before_blockers_declared`**

This test needs attackers declared (non-empty, so DB doesn't auto-skip) and a P1 blocker that can legally block (so `has_valid_blockers` returns true and DB doesn't auto-skip):

```rust
#[test]
fn advance_step_blocked_before_blockers_declared() {
    use mecha_oracle::types::{CardObject, Zone};
    let config = vec![
        (0..10).map(|_| "Forest".to_string()).collect(),
        (0..10).map(|_| "Forest".to_string()).collect(),
    ];
    let db = test_db();
    let mut gs = build_game_state(config, &db, false).unwrap();
    // P0 attacker — prevents DA auto-skip and makes DB non-trivially skippable
    let p0_id = {
        let id = gs.alloc_id();
        let obj = CardObject::new(
            id,
            db.get("Grizzly Bears").unwrap().clone(),
            PlayerId(0),
            Zone::Battlefield,
        );
        let mut perm = PermanentState::new(&obj.definition);
        perm.controller_since_turn = 0;
        gs.battlefield.insert(id, perm);
        gs.add_object(obj);
        id
    };
    // P1 blocker — makes has_valid_blockers return true, preventing DB auto-skip
    {
        let id = gs.alloc_id();
        let obj = CardObject::new(
            id,
            db.get("Grizzly Bears").unwrap().clone(),
            PlayerId(1),
            Zone::Battlefield,
        );
        let mut perm = PermanentState::new(&obj.definition);
        perm.controller_since_turn = 0;
        gs.battlefield.insert(id, perm);
        gs.add_object(obj);
    }
    for _ in 0..4 {
        gs = dispatch_action(gs, ActionRequest::AdvanceStep).unwrap();
    }
    assert_eq!(gs.step(), Step::DeclareAttackers);
    gs = dispatch_action(
        gs,
        ActionRequest::DeclareAttackers {
            attacker_ids: vec![p0_id.0],
        },
    )
    .unwrap();
    for _ in 0..2 {
        gs = dispatch_action(gs, ActionRequest::AdvanceStep).unwrap();
    }
    assert_eq!(gs.step(), Step::DeclareBlockers);
    assert!(!gs.combat.blockers_declared);

    assert!(dispatch_action(gs, ActionRequest::AdvanceStep).is_err());
}
```

- [ ] **Step 9: Fix `game_view_includes_combat_declared_flags`**

Add a P0 creature so DA doesn't auto-skip:

```rust
#[test]
fn game_view_includes_combat_declared_flags() {
    use mecha_oracle::types::{CardObject, Zone};
    let config = vec![
        (0..10).map(|_| "Forest".to_string()).collect(),
        (0..10).map(|_| "Forest".to_string()).collect(),
    ];
    let db = test_db();
    let mut gs = build_game_state(config, &db, false).unwrap();
    // Add P0 creature to prevent DA auto-skip
    let id = gs.alloc_id();
    let obj = CardObject::new(
        id,
        db.get("Grizzly Bears").unwrap().clone(),
        PlayerId(0),
        Zone::Battlefield,
    );
    let mut perm = PermanentState::new(&obj.definition);
    perm.controller_since_turn = 0;
    gs.battlefield.insert(id, perm);
    gs.add_object(obj);

    // Navigate to DeclareAttackers: 4 passes (2 per step × 2 steps)
    for _ in 0..4 {
        gs = dispatch_action(gs, ActionRequest::AdvanceStep).unwrap();
    }
    assert!(!build_game_view(&gs).attackers_declared);

    gs = dispatch_action(
        gs,
        ActionRequest::DeclareAttackers {
            attacker_ids: vec![],
        },
    )
    .unwrap();
    assert!(build_game_view(&gs).attackers_declared);
}
```

- [ ] **Step 10: Fix `advancing_from_end_step_auto_advances_to_next_upkeep`**

With an all-Forests deck, DA and DB now auto-skip. Remove the manual declarations and update the pass counts. Trace: 2 passes → BOC; 2 passes → DA/DB auto-skip → CD (auto-resolves); 2 passes → EOC; 2 passes → PostCombatMain; 2 passes → End = 10 total passes to End.

```rust
#[test]
fn advancing_from_end_step_auto_advances_to_next_upkeep() {
    use mecha_oracle::types::Step;
    let config = vec![
        (0..10).map(|_| "Forest".to_string()).collect(),
        (0..10).map(|_| "Forest".to_string()).collect(),
    ];
    let db = test_db();
    let mut gs = build_game_state(config, &db, false).unwrap();
    // PC (2) + BOC (2) → DA/DB auto-skipped → CD (2) → EOC (2) → PC2 (2) → End
    for _ in 0..10 {
        gs = dispatch_action(gs, ActionRequest::AdvanceStep).unwrap();
    }
    assert_eq!(gs.step(), Step::End);
    assert_eq!(gs.active_player, PlayerId(0));

    // Two more passes → Cleanup (auto) → Untap (auto) → Upkeep for P1
    let gs = dispatch_action(gs, ActionRequest::AdvanceStep).unwrap();
    assert_eq!(gs.step(), Step::End); // still End after first pass
    let gs = dispatch_action(gs, ActionRequest::AdvanceStep).unwrap();
    assert_eq!(gs.step(), Step::Upkeep);
    assert_eq!(gs.active_player, PlayerId(1));
}
```

- [ ] **Step 11: Run full test suite — all must pass**

```bash
cargo test 2>&1 | grep -E "^test result|FAILED|error\["
```

Expected: `test result: ok.`

- [ ] **Step 12: Run clippy**

```bash
cargo clippy --all-targets 2>&1 | grep -E "^error|^warning\[" | head -20
```

Expected: clean.

- [ ] **Step 13: Commit**

```bash
git add src/serve.rs
git commit -m "feat: auto-skip combat declaration steps when no valid options exist"
```
