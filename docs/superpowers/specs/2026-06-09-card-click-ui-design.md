# Card-Click UI Revamp — Design Spec
**Date:** 2026-06-09

## Goal

Replace the sidebar action-button model with a card-click model. Any visible card that has an associated action can be activated by clicking it directly. The sidebar is removed entirely. Server-side validation is the single source of truth; the UI sends all clicks to the server and surfaces rejections via toast and log.

---

## Layout

- **Sidebar removed.** The `#board` div expands to fill the full width.
- **Stack column** stays on the right (unchanged).
- **Log drawer** stays (unchanged, toggled via the Log button in the action bar).
- **Action bar** (the current turn-tracker bar) stays in its position dividing the two player sections. It gains a Pass Priority button and a Log button, and switches to a combat mode during DeclareAttackers/DeclareBlockers.

---

## Action Bar — Two Modes

The bar occupies the same fixed height in both modes.

### Normal mode (all steps except DeclareAttackers/DeclareBlockers)

```
[T2] [Untap · … · Main 1 · Combat · …]    [P1 priority · Space to pass]  [Pass Priority →]  [Log]
```

- Background: `#161b22` (current dark navy)
- Left: turn number + step chips (current rendering, unchanged)
- Right: priority hint text + **Pass Priority →** button + **Log** button
- Space bar passes priority (existing shortcut, preserved)

### Combat mode (DeclareAttackers or DeclareBlockers)

```
[Declare Attackers · N selected · click to toggle · Enter to confirm]    [Confirm Attackers ✓]  [Log]
```

- Background: slightly warm dark (`#161900`, yellow-tinted) to visually distinguish
- Left: combat context text ("Declare Attackers" or "Declare Blockers"), selected count, interaction hints
- Right: **Confirm Attackers ✓** / **Confirm Blockers ✓** button (same size/position as Pass Priority) + **Log** button
- Enter key confirms (Space does not — avoids accidental confusion with pass)
- "Space to pass" and "Enter to confirm" hints are aligned consistently in their respective modes

The two modes are the same HTML element, not two stacked elements — the inner content is swapped.

---

## Card Visual States

Three states, applied per card based on server flags:

| State | Border | Glow | Text |
|---|---|---|---|
| **Non-actionable** | `#2a2a2a` (faint) | none | Dimmed (`#555`) |
| **Actionable** | `#506050` (subtle green-grey) | `0 0 6px rgba(200,220,200,0.18)` (white-grey) | Normal (`#ddd`) |
| **Selected** (toggled attacker/blocker) | `#ffdd57` (gold, 2px) | `0 0 10px rgba(255,220,80,0.7)` | White |

After attackers are confirmed: attacking creatures use `#ff6b6b` (red) border/glow.

Bright colours (gold, red) are reserved for selection and combat states only. The actionable glow is intentionally subtle.

**Determining actionable state** (uses existing server flags and view fields):
- Hand card (land): `can_cast` is always false for lands (no mana cost). Use client-side check: `lands_played_this_turn == 0 && step ∈ {PreCombatMain, PostCombatMain} && active_player == this_player`. These fields already exist in the game view; no server changes needed.
- Hand card (spell): `can_cast == true`
- Battlefield land (untapped): has any `activated_abilities` entry with `can_activate == true`
- Battlefield creature: `can_attack == true` during DeclareAttackers; `can_block == true` during DeclareBlockers; or has any `activated_abilities` entry with `can_activate == true`

Non-actionable cards are still clickable — the click is sent to the server which will reject it with an error. No UI-side gating.

---

## Card Click Actions

### Hand cards

| Card type | Click action |
|---|---|
| Land | `play_land` |
| Spell (any) | `cast_spell` |

Single action — fires immediately, no popup.

### Battlefield lands

| Scenario | Click action |
|---|---|
| Single mana ability | `tap_land` directly |
| Multiple mana abilities (dual land, etc.) | Popup menu listing each ability |

### Battlefield creatures

| Step | Click action |
|---|---|
| DeclareAttackers (AP, `can_attack`) | Toggle attacker selection (local state) |
| DeclareBlockers (NAP, `can_block`) | Popup: "Block [attacker name]" per attacker; click to assign/reassign |
| Any step with activated abilities | If one ability: `activate_ability` directly. If multiple: popup menu |
| DeclareAttackers + has activated abilities | Popup: "Declare as attacker" and each activated ability as options |

### Other permanents

Popup menu of available activated abilities, or direct fire if only one.

---

## Popup Disambiguation Menu

Appears on click of a card with multiple valid actions. Floats near the card.

- Small card: `background: #1e2430`, `border: 1px solid #4a6a9a`, `border-radius: 6px`
- Header label: "Actions" or context-appropriate label (e.g., "Tap for mana", "Assign blocker")
- One button per action, styled as current `.action-btn`
- Dismiss: click outside the popup, or press Escape
- Sends the chosen action to server on click

---

## Combat Flow Detail

### DeclareAttackers

1. AP's eligible creatures (`can_attack == true`) are shown as actionable.
2. Clicking an eligible creature toggles it in local `attackersSelected` state (gold border when selected).
3. Action bar switches to combat mode showing selection count.
4. Enter or "Confirm Attackers" button sends `declare_attackers` with the current selection.
5. Pass/Space is disabled until confirmed.

### DeclareBlockers

1. NAP's eligible creatures (`can_block == true`) are shown as actionable.
2. Clicking an eligible creature opens a popup listing each attacker by name ("Block [name]").
3. Selecting an attacker records the assignment in local `blockersAssignment` state (gold border on blocker).
4. Clicking a blocker that already has an assignment: same popup appears with the currently assigned attacker's button visually highlighted. Clicking the same attacker deselects (removes the assignment); clicking a different attacker reassigns.
5. Enter or "Confirm Blockers" button sends `declare_blockers`.

---

## Error Toast

On any server rejection (`ok: false` in action response):

- Toast appears bottom-center of the viewport
- Style: `background: #2a1a1a`, `border: 1px solid #aa3333`, `color: #ee6666`, `border-radius: 6px`
- Message: `✕ [error string from server]`
- Auto-dismisses after ~3 seconds
- A log entry is also appended (existing behaviour preserved)

---

## Reset Mana

The mana pool display in the player header becomes interactive when a checkpoint exists (`can_reset_mana == true`):

- Gains a subtle border and a `↩` icon suffix
- `cursor: pointer`, `title` attribute explains the action
- Click sends `reset_mana`
- When no checkpoint exists, the display is inert (no border, no cursor change)

---

## Items Removed

- The entire `#sidebar` element and its CSS
- All `.pane-*`, `.action-group*`, `.action-btn` styles that were sidebar-only
- The `renderPanes()` function (card click handlers replace its logic)
- The `group()` and `btn()` helper functions (sidebar-specific)

The `btn-pass` and `btn-confirm` in the action bar get their own minimal styles.

---

## Items Not Changed

- Stack column rendering (`renderStack`, `#stack-col`)
- Log drawer (`#log-drawer`, `appendLog`, `toggleLog`)
- GY pile display and modal
- Tooltip rendering and positioning
- Attacker/blocker local state variables (`attackersSelected`, `blockersAssignment`)
- Server API — no Rust changes required
- Spacebar shortcut (passes priority, same as before)
