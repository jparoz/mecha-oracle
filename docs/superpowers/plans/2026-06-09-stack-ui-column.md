# Stack UI Column Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add an always-visible stack column between the play area and sidebar, rendering stack items as animated, staggered, overlapping cards centred vertically.

**Architecture:** All changes are in `src/serve.html` (the single-file UI). CSS custom properties drive player colour theming. Stack cards are `position: absolute` within a relative container; JS computes `transform: translate(X, Y)` for each card, and CSS transitions animate all movement. A `renderStack()` function diffs the current DOM against the incoming state on every `render()` call.

**Tech Stack:** Vanilla HTML/CSS/JS in `src/serve.html`. No build step. Visual verification requires `cargo run -- serve` (or `cargo run -- serve --deck docs/test-decks/basic.json`).

> **Note on TDD:** There is no JS test infrastructure for this UI. Each task ends with a visual verification step instead of an automated test run.

---

## File Map

| File | Change |
|------|--------|
| `src/serve.html` | Add CSS vars, stack column styles, HTML structure, `renderStack()` function, wire into `render()` |

---

### Task 1: CSS — player colour variables and stack column styles

**Files:**
- Modify: `src/serve.html` — `<style>` block (lines 6–185)

- [ ] **Step 1: Add CSS custom properties for player colours**

In `src/serve.html`, add at the very start of the `<style>` block (after the `*` reset rule on line 7, before `body`):

```css
:root {
  --p1-color: #51cf66;
  --p2-color: #ff7b7b;
}
```

- [ ] **Step 2: Add `#stack-col` container styles**

Append to the `<style>` block, after the `/* Log drawer */` section (after line 185):

```css
/* Stack column */
#stack-col {
  width: 90px;
  background: #0f1419;
  border-left: 1px solid #30363d;
  border-right: 1px solid #30363d;
  display: flex;
  flex-direction: column;
  flex-shrink: 0;
}
.stack-edge-label {
  font-size: 9px;
  color: #2a2a2a;
  text-transform: uppercase;
  letter-spacing: 1px;
  text-align: center;
  padding: 5px 0;
  flex-shrink: 0;
}
#stack-items {
  flex: 1;
  position: relative;
  overflow: hidden;
}
#stack-empty {
  position: absolute;
  inset: 0;
  display: flex;
  align-items: center;
  justify-content: center;
  font-size: 10px;
  color: #1e1e1e;
  text-transform: uppercase;
  letter-spacing: 2px;
  pointer-events: none;
  writing-mode: vertical-rl;
}
```

- [ ] **Step 3: Add `.stack-card` styles**

Still appending to `<style>`:

```css
/* Stack cards */
.stack-card {
  position: absolute;
  left: 50%;
  top: 50%;
  width: 56px;
  height: 78px;
  border-radius: 4px;
  border: 1px solid #444;
  background: #161b22;
  display: flex;
  flex-direction: column;
  padding: 4px;
  transition: transform 0.35s cubic-bezier(0.4, 0, 0.2, 1),
              opacity 0.25s ease;
  pointer-events: none;
  user-select: none;
}
.stack-card.p1 { border-color: var(--p1-color); background: #0d1a10; }
.stack-card.p2 { border-color: var(--p2-color); background: #1a0d0d; }
.stack-card-name {
  font-size: 7.5px;
  font-weight: bold;
  color: #ddd;
  line-height: 1.2;
  flex: 1;
}
.stack-kind {
  font-size: 7px;
  color: #444;
  text-transform: uppercase;
  letter-spacing: 0.5px;
  margin-top: auto;
}
```

- [ ] **Step 4: Commit**

```bash
git add src/serve.html
git commit -m "feat: add CSS variables and stack column styles"
```

---

### Task 2: HTML — insert `#stack-col` structure

**Files:**
- Modify: `src/serve.html` — `<body>` / `#root` div (around line 228)

- [ ] **Step 1: Insert the stack column HTML between `#board` and `#sidebar`**

In `src/serve.html`, locate the closing `</div>` of `#board` (the line just before `<div id="sidebar">`). Insert after it:

```html
  <div id="stack-col">
    <div class="stack-edge-label">top</div>
    <div id="stack-items">
      <div id="stack-empty">Stack</div>
    </div>
    <div class="stack-edge-label">bottom</div>
  </div>
```

The result in `#root` should read:
```
#board  →  #stack-col  →  #sidebar  →  #log-drawer
```

- [ ] **Step 2: Start the server and visually verify the empty column**

```bash
cargo run -- serve
```

Open `http://localhost:3000` (or whichever port the server binds). You should see a narrow dark column between the board and the sidebar. It should contain faint "TOP" / "BOTTOM" labels and a dim vertical "STACK" watermark in the centre. The rest of the UI should be unchanged.

- [ ] **Step 3: Commit**

```bash
git add src/serve.html
git commit -m "feat: add stack column HTML structure"
```

---

### Task 3: JS — `renderStack()` function and wiring

