# Priority Model, Combat UX, and Step Guard Removal — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Implement CR-117-compliant two-player priority passing, auto-resolve combat damage per CR 510.2/510.3, remove all step guards from engine functions, and replace the single action sidebar with a two-section per-player layout.

**Architecture:** The backend gains a proper two-phase priority model (`AdvanceStep` shifts priority to the opponent; only when the second player passes does the step actually advance). Combat damage auto-resolves in `apply_step_start` when the engine enters `CombatDamage`, leaving the loop to break so players still get a priority window. The frontend sidebar splits into two stacked player panes, each with its own actions and a bottom-pinned "Pass priority" button.

**Tech Stack:** Rust 2024 (axum, serde), vanilla HTML/CSS/JS single-file UI at `src/serve.html`.

---

## File Map

| File | Change |
|------|--------|
| `src/types/ids.rs` | Add `Serialize + #[serde(transparent)]` to `ObjectId` and `PlayerId` |
| `src/types/game_state.rs` | Add `attackers_declared`, `blockers_declared` to `CombatState` |
| `src/engine/combat.rs` | Remove step guards; `deal_combat_damage` → infallible `GameState → GameState`; set declared flags; delete obsolete tests |
| `src/engine/casting.rs` | Remove step guards; delete obsolete test |
| `src/engine/turn.rs` | `advance_step` resets `priority_player`; `apply_step_start` handles `CombatDamage` |
| `src/serve.rs` | `GameView`/`CardView` types; `priority_player`; remove `DealCombatDamage`; two-phase `AdvanceStep`; fix tests |
| `src/serve.html` | Two-section sidebar; log drawer; spacebar hotkey; player ID 0/1 convention |

---

## Task 1 — Transparent Serialize for `PlayerId` and `ObjectId`

**Files:**
- Modify: `src/types/ids.rs`

- [ ] **Write failing test**

Add to the `tests` block in `src/types/ids.rs`:

```rust
#[test]
fn player_id_serializes_as_inner_u8() {
    let id = PlayerId(1);
    assert_eq!(serde_json::to_string(&id).unwrap(), "1");
}

#[test]
fn object_id_serializes_as_inner_u64() {
    let id = ObjectId(42);
    assert_eq!(serde_json::to_string(&id).unwrap(), "42");
}
```

- [ ] **Run to confirm failure**

```
cargo test -p mecha-oracle types::ids
```

Expected: compile error — `PlayerId` doesn't implement `Serialize`.

- [ ] **Implement**

Replace the top of `src/types/ids.rs` (the two struct definitions) with:

```rust
use serde::Serialize;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize)]
#[serde(transparent)]
pub struct ObjectId(pub u64);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize)]
#[serde(transparent)]
pub struct PlayerId(pub u8);
```

- [ ] **Run to confirm pass**

```
cargo test -p mecha-oracle types::ids
```

Expected: all 5 tests pass.

- [ ] **Commit**

```bash
git add src/types/ids.rs
git commit -m "feat: add transparent Serialize to PlayerId and ObjectId"
```

---

## Task 2 — `attackers_declared` and `blockers_declared` flags

**Files:**
- Modify: `src/types/game_state.rs`

- [ ] **Write failing test**

Add to the `tests` block in `src/types/game_state.rs`:

```rust
#[test]
fn combat_state_empty_has_declared_flags_false() {
    let cs = CombatState::empty();
    assert!(!cs.attackers_declared);
    assert!(!cs.blockers_declared);
}
```

- [ ] **Run to confirm failure**

```
cargo test -p mecha-oracle types::game_state
```

Expected: compile error — `CombatState` has no field `attackers_declared`.

- [ ] **Implement**

In `src/types/game_state.rs`, update `CombatState`:

```rust
#[derive(Debug, Clone)]
pub struct CombatState {
    pub attackers: Vec<ObjectId>,
    pub blocking_map: HashMap<ObjectId, Vec<ObjectId>>,
    pub first_strike_done: bool,
    pub attackers_declared: bool,
    pub blockers_declared: bool,
}

impl CombatState {
    pub fn empty() -> Self {
        Self {
            attackers: vec![],
            blocking_map: HashMap::new(),
            first_strike_done: false,
            attackers_declared: false,
            blockers_declared: false,
        }
    }
}
```

- [ ] **Run to confirm pass**

```
cargo test -p mecha-oracle types::game_state
```

Expected: all tests pass.

- [ ] **Commit**

```bash
git add src/types/game_state.rs
git commit -m "feat: add attackers_declared and blockers_declared to CombatState"
```

---

## Task 3 — Remove step guards and infallible `deal_combat_damage` (combat.rs)

**Files:**
- Modify: `src/engine/combat.rs`

- [ ] **Write new tests and delete obsolete ones**

**Delete** the test `deal_combat_damage_requires_combat_damage_step` entirely.

**Add** to the `tests` block:

```rust
#[test]
fn declare_attackers_sets_declared_flag() {
    let mut gs = make_combat_state();
    assert!(!gs.combat.attackers_declared);
    let gs = declare_attackers(gs, PlayerId(0), &[]).unwrap();
    assert!(gs.combat.attackers_declared);
}

#[test]
fn declare_blockers_sets_declared_flag() {
    let mut gs = make_combat_state();
    assert!(!gs.combat.blockers_declared);
    let gs = declare_blockers(gs, PlayerId(1), &[]).unwrap();
    assert!(gs.combat.blockers_declared);
}
```

