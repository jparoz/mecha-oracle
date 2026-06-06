# Priority Model, Combat UX, and Step Guard Removal

**Date:** 2026-06-05

**Terminology:** AP = active player (the player whose turn it is); NAP = non-active player (their opponent).

Three related issues are addressed together:

1. `DeclareAttackers` behaved inconsistently depending on whether attackers were selected.
2. The "Resolve Damage" button could be clicked multiple times and didn't auto-advance (violates CR 510.2).
3. Priority (CR 117) was untracked — a single "Pass priority" click always advanced the step, regardless of whose turn it was to act.

---

## 1. Priority Model (CR 117)

### Backend

**`GameState.priority_player`** (already exists) is now actively maintained:

- `advance_step` (in `engine/turn.rs`) sets `state.priority_player = state.active_player` at the very start, before any step transition. This ensures every new step — including extra steps popped from `extra_steps` — begins with priority on the active player.

- `dispatch_action` for `AdvanceStep` becomes a two-phase pass:
  - If `priority_player == active_player` (AP passing first): set `priority_player = nap`, return the updated state. Step does **not** advance.
  - If `priority_player == nap` (NAP passing, completing the round): call `advance_with_auto_steps`. Step advances and `priority_player` resets (via `advance_step`).

- `declare_attackers` and `declare_blockers` are **turn-based actions**, not priority actions. They do not touch `priority_player`. After either confirmation, priority sits with AP (as set at the start of the step), so both players must still pass before the step ends. This makes "Confirm with 0 attackers" and "Confirm with N attackers" behave identically.

**`CombatState`** gains two boolean fields: `attackers_declared` (set `true` in `declare_attackers`) and `blockers_declared` (set `true` in `declare_blockers`). Both reset to `false` in `CombatState::empty()`. These are exposed in `GameView` so the UI can distinguish "haven't declared yet" from "declared with 0".

**`GameView`** gains `priority_player: PlayerId` and `attackers_declared: bool` / `blockers_declared: bool`.

### `PlayerId` and `ObjectId` serialization

`PlayerId` and `ObjectId` both get `#[derive(Serialize)]` and `#[serde(transparent)]` so they serialize as their inner `u8`/`u64` values directly. For `PlayerId` this changes the convention from 1/2 to 0/1; the JS layer is updated throughout. `CardView.id` becomes `ObjectId` (eliminating the manual `.0` extraction). `GameView.active_player`, `winner`, and `priority_player` all become `PlayerId` / `Option<PlayerId>`.

---

## 2. Automatic Combat Damage (CR 510.2 / 510.3)

### Backend

`deal_combat_damage` (in `engine/combat.rs`) loses its `Result` return — it becomes `fn(GameState) -> GameState`. The step guard is removed (see section 3). It can panic on bugs (impossible inputs) but has no fallible cases once called from the correct context.

`apply_step_start` gains a `Step::CombatDamage` arm that calls `deal_combat_damage` directly. Damage resolves automatically when entering the step.

**`advance_with_auto_steps` is unchanged** — its continue condition remains `Untap | Cleanup` only. The loop therefore **breaks** after entering `CombatDamage` (damage has resolved, but the loop does not continue past it). Per CR 510.3, players receive a priority window after each round of combat damage before the step ends.

**No-first-strike trace:**

1. Both pass on `DeclareBlockers` → `advance_with_auto_steps` called
2. `advance_step` → `CombatDamage`, `priority_player = AP`
3. `apply_step_start` → `deal_combat_damage` (single round, no extra steps queued)
4. Loop condition: `CombatDamage` → **break**
5. Both players get priority at `CombatDamage`; both pass → `advance_with_auto_steps` called
6. `advance_step` → `EndOfCombat`, `priority_player = AP`; loop breaks

**First-strike trace:**

1. Both pass on `DeclareBlockers` → `advance_with_auto_steps` called
2. `advance_step` → `CombatDamage`, `priority_player = AP`
3. `apply_step_start` → `deal_combat_damage` (first-strike round) → queues extra `CombatDamage`
4. Loop breaks → both players get priority at `CombatDamage` (after first-strike damage)
5. Both pass → `advance_with_auto_steps` called
6. `advance_step` pops extra `CombatDamage` → step stays `CombatDamage`, `priority_player = AP`
7. `apply_step_start` → `deal_combat_damage` (second round) → no extra steps
8. Loop breaks → both players get priority at `CombatDamage` (after second-round damage)
9. Both pass → `advance_with_auto_steps`; `advance_step` → `EndOfCombat`; loop breaks

### Frontend

The "Resolve Damage" button is removed from `renderActions`. The `DealCombatDamage` action is removed entirely: enum variant, `dispatch_action` arm, no dead code kept.

