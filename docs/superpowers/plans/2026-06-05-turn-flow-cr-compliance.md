# Turn Flow CR Compliance Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Fix eight CR-compliance gaps in turn sequencing, mana handling, and combat state across the engine and serve layers.

**Architecture:** Each fix is isolated to a small set of functions. Tasks are ordered so later tasks build on earlier ones (e.g. `deal_combat_damage` becomes `Result` before combat tests need `.unwrap()`; `skip_to_first_main` lands before the auto-advance helper; the `ManaCheckpoint` data model lands before functions that reference it).

**Tech Stack:** Rust (cargo), axum (serve layer), vanilla JS (serve.html).

---

## File Map

| File | What changes |
|------|-------------|
| `src/parser/oracle.rs` | comment fix |
| `src/engine/turn.rs` | mana drain + CombatState clear in `advance_step`; new `skip_to_first_main`; clear checkpoint in `advance_step` |
| `src/engine/combat.rs` | `deal_combat_damage` → `Result`, step guard, clear checkpoint |
| `src/engine/mana.rs` | checkpoint save in `tap_land_for_mana`; new `reset_mana` |
| `src/engine/casting.rs` | clear checkpoint in `play_land`, `cast_creature` |
| `src/engine/mod.rs` | new `NoManaCheckpoint` error variant |
| `src/types/game_state.rs` | new `ManaCheckpoint` struct, field on `GameState` |
| `src/types/mod.rs` | re-export `ManaCheckpoint` |
| `src/serve.rs` | step-aware `can_attack`/`can_block`; start at `PreCombatMain`; auto-advance helper; `ResetMana` action; `can_reset_mana` in `GameView` |
| `src/serve.html` | "Reset mana" button in actions panel |
| `tests/scripted_game.rs` | `.unwrap()` on `deal_combat_damage`; new `advance_to_combat_damage` helper; step set in `player_dies_at_zero_life_ends_game` |

---

## Task 1: Fix parser CR reference

**Files:**
- Modify: `src/parser/oracle.rs:5`

- [ ] **Step 1: Apply the fix**

Change line 5 of `src/parser/oracle.rs` from:
```rust
/// CR 305.6: parenthetical text on basic lands is reminder text, not rules text.
```
to:
```rust
/// CR 207.2b: parenthetical text on basic lands is reminder text, not rules text.
```

- [ ] **Step 2: Verify compile**

```
cargo check
```
Expected: no errors.

- [ ] **Step 3: Commit**

```bash
git add src/parser/oracle.rs
git commit -m "fix: update strip_reminder_text CR reference to 207.2b"
```

---

## Task 2: Mana drain at end of each step + clear CombatState after EndOfCombat

**Files:**
- Modify: `src/engine/turn.rs`

- [ ] **Step 1: Write the failing tests**

Add these two tests to the `tests` module at the bottom of `src/engine/turn.rs`:

```rust
#[test]
fn advance_step_drains_all_mana_pools() {
    let mut gs = make_state();
    gs.step = Step::PreCombatMain;
    gs.get_player_mut(PlayerId(0)).unwrap().mana_pool.green += 2;
    gs.get_player_mut(PlayerId(1)).unwrap().mana_pool.red += 1;

    let gs = advance_step(gs);

    assert!(gs.get_player(PlayerId(0)).unwrap().mana_pool.is_empty());
    assert!(gs.get_player(PlayerId(1)).unwrap().mana_pool.is_empty());
}

#[test]
fn advance_from_end_of_combat_clears_combat_state() {
    let mut gs = make_state();
    gs.step = Step::EndOfCombat;
    gs.combat.attackers.push(ObjectId(99));

    let gs = advance_step(gs);

    assert_eq!(gs.step(), Step::PostCombatMain);
    assert!(gs.combat.attackers.is_empty());
    assert!(gs.combat.blocking_map.is_empty());
}
```

- [ ] **Step 2: Run to confirm they fail**

```
cargo test -p mecha-oracle advance_step_drains advance_from_end_of_combat
```
Expected: both FAIL.

- [ ] **Step 3: Implement the fix**

Replace the existing `advance_step` function in `src/engine/turn.rs` with:

```rust
pub fn advance_step(mut state: GameState) -> GameState {
    // CR 106.4: mana pools empty at end of each step and phase.
    for player in state.players.iter_mut() {
        player.mana_pool = Default::default();
    }
    if let Some(next) = state.extra_steps.pop_front() {
        state.step = next;
        return state;
    }
    match state.step {
        Step::Untap => set(state, Step::Upkeep),
        Step::Upkeep => set(state, Step::Draw),
        Step::Draw => set(state, Step::PreCombatMain),
        Step::PreCombatMain => set(state, Step::BeginningOfCombat),
        Step::BeginningOfCombat => set(state, Step::DeclareAttackers),
        Step::DeclareAttackers => set(state, Step::DeclareBlockers),
        Step::DeclareBlockers => set(state, Step::CombatDamage),
        Step::CombatDamage => set(state, Step::EndOfCombat),
        Step::EndOfCombat => {
            let mut s = set(state, Step::PostCombatMain);
            s.combat = CombatState::empty();
            s
        }
        Step::PostCombatMain => set(state, Step::End),
        Step::End => set(state, Step::Cleanup),
        Step::Cleanup => start_next_turn(state),
    }
}
```