**Files:**
- Modify: `src/serve.html` — `<script>` block

- [ ] **Step 1: Add the `renderStack()` function**

In `src/serve.html`, add the following function in the `<script>` block, just before the `// ── Boot ──` section (before `document.addEventListener('keydown', ...)`):

```javascript
// ── Stack column ──────────────────────────────────────────────────────────────

const STACK_ITEM_STEP = 60; // px between consecutive card centres (78px tall - 18px overlap)
const STACK_STAGGER_X = 8;  // px left/right alternating offset

function renderStack(stack) {
  const container = document.getElementById('stack-items');
  const emptyEl   = document.getElementById('stack-empty');

  emptyEl.style.display = stack.length === 0 ? 'flex' : 'none';

  const n = stack.length;

  // Index existing DOM cards by stack id
  const existing = {};
  container.querySelectorAll('.stack-card').forEach(el => {
    existing[el.dataset.stackId] = el;
  });

  // Remove cards no longer in the stack (leaving animation)
  const incomingIds = new Set(stack.map(item => String(item.id)));
  for (const [id, el] of Object.entries(existing)) {
    if (!incomingIds.has(id)) {
      const savedX = el._stackX || 0;
      const savedY = el._stackY || 0;
      el.style.opacity = '0';
      el.style.transform = `translate(calc(-50% + ${savedX}px), calc(-50% + ${savedY - 12}px))`;
      setTimeout(() => el.remove(), 220);
    }
  }

  // Add or reposition cards
  stack.forEach((item, i) => {
    const staggerX = (i % 2 === 0) ? -STACK_STAGGER_X : STACK_STAGGER_X;
    // (n-1)/2 - i: index 0 (bottom) gets largest Y (bottom of screen);
    // index n-1 (top/next to resolve) gets smallest Y (top of screen).
    const offsetY  = ((n - 1) / 2 - i) * STACK_ITEM_STEP;
    const zIndex   = i + 1;
    const idStr    = String(item.id);
    let el = existing[idStr];

    if (!el) {
      // New card — create at entering position, then animate to final
      const kindLabel = item.kind === 'spell'              ? 'SPELL'
                      : item.kind === 'activated_ability' ? 'ACT'
                      : 'TRIG'; // triggered_ability
      el = document.createElement('div');
      el.className       = 'stack-card ' + (item.controller === 0 ? 'p1' : 'p2');
      el.dataset.stackId = idStr;
      el.innerHTML =
        `<span class="stack-card-name">${esc(item.label)}</span>` +
        `<span class="stack-kind">${kindLabel}</span>`;
      el.style.opacity   = '0';
      el.style.zIndex    = zIndex;
      // Start 12px below final position
      el.style.transform = `translate(calc(-50% + ${staggerX}px), calc(-50% + ${offsetY + 12}px))`;
      el._stackX = staggerX;
      el._stackY = offsetY;
      container.appendChild(el);

      requestAnimationFrame(() => {
        el.style.opacity   = '1';
        el.style.transform = `translate(calc(-50% + ${staggerX}px), calc(-50% + ${offsetY}px))`;
      });
    } else {
      // Existing card — slide to new position
      el._stackX = staggerX;
      el._stackY = offsetY;
      el.style.zIndex    = zIndex;
      el.style.transform = `translate(calc(-50% + ${staggerX}px), calc(-50% + ${offsetY}px))`;
    }
  });
}
```

- [ ] **Step 2: Wire `renderStack` into `render()`**

In `src/serve.html`, find the `render()` function. At the end of it, just before the closing `}`, add:

```javascript
  renderStack(s.stack);
```

The tail of `render()` should now look like:

```javascript
  renderTurnTracker(s);
  renderPanes(s);
  renderStack(s.stack);
}
```

- [ ] **Step 3: Start the server and visually verify with an empty stack**

```bash
cargo run -- serve
```

Open `http://localhost:3000`. The UI should look identical to before — the "STACK" watermark is visible, and no cards appear in the column.

- [ ] **Step 4: Verify stack items appear and animate**

To get cards onto the stack you'll need to cast a spell during a main phase. If you have a test deck with creatures, cast one. Alternatively you can temporarily verify the function by opening the browser console and calling:

```javascript
renderStack([
  { id: 1, kind: 'spell', label: 'Lightning Bolt', controller: 0 },
  { id: 2, kind: 'triggered', label: 'Llanowar Elves ETB', controller: 1 },
  { id: 3, kind: 'activated', label: 'Llanowar Elves: {T}', controller: 0 },
]);
```

Expected: three stacked cards appear centred in the column. The top card (index 2, id 3) is fully visible, the lower ones are partially covered. Cards alternate slightly left and right. The "STACK" watermark is gone.

Call `renderStack([])` to verify the leaving animation and watermark return.

- [ ] **Step 5: Commit**

```bash
git add src/serve.html
git commit -m "feat: render stack items with animation in stack column"
```
