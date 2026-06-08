# Stack Implementation Design

**Date:** 2026-06-09
**CR references:** 405 (Stack), 116/117 (Priority), 603 (Triggered Abilities), 605 (Mana Abilities)

---

## Overview

Implement CR 405 — the stack — so that spells and non-mana abilities use the stack with proper priority passing before resolving, rather than resolving immediately. Players can respond to spells and abilities before they resolve.

**Scope:**
- Spells (creature spells for now) go on the stack
- Triggered abilities (e.g. ETB triggers) go on the stack
- Non-mana activated abilities go on the stack
- Mana abilities continue to resolve immediately (CR 405.6c)
- Special actions (playing lands) do not use the stack (CR 405.6d) but still require priority

---

## Data Model

### New file: `src/types/stack.rs`

```rust
pub struct StackId(pub u64);

pub enum StackPayload {
    Spell { card_id: ObjectId },
    TriggeredAbility { source_id: ObjectId, effect: Effect, label: String },
    ActivatedAbility  { source_id: ObjectId, effect: Effect, label: String },
}

pub struct StackObject {
    pub id: StackId,
    pub payload: StackPayload,
    pub controller: PlayerId,
}
```

A `Spell`'s `card_id` references an entry in `objects` with `zone = Zone::Stack`. Abilities carry their `Effect` directly and have no card entry in `objects`, matching CR 405.4.

### `GameState` changes (`src/types/game_state.rs`)

| Field | Before | After |
|---|---|---|
| `stack` | `Vec<ObjectId>` | `Vec<StackId>` — last element = top (CR 405.2) |
| `stack_objects` | *(absent)* | `HashMap<StackId, StackObject>` |
| `next_stack_id` | *(absent)* | `u64` — separate counter from `next_object_id` |
| `consecutive_passes` | *(absent)* | `u32` — counts consecutive passes since last stack action |

Add `alloc_stack_id(&mut self) -> StackId` helper mirroring `alloc_id`.

---

## Priority System

### New engine function: `pass_priority(state, player_id) -> Result<GameState, EngineError>`

1. Return `Err(EngineError::NotYourPriority)` if `player_id != state.priority_player`
2. Increment `state.consecutive_passes`
3. If `consecutive_passes >= 2` (all players passed in succession — CR 405.5):
   - Stack non-empty → call `resolve_top(state)`, reset `consecutive_passes = 0`, set `priority_player = active_player`
   - Stack empty → call `advance_step(state)`
4. Else: set `priority_player` to the other player

### After any spell/ability is added to the stack

- Reset `consecutive_passes = 0`
- The player who cast/activated retains priority (CR 117.3c)

### Priority enforcement

`cast_creature`, `play_land`, and non-mana `activate_ability` all check `priority_player == player_id` before proceeding. Violation returns `Err(EngineError::NotYourPriority)`.

Mana-producing activated abilities are exempt — they can be activated any time a mana payment is needed (CR 605.3a), and the engine's existing mana-checkpoint mechanism handles this.

### New `EngineError` variant

```rust
NotYourPriority,
```

---

## Resolution

### New engine function: `resolve_top(state) -> GameState`

1. Pop `stack.last()` to get the `StackId`; remove from both `stack` and `stack_objects`
2. Match on `StackPayload`:

   **`Spell { card_id }`** (permanent spells):
   - Move card `Zone::Stack → Zone::Battlefield`
   - Set `summoning_sick = true`
   - Call `collect_etb_triggers(state, card_id)` → `Vec<StackObject>`
   - Push triggers onto the stack in APNAP order (CR 405.3, 603.3b): active player's triggers first, then opponent's
   - Call `check_and_apply_sbas`

   **`TriggeredAbility { effect, controller, … }` or `ActivatedAbility { effect, controller, … }`:**
   - Execute `effect` steps against `controller` (draw cards, gain life, etc.)
   - Call `check_and_apply_sbas`

3. Reset `consecutive_passes = 0`, set `priority_player = active_player` (CR 117.3b: after a triggered ability is put on the stack, the active player receives priority — distinct from CR 117.3c where the *caster* retains priority after casting)

---

## Changes to Existing Engine Functions

### `cast_creature` (casting.rs)

**Before:** pay mana → move card to battlefield → fire ETB triggers immediately → check SBAs

