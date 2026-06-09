# Stack UI Column — Design Spec
_Date: 2026-06-09_

## Overview

Add a permanent stack visualisation column between the play area and the sidebar. Items on the stack are displayed as small cards, vertically centred as a group, fanned with overlap (top item fully visible), and horizontally staggered to make item count obvious at a glance. CSS transitions animate all position changes.

---

## Layout

`#root` flex row becomes: `#board | #stack-col | #sidebar | #log-drawer`

**`#stack-col`** — 90 px wide, full height, always visible.

Internal structure:
```
#stack-col
  .stack-top-label     ("TOP"  — fixed at top of column)
  #stack-items         (position: relative; fills remaining height)
  .stack-bot-label     ("BOTTOM" — fixed at bottom of column)
```

When the stack is empty, `#stack-items` shows a centred dim watermark label "Stack".

---

## Card Appearance

Each item renders as `div.stack-card` (56 × 78 px), matching the existing `.card` dimensions.

Contents:
- Card name / ability label (`.stack-card-name`, same style as `.card-name`)
- Kind badge (`.stack-kind`): `SPELL` | `ACT` | `TRIG`
- Controller border tint: derived from CSS variables (see Theming below)

---

## Theming (CSS variables)

```css
:root {
  --p1-color: #51cf66;   /* p1 accent — used for border tint and future backgrounds */
  --p2-color: #ff7b7b;   /* p2 accent */
}
```

Stack card borders use these variables directly so both the tint and any future background colour can be updated in one place.

---

## Positioning (absolute + transform)

All `.stack-card` elements are `position: absolute; left: 50%; top: 50%`.

Transform applied to each card at index `i` (0 = bottom, n−1 = top):

```
offsetY  = (i − (n−1)/2) × ITEM_STEP      // centres the group; top card has smallest Y
staggerX = (i % 2 === 0) ? −8px : +8px    // alternates left/right starting from bottom
transform: translate(calc(-50% + staggerX), offsetY)
```

Constants:
- `ITEM_STEP = 60px` (cards are 78 px tall; 18 px overlap means each step is 60 px)

Z-index: higher index = higher z-index, so the top item renders on top.

---

## Animations

### Item added
1. Card is inserted into DOM with `opacity: 0` and its transform offset by `+12 px` in Y (the "from" state).
2. On the next animation frame (`requestAnimationFrame`), the transform is set to its final value and opacity to 1 — the CSS transition plays the animation.
3. All other cards simultaneously animate to their new offsets via the shared `transition`.

### Item removed
1. Class `stack-card-leaving` is added to the departing card:
   - `opacity: 1 → 0` (0.2 s)
   - `transform` shifts up by 12 px (0.2 s)
2. After 200 ms (`setTimeout`), the element is removed from the DOM.
3. Remaining cards slide to their new centred positions simultaneously.

### Base transition
```css
.stack-card {
  transition: transform 0.35s cubic-bezier(0.4, 0, 0.2, 1),
              opacity   0.25s ease;
}
```

---

## Integration with `render()`

`renderStack(s.stack)` is called from `render()` on every state update.

The function diffs the current DOM cards against the incoming stack array:
- Cards present in DOM but absent from new state → trigger leaving animation, then remove.
- Cards absent from DOM but present in new state → insert and trigger entering animation.
- Cards present in both → update transform in place (position change animates automatically).

Stack items are keyed by `item.id` (stable across re-renders).

---

## Data shape (from server)

```json
{
  "id": 1,
  "kind": "spell" | "triggered" | "activated",
  "label": "Lightning Bolt",
  "controller": 0,
  "card": { ... } | null
}
```

Index 0 = bottom of stack; last index = top (next to resolve).

---

## Out of scope

- Click/interaction on stack items
- Refactoring existing player-colour classes to use CSS variables (only new stack code uses variables)
