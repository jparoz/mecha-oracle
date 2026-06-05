# Turn Flow CR Compliance

**Date:** 2026-06-05  
**Scope:** Bring turn sequencing, mana handling, and combat state closer to the Comprehensive Rules. Eight discrete fixes across the engine and serve layer.

---

## Fix 1 — Mana drain at end of each step (CR 106.4)

**Problem:** `advance_step` never drains mana pools, so floating mana carries across step boundaries indefinitely.

**Fix:** At the top of `advance_step` in `engine/turn.rs`, drain every player's `mana_pool` to default before the step transition. This fires once per step boundary, covering all steps and phases (since steps are our atomic unit). The drain is a no-op in the common case where the pool is already empty.

**Affected files:** `src/engine/turn.rs`  
**New tests:** mana drained when advancing from PreCombatMain; mana drained even with no actions taken.

---

## Fix 2 — Mana undo within a priority window

**Problem:** No mechanism to let a player experiment with different mana configurations before committing to a cast. Once a land is tapped, it stays tapped regardless of whether mana was actually spent.

**Behaviour (matches Arena/MTGO convention):** While a player holds priority and has not yet committed to any non-mana action, they can freely tap lands and then reset all of those taps — lands are untapped, mana pools are restored to the pre-tap state. Committing happens when the player: passes priority, plays a land, casts a spell, declares attackers, declares blockers, or resolves combat damage.

**Data model — add to `GameState`:**
```rust
pub mana_checkpoint: Option<ManaCheckpoint>

pub struct ManaCheckpoint {
    pub pools: HashMap<PlayerId, ManaPool>,
    pub tapped_lands: Vec<ObjectId>,
}
```

**Behaviour in `tap_land_for_mana`:** If `mana_checkpoint` is `None`, save the current mana pools and an empty `tapped_lands` list before applying the tap. Then append the land's id to `tapped_lands`.

**New engine function `reset_mana(state) -> Result<GameState, EngineError>`:** If checkpoint is `None`, return `Err(EngineError::NoManaCheckpoint)`. Otherwise restore each player's pool from the checkpoint, untap every land in `tapped_lands`, and clear `mana_checkpoint`.

**Checkpoint cleared (committed) in:** `advance_step` (which covers turn transitions via `start_next_turn`), `play_land`, `cast_creature`, `declare_attackers`, `declare_blockers`, `deal_combat_damage`.

**View:** Add `can_reset_mana: bool` to `GameView` — true when `mana_checkpoint.is_some()`. UI renders a "Reset Mana" button in the actions panel when true.

**New `EngineError` variant:** `NoManaCheckpoint`.

**Affected files:** `src/types/game_state.rs`, `src/engine/mana.rs`, `src/engine/turn.rs`, `src/engine/casting.rs`, `src/engine/combat.rs`, `src/serve.rs`  
**New tests:** checkpoint created on first tap; second tap appends, doesn't overwrite; reset untaps lands and restores pools; reset fails with no checkpoint; checkpoint cleared after advance_step; checkpoint cleared after cast.

---

## Fix 3 — Step-aware `can_attack` / `can_block` in `CardView`

**Problem:** `CardView.can_attack` and `can_block` are computed from step-agnostic `CardObject` methods, so they reflect "could attack/block in theory" rather than "can be selected right now." The UI works around this by gating on step in JS, but the server state is misleading.

**Fix:** In `build_player_view` in `serve.rs`, replace the direct method calls:

- `can_attack` → `state.step == Step::DeclareAttackers && pid == state.active_player && obj.can_attack()`
- `can_block` → `state.step == Step::DeclareBlockers && pid != state.active_player && obj.can_block()`

`CardObject::can_attack()` and `can_block()` remain step-agnostic for engine validation use.

**Affected files:** `src/serve.rs`

---

## Fix 4 — Clear `CombatState` after `EndOfCombat`