---

## 3. Step Guard Removal

Step guards — checks of the form "return error if step ≠ X" — are removed from all engine functions. The call site (the `dispatch_action` match or `apply_step_start` match arm) is the verification; the guard in the function body adds no new information.

**Removed guards:**

| File | Function | Guard removed |
|------|----------|---------------|
| `engine/combat.rs` | `declare_attackers` | `step != DeclareAttackers` |
| `engine/combat.rs` | `declare_blockers` | `step != DeclareBlockers` |
| `engine/combat.rs` | `deal_combat_damage` | `step != CombatDamage` (+ `Result` return) |
| `engine/casting.rs` | `play_land` | `step ∉ {PreCombatMain, PostCombatMain}` |
| `engine/casting.rs` | `cast_creature` | `step ∉ {PreCombatMain, PostCombatMain}` |

All tests that specifically test these removed guards are deleted. All other tests that call these functions are updated as needed (e.g. `deal_combat_damage(gs).unwrap()` → `deal_combat_damage(gs)`).

---

## 4. Two-Section Sidebar (Frontend)

The 240px right sidebar is split into two equal vertically-stacked sections:

```
┌─────────────────────────────────────────────────────┐
│                              │  ┌─────────────────┐ │
│   Board                      │  │  P2 section     │ │
│                              │  │  (top half)     │ │
│                              │  │─────────────────│ │
│                              │  │  P1 section     │ │
│                              │  │  (bottom half)  │ │
│                              │  └─────────────────┘ │
└─────────────────────────────────────────────────────┘
```

**Each section contains:**
- Header: player name + `[HAS PRIORITY]` badge (bright yellow) or `[waiting]` (dim)
- Actions area (`flex: 1; overflow-y: auto`): mana tapping, land playing, casting, attacker/blocker declaration — shown only when that player has priority in the appropriate step
- "Pass priority →" button at the bottom of the section (always rendered, disabled/dimmed when not that player's priority)

**Combat declaration UI:**
Per CR 508–509, the declaring-player's turn-based action happens **before** priority opens. Pass Priority buttons for both players are disabled (rendered but inert) until the required declaration is complete:

- `DeclareAttackers`, before confirmation (`!s.attackers_declared`): AP section shows attacker selection + "Confirm Attackers"; neither Pass Priority button is active
- `DeclareAttackers`, after confirmation (`s.attackers_declared`): AP section shows "Attackers declared" summary + active Pass Priority; the priority badge is on AP
- `DeclareBlockers`, before confirmation (`!s.blockers_declared`): NAP section shows blocker assignment + "Confirm Blockers"; neither Pass Priority button is active; priority badge on AP (per `advance_step` reset) but grayed out
- `DeclareBlockers`, after confirmation (`s.blockers_declared`): AP section shows active Pass Priority (AP gets priority first per CR 509.4); NAP section shows disabled Pass Priority until it's their turn

**Log drawer:**
- A "Log" toggle button lives in a thin strip between the sidebar and the right edge of the window
- Clicking it opens/closes a 220px panel to the right of the sidebar (a flex sibling of `#sidebar` and `#board`), sliding the board slightly
- The drawer is `display: none` by default; toggled to `display: flex; flex-direction: column` with `overflow-y: auto`

**Spacebar hotkey:**
- `keydown` listener: if `e.code === 'Space'` and focus is not on an input/button, call `sendAction({type: 'advance_step'})`
- The server already knows who holds priority; no client-side player tracking needed

---

## Files Changed

**Rust:**
- `src/types/ids.rs` — add `Serialize + serde(transparent)` to both `PlayerId` and `ObjectId`
- `src/types/game_state.rs` — `CombatState`: add `attackers_declared`, `blockers_declared`
- `src/engine/combat.rs` — remove step guards; `deal_combat_damage` → infallible; set declared flags; remove obsolete tests
- `src/engine/casting.rs` — remove step guards; remove obsolete step-guard tests
- `src/engine/turn.rs` — `advance_step` resets `priority_player`; `apply_step_start` handles `CombatDamage` (damage auto-resolves on entry; loop still breaks at `CombatDamage` for priority window per CR 510.3)
- `src/serve.rs` — `GameView`: `PlayerId` fields, `priority_player`, `attackers_declared`, `blockers_declared`; remove `DealCombatDamage`; two-phase `AdvanceStep`; update `build_game_view`; update tests for 0/1 player IDs

**Frontend:**
- `src/serve.html` — two-section sidebar; log drawer; spacebar hotkey; 0/1 player IDs throughout; remove Resolve Damage button; `attackers_declared`/`blockers_declared` state handling