- [ ] **Step 4: Run tests**

```
cargo test -p mecha-oracle advance_step_drains advance_from_end_of_combat
```
Expected: both PASS.

- [ ] **Step 5: Run full suite**

```
cargo test
```
Expected: all pass (the scripted game's `advance_step` calls have no mana to drain, so no regressions).

- [ ] **Step 6: Commit**

```bash
git add src/engine/turn.rs
git commit -m "fix: drain mana at end of each step (CR 106.4) and clear CombatState after EndOfCombat"
```

---

## Task 3: Step guard on `deal_combat_damage`

**Files:**
- Modify: `src/engine/combat.rs`
- Modify: `src/serve.rs`
- Modify: `tests/scripted_game.rs`

- [ ] **Step 1: Write the failing test**

Add to the `tests` module in `src/engine/combat.rs`:

```rust
#[test]
fn deal_combat_damage_requires_combat_damage_step() {
    let mut gs = make_combat_state();
    gs.step = Step::DeclareAttackers;

    assert!(matches!(
        deal_combat_damage(gs),
        Err(EngineError::CannotCastNow)
    ));
}
```

- [ ] **Step 2: Run to confirm it fails to compile**

```
cargo test -p mecha-oracle deal_combat_damage_requires
```
Expected: compile error — `deal_combat_damage` doesn't return `Result`.

- [ ] **Step 3: Change the signature and add the guard**

Replace the opening of `deal_combat_damage` in `src/engine/combat.rs`. The function currently starts:

```rust
pub fn deal_combat_damage(mut state: GameState) -> GameState {
    use std::collections::HashSet;
```

Change it to:

```rust
pub fn deal_combat_damage(mut state: GameState) -> Result<GameState, EngineError> {
    if state.step != Step::CombatDamage {
        return Err(EngineError::CannotCastNow);
    }
    use std::collections::HashSet;
```

The function currently ends with `check_and_apply_sbas(state)`. Wrap the final return:

```rust
    Ok(check_and_apply_sbas(state))
}
```

- [ ] **Step 4: Add `.unwrap()` to every `deal_combat_damage` call inside `src/engine/combat.rs` tests**

Every test in combat.rs that calls `deal_combat_damage` already sets `gs.step = Step::CombatDamage` via the declare/advance sequence. Add `.unwrap()` to each:

```rust
// Before:
let gs = deal_combat_damage(gs);
// After:
let gs = deal_combat_damage(gs).unwrap();
```

There are 13 such call sites (the first-strike and double-strike tests each call it twice). Update them all.

- [ ] **Step 5: Update `dispatch_action` in `src/serve.rs`**

Find:
```rust
ActionRequest::DealCombatDamage => Ok(deal_combat_damage(state)),
```

Replace with:
```rust
ActionRequest::DealCombatDamage => deal_combat_damage(state).map_err(|e| format!("{e:?}")),
```

- [ ] **Step 6: Fix `tests/scripted_game.rs`**

The scripted tests have three kinds of changes needed.

**6a.** Add a new helper function after `advance_to_declare_attackers`:

```rust
fn advance_to_combat_damage(gs: GameState) -> GameState {
    let gs = advance_step(gs); // Untap → Upkeep
    let gs = advance_step(gs); // Upkeep → Draw
    let gs = advance_step(gs); // Draw → PreCombatMain
    let gs = advance_step(gs); // PreCombatMain → BeginningOfCombat
    let gs = advance_step(gs); // BeginningOfCombat → DeclareAttackers
    let gs = advance_step(gs); // DeclareAttackers → DeclareBlockers
    let gs = advance_step(gs); // DeclareBlockers → CombatDamage
    assert_eq!(gs.step(), Step::CombatDamage);
    gs
}
```

**6b.** In `player_dies_at_zero_life_ends_game`, insert `advance_to_combat_damage` before the `deal_combat_damage` call, and add `.unwrap()`:

The test currently ends with:
```rust
    gs.combat.attackers = vec![id];
    gs.combat.blocking_map.insert(id, vec![]);

    let gs = deal_combat_damage(gs);
```

Change to:
```rust
    gs.combat.attackers = vec![id];
    gs.combat.blocking_map.insert(id, vec![]);

    let gs = advance_to_combat_damage(gs);
    let gs = deal_combat_damage(gs).unwrap();
```

**6c.** Add `.unwrap()` to all remaining `deal_combat_damage` calls in scripted_game.rs (they already set the correct step via the declare/advance sequence):

```
scripted_game_runs_to_completion       — 1 call → add .unwrap()
first_striker_kills_blocker_and_survives_unscathed — 2 calls → add .unwrap()
trample_excess_kills_player            — 1 call → add .unwrap()
deathtouch_rat_kills_hill_giant        — 1 call → add .unwrap()
```

- [ ] **Step 7: Run all tests**

```
cargo test
```
Expected: all pass.

- [ ] **Step 8: Commit**

```bash
git add src/engine/combat.rs src/serve.rs tests/scripted_game.rs
git commit -m "fix: add step guard to deal_combat_damage, change return to Result"
```

---

## Task 4: Start game at PreCombatMain (CR 103.8a)

**Files:**
- Modify: `src/engine/turn.rs`
- Modify: `src/serve.rs`

- [ ] **Step 1: Write the failing test**

In `src/serve.rs` tests module, find the test `build_game_state_starts_at_untap`. It currently asserts `Step::Untap`. This test will become the spec for the new behaviour — rename it and flip the assertion. But first, write a new failing test alongside it:

```rust
#[test]
fn build_game_state_starts_at_pre_combat_main() {
    let config = vec![
        (0..10).map(|_| "Forest".to_string()).collect(),
        (0..10).map(|_| "Forest".to_string()).collect(),
    ];
    let db = test_db();
    let gs = build_game_state(config, &db, false).unwrap();
    assert_eq!(gs.step(), Step::PreCombatMain);
}
```

You also need `Step` imported in the serve.rs test module:
```rust
use mecha_oracle::types::Step;
```

- [ ] **Step 2: Run to confirm failure**

```
cargo test -p mecha-oracle build_game_state_starts_at_pre
```
Expected: FAIL (currently starts at `Untap`).

- [ ] **Step 3: Add `skip_to_first_main` to `src/engine/turn.rs`**

Add this public function anywhere in `src/engine/turn.rs` (after the existing helpers is fine):

```rust
/// Advance the initial game state to the first main phase of the starting player's
/// first turn. Skips Untap (nothing to untap at game start), Upkeep (no Phase 1
/// triggers), and Draw (CR 103.8a: the starting player draws no cards).
pub fn skip_to_first_main(mut state: GameState) -> GameState {
    state.step = Step::PreCombatMain;
    state
}
```

- [ ] **Step 4: Update `build_game_state` in `src/serve.rs`**

Add `skip_to_first_main` to the import:
```rust
use mecha_oracle::engine::turn::{advance_step, apply_step_start, draw_card, skip_to_first_main};
```

Find the end of `build_game_state`:
```rust
    gs = apply_step_start(gs);

    Ok(gs)
```

Replace with:
```rust
    gs = skip_to_first_main(gs);

    Ok(gs)
```

- [ ] **Step 5: Update stale tests in `src/serve.rs`**

**5a.** Delete `build_game_state_starts_at_untap` (now replaced by `build_game_state_starts_at_pre_combat_main`).

**5b.** In `build_game_view_initial_life_and_step`, change:
```rust
assert_eq!(view.step, "Untap");
```
to:
```rust
assert_eq!(view.step, "PreCombatMain");
```

**5c.** In `dispatch_advance_step_moves_to_upkeep`, rewrite it to test what now makes sense — advancing from the initial `PreCombatMain`:

```rust
#[test]
fn dispatch_advance_step_from_pre_combat_main_to_beginning_of_combat() {
    let config = vec![
        (0..10).map(|_| "Forest".to_string()).collect(),
        (0..10).map(|_| "Forest".to_string()).collect(),
    ];
    let db = test_db();
    let gs = build_game_state(config, &db, false).unwrap();
    assert_eq!(gs.step(), Step::PreCombatMain);
    let gs2 = dispatch_action(gs, ActionRequest::AdvanceStep).unwrap();
    assert_eq!(gs2.step(), Step::BeginningOfCombat);
}
```

- [ ] **Step 6: Run all tests**

```
cargo test
```
Expected: all pass. (The scripted game tests use `GameState::new()` directly, which still starts at `Untap` — those tests are unaffected.)

- [ ] **Step 7: Commit**

```bash
git add src/engine/turn.rs src/serve.rs
git commit -m "fix: start game at PreCombatMain, skip draw for first player (CR 103.8a)"
```

---

## Task 5: Auto-advance through no-priority steps (CR 117.3a)

**Files:**
- Modify: `src/serve.rs`

- [ ] **Step 1: Write the failing test**

Add to the `src/serve.rs` tests module. This test verifies that advancing from the `End` step lands at `Upkeep` of the next player's turn (skipping `Cleanup` and `Untap` automatically):

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
    // Advance from PreCombatMain through all steps to End (7 steps).
    for _ in 0..7 {
        gs = dispatch_action(gs, ActionRequest::AdvanceStep).unwrap();
    }
    assert_eq!(gs.step(), Step::End);
    assert_eq!(gs.active_player, PlayerId(0));

    // One more advance should skip Cleanup and the new turn's Untap, landing at Upkeep.
    let gs = dispatch_action(gs, ActionRequest::AdvanceStep).unwrap();

    assert_eq!(gs.step(), Step::Upkeep);
    assert_eq!(gs.active_player, PlayerId(1));
}
```

- [ ] **Step 2: Run to confirm failure**

```
cargo test -p mecha-oracle advancing_from_end_step
```
Expected: FAIL — currently lands at `Cleanup`, not `Upkeep`.

- [ ] **Step 3: Add `advance_with_auto_steps` and update dispatch**

In `src/serve.rs`, ensure `Step` is imported at the top (add it if not already there from Task 4):
```rust
use mecha_oracle::types::{CardObject, GameState, ObjectId, Player, PlayerId, Step, Zone};
```

Add this private helper function (place it near `dispatch_action`):

```rust
fn advance_with_auto_steps(mut state: GameState) -> GameState {
    loop {
        state = advance_step(state);
        state = apply_step_start(state);
        if !matches!(state.step(), Step::Untap | Step::Cleanup) || state.is_game_over() {
            break;
        }
    }
    state
}
```

In `dispatch_action`, replace the `AdvanceStep` arm:
```rust
// Before:
ActionRequest::AdvanceStep => {
    let s = advance_step(state);
    Ok(apply_step_start(s))
}
// After:
ActionRequest::AdvanceStep => Ok(advance_with_auto_steps(state)),
```

- [ ] **Step 4: Run all tests**

```
cargo test
```
Expected: all pass.

- [ ] **Step 5: Commit**

```bash
git add src/serve.rs
git commit -m "fix: auto-advance through Untap and Cleanup steps (CR 117.3a)"
```

---

## Task 6: Step-aware `can_attack` / `can_block` in `CardView`

**Files:**
- Modify: `src/serve.rs`

- [ ] **Step 1: Write the failing tests**

Add to `src/serve.rs` tests module:

```rust
#[test]
fn can_attack_true_only_for_active_player_at_declare_attackers() {
    use mecha_oracle::types::{CardObject, ObjectId, Step, Zone};
    let db = test_db();
    let config = vec![
        (0..10).map(|_| "Forest".to_string()).collect(),
        (0..10).map(|_| "Forest".to_string()).collect(),
    ];
    let mut gs = build_game_state(config, &db, false).unwrap();

    // Place one untapped, non-sick creature for each player.
    let p1_id = {
        let id = gs.alloc_id();
        let mut obj = CardObject::new(id, db.get("Grizzly Bears").unwrap().clone(), PlayerId(0), Zone::Battlefield);
        obj.summoning_sick = false;
        gs.battlefield.push(id);
        gs.add_object(obj);
        id.0
    };
    let p2_id = {
        let id = gs.alloc_id();
        let mut obj = CardObject::new(id, db.get("Grizzly Bears").unwrap().clone(), PlayerId(1), Zone::Battlefield);
        obj.summoning_sick = false;
        gs.battlefield.push(id);
        gs.add_object(obj);
        id.0
    };

    // PreCombatMain → BeginningOfCombat → DeclareAttackers (2 advances).
    let gs = dispatch_action(gs, ActionRequest::AdvanceStep).unwrap();
    let gs = dispatch_action(gs, ActionRequest::AdvanceStep).unwrap();
    assert_eq!(gs.step(), Step::DeclareAttackers);

    let view = build_game_view(&gs);
    let p1_c = view.p1.creatures.iter().find(|c| c.id == p1_id).unwrap();
    let p2_c = view.p2.creatures.iter().find(|c| c.id == p2_id).unwrap();

    assert!(p1_c.can_attack,  "active player's creature shows can_attack");
    assert!(!p2_c.can_attack, "defending player's creature does not show can_attack");
    assert!(!p1_c.can_block,  "can_block is false outside DeclareBlockers");
    assert!(!p2_c.can_block,  "can_block is false outside DeclareBlockers");
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
        let mut obj = CardObject::new(id, db.get("Grizzly Bears").unwrap().clone(), PlayerId(0), Zone::Battlefield);
        obj.summoning_sick = false;
        gs.battlefield.push(id);
        gs.add_object(obj);
        id.0
    };
    let p2_id = {
        let id = gs.alloc_id();
        let mut obj = CardObject::new(id, db.get("Grizzly Bears").unwrap().clone(), PlayerId(1), Zone::Battlefield);
        obj.summoning_sick = false;
        gs.battlefield.push(id);
        gs.add_object(obj);
        id.0
    };

    // PreCombatMain → BeginningOfCombat → DeclareAttackers.
    let gs = dispatch_action(gs, ActionRequest::AdvanceStep).unwrap();
    let gs = dispatch_action(gs, ActionRequest::AdvanceStep).unwrap();

    // Declare P1's bear as attacker, then advance to DeclareBlockers.
    let gs = dispatch_action(gs, ActionRequest::DeclareAttackers { attacker_ids: vec![p1_id] }).unwrap();
    let gs = dispatch_action(gs, ActionRequest::AdvanceStep).unwrap();
    assert_eq!(gs.step(), Step::DeclareBlockers);

    let view = build_game_view(&gs);
    let p1_c = view.p1.creatures.iter().find(|c| c.id == p1_id).unwrap();
    let p2_c = view.p2.creatures.iter().find(|c| c.id == p2_id).unwrap();

    assert!(!p1_c.can_block, "active player's creature does not show can_block");
    assert!(p2_c.can_block,  "defending player's creature shows can_block");
    assert!(!p1_c.can_attack, "declared attacker (tapped) does not show can_attack");
}
```

- [ ] **Step 2: Run to confirm failure**

```
cargo test -p mecha-oracle can_attack_true_only can_block_true_only
```
Expected: both FAIL (currently `can_attack`/`can_block` are step-agnostic).

- [ ] **Step 3: Update `build_player_view` in `src/serve.rs`**

In `build_player_view`, find the `to_card_view` closure. Replace:

```rust
can_attack: obj.can_attack(),
can_block: obj.can_block(),
```

with:

```rust
can_attack: state.step() == Step::DeclareAttackers
    && pid == state.active_player
    && obj.can_attack(),