- [ ] **Run to confirm the new tests fail**

```
cargo test -p mecha-oracle engine::combat
```

Expected: the two new tests fail (fields not set yet); the deleted test is gone.

- [ ] **Remove step guard from `declare_attackers` and set the flag**

In `src/engine/combat.rs`, inside `declare_attackers`, remove this block:

```rust
    if state.step != Step::DeclareAttackers {
        return Err(EngineError::CannotCastNow);
    }
```

Then after `state.combat.blocking_map = …` and before `Ok(state)`, add:

```rust
    state.combat.attackers_declared = true;
```

- [ ] **Remove step guard from `declare_blockers` and set the flag**

In `src/engine/combat.rs`, inside `declare_blockers`, remove:

```rust
    if state.step != Step::DeclareBlockers {
        return Err(EngineError::CannotCastNow);
    }
```

Replace the final `Ok(state)` with:

```rust
    state.combat.blockers_declared = true;
    Ok(state)
```

- [ ] **Remove step guard from `deal_combat_damage` and make it infallible**

Change the function signature from:

```rust
pub fn deal_combat_damage(mut state: GameState) -> Result<GameState, EngineError> {
```

to:

```rust
pub fn deal_combat_damage(mut state: GameState) -> GameState {
```

Delete this block at the top of the function body:

```rust
    if state.step != Step::CombatDamage {
        return Err(EngineError::CannotCastNow);
    }
```

Replace the final `Ok(check_and_apply_sbas(state))` with:

```rust
    check_and_apply_sbas(state)
```

- [ ] **Update all test call sites** — replace every `deal_combat_damage(gs).unwrap()` with `deal_combat_damage(gs)` throughout the test module. The affected tests are: `unblocked_attacker_deals_damage_to_player`, `blocked_creatures_deal_damage_to_each_other`, `larger_creature_kills_smaller_and_survives`, `first_striker_kills_blocker_before_it_can_deal_damage` (two calls), `double_striker_deals_damage_in_both_rounds` (two calls), `no_first_strikers_means_single_round_and_no_extra_step`, `trample_sends_excess_to_player`, `trample_deathtouch_one_damage_is_lethal_per_blocker`, `lifelink_attacker_gains_life_from_combat_damage`, `deathtouch_marks_target_for_sba`, `multiple_blockers_take_damage_in_order`.

- [ ] **Run to confirm pass**

```
cargo test -p mecha-oracle engine::combat
```

Expected: all remaining tests pass.

- [ ] **Commit**

```bash
git add src/engine/combat.rs
git commit -m "refactor: remove step guards from combat engine; deal_combat_damage is now infallible"
```

---

## Task 4 — Remove step guards from casting.rs

**Files:**
- Modify: `src/engine/casting.rs`

- [ ] **Delete the obsolete test**

Delete the test `cannot_play_land_outside_main_phase` from the `tests` block in `src/engine/casting.rs`.

- [ ] **Run to confirm all remaining tests still pass before touching main code**

```
cargo test -p mecha-oracle engine::casting
```

Expected: all remaining tests pass.

- [ ] **Remove step guard from `play_land`**

In `src/engine/casting.rs`, delete:

```rust
    if !matches!(state.step, Step::PreCombatMain | Step::PostCombatMain) {
        return Err(EngineError::CannotCastNow);
    }
```

- [ ] **Remove step guard from `cast_creature`**

Delete:

```rust
    if !matches!(state.step, Step::PreCombatMain | Step::PostCombatMain) {
        return Err(EngineError::CannotCastNow);
    }
```

- [ ] **Run to confirm pass**

```
cargo test -p mecha-oracle engine::casting
```

Expected: all tests pass.

- [ ] **Commit**

```bash
git add src/engine/casting.rs
git commit -m "refactor: remove step guards from play_land and cast_creature"
```

---

## Task 5 — `advance_step` resets `priority_player`

**Files:**
- Modify: `src/engine/turn.rs`

- [ ] **Write failing test**

Add to the `tests` block in `src/engine/turn.rs`:

```rust
#[test]
fn advance_step_resets_priority_to_active_player() {
    let mut gs = make_state();
    gs.step = Step::PreCombatMain;
    gs.priority_player = PlayerId(1); // manually set to NAP

    let gs = advance_step(gs);

    assert_eq!(gs.priority_player, PlayerId(0)); // reset to AP
    assert_eq!(gs.step(), Step::BeginningOfCombat);
}
```

- [ ] **Run to confirm failure**

```
cargo test -p mecha-oracle engine::turn::tests::advance_step_resets_priority_to_active_player
```

Expected: FAIL — `priority_player` is still `PlayerId(1)`.

- [ ] **Implement**

In `src/engine/turn.rs`, in `advance_step`, add one line after the mana drain loop and the `state.mana_checkpoint = None;` line, before the `extra_steps` check:

```rust
    state.priority_player = state.active_player;
```

The start of `advance_step` becomes:

```rust
pub fn advance_step(mut state: GameState) -> GameState {
    for player in state.players.iter_mut() {
        player.mana_pool = Default::default();
    }
    state.mana_checkpoint = None;
    state.priority_player = state.active_player;
    if let Some(next) = state.extra_steps.pop_front() {
        state.step = next;
        return state;
    }
    // ... rest unchanged
```