**After:**
1. Check `priority_player == player_id` (else `NotYourPriority`)
2. Pay mana cost (unchanged)
3. Move card `Hand → Stack` (`zone = Zone::Stack`)
4. Allocate `StackId`, create `StackObject { payload: Spell { card_id }, controller: player_id }`
5. Push to `stack` / `stack_objects`
6. Reset `consecutive_passes = 0`; caster retains priority (CR 117.3c)

No ETB fires here — that happens inside `resolve_top` when the spell resolves.

### `activate_ability` (activated.rs)

The function is already split conceptually by `produces_mana`:

- **Mana path** (`produces_mana = true`): unchanged — pay costs, apply `AddMana` effect immediately, maintain checkpoint (CR 405.6c)
- **Non-mana path** (`produces_mana = false`):
  1. Check `priority_player == player_id`
  2. Pay costs (unchanged)
  3. Allocate `StackId`, create `StackObject { payload: ActivatedAbility { source_id, effect, label }, controller }`
  4. Push to stack; reset `consecutive_passes = 0`; activator retains priority (CR 117.3c)
  5. Effect execution block moves into `resolve_top`

### `triggered.rs`

`fire_etb_triggers(state, entering_id) -> GameState`
→ renamed to `collect_etb_triggers(state, entering_id) -> Vec<StackObject>`

Returns stack entries instead of executing effects. The call site in `casting.rs` is removed (triggers are now collected inside `resolve_top` after the spell moves to the battlefield).

### `engine/mod.rs`

Add `pub mod stack`.

### `types/mod.rs`

Export `StackId`, `StackObject`, `StackPayload` from `types::stack`.

---

## Server / API Changes (`serve.rs`)

### New endpoint: `POST /pass_priority`

```json
{ "player_id": 0 }
```

Calls `pass_priority(state, player_id)`. Returns the updated `GameView` on success, or an error string.

### New `EngineError` → HTTP mapping

`NotYourPriority` → 400 Bad Request with message `"not your priority"`.

### `GameView` additions

```rust
struct StackItemView {
    id: u64,                 // StackId — for future targeting (counterspells, etc.)
    kind: String,            // "spell" | "triggered_ability" | "activated_ability"
    label: String,           // card name for spells; ability text for abilities
    controller: u8,          // PlayerId
    card: Option<CardView>,  // Some(_) for spells, None for abilities
}

// added to GameView:
stack: Vec<StackItemView>,   // index 0 = bottom, last = top
consecutive_passes: u32,     // UI can show "waiting for opponent response"
```

### Existing endpoints

`POST /advance_step` remains as a direct call to `advance_step`, bypassing the priority system. This preserves the spacebar-to-skip-step UX. Priority enforcement on cast/play/activate endpoints is sufficient for correctness.

---

## Testing Strategy

Tests live in the same file as the code they test (existing convention).

### `src/engine/stack.rs`

- `pass_priority_wrong_player_returns_error`
- `pass_priority_once_shifts_priority`
- `pass_priority_twice_with_spell_resolves_top`
- `pass_priority_twice_with_empty_stack_advances_step`
- `resolve_top_spell_moves_card_to_battlefield`
- `resolve_top_spell_collects_etb_triggers_onto_stack`
- `resolve_top_triggered_ability_executes_effect`
- `resolve_top_triggered_ability_draw_card`
- `resolve_top_triggered_ability_gain_life`
- `apnap_ordering_for_simultaneous_etb_triggers` (two creatures entering simultaneously — future)

### `src/engine/casting.rs`

- `cast_creature_puts_spell_on_stack_not_battlefield`
- `cast_creature_caster_retains_priority`
- `cast_creature_resets_consecutive_passes`
- `cast_creature_not_your_priority_returns_error`

### `src/engine/triggered.rs`

- `collect_etb_triggers_returns_stack_objects_not_applying_effects`
- `collect_etb_draw_trigger_returns_draw_entry`
- `collect_etb_no_triggers_returns_empty`

### `src/engine/activated.rs`

- `non_mana_activate_puts_on_stack`
- `non_mana_activate_not_your_priority_returns_error`
- `mana_activate_still_resolves_immediately`

---

## What This Does Not Cover

- Instant-speed casting (flash, instants) — casting.rs currently enforces sorcery speed only
- Non-creature spell resolution (instants, sorceries move to graveyard on resolution)
- Counterspells and other "target stack object" effects — `StackId` is exposed in the view for this future use
- Discard-to-hand-size enforcement in cleanup (existing known gap)
- Multi-player APNAP with more than 2 players