can_block: state.step() == Step::DeclareBlockers
    && pid != state.active_player
    && obj.can_block(),
```

(`Step` is already imported from Task 5.)

- [ ] **Step 4: Run all tests**

```
cargo test
```
Expected: all pass.

- [ ] **Step 5: Commit**

```bash
git add src/serve.rs
git commit -m "fix: make can_attack/can_block step- and player-aware in CardView"
```

---

## Task 7: ManaCheckpoint data model

**Files:**
- Modify: `src/types/game_state.rs`
- Modify: `src/types/mod.rs`
- Modify: `src/engine/mod.rs`

No tests in this task — it's pure data model plumbing. Correctness is verified by subsequent tasks.

- [ ] **Step 1: Add `ManaCheckpoint` and field to `GameState`**

In `src/types/game_state.rs`, add the `ManaPool` import at the top (alongside the existing `Player` import):

```rust
use super::mana::ManaPool;
```

Add the `ManaCheckpoint` struct before (or after) `CombatState`:

```rust
#[derive(Debug, Clone)]
pub struct ManaCheckpoint {
    /// Mana pool state for every player at the moment the first mana tap was made.
    pub pools: HashMap<PlayerId, ManaPool>,
    /// Lands tapped for mana since the checkpoint was created, in tap order.
    pub tapped_lands: Vec<ObjectId>,
}
```

Add the field to `GameState`:
```rust
pub struct GameState {
    // ... existing fields ...
    pub mana_checkpoint: Option<ManaCheckpoint>,
    // ...
}
```

Initialize it in `GameState::new`:
```rust
Self {
    // ... existing fields ...
    mana_checkpoint: None,
    // ...
}
```

- [ ] **Step 2: Re-export `ManaCheckpoint` from `src/types/mod.rs`**

Find:
```rust
pub use game_state::{CombatState, GameState, Phase, Step};
```

Replace with:
```rust
pub use game_state::{CombatState, GameState, ManaCheckpoint, Phase, Step};
```

- [ ] **Step 3: Add `NoManaCheckpoint` to `EngineError`**

In `src/engine/mod.rs`, add the new variant:

```rust
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
}
```

- [ ] **Step 4: Verify compile**

```
cargo check
```
Expected: no errors.

- [ ] **Step 5: Commit**

```bash
git add src/types/game_state.rs src/types/mod.rs src/engine/mod.rs
git commit -m "feat: add ManaCheckpoint data model and NoManaCheckpoint error"
```

---

## Task 8: Checkpoint save in `tap_land_for_mana` and new `reset_mana`

**Files:**
- Modify: `src/engine/mana.rs`

- [ ] **Step 1: Write failing tests**

Add to the `tests` module in `src/engine/mana.rs`:

```rust
#[test]
fn tap_land_for_mana_creates_checkpoint_on_first_tap() {
    let db = test_db();
    let mut gs = make_state();
    let forest_id = add_land(&mut gs, PlayerId(0), db.get("Forest").unwrap().clone());

    assert!(gs.mana_checkpoint.is_none());
    let gs = tap_land_for_mana(gs, forest_id).unwrap();

    let cp = gs.mana_checkpoint.as_ref().expect("checkpoint created");
    assert_eq!(cp.tapped_lands, vec![forest_id]);
    assert!(cp.pools[&PlayerId(0)].is_empty()); // pool was empty before the tap
}