**Problem:** `is_attacking` and `is_blocking` remain true on cards through PostCombatMain, End, and Cleanup, because `CombatState` is only wiped in `untap_step` (next turn's beginning).

**Fix:** In the `EndOfCombat` arm of `advance_step`, call `state.combat = CombatState::empty()` before returning.

**Affected files:** `src/engine/turn.rs`  
**New tests:** after advancing from EndOfCombat, combat.attackers is empty; after advancing from EndOfCombat, blocking_map is empty.

---

## Fix 5 — Step guard on `deal_combat_damage`

**Problem:** Every other state-mutating function (`play_land`, `cast_creature`, `declare_attackers`, `declare_blockers`) validates the current step. `deal_combat_damage` takes `GameState` by value and returns `GameState`, with no way to signal an error.

**Fix:** Change signature to `pub fn deal_combat_damage(state: GameState) -> Result<GameState, EngineError>`. Add guard: if `state.step != Step::CombatDamage`, return `Err(EngineError::CannotCastNow)`. Update `dispatch_action` in `serve.rs` to unwrap via `.map_err`.

**Affected files:** `src/engine/combat.rs`, `src/serve.rs`  
**Test updates:** all existing `deal_combat_damage` call sites in tests now need `.unwrap()`.

---

## Fix 6 — Parser comment (CR reference)

**Problem:** `strip_reminder_text` in `src/parser/oracle.rs` cites CR 305.6 (Land subtypes). The correct rule is CR 207.2b (reminder text defined).

**Fix:** Update the doc comment on line 5 of `src/parser/oracle.rs`.

**Affected files:** `src/parser/oracle.rs`

---

## Fix 7 — Start game at PreCombatMain (CR 103.8a)

**Problem:** `build_game_state` currently initialises the game at `Step::Untap` and calls `apply_step_start`. But the starting player's first turn has nothing to untap, no upkeep triggers (Phase 1), and no draw (CR 103.8a). Starting at Untap is wasteful and forces the player to manually advance through meaningless steps.

**Fix:** In `build_game_state`, remove the `apply_step_start` call. Instead, set `gs.step = Step::PreCombatMain` directly before returning. All other state fields (`lands_played_this_turn = 0`, `combat = CombatState::empty()`, mana pools empty, etc.) are already correct from `GameState::new`.

**Test updates in `serve.rs`:**
- `build_game_state_starts_at_untap` → rename and assert `Step::PreCombatMain`
- `build_game_view_initial_life_and_step` → assert `step == "PreCombatMain"`, `active_player == 1`
- `dispatch_advance_step_moves_to_upkeep` → rebuild to reflect new initial step; test advancing from PreCombatMain to BeginningOfCombat instead (or test a step that now makes sense to advance from)

**Affected files:** `src/serve.rs`

---

## Fix 8 — Auto-advance through no-priority steps (CR 117.3a)

**Problem:** Players currently receive a "Pass priority" prompt in every step, including Untap and Cleanup where CR 117.3a says players don't normally get priority. This requires unnecessary manual clicks and misrepresents the rules.

**Fix:** In `serve.rs`, extract a helper used inside `dispatch_action`'s `AdvanceStep` arm:

```rust
fn advance_with_auto_steps(mut state: GameState) -> GameState {
    loop {
        state = advance_step(state);
        state = apply_step_start(state);
        if !matches!(state.step, Step::Untap | Step::Cleanup) || state.is_game_over() {
            break;
        }
    }
    state
}
```

Replace the current `AdvanceStep` dispatch arm body with a call to this function. Fix 7 handles the initial-state case (game starts at PreCombatMain, no auto-stepping needed on init).

**Phase 1 note:** Since there are no triggered abilities, Cleanup never spawns an additional Cleanup step. Always auto-advancing Cleanup is correct for this phase.

**Mana drain interaction:** `advance_step` (fix 1) drains mana at each call. During automated Untap and Cleanup transitions there is no mana to drain, so the drain is a no-op.

**Test updates:** the existing `dispatch_advance_step_moves_to_upkeep` test exercises `AdvanceStep` starting from a manually-set Untap step — update it to call `advance_with_auto_steps` or restructure it to test what the dispatch now guarantees (advancing from Cleanup lands at Upkeep).

**Affected files:** `src/serve.rs`

---

## Summary of affected files

| File | Fixes |
|------|-------|
| `src/engine/turn.rs` | 1, 2 (clear checkpoint in advance_step), 4 |
| `src/engine/mana.rs` | 2 (checkpoint logic in tap_land_for_mana, new reset_mana) |
| `src/engine/casting.rs` | 2 (clear checkpoint in play_land, cast_creature) |
| `src/engine/combat.rs` | 2 (clear checkpoint in declare_attackers/blockers, deal_combat_damage), 5 |
| `src/engine/mod.rs` | 2 (new NoManaCheckpoint error variant) |
| `src/types/game_state.rs` | 2 (ManaCheckpoint struct, field on GameState) |
| `src/parser/oracle.rs` | 6 |
| `src/serve.rs` | 2 (GameView field, ResetMana action), 3, 7, 8 (advance_with_auto_steps helper) |