- [ ] **Run to confirm pass**

```
cargo test -p mecha-oracle engine::turn
```

Expected: all tests pass including the new one.

- [ ] **Commit**

```bash
git add src/engine/turn.rs
git commit -m "feat: advance_step resets priority_player to active player on each new step"
```

---

## Task 6 — `apply_step_start` auto-resolves `CombatDamage`

**Files:**
- Modify: `src/engine/turn.rs`

- [ ] **Write failing test**

Add to the `tests` block in `src/engine/turn.rs`:

```rust
#[test]
fn apply_step_start_resolves_combat_damage() {
    let db = test_db();
    let mut gs = make_state();

    // Put an unblocked 2/2 attacker for P0
    let id = gs.alloc_id();
    let mut obj = CardObject::new(
        id,
        db.get("Grizzly Bears").unwrap().clone(),
        PlayerId(0),
        Zone::Battlefield,
    );
    obj.summoning_sick = false;
    gs.battlefield.push(id);
    gs.add_object(obj);
    gs.combat.attackers = vec![id];
    gs.combat.blocking_map.insert(id, vec![]);
    gs.step = Step::CombatDamage;

    let gs = apply_step_start(gs);

    // Unblocked 2/2 deals 2 damage to P1
    assert_eq!(gs.get_player(PlayerId(1)).unwrap().life, 18);
}
```

- [ ] **Run to confirm failure**

```
cargo test -p mecha-oracle engine::turn::tests::apply_step_start_resolves_combat_damage
```

Expected: FAIL — P1 still has 20 life.

- [ ] **Implement**

In `src/engine/turn.rs`, add the `use` import for `deal_combat_damage` at the top of the file (alongside the existing `use super::` imports at the top of the `turn.rs` module):

```rust
use super::combat::deal_combat_damage;
```

Then in `apply_step_start`, add the `CombatDamage` arm:

```rust
pub fn apply_step_start(state: GameState) -> GameState {
    match state.step {
        Step::Untap => untap_step(state),
        Step::Draw => draw_step(state),
        Step::Cleanup => cleanup_step(state),
        Step::CombatDamage => deal_combat_damage(state),
        _ => state,
    }
}
```

- [ ] **Run to confirm pass**

```
cargo test -p mecha-oracle engine::turn
```

Expected: all tests pass.

- [ ] **Confirm the full test suite passes**

```
cargo test
```

Expected: all tests pass.

- [ ] **Commit**

```bash
git add src/engine/turn.rs
git commit -m "feat: auto-resolve combat damage in apply_step_start (CR 510.2)"
```

---

## Task 7 — `GameView` type updates and `DealCombatDamage` removal

**Files:**
- Modify: `src/serve.rs`

This task updates the view model types (use `PlayerId`/`ObjectId` directly, add new fields) and removes the `DealCombatDamage` action entirely.

- [ ] **Write failing tests**

In the `tests` block in `src/serve.rs`, **replace** the existing `build_game_view_initial_life_and_step` test body with:

```rust
#[test]
fn build_game_view_initial_life_and_step() {
    let config = vec![
        (0..10).map(|_| "Forest".to_string()).collect(),
        (0..10).map(|_| "Forest".to_string()).collect(),
    ];
    let db = test_db();
    let gs = build_game_state(config, &db, false).unwrap();
    let view = build_game_view(&gs);
    assert_eq!(view.p1.life, 20);
    assert_eq!(view.p2.life, 20);
    assert_eq!(view.active_player, PlayerId(0));
    assert_eq!(view.priority_player, PlayerId(0));
    assert_eq!(view.step, "PreCombatMain");
    assert_eq!(view.turn, 1);
    assert!(!view.attackers_declared);
    assert!(!view.blockers_declared);
}
```

**Add** a new test:

```rust
#[test]
fn game_view_includes_combat_declared_flags() {
    let config = vec![
        (0..10).map(|_| "Forest".to_string()).collect(),
        (0..10).map(|_| "Forest".to_string()).collect(),
    ];
    let db = test_db();
    let mut gs = build_game_state(config, &db, false).unwrap();
    // Navigate to DeclareAttackers: 4 passes (2 per step × 2 steps)
    for _ in 0..4 {
        gs = dispatch_action(gs, ActionRequest::AdvanceStep).unwrap();
    }
    assert!(!build_game_view(&gs).attackers_declared);

    gs = dispatch_action(gs, ActionRequest::DeclareAttackers { attacker_ids: vec![] }).unwrap();
    assert!(build_game_view(&gs).attackers_declared);
}
```

- [ ] **Run to confirm failures**

```
cargo test -p mecha-oracle serve
```

Expected: compilation errors about type mismatches (`u8` vs `PlayerId`, missing fields).

- [ ] **Update `CardView` to use `ObjectId`**

In `src/serve.rs`, change `CardView.id`:

```rust
#[derive(Serialize)]
struct CardView {
    id: ObjectId,
    // ... rest unchanged
```

- [ ] **Update `GameView` to use `PlayerId` and add new fields**

Replace the `GameView` struct:

```rust
#[derive(Serialize)]
struct GameView {
    turn: u32,
    step: String,
    active_player: PlayerId,
    priority_player: PlayerId,
    lands_played_this_turn: u32,
    game_over: bool,
    winner: Option<PlayerId>,
    p1: PlayerView,
    p2: PlayerView,
    can_reset_mana: bool,
    attackers_declared: bool,
    blockers_declared: bool,
}
```

- [ ] **Update `build_game_view` to populate the new fields**

Replace `build_game_view`:

```rust
fn build_game_view(state: &GameState) -> GameView {
    GameView {
        turn: state.turn_number,
        step: format!("{:?}", state.step()),
        active_player: state.active_player,
        priority_player: state.priority_player,
        lands_played_this_turn: state.lands_played_this_turn,
        game_over: state.is_game_over(),
        winner: state.winner(),
        p1: build_player_view(state, PlayerId(0)),
        p2: build_player_view(state, PlayerId(1)),
        can_reset_mana: state.mana_checkpoint.is_some(),
        attackers_declared: state.combat.attackers_declared,
        blockers_declared: state.combat.blockers_declared,
    }
}
```

- [ ] **Update `to_card_view` in `build_player_view` to use `obj.id` directly**

In `build_player_view`, change:

```rust
        id: obj.id.0,
```

to:

```rust
        id: obj.id,
```

- [ ] **Remove `DealCombatDamage` from `ActionRequest` and `dispatch_action`**

Remove the `DealCombatDamage` variant from the `ActionRequest` enum:

```rust
#[derive(Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
enum ActionRequest {
    TapLand { object_id: u64 },
    PlayLand { object_id: u64 },
    CastCreature { object_id: u64 },
    DeclareAttackers { attacker_ids: Vec<u64> },
    DeclareBlockers { blocks: Vec<[u64; 2]> },
    AdvanceStep,
    ResetMana,
}
```

Remove the `DealCombatDamage` arm from `dispatch_action`. The import `deal_combat_damage` in `serve.rs` is no longer needed there either — remove it from the `use` statement at the top:

```rust
use mecha_oracle::engine::combat::{declare_attackers, declare_blockers};
```

- [ ] **Run to confirm pass**

```
cargo test -p mecha-oracle serve
```

Expected: all tests pass. If `build_game_view_initial_life_and_step` fails on other fields, adjust the assertions to match actual values.

- [ ] **Commit**

```bash
git add src/serve.rs
git commit -m "feat: update GameView to use PlayerId/ObjectId; add priority_player and declared flags; remove DealCombatDamage"
```

---

## Task 8 — Two-phase priority in `AdvanceStep`

**Files:**
- Modify: `src/serve.rs`

- [ ] **Write failing tests**

**Replace** `dispatch_advance_step_from_pre_combat_main_to_beginning_of_combat` with:

```rust
#[test]
fn ap_passing_priority_shifts_to_nap_without_advancing_step() {
    let config = vec![
        (0..10).map(|_| "Forest".to_string()).collect(),
        (0..10).map(|_| "Forest".to_string()).collect(),
    ];
    let db = test_db();
    let gs = build_game_state(config, &db, false).unwrap();
    assert_eq!(gs.step(), Step::PreCombatMain);
    assert_eq!(gs.priority_player, PlayerId(0));

    let gs = dispatch_action(gs, ActionRequest::AdvanceStep).unwrap();

    assert_eq!(gs.step(), Step::PreCombatMain); // step did NOT advance
    assert_eq!(gs.priority_player, PlayerId(1)); // priority shifted to NAP
}

#[test]
fn nap_passing_priority_advances_step() {
    let config = vec![
        (0..10).map(|_| "Forest".to_string()).collect(),
        (0..10).map(|_| "Forest".to_string()).collect(),
    ];
    let db = test_db();
    let gs = build_game_state(config, &db, false).unwrap();

    let gs = dispatch_action(gs, ActionRequest::AdvanceStep).unwrap(); // AP passes
    let gs = dispatch_action(gs, ActionRequest::AdvanceStep).unwrap(); // NAP passes → advances

    assert_eq!(gs.step(), Step::BeginningOfCombat);
    assert_eq!(gs.priority_player, PlayerId(0)); // resets to AP
}
```

**Update** `advancing_from_end_step_auto_advances_to_next_upkeep` — replace the 7-pass loop and the final assertion:

```rust
#[test]
fn advancing_from_end_step_auto_advances_to_next_upkeep() {
    let config = vec![
        (0..10).map(|_| "Forest".to_string()).collect(),
        (0..10).map(|_| "Forest".to_string()).collect(),
    ];
    let db = test_db();
    let mut gs = build_game_state(config, &db, false).unwrap();
    // Two passes per step × 7 steps (PC→BOC→DA→DB→CD→EOC→PC2→End)
    for _ in 0..14 {
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

**Update** `can_attack_true_only_for_active_player_at_declare_attackers` — replace the 2-pass navigation:

```rust
    // 4 passes to reach DeclareAttackers (2 per step × 2 steps)
    for _ in 0..4 {
        gs = dispatch_action(gs, ActionRequest::AdvanceStep).unwrap();
    }
    assert_eq!(gs.step(), Step::DeclareAttackers);