#[test]
fn second_tap_appends_to_existing_checkpoint() {
    let db = test_db();
    let mut gs = make_state();
    let f1 = add_land(&mut gs, PlayerId(0), db.get("Forest").unwrap().clone());
    let f2 = add_land(&mut gs, PlayerId(0), db.get("Forest").unwrap().clone());

    let gs = tap_land_for_mana(gs, f1).unwrap();
    let gs = tap_land_for_mana(gs, f2).unwrap();

    let cp = gs.mana_checkpoint.as_ref().unwrap();
    assert_eq!(cp.tapped_lands, vec![f1, f2]);
    assert_eq!(cp.pools[&PlayerId(0)].is_empty(), true); // pool at checkpoint was empty
}

#[test]
fn reset_mana_restores_pools_and_untaps_lands() {
    use super::reset_mana;
    let db = test_db();
    let mut gs = make_state();
    let forest_id = add_land(&mut gs, PlayerId(0), db.get("Forest").unwrap().clone());

    let gs = tap_land_for_mana(gs, forest_id).unwrap();
    assert!(gs.objects[&forest_id].tapped);
    assert_eq!(gs.get_player(PlayerId(0)).unwrap().mana_pool.green, 1);

    let gs = reset_mana(gs).unwrap();

    assert!(!gs.objects[&forest_id].tapped, "land untapped after reset");
    assert!(gs.get_player(PlayerId(0)).unwrap().mana_pool.is_empty(), "pool restored");
    assert!(gs.mana_checkpoint.is_none(), "checkpoint cleared");
}

