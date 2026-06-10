# PermanentState / CardObject Split

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Status: Complete** — merged 2026-06-10 (`82741cd`).

**Goal:** Separate battlefield-specific mutable state out of `CardObject` into a new `PermanentState` struct. `CardObject` becomes zone-agnostic card identity; `PermanentState` exists only while a card is on the battlefield. Fixes a structural bug where `summoning_sick: true` leaked onto cards in every zone (e.g. hand cards showing "Summoning sickness" in the tooltip).

**Architecture:**
- New `src/types/permanent.rs` — `PermanentState` with the six battlefield fields plus `can_attack()`, `can_block()`, `has_keyword()`, `is_creature()`, `is_land()`, `effective_power()`, `effective_toughness()`. Clones `CardDefinition` on ETB (see field comment re: sync caveat if definitions ever mutate).
- `GameState.battlefield` changes from `Vec<ObjectId>` to `HashMap<ObjectId, PermanentState>`. Map membership is the "on battlefield" invariant; entering/leaving the battlefield is a single `insert`/`remove`.
- `CardObject` retains `id`, `definition`, `controller`, `owner`, `zone`, `has_keyword()`, `is_creature()`, `is_land()`.
- No `fn permanent()` paired helper on `GameState` — callers look up `objects` and `battlefield` separately.
- `current_power`/`current_toughness` kept on `PermanentState` now; retrofitting them for the effects layer later would be more disruptive than carrying them forward.

**Tech Stack:** Rust, no build step changes.

---

## Files

- New: `src/types/permanent.rs`
- Modify: `src/types/card_object.rs` — remove 6 battlefield fields + `can_attack`/`can_block`
- Modify: `src/types/game_state.rs` — `battlefield: HashMap<ObjectId, PermanentState>`
- Modify: `src/types/mod.rs` — export `PermanentState`
- Modify: `src/engine/turn.rs`
- Modify: `src/engine/stack.rs`
- Modify: `src/engine/casting.rs`
- Modify: `src/engine/state_based_actions.rs`
- Modify: `src/engine/activated.rs`
- Modify: `src/engine/mana.rs`
- Modify: `src/engine/combat.rs`
- Modify: `src/engine/triggered.rs` (test helper)
- Modify: `src/serve.rs`
- Modify: `tests/scripted_game.rs`

---

### Task 1: Create `PermanentState`

- [x] Write `src/types/permanent.rs` with struct, `new()`, `can_attack()`, `can_block()`, `has_keyword()`, `is_creature()`, `is_land()`, `effective_power/toughness()`, and unit tests
- [x] Export from `src/types/mod.rs`

### Task 2: Strip `CardObject`

- [x] Remove `tapped`, `summoning_sick`, `damage_marked`, `damaged_by_deathtouch`, `current_power`, `current_toughness` fields
- [x] Remove `can_attack()`, `can_block()`, `effective_power()`, `effective_toughness()` methods
- [x] Simplify `new()` (no battlefield fields to initialize)
- [x] Update `card_object.rs` tests

### Task 3: Change `GameState.battlefield`

- [x] `battlefield: HashMap<ObjectId, PermanentState>` in struct definition
- [x] `battlefield: HashMap::new()` in `GameState::new()`
- [x] Import `PermanentState` in `game_state.rs`

### Task 4: Update engine files

- [x] `engine/state_based_actions.rs` — iterate `battlefield` as `(&id, perm)`, `move_to_graveyard` uses `remove()`
- [x] `engine/turn.rs` — untap via `battlefield.get_mut()`, cleanup via `battlefield.values_mut()`
- [x] `engine/stack.rs` — ETB inserts into `battlefield` map (clone def before mutable borrow)
- [x] `engine/casting.rs` — `play_land` inserts into `battlefield` map
- [x] `engine/activated.rs` — tap-cost checks via `battlefield.get()`
- [x] `engine/mana.rs` — `tap_land_for_mana` tapped check via `battlefield`, `reset_mana` untap via `battlefield`
- [x] `engine/combat.rs` — attacker/blocker validation via perm; damage application via perm; power/toughness via perm
- [x] `engine/triggered.rs` — test helper updated

### Task 5: Update `serve.rs`

- [x] `to_card_view` closure looks up `perm = state.battlefield.get(&obj.id)` and reads all battlefield fields from it (falls back to `false`/`0`/`None` for hand/graveyard cards)
- [x] `bf_objects` iteration uses `battlefield.keys()`
- [x] Stack view uses `c.definition.power/toughness` for on-stack cards
- [x] Test helpers use `battlefield.insert(id, PermanentState::new(...))`

### Task 6: Update integration tests

- [x] `tests/scripted_game.rs` — `tap_all_lands_for_player` iterates `battlefield` as `(id, perm)`, `can_attack()` check via `battlefield.get()`, all inline creature setups use `PermanentState::new()`
- [x] All `battlefield.contains()` → `battlefield.contains_key()` across all files