```

**Update** `can_block_true_only_for_defending_player_at_declare_blockers` — replace the navigation section:

```rust
    // 4 passes to reach DeclareAttackers
    for _ in 0..4 {
        gs = dispatch_action(gs, ActionRequest::AdvanceStep).unwrap();
    }
    // Declare P1's bear as attacker
    let gs = dispatch_action(
        gs,
        ActionRequest::DeclareAttackers { attacker_ids: vec![p1_id] },
    ).unwrap();
    // 2 passes to advance DeclareAttackers → DeclareBlockers
    let gs = dispatch_action(gs, ActionRequest::AdvanceStep).unwrap();
    let gs = dispatch_action(gs, ActionRequest::AdvanceStep).unwrap();
    assert_eq!(gs.step(), Step::DeclareBlockers);
```

- [ ] **Run to confirm failures**

```
cargo test -p mecha-oracle serve
```

Expected: the new priority tests fail; the updated navigation tests fail with wrong step counts.

- [ ] **Implement two-phase priority in `dispatch_action`**

Replace the `AdvanceStep` arm in `dispatch_action`:

```rust
        ActionRequest::AdvanceStep => {
            let nap = state.opponent_of(state.active_player);
            if state.priority_player == state.active_player {
                state.priority_player = nap;
                Ok(state)
            } else {
                Ok(advance_with_auto_steps(state))
            }
        }
```

- [ ] **Run to confirm pass**

```
cargo test -p mecha-oracle serve
```

Expected: all tests pass.

- [ ] **Run full suite**

```
cargo test
```

Expected: all tests pass.

- [ ] **Commit**

```bash
git add src/serve.rs
git commit -m "feat: implement two-phase priority passing — step only advances after both players pass (CR 117)"
```

---

## Task 9 — Frontend: player ID convention and remove Resolve Damage button

**Files:**
- Modify: `src/serve.html`

All occurrences of `=== 1` / `=== 2` that test player identity now use 0/1. Display labels (shown to user) still say "Player 1" / "Player 2" via `playerId + 1`.

- [ ] **Update `describeAction`**

Replace the entire `describeAction` function:

```javascript
function describeAction(action) {
  const ap = currentState.active_player; // 0 or 1
  const apLabel = ap + 1;               // display: 1 or 2
  switch (action.type) {
    case 'tap_land':       return `<span class="who">P${apLabel}</span> tapped a land for mana`;
    case 'play_land':      return `<span class="who">P${apLabel}</span> played a land`;
    case 'cast_creature':  return `<span class="who">P${apLabel}</span> cast a creature`;
    case 'declare_attackers': return `<span class="who">P${apLabel}</span> declared attackers`;
    case 'declare_blockers': {
      const defLabel = ap === 0 ? 2 : 1;
      return `<span class="who">P${defLabel}</span> declared blockers`;
    }
    case 'advance_step': {
      // priority_player is now whoever RECEIVED priority — the passer was the other one
      const passerLabel = currentState.priority_player === 0 ? 2 : 1;
      return `<span class="log-engine">— P${passerLabel} passed priority —</span>`;
    }
    case 'reset_mana': return `<span class="who">P${apLabel}</span> reset mana`;
    default: return JSON.stringify(action);
  }
}
```

- [ ] **Update `actionLogClass`**

Replace:

```javascript
function actionLogClass(action) {
  if (action.type === 'advance_step') return 'log-engine';
  if (action.type === 'declare_blockers') {
    return currentState.active_player === 0 ? 'log-p2' : 'log-p1';
  }
  return currentState.active_player === 0 ? 'log-p1' : 'log-p2';
}
```

- [ ] **Update `renderTurnTracker`**

Change the active player label:

```javascript
  const ap = s.active_player === 0 ? 'Player 1' : 'Player 2';
```

- [ ] **Update game-over winner display in `renderActions`**

Change:

```javascript
    const winner = s.winner != null ? `Player ${s.winner + 1} wins!` : 'Draw!';
```

- [ ] **Start the dev server and verify no console errors**

```
cargo run -- serve
```

Open `http://localhost:3000` in a browser. Check console for JS errors. The board should render correctly; the sidebar will still work (though it looks the same for now). Log entries should say "P1 passed priority —" etc.

- [ ] **Commit**

```bash
git add src/serve.html
git commit -m "fix: update frontend to use 0/1 player IDs; remove deal_combat_damage references"
```

---

## Task 10 — Two-section sidebar HTML/CSS

**Files:**
- Modify: `src/serve.html`

Replace the sidebar structure so there are two equal player panes, each with a header, scrollable actions area, and bottom-pinned pass button.

- [ ] **Replace the sidebar CSS**

Remove the existing `#sidebar`, `#sidebar-header`, `#actions`, and `#log` rules. Add:

```css
#sidebar {
  width: 240px; background: #161b22; border-left: 1px solid #30363d;
  display: flex; flex-direction: column; flex-shrink: 0;
}
.player-pane {
  flex: 1; display: flex; flex-direction: column; min-height: 0; overflow: hidden;
}
.player-pane.p2 { border-bottom: 2px solid #30363d; }
.pane-header {
  padding: 6px 10px; border-bottom: 1px solid #30363d;
  display: flex; align-items: center; gap: 6px; flex-shrink: 0;
}
.pane-name { font-weight: bold; font-size: 12px; }
.pane-name.p2 { color: #ff7b7b; }
.pane-name.p1 { color: #7bff9a; }
.priority-badge {
  font-size: 9px; padding: 1px 5px; border-radius: 3px;
  font-weight: bold; text-transform: uppercase; letter-spacing: 0.5px;
}
.priority-badge.active { background: #2a2500; border: 1px solid #5a4a00; color: #ffd700; }
.priority-badge.waiting { background: #1e2430; border: 1px solid #2a3a4a; color: #444; }
.pane-actions { flex: 1; overflow-y: auto; padding: 6px 8px; }
.pane-pass { flex-shrink: 0; padding: 6px 8px; border-top: 1px solid #30363d; }
```

- [ ] **Replace the sidebar HTML**

Replace the `<div id="sidebar">` block (currently contains `#sidebar-header`, `#actions`, and `#log`) with:

```html
<div id="sidebar">
  <div class="player-pane p2" id="pane-p2">
    <div class="pane-header">
      <span class="pane-name p2">Player 2</span>
      <span class="priority-badge waiting" id="badge-p2">waiting</span>
    </div>
    <div class="pane-actions" id="actions-p2"></div>
    <div class="pane-pass">
      <button class="action-btn pass" id="pass-p2" disabled
              onclick="sendAction({type:'advance_step'})">Pass priority →</button>
    </div>
  </div>
  <div class="player-pane p1" id="pane-p1">
    <div class="pane-header">
      <span class="pane-name p1">Player 1</span>
      <span class="priority-badge waiting" id="badge-p1">waiting</span>
    </div>
    <div class="pane-actions" id="actions-p1"></div>
    <div class="pane-pass">
      <button class="action-btn pass" id="pass-p1" disabled
              onclick="sendAction({type:'advance_step'})">Pass priority →</button>
    </div>
  </div>
</div>
```

Note: the log `<div id="log">` is removed here — it moves to the drawer in Task 12.

- [ ] **Add a log button to the turn tracker** 

In the `renderTurnTracker` function, append a small log-toggle button to the inner HTML:

```javascript
function renderTurnTracker(s) {
  const cur = STEP_ORDER.indexOf(s.step);
  const chips = STEP_ORDER.map((step, i) => {
    const cls = i < cur ? 'done' : i === cur ? 'active' : 'upcoming';
    return `<span class="step-chip ${cls}">${STEP_LABELS[step]}</span>`;
  }).join('<span class="step-sep">·</span>');
  const ap = s.active_player === 0 ? 'Player 1' : 'Player 2';
  document.getElementById('turn-tracker').innerHTML =
    `<span style="color:#888;margin-right:4px">Turn ${s.turn}</span>${chips}` +
    `<span class="active-label">Active: ${ap}</span>` +
    `<button onclick="toggleLog()" style="margin-left:8px;background:#1c2a3a;border:1px solid #2a4a6a;border-radius:3px;padding:2px 8px;color:#7ab8e8;font-size:10px;cursor:pointer">Log</button>`;
}
```