#[test]
fn reset_mana_returns_err_with_no_checkpoint() {
    use super::reset_mana;
    let gs = make_state();
    assert!(matches!(
        reset_mana(gs),
        Err(super::EngineError::NoManaCheckpoint)
    ));
}
```

- [ ] **Step 2: Run to confirm failure**

```
cargo test -p mecha-oracle tap_land_for_mana_creates second_tap_appends reset_mana
```
Expected: compile errors or test failures.

- [ ] **Step 3: Update `tap_land_for_mana` to save the checkpoint**

Add `ManaCheckpoint` to the imports at the top of `src/engine/mana.rs`:

```rust
use crate::types::{GameState, ManaCheckpoint, ManaColor, ManaCost, ObjectId, PlayerId, Zone};
```

Replace the existing `tap_land_for_mana` body with the version that lazily saves a checkpoint. The validation block stays identical; the mutation block gains checkpoint bookkeeping:

```rust
pub fn tap_land_for_mana(
    mut state: GameState,
    object_id: ObjectId,
) -> Result<GameState, EngineError> {
    let (controller, color) = {
        let obj = state
            .objects
            .get(&object_id)
            .ok_or(EngineError::CardNotFound)?;
        if obj.zone != Zone::Battlefield {
            return Err(EngineError::CardNotOnBattlefield);
        }
        if obj.tapped {
            return Err(EngineError::AlreadyTapped);
        }
        if !obj.is_land() {
            return Err(EngineError::NotALand);
        }
        (
            obj.controller,
            land_produces(&obj.definition.type_line.subtypes),
        )
    };

    // Lazily create a checkpoint before the first mana tap in this priority window.
    if state.mana_checkpoint.is_none() {
        let pools = state.players.iter().map(|p| (p.id, p.mana_pool.clone())).collect();
        state.mana_checkpoint = Some(ManaCheckpoint {
            pools,
            tapped_lands: vec![],
        });
    }
    state.mana_checkpoint.as_mut().unwrap().tapped_lands.push(object_id);

    state.objects.get_mut(&object_id).unwrap().tapped = true;
    state
        .get_player_mut(controller)
        .unwrap()
        .mana_pool
        .add(color, 1);
    Ok(state)
}
```

- [ ] **Step 4: Add `reset_mana`**

Add this new public function to `src/engine/mana.rs`, below `tap_land_for_mana`:

```rust
/// Undo all mana taps made in the current priority window. Restores each player's
/// mana pool and untaps every land recorded in the checkpoint, then clears it.
/// Returns `Err(NoManaCheckpoint)` if no taps have been made since the last commit.
pub fn reset_mana(mut state: GameState) -> Result<GameState, EngineError> {
    let checkpoint = state
        .mana_checkpoint
        .take()
        .ok_or(EngineError::NoManaCheckpoint)?;
    for player in state.players.iter_mut() {
        if let Some(pool) = checkpoint.pools.get(&player.id) {
            player.mana_pool = pool.clone();
        }
    }
    for &id in &checkpoint.tapped_lands {
        if let Some(obj) = state.objects.get_mut(&id) {
            obj.tapped = false;
        }
    }
    Ok(state)
}
```

- [ ] **Step 5: Run all tests**

```
cargo test
```
Expected: all pass.

- [ ] **Step 6: Commit**

```bash
git add src/engine/mana.rs
git commit -m "feat: save mana checkpoint on first tap and implement reset_mana"
```

---

## Task 9: Clear checkpoint when committing to an action

**Files:**
- Modify: `src/engine/turn.rs`
- Modify: `src/engine/casting.rs`
- Modify: `src/engine/combat.rs`

Passing priority, playing a land, casting a creature, declaring attackers/blockers, and resolving combat damage all commit the player's mana choices.

- [ ] **Step 1: Write failing tests**

Add to `src/engine/turn.rs` tests:

```rust
#[test]
fn advance_step_clears_mana_checkpoint() {
    use crate::engine::mana::tap_land_for_mana;
    let db = test_db();
    let mut gs = make_state();
    gs.step = Step::PreCombatMain;
    // add_land_to_battlefield creates a tapped land; untap it.
    let forest_id = add_land_to_battlefield(&mut gs, PlayerId(0));
    gs.objects.get_mut(&forest_id).unwrap().tapped = false;

    let gs = tap_land_for_mana(gs, forest_id).unwrap();
    assert!(gs.mana_checkpoint.is_some());

    let gs = advance_step(gs);

    assert!(gs.mana_checkpoint.is_none());
}
```

Add to `src/engine/casting.rs` tests:

```rust
#[test]
fn cast_creature_clears_mana_checkpoint() {
    let db = test_db();
    let mut gs = make_state();
    // Give player enough mana and a checkpoint.
    gs.get_player_mut(PlayerId(0)).unwrap().mana_pool.green += 2;
    // Create a minimal checkpoint manually to simulate a prior tap.
    gs.mana_checkpoint = Some(crate::types::ManaCheckpoint {
        pools: std::collections::HashMap::new(),
        tapped_lands: vec![],
    });
    let bear_id = put_in_hand(&mut gs, PlayerId(0), db.get("Grizzly Bears").unwrap().clone());

    let gs = cast_creature(gs, PlayerId(0), bear_id).unwrap();

    assert!(gs.mana_checkpoint.is_none());
}

#[test]
fn play_land_clears_mana_checkpoint() {
    let db = test_db();
    let mut gs = make_state();
    gs.mana_checkpoint = Some(crate::types::ManaCheckpoint {
        pools: std::collections::HashMap::new(),
        tapped_lands: vec![],
    });
    let forest_id = put_in_hand(&mut gs, PlayerId(0), db.get("Forest").unwrap().clone());

    let gs = play_land(gs, PlayerId(0), forest_id).unwrap();

    assert!(gs.mana_checkpoint.is_none());
}
```

Add to `src/engine/combat.rs` tests:

```rust
#[test]
fn declare_blockers_clears_mana_checkpoint() {
    let db = test_db();
    let mut gs = make_combat_state();
    let attacker = add_creature(&mut gs, PlayerId(0), db.get("Grizzly Bears").unwrap().clone());
    let blocker = add_creature(&mut gs, PlayerId(1), db.get("Grizzly Bears").unwrap().clone());
    gs = declare_attackers(gs, PlayerId(0), &[attacker]).unwrap();
    gs.step = Step::DeclareBlockers;
    gs.mana_checkpoint = Some(crate::types::ManaCheckpoint {
        pools: std::collections::HashMap::new(),
        tapped_lands: vec![],
    });

    let gs = declare_blockers(gs, PlayerId(1), &[(blocker, attacker)]).unwrap();

    assert!(gs.mana_checkpoint.is_none());
}