- [ ] **Initialize `appendLog` to target the drawer log div** (will be created in Task 12 — for now keep the function but point it at a new target id `log-entries` which we'll add in Task 12).

Leave `appendLog` unchanged for now; it will be updated in Task 12.

- [ ] **Start dev server and verify layout**

```
cargo run -- serve
```

Open `http://localhost:3000`. You should see two stacked panes in the sidebar — one red-labelled "Player 2" at top, one green-labelled "Player 1" at bottom. Both "Pass priority →" buttons are visible but disabled/grey. No actions shown yet (they'll be wired in Task 11).

- [ ] **Commit**

```bash
git add src/serve.html
git commit -m "feat: replace sidebar with two-section per-player panes"
```

---

## Task 11 — Per-player action rendering and priority state

**Files:**
- Modify: `src/serve.html`

Replace the `renderActions` function with `renderPanes`, which populates each player pane independently.

- [ ] **Replace `renderActions` with `renderPanes`**

Delete the existing `renderActions` function entirely. Add:

```javascript
function renderPanes(s) {
  if (s.game_over) {
    const winner = s.winner != null ? `Player ${s.winner + 1} wins!` : 'Draw!';
    const msg = `<div style="background:#2a1a00;border:1px solid #8a5a00;border-radius:4px;padding:8px 10px;font-size:12px;color:#ffcc44;text-align:center">Game Over — ${winner}</div>`;
    document.getElementById('actions-p1').innerHTML = msg;
    document.getElementById('actions-p2').innerHTML = '';
    document.getElementById('pass-p1').disabled = true;
    document.getElementById('pass-p2').disabled = true;
    return;
  }

  const passBlocked =
    (s.step === 'DeclareAttackers' && !s.attackers_declared) ||
    (s.step === 'DeclareBlockers'  && !s.blockers_declared);

  [0, 1].forEach(pid => {
    const suffix    = pid === 0 ? 'p1' : 'p2';
    const myData    = pid === 0 ? s.p1 : s.p2;
    const oppData   = pid === 0 ? s.p2 : s.p1;
    const iAmAP     = pid === s.active_player;
    const iAmNAP    = !iAmAP;
    const hasPriority = pid === s.priority_player && !passBlocked;
    const canPass   = hasPriority;

    // Priority badge
    const badge = document.getElementById(`badge-${suffix}`);
    badge.textContent = hasPriority ? 'has priority' : 'waiting';
    badge.className   = 'priority-badge ' + (hasPriority ? 'active' : 'waiting');

    // Pass button
    const passBtn = document.getElementById(`pass-${suffix}`);
    passBtn.disabled      = !canPass;
    passBtn.style.opacity = canPass ? '1' : '0.35';

    let html = '';

    // Main phase: land/creature actions for AP
    if (iAmAP && (s.step === 'PreCombatMain' || s.step === 'PostCombatMain')) {
      const untapped = myData.lands.filter(c => !c.tapped);
      if (untapped.length > 0) {
        html += group('Tap for mana', untapped.map(c =>
          btn(c.name, `sendAction({type:'tap_land',object_id:${c.id}})`)));
      }
      if (s.lands_played_this_turn === 0) {
        const lands = myData.hand.filter(c => c.type_line.includes('Land'));
        if (lands.length > 0) {
          html += group('Play land', lands.map(c =>
            btn('Play ' + esc(c.name), `sendAction({type:'play_land',object_id:${c.id}})`)));
        }
      }
      const castable = myData.hand.filter(c => c.type_line.includes('Creature'));
      if (castable.length > 0) {
        html += group('Cast creature', castable.map(c =>
          btn(esc(c.name), `sendAction({type:'cast_creature',object_id:${c.id}})`, c.mana_cost || '')));
      }
    }

    // Declare attackers: selection UI (AP, before confirmation)
    if (iAmAP && s.step === 'DeclareAttackers' && !s.attackers_declared) {
      const eligible = myData.creatures.filter(c => c.can_attack);
      const btns = eligible.map(c => {
        const sel = attackersSelected.includes(c.id);
        return `<button class="action-btn${sel ? ' selected' : ''}" onclick="toggleAttacker(${c.id})">${esc(c.name)}${sel ? ' ✓' : ''}</button>`;
      });
      btns.push(btn('Confirm Attackers', 'confirmAttackers()'));
      html += group('Select attackers', btns.length > 1 ? btns : [btn('Confirm Attackers (no attackers)', 'confirmAttackers()')]);
    }

    // Declare attackers: summary after confirmation
    if (iAmAP && s.step === 'DeclareAttackers' && s.attackers_declared) {
      const n = myData.creatures.filter(c => c.is_attacking).length;
      html += `<p style="font-size:11px;color:#666;padding:4px">${n} attacker${n === 1 ? '' : 's'} declared</p>`;
    }

    // Declare blockers: assignment UI (NAP, before confirmation)
    if (iAmNAP && s.step === 'DeclareBlockers' && !s.blockers_declared) {
      const attackers = oppData.creatures.filter(c => c.is_attacking);
      const blockers  = myData.creatures.filter(c => c.can_block);
      if (attackers.length > 0 && blockers.length > 0) {
        let inner = '';
        for (const atk of attackers) {
          inner += `<div style="margin-bottom:6px"><div class="action-group-label">Block ${esc(atk.name)}</div>`;
          for (const blk of blockers) {
            const assigned = blockersAssignment[blk.id] === atk.id;
            inner += `<button class="action-btn${assigned ? ' selected' : ''}" onclick="toggleBlocker(${blk.id},${atk.id})">${esc(blk.name)}${assigned ? ' ✓' : ''}</button>`;
          }
          inner += '</div>';
        }
        inner += btn('Confirm Blockers', 'confirmBlockers()');
        html += group('Assign blockers', [inner]);
      } else {
        html += group('Assign blockers', [btn('Confirm Blockers (none)', 'confirmBlockers()')]);
      }
    }

    // Declare blockers: summary after confirmation
    if (iAmNAP && s.step === 'DeclareBlockers' && s.blockers_declared) {
      html += `<p style="font-size:11px;color:#666;padding:4px">Blockers declared</p>`;
    }

    // Reset mana (AP only, when checkpoint exists)
    if (iAmAP && s.can_reset_mana) {
      html += group('Mana', [`<button class="action-btn" onclick="sendAction({type:'reset_mana'})">↩ Reset mana</button>`]);
    }

    document.getElementById(`actions-${suffix}`).innerHTML = html;
  });
}
```

- [ ] **Update `render` to call `renderPanes` instead of `renderActions`**

In the `render` function, replace:

```javascript
  renderActions(s);
```

with:

```javascript
  renderPanes(s);
```

- [ ] **Start dev server and do a full manual walkthrough**

```
cargo run -- serve
```

Open `http://localhost:3000`. Play through an entire game sequence:

1. Verify P1's pane shows "has priority" badge and enabled "Pass priority →" at start
2. P1 taps land, casts creature in Main 1 — verify actions appear correctly in P1's pane
3. Pass priority (P1): badge moves to P2; P1's button disables
4. Pass priority (P2): step advances to Beginning of Combat; P1 badge re-activates
5. Advance to Declare Attackers; confirm P1 sees attacker selection UI, P2 sees nothing (both pass buttons disabled until declaration)
6. Confirm attackers; verify "1 attacker declared" summary in P1 pane, pass buttons re-enable for P1
7. Both pass → Declare Blockers; verify P2 sees blocker assignment, P1 has active pass priority
8. Confirm blockers; verify both pass, step auto-advances to CombatDamage, damage auto-resolves, land at CombatDamage priority window
9. Both pass through CombatDamage and EndOfCombat
10. Verify game log entries are correct throughout

- [ ] **Commit**

```bash
git add src/serve.html
git commit -m "feat: per-player action rendering with priority-gated pass buttons"
```

---

## Task 12 — Log drawer

**Files:**
- Modify: `src/serve.html`

- [ ] **Add the drawer CSS**

Add to the `<style>` block:

```css
#log-drawer {
  display: none; width: 220px; background: #161b22;
  border-left: 1px solid #30363d; flex-direction: column; flex-shrink: 0;
}
#log-drawer.open { display: flex; }
#log-drawer-header {
  padding: 6px 10px; border-bottom: 1px solid #30363d;
  display: flex; align-items: center; justify-content: space-between; flex-shrink: 0;
}
#log-entries { flex: 1; overflow-y: auto; padding: 8px; }
```

- [ ] **Add the drawer HTML** after the closing `</div>` of `#sidebar` (and before `</div>` of `#root`):

```html
<div id="log-drawer">
  <div id="log-drawer-header">
    <span style="font-size:10px;text-transform:uppercase;letter-spacing:1px;color:#888">Game Log</span>
    <button onclick="toggleLog()" style="background:none;border:none;color:#555;cursor:pointer;font-size:14px;line-height:1">✕</button>
  </div>
  <div id="log-entries"></div>
</div>
```

- [ ] **Add `toggleLog` function**

```javascript
function toggleLog() {
  document.getElementById('log-drawer').classList.toggle('open');
}
```

- [ ] **Update `appendLog` to target `log-entries`**

Replace:

```javascript
function appendLog(html, cls) {
  const log = document.getElementById('log');
  const entry = document.createElement('div');
  entry.className = 'log-entry ' + (cls || '');
  entry.innerHTML = html;
  log.appendChild(entry);
  log.scrollTop = log.scrollHeight;
}
```

with:

```javascript
function appendLog(html, cls) {
  const log = document.getElementById('log-entries');
  const entry = document.createElement('div');
  entry.className = 'log-entry ' + (cls || '');
  entry.innerHTML = html;
  log.appendChild(entry);
  log.scrollTop = log.scrollHeight;
}
```

- [ ] **Verify**

```
cargo run -- serve
```

Click "Log" in the turn tracker — a 220px panel should slide in to the right of the sidebar, with the board narrowing slightly. Board, sidebar, and drawer should all render without overlap. Log entries appear as actions are taken. Clicking "✕" closes it.

- [ ] **Commit**

```bash
git add src/serve.html
git commit -m "feat: add toggleable game log drawer"
```

---

## Task 13 — Spacebar hotkey for priority passing

**Files:**
- Modify: `src/serve.html`

- [ ] **Add the event listener**

At the bottom of the `<script>` block, just before the `fetchState()` call, add:

```javascript
document.addEventListener('keydown', e => {
  if (e.code === 'Space' && !e.target.closest('input, textarea, button')) {
    e.preventDefault();
    sendAction({ type: 'advance_step' });
  }
});
```

- [ ] **Verify**

```
cargo run -- serve
```

Click somewhere on the board (not on a button) and press Space. It should pass priority for the player who currently holds it — the priority badge should update, or the step should advance (if it was NAP's turn). Pressing Space while focused on a button should NOT trigger it (normal button activation instead).

- [ ] **Commit**

```bash
git add src/serve.html
git commit -m "feat: spacebar hotkey passes priority for the current priority holder"
```

---

## Self-Review

**Spec coverage check:**

| Spec requirement | Task |
|-----------------|------|
| PlayerId/ObjectId transparent Serialize | Task 1 |
| attackers_declared, blockers_declared flags | Task 2 |
| Remove step guards: declare_attackers, declare_blockers, deal_combat_damage | Task 3 |
| Remove step guards: play_land, cast_creature | Task 4 |
| advance_step resets priority_player | Task 5 |
| apply_step_start auto-resolves CombatDamage | Task 6 |
| GameView: priority_player, PlayerId, ObjectId, declared flags | Task 7 |
| Remove DealCombatDamage action | Task 7 |
| Two-phase AdvanceStep priority | Task 8 |
| frontend: 0/1 player IDs | Task 9 |
| frontend: two-section sidebar | Task 10 |
| frontend: per-player actions, priority badges, pass buttons bottom-aligned | Task 11 |
| frontend: pass buttons disabled before declaration in combat steps | Task 11 |
| frontend: log drawer | Task 12 |
| frontend: spacebar hotkey | Task 13 |

All spec requirements covered. ✓

**Type consistency check:**

- `PlayerId` used throughout `GameView` fields (Tasks 7, 8) ✓
- `ObjectId` used in `CardView.id` (Task 7); JS accesses as `c.id` (numeric, unchanged) ✓
- `attackers_declared`/`blockers_declared` defined in Task 2, set in Task 3, exposed in Task 7, consumed in Task 11 ✓
- `priority_player` defined in `GameView` Task 7, consumed in Task 11 renderPanes ✓
- `renderPanes` defined in Task 11, called in `render` in Task 11 ✓
- `appendLog` targets `log-entries` set in Task 12; `toggleLog` defined in Task 12; Log button wired in Task 10 ✓