#[test]
fn declare_attackers_clears_mana_checkpoint() {
    let db = test_db();
    let mut gs = make_combat_state();
    let bear_id = add_creature(&mut gs, PlayerId(0), db.get("Grizzly Bears").unwrap().clone());
    gs.mana_checkpoint = Some(crate::types::ManaCheckpoint {
        pools: std::collections::HashMap::new(),
        tapped_lands: vec![],
    });

    let gs = declare_attackers(gs, PlayerId(0), &[bear_id]).unwrap();

    assert!(gs.mana_checkpoint.is_none());
}
```

- [ ] **Step 2: Run to confirm failure**

```
cargo test -p mecha-oracle advance_step_clears_mana cast_creature_clears play_land_clears declare_attackers_clears
```
Expected: all FAIL.

- [ ] **Step 3: Clear checkpoint in `advance_step`**

In `src/engine/turn.rs`, add one line at the top of `advance_step`, immediately after the mana drain loop:

```rust
pub fn advance_step(mut state: GameState) -> GameState {
    // CR 106.4: mana pools empty at end of each step and phase.
    for player in state.players.iter_mut() {
        player.mana_pool = Default::default();
    }
    // Passing priority commits mana choices.
    state.mana_checkpoint = None;
    // ... rest unchanged ...
```

- [ ] **Step 4: Clear checkpoint in `play_land` and `cast_creature`**

In `src/engine/casting.rs`:

In `play_land`, add the clear after the hand/zone checks pass (before the mutations):
```rust
    // (after the block that validates hand membership and land type)
    state.mana_checkpoint = None;
    state
        .hands
        .get_mut(&player_id)
        .unwrap()
        .retain(|&id| id != object_id);
    // ...
```

In `cast_creature`, add the clear after `pay_mana_cost` succeeds:
```rust
    state = pay_mana_cost(state, player_id, &cost)?;
    state.mana_checkpoint = None;
    state
        .hands
        .get_mut(&player_id)
        // ...
```

- [ ] **Step 5: Clear checkpoint in `declare_attackers`, `declare_blockers`, `deal_combat_damage`**

In `src/engine/combat.rs`:

In `declare_attackers`, add the clear before the mutation loop (after all validation):
```rust
    // (after the validation for-loop, before the mutation for-loop)
    state.mana_checkpoint = None;
    for &id in attacker_ids {
        if !state
            .objects
            // ...
```

In `declare_blockers`, add the clear before the final `Ok(state)`:
```rust
    // (after the menace check, before return)
    state.mana_checkpoint = None;
    Ok(state)
```

In `deal_combat_damage`, add the clear after the step guard:
```rust
pub fn deal_combat_damage(mut state: GameState) -> Result<GameState, EngineError> {
    if state.step != Step::CombatDamage {
        return Err(EngineError::CannotCastNow);
    }
    state.mana_checkpoint = None;
    // ... rest unchanged ...
```

- [ ] **Step 6: Run all tests**

```
cargo test
```
Expected: all pass.

- [ ] **Step 7: Commit**

```bash
git add src/engine/turn.rs src/engine/casting.rs src/engine/combat.rs
git commit -m "feat: clear mana checkpoint on all committed actions"
```

---

## Task 10: `ResetMana` action in serve layer and UI button

**Files:**
- Modify: `src/serve.rs`
- Modify: `src/serve.html`

- [ ] **Step 1: Write a failing test**

Add to `src/serve.rs` tests module:

```rust
#[test]
fn reset_mana_action_untaps_land_and_restores_pool() {
    use mecha_oracle::engine::casting::play_land;
    use mecha_oracle::types::{CardObject, Step, Zone};
    let db = test_db();
    let config = vec![
        vec![
            "Forest".into(), "Forest".into(), "Forest".into(), "Forest".into(),
            "Grizzly Bears".into(), "Grizzly Bears".into(), "Grizzly Bears".into(),
            "Grizzly Bears".into(), "Forest".into(), "Forest".into(),
        ],
        (0..10).map(|_| "Forest".to_string()).collect(),
    ];
    let mut gs = build_game_state(config, &db, false).unwrap();

    // Play a land from hand so we have one untapped land on the battlefield.
    let land_id = gs.hands[&PlayerId(0)]
        .iter()
        .find(|id| gs.objects[*id].is_land())
        .copied()
        .unwrap();
    gs = play_land(gs, PlayerId(0), land_id).unwrap();

    // Tap it for mana via the action dispatcher.
    gs = dispatch_action(gs, ActionRequest::TapLand { object_id: land_id.0 }).unwrap();
    assert!(gs.objects[&land_id].tapped);
    assert_eq!(gs.get_player(PlayerId(0)).unwrap().mana_pool.green, 1);
    assert!(gs.mana_checkpoint.is_some());
    let view = build_game_view(&gs);
    assert!(view.can_reset_mana);

    // Reset mana.
    gs = dispatch_action(gs, ActionRequest::ResetMana).unwrap();

    assert!(!gs.objects[&land_id].tapped, "land untapped");
    assert!(gs.get_player(PlayerId(0)).unwrap().mana_pool.is_empty(), "pool empty");
    assert!(gs.mana_checkpoint.is_none());
    let view = build_game_view(&gs);
    assert!(!view.can_reset_mana);
}
```

- [ ] **Step 2: Run to confirm failure**

```
cargo test -p mecha-oracle reset_mana_action_untaps
```
Expected: compile error — `ActionRequest::ResetMana` doesn't exist yet.

- [ ] **Step 3: Add `reset_mana` to imports in `src/serve.rs`**

Find:
```rust
use mecha_oracle::engine::mana::tap_land_for_mana;
```

Replace with:
```rust
use mecha_oracle::engine::mana::{reset_mana, tap_land_for_mana};
```

- [ ] **Step 4: Add `ResetMana` to `ActionRequest` and `dispatch_action`**

In the `ActionRequest` enum, add:
```rust
ResetMana,
```

In `dispatch_action`, add a new arm:
```rust
ActionRequest::ResetMana => reset_mana(state).map_err(|e| format!("{e:?}")),
```

- [ ] **Step 5: Add `can_reset_mana` to `GameView`**

In the `GameView` struct, add:
```rust
can_reset_mana: bool,
```

In `build_game_view`, add the field:
```rust
can_reset_mana: state.mana_checkpoint.is_some(),
```

- [ ] **Step 6: Run all tests**

```
cargo test
```
Expected: all pass.

- [ ] **Step 7: Add the UI button in `src/serve.html`**

In `describeAction`, add a case before `default`:
```javascript
case 'reset_mana':     return `<span class="who">P${ap}</span> reset mana`;
```

In `renderActions`, add the "Reset mana" button block immediately before the existing `html += group('Priority', ...)` line:
```javascript
if (s.can_reset_mana) {
    html += group('Mana', [`<button class="action-btn" onclick="sendAction({type:'reset_mana'})">↩ Reset mana</button>`]);
}
```

- [ ] **Step 8: Verify the UI manually**

```
cargo run -- serve
```

Open http://localhost:3000. Tap a land — the "Reset mana" button should appear. Click it — the land should untap and the mana pool display should clear. Tap again and then play a land or advance the step — the reset button should disappear.

- [ ] **Step 9: Commit**

```bash
git add src/serve.rs src/serve.html
git commit -m "feat: add ResetMana action and mana undo button in UI"
```
