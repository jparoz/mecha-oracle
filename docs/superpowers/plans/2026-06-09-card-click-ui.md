# Card-Click UI Revamp Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Replace the sidebar action-button model with a card-click model where clicking any visible card fires its action directly, the sidebar is removed, and server validation is the sole authority.

**Architecture:** All changes are in `src/serve.html` (single file — HTML, CSS, JS). No Rust changes. The action bar (current turn-tracker bar) is refactored into a two-mode bar: normal (step chips + Pass Priority button) and combat (confirm strip). Card clicks dispatch to server; rejections surface as toasts.

**Tech Stack:** Vanilla JS, inline CSS, axum HTML endpoint. Run server with `cargo run -- --deck tests/fixtures/decks/blue_test.json` (or similar) and test in a browser at `http://localhost:3000`.

---

## File Map

| File | Changes |
|---|---|
| `src/serve.html` | All changes. Remove sidebar CSS + HTML + JS. Refactor action bar. Add card click handlers, popup, toast, card visual states. |

No other files are created or modified.

---

### Task 1: Remove sidebar — CSS, HTML, and dead JS

**Files:**
- Modify: `src/serve.html`

Remove the sidebar pane entirely, its CSS, and the JS functions that rendered it. This leaves the board without action buttons; that's expected — the following tasks restore all functionality via card clicks and the action bar.

- [ ] **Step 1: Delete sidebar CSS**

In the `<style>` block, find and delete the following rule groups (search for each selector):

```
/* Sidebar */
.player-pane { … }
.player-pane.p2 { … }
.pane-header { … }
.pane-name { … }
.pane-name.p2 { … }
.pane-name.p1 { … }
.priority-badge { … }
.priority-badge.active { … }
.priority-badge.waiting { … }
.pane-actions { … }
.pane-pass { … }
.action-group { … }
.action-group-label { … }
.action-btn { … }
.action-btn:hover { … }
.action-btn.pass { … }
.action-btn.pass:hover { … }
.action-btn:disabled { … }
.action-btn.selected { … }
.action-btn .cost { … }
```

Also delete the `#sidebar` rule itself:
```css
#sidebar { width: 240px; background: #161b22; border-left: 1px solid #30363d; display: flex; flex-direction: column; flex-shrink: 0; }
```

- [ ] **Step 2: Delete sidebar HTML**

Find and delete the entire `<div id="sidebar">` element (currently contains `#pane-p2` and `#pane-p1`):

```html
  <div id="sidebar">
    <div class="player-pane p2" id="pane-p2">
      ...
    </div>
    <div class="player-pane p1" id="pane-p1">
      ...
    </div>
  </div>
```

- [ ] **Step 3: Delete sidebar JS functions**

Delete the following functions from the `<script>` block:

- `renderPanes(s)` — the entire function (lines ~607–741)
- `group(label, btns)` — two-line helper
- `btn(label, onclick, cost)` — three-line helper

Also remove the `renderPanes(s)` call inside `render(s)`.

- [ ] **Step 4: Verify the server still starts and the board renders**

```bash
cargo run -- --deck tests/fixtures/decks/blue_test.json 2>&1 | head -5
```

Open `http://localhost:3000`. The board should display both player sections, hands, the stack column, and the log drawer. The sidebar is gone; the board fills the full width. No JS errors in the browser console.

- [ ] **Step 5: Commit**

```bash
git add src/serve.html
git commit -m "feat: remove sidebar — board expands full width"
```

---

### Task 2: Refactor action bar — normal mode with Pass Priority and Log buttons

**Files:**
- Modify: `src/serve.html`

Replace `renderTurnTracker(s)` with `renderActionBar(s)` that renders the same step chips on the left but adds a priority hint, a **Pass Priority →** button, and a **Log** button on the right. Also add CSS for the two action bar button styles. The spacebar shortcut is unchanged.

- [ ] **Step 1: Add action bar CSS to the `<style>` block**

Replace the existing `#turn-tracker` rule:
```css
#turn-tracker { background: #161b22; border-top: 1px solid #30363d; border-bottom: 1px solid #30363d; padding: 5px 12px; display: flex; align-items: center; gap: 4px; font-size: 11px; flex-shrink: 0; flex-wrap: wrap; }
```

With:
```css
#action-bar {
  border-top: 1px solid #30363d; border-bottom: 1px solid #30363d;
  padding: 0 12px; display: flex; align-items: center; gap: 4px;
  font-size: 11px; flex-shrink: 0; height: 32px;
}
#action-bar.normal { background: #161b22; }
#action-bar.combat { background: #161900; border-color: #404000; }
.bar-right { margin-left: auto; display: flex; align-items: center; gap: 6px; }
.bar-hint { color: #444; font-size: 10px; }
.bar-btn {
  padding: 3px 12px; border-radius: 4px; font-size: 11px; cursor: pointer;
  height: 22px; display: flex; align-items: center; border: 1px solid;
}
.bar-btn-pass { background: #1a2a1a; border-color: #4a8a4a; color: #7aaa7a; }
.bar-btn-pass:hover { background: #243524; }
.bar-btn-confirm { background: #262600; border-color: #6a6a00; color: #cccc44; }
.bar-btn-confirm:hover { background: #333300; }
.bar-btn-log { background: #1c2a3a; border: 1px solid #2a4a6a; border-radius: 3px; padding: 1px 8px; color: #7ab8e8; font-size: 10px; cursor: pointer; height: 22px; display: flex; align-items: center; }
.bar-btn-log:hover { background: #243650; }
```

- [ ] **Step 2: Update the HTML element id from `turn-tracker` to `action-bar`**

In the HTML body, change:
```html
<div id="turn-tracker"></div>
```
To:
```html
<div id="action-bar"></div>
```

- [ ] **Step 3: Replace `renderTurnTracker` with `renderActionBar` in the script**

Delete the `renderTurnTracker(s)` function and replace with:

```javascript
function renderActionBar(s) {
  const bar = document.getElementById('action-bar');
  const isCombat = s.step === 'DeclareAttackers' || s.step === 'DeclareBlockers';

  if (isCombat) {
    renderActionBarCombat(s, bar);
  } else {
    renderActionBarNormal(s, bar);
  }
}

function renderActionBarNormal(s, bar) {
  bar.className = 'normal';
  const cur = STEP_ORDER.indexOf(s.step);
  const chips = STEP_ORDER.map((step, i) => {
    const cls = i < cur ? 'done' : i === cur ? 'active' : 'upcoming';
    return `<span class="step-chip ${cls}">${STEP_LABELS[step]}</span>`;
  }).join('<span class="step-sep">·</span>');

  const ap = s.active_player === 0 ? 'P1' : 'P2';
  const pp = s.priority_player === 0 ? 'P1' : 'P2';
  const passBlocked =
    (s.step === 'DeclareAttackers' && !s.attackers_declared) ||
    (s.step === 'DeclareBlockers'  && !s.blockers_declared);

  bar.innerHTML =
    `<span style="color:#555;margin-right:4px">Turn ${s.turn}</span>${chips}` +
    `<div class="bar-right">` +
      `<span class="bar-hint">${pp} priority · Space to pass</span>` +
      `<button class="bar-btn bar-btn-pass"${passBlocked ? ' disabled style="opacity:0.35;cursor:not-allowed"' : ''}` +
        ` onclick="sendAction({type:'advance_step'})">Pass Priority →</button>` +
      `<button class="bar-btn-log" onclick="toggleLog()">Log</button>` +
    `</div>`;
}

// renderActionBarCombat is added in Task 9.
// For now, fall back to normal rendering during combat steps.
function renderActionBarCombat(s, bar) {
  renderActionBarNormal(s, bar);
}
```

- [ ] **Step 4: Update the call site in `render(s)`**

Find the line `renderTurnTracker(s);` and change it to `renderActionBar(s);`.

- [ ] **Step 5: Verify in browser**

Reload `http://localhost:3000`. The action bar should show step chips on the left and a "Pass Priority →" button + "Log" button on the right. Pressing Space or clicking "Pass Priority →" should advance the step. The Log button should toggle the log drawer.

- [ ] **Step 6: Commit**

```bash
git add src/serve.html
git commit -m "feat: refactor action bar — pass priority and log buttons"
```

---

### Task 3: Add toast notification system

**Files:**
- Modify: `src/serve.html`

Add a toast element that appears at the bottom-centre of the screen when the server rejects an action. Auto-dismisses after 3 seconds.

- [ ] **Step 1: Add toast CSS to the `<style>` block**

```css
#toast {
  display: none; position: fixed; bottom: 24px; left: 50%; transform: translateX(-50%);
  background: #2a1a1a; border: 1px solid #aa3333; border-radius: 6px;
  padding: 6px 16px; font-size: 11px; color: #ee6666;
  white-space: nowrap; box-shadow: 0 2px 12px rgba(0,0,0,0.5);
  z-index: 600; pointer-events: none;
  opacity: 1; transition: opacity 0.4s ease;
}
#toast.hiding { opacity: 0; }
```

- [ ] **Step 2: Add toast HTML inside `<body>`, after the server-overlay div**

```html
<div id="toast"></div>
```

- [ ] **Step 3: Add toast JS functions to the script**

```javascript
let toastTimer = null;

function showToast(msg) {
  const el = document.getElementById('toast');
  el.textContent = '✕ ' + msg;
  el.style.display = 'block';
  el.classList.remove('hiding');
  if (toastTimer) clearTimeout(toastTimer);
  toastTimer = setTimeout(() => {
    el.classList.add('hiding');
    setTimeout(() => { el.style.display = 'none'; }, 400);
  }, 3000);
}
```

- [ ] **Step 4: Wire toast into `sendAction`**

In `sendAction`, find the error branch:
```javascript
  } else {
    appendLog('Engine: ' + data.error, 'log-error');
  }
```

Change to:
```javascript
  } else {
    appendLog('Engine: ' + data.error, 'log-error');
    showToast(data.error);
  }
```

- [ ] **Step 5: Verify in browser**

Open `http://localhost:3000`. Open the browser console and call `showToast('test error message')`. A red toast should appear bottom-centre and auto-dismiss after 3 seconds.

- [ ] **Step 6: Commit**

```bash
git add src/serve.html
git commit -m "feat: add error toast for server rejections"
```

---

### Task 4: Add popup disambiguation menu

**Files:**
- Modify: `src/serve.html`

Add a floating popup element that appears near a clicked card when multiple actions are available. Dismiss on click-outside or Escape.

- [ ] **Step 1: Add popup CSS to the `<style>` block**

```css
#popup {
  display: none; position: fixed; z-index: 400;
  background: #1e2430; border: 1px solid #4a6a9a; border-radius: 6px;
  padding: 8px; min-width: 150px; max-width: 220px;
  box-shadow: 0 4px 16px rgba(0,0,0,0.6);
}
.popup-header { font-size: 10px; color: #666; text-transform: uppercase; letter-spacing: 0.5px; margin-bottom: 6px; }
.popup-item {
  display: block; width: 100%; background: #1c2a3a; border: 1px solid #2a4a6a;
  border-radius: 4px; padding: 6px 10px; color: #7ab8e8; font-size: 12px;
  cursor: pointer; text-align: left; margin-bottom: 3px;
}
.popup-item:last-child { margin-bottom: 0; }
.popup-item:hover { background: #243650; border-color: #4a7aaa; }
.popup-item.active { border-color: #ffdd57; color: #ffdd57; background: #2a2500; }
```

- [ ] **Step 2: Add popup HTML inside `<body>`, after the toast div**

```html
<div id="popup"></div>
```

- [ ] **Step 3: Add popup JS functions to the script**

```javascript
let popupDismissHandler = null;

// items: [{ label, onClick, active }]   active = true highlights the item (used for blocker reassignment)
// header: optional string shown above items
function openPopup(items, anchorEl, header) {
  const popup = document.getElementById('popup');
  popup.innerHTML =
    (header ? `<div class="popup-header">${esc(header)}</div>` : '') +
    items.map((item, i) =>
      `<button class="popup-item${item.active ? ' active' : ''}" data-idx="${i}">${esc(item.label)}</button>`
    ).join('');

  // Position near anchor
  const rect = anchorEl.getBoundingClientRect();
  popup.style.display = 'block';
  const pw = popup.offsetWidth;
  const ph = popup.offsetHeight;
  let left = rect.right + 6;
  if (left + pw > window.innerWidth) left = rect.left - pw - 6;
  let top = rect.top;
  if (top + ph > window.innerHeight) top = window.innerHeight - ph - 8;
  popup.style.left = left + 'px';
  popup.style.top  = Math.max(8, top) + 'px';

  // Wire button clicks
  popup.querySelectorAll('.popup-item').forEach((btn, i) => {
    btn.addEventListener('click', e => {
      e.stopPropagation();
      closePopup();
      items[i].onClick();
    });
  });

  // Dismiss on outside click or Escape
  if (popupDismissHandler) document.removeEventListener('mousedown', popupDismissHandler);
  popupDismissHandler = e => {
    if (!popup.contains(e.target)) closePopup();
  };
  setTimeout(() => document.addEventListener('mousedown', popupDismissHandler), 0);
}

function closePopup() {
  document.getElementById('popup').style.display = 'none';
  if (popupDismissHandler) {
    document.removeEventListener('mousedown', popupDismissHandler);
    popupDismissHandler = null;
  }
}
```

- [ ] **Step 4: Add Escape key handler**

In the existing `document.addEventListener('keydown', ...)` block (near the bottom of the script), add an Escape handler before the Space handler:

```javascript
document.addEventListener('keydown', e => {
  if (e.code === 'Escape') {
    closePopup();
    return;
  }
  if (e.code === 'Space' && !e.target.closest('input, textarea, button')) {
    e.preventDefault();
    sendAction({ type: 'advance_step' });
  }
});
```

(Replace the existing keydown listener entirely with the above.)

- [ ] **Step 5: Verify in browser (console test)**

In the browser console:
```javascript
openPopup(
  [{ label: 'Tap for {B}', onClick: () => console.log('B'), active: false },
   { label: 'Tap for {R}', onClick: () => console.log('R'), active: true }],
  document.querySelector('.card') || document.body,
  'Tap for mana'
);
```
A popup should appear near the first card (or edge of screen). Clicking an item should log to console and close the popup. Escape should also close it.

- [ ] **Step 6: Commit**

```bash
git add src/serve.html
git commit -m "feat: add popup disambiguation menu"
```

---

### Task 5: Card visual states — CSS and helper function

**Files:**
- Modify: `src/serve.html`

Add CSS classes for the three card states (dim / actionable / selected) and a JS helper `isCardActionable(card, s, pid)` that determines which state applies.

- [ ] **Step 1: Add card state CSS to the `<style>` block**

Add after the existing `.card:hover` rule:

```css
.card.dim { border-color: #2a2a2a !important; box-shadow: none !important; }
.card.dim .card-name { color: #555; }
.card.dim .card-cost { color: #444; }
.card.dim .card-type { color: #3a3a3a; }
.card.dim .card-pt   { color: #444; border-top-color: #222; }
.card.actionable { border-color: #506050; box-shadow: 0 0 6px rgba(200,220,200,0.18); cursor: pointer; }
.card.land.actionable { border-color: #7a6030; box-shadow: 0 0 6px rgba(220,200,160,0.18); }
```

Note: `.card.selected` and `.card.attacking` already exist in the stylesheet and use the correct bright gold/red colours. Leave those unchanged.

- [ ] **Step 2: Add `isCardActionable(card, s, pid)` helper to the script**

Add this function near the top of the script (after the state variables):

```javascript
// Returns true if this card has at least one available action right now.
// Used for visual state only — clicking is always allowed regardless.
function isCardActionable(card, s, pid) {
  const isMyPid = pid === s.priority_player || pid === s.active_player;

  // Hand cards
  if (!card.tapped && card.type_line.includes('Land')) {
    // Lands in hand: actionable at sorcery speed when we haven't played a land yet
    return s.active_player === pid &&
      s.lands_played_this_turn === 0 &&
      (s.step === 'PreCombatMain' || s.step === 'PostCombatMain');
  }
  if (card.can_cast) return true;

  // Battlefield cards
  if (card.can_attack) return true;
  if (card.can_block) return true;
  if (card.activated_abilities && card.activated_abilities.some(a => a.can_activate)) return true;

  return false;
}
```

- [ ] **Step 3: Update `cardHTML` to apply the correct state class**

`cardHTML` currently builds a `classes` string starting with `'card'`. It adds `land`, `tapped`, `attacking`, `blocking`, `selected`. We need to also add `dim` or `actionable`.

Find the section in `cardHTML` that builds `classes`:

```javascript
  let classes = 'card';
  if (isLand) classes += ' land';
  if (card.tapped) classes += ' tapped';
  if (card.is_attacking) classes += ' attacking';
  if (card.is_blocking) classes += ' blocking';
  if (isSelected) classes += ' selected';
```

`cardHTML` doesn't currently receive `pid` or `s`. It needs them for `isCardActionable`. Change the signature and all call sites to pass both.

Change the function signature from:
```javascript
function cardHTML(card) {
```
To:
```javascript
function cardHTML(card, s, pid) {
```

Then, after the existing `isSelected` line, add:

```javascript
  const actionable = !isSelected && !card.is_attacking && isCardActionable(card, s, pid);
  if (actionable) classes += ' actionable';
  else if (!isSelected && !card.is_attacking && !card.is_blocking) classes += ' dim';
```

- [ ] **Step 4: Update all `cardHTML` call sites in `render(s)` to pass `s` and `pid`**

Find every call to `cardHTML(c)` inside `render(s)` and update:

```javascript
// Before:
document.getElementById('p1-hand').innerHTML     = s.p1.hand.map(c => cardHTML(c)).join('');
document.getElementById('p2-hand').innerHTML     = s.p2.hand.map(c => cardHTML(c)).join('');
document.getElementById('p1-lands').innerHTML    = s.p1.lands.map(c => cardHTML(c)).join('');
document.getElementById('p2-lands').innerHTML    = s.p2.lands.map(c => cardHTML(c)).join('');
document.getElementById('p1-creatures').innerHTML = s.p1.creatures.map(c => cardHTML(c)).join('');
document.getElementById('p2-creatures').innerHTML = s.p2.creatures.map(c => cardHTML(c)).join('');

// After:
document.getElementById('p1-hand').innerHTML      = s.p1.hand.map(c => cardHTML(c, s, 0)).join('');
document.getElementById('p2-hand').innerHTML      = s.p2.hand.map(c => cardHTML(c, s, 1)).join('');
document.getElementById('p1-lands').innerHTML     = s.p1.lands.map(c => cardHTML(c, s, 0)).join('');
document.getElementById('p2-lands').innerHTML     = s.p2.lands.map(c => cardHTML(c, s, 1)).join('');
document.getElementById('p1-creatures').innerHTML = s.p1.creatures.map(c => cardHTML(c, s, 0)).join('');
document.getElementById('p2-creatures').innerHTML = s.p2.creatures.map(c => cardHTML(c, s, 1)).join('');
```

Also update the call inside `openGY`:
```javascript
// Before:
`<div class="gy-cards-grid">${cards.map(c => cardHTML(c)).join('')}</div>`
// After (graveyard cards are never actionable; pass null/0 and they'll dim):
`<div class="gy-cards-grid">${cards.map(c => cardHTML(c, currentState, -1)).join('')}</div>`
```

- [ ] **Step 5: Verify in browser**

Reload `http://localhost:3000`. In Main 1 for P1 (who has priority): untapped lands and non-sick creatures should have a faint green-grey glow. P2's cards should be dimmed. Tapped lands should be dimmed. Spells in hand that `can_cast == true` should glow; those that can't should be dimmed.

- [ ] **Step 6: Commit**

```bash
git add src/serve.html
git commit -m "feat: add card visual states — dim/actionable/selected"
```

---

### Task 6: Card click handlers — hand cards

**Files:**
- Modify: `src/serve.html`

Wire onclick to hand cards. Clicking a land in hand sends `play_land`; clicking a spell sends `cast_spell`. Single action — no popup needed.

- [ ] **Step 1: Add `getHandActions(card)` helper**

```javascript
function getHandActions(card) {
  if (card.type_line.includes('Land')) {
    return [{ label: 'Play land', onClick: () => sendAction({ type: 'play_land', object_id: card.id }) }];
  }
  return [{ label: `Cast ${card.name}`, onClick: () => sendAction({ type: 'cast_spell', object_id: card.id }) }];
}
```

- [ ] **Step 2: Add `handleCardClick(cardId, pid, event)` routing function**

This is the central dispatcher. For now it only handles hand cards; later tasks extend it for battlefield cards.

```javascript
function handleCardClick(cardId, pid, event) {
  if (!currentState) return;
  closePopup();

  const s = currentState;
  const playerData = pid === 0 ? s.p1 : s.p2;

  // Check hand
  const handCard = playerData.hand.find(c => c.id === cardId);
  if (handCard) {
    const actions = getHandActions(handCard);
    if (actions.length === 1) { actions[0].onClick(); return; }
    openPopup(actions, event.currentTarget);
    return;
  }

  // Battlefield dispatch — added in Tasks 7, 8, 9
}
```

- [ ] **Step 3: Wire onclick into `cardHTML` for hand cards**

In `cardHTML`, the card div is currently rendered as:
```javascript
  return `<div class="${wrap}"><div class="${classes}" data-id="${card.id}">
```

Add an `onclick` attribute to the inner card div:
```javascript
  const clickAttr = `onclick="handleCardClick(${card.id}, ${pid}, event)"`;
  return `<div class="${wrap}"><div class="${classes}" data-id="${card.id}" ${clickAttr}>
```

This fires for all cards, not just hand cards. The `handleCardClick` dispatcher handles routing.

- [ ] **Step 4: Verify in browser**

In Main 1 (P1's turn), click a land in P1's hand. The land should play to the battlefield. Click a creature card in P1's hand (with enough mana). It should be cast. If you click P2's hand card, the server should reject and show a toast (e.g., "you don't have priority").

- [ ] **Step 5: Commit**

```bash
git add src/serve.html
git commit -m "feat: hand card clicks — play land and cast spell"
```

---

### Task 7: Card click handlers — battlefield lands

**Files:**
- Modify: `src/serve.html`

Clicking a battlefield land fires `tap_land` (if single mana ability or no parsed abilities) or opens a popup with each `activate_ability` option (if the land has multiple parsed activated abilities).

- [ ] **Step 1: Add `getBattlefieldLandActions(card)` helper**

Basic lands often have no parsed `activated_abilities` (their mana ability is intrinsic). Dual lands and others have parsed abilities. Handle both:

```javascript
function getBattlefieldLandActions(card) {
  // If the land has parsed activated abilities, offer each as a popup option
  if (card.activated_abilities && card.activated_abilities.length > 1) {
    return card.activated_abilities.map(ab => ({
      label: ab.label,
      onClick: () => sendAction({ type: 'activate_ability', object_id: card.id, ability_index: ab.index }),
    }));
  }
  if (card.activated_abilities && card.activated_abilities.length === 1) {
    const ab = card.activated_abilities[0];
    return [{ label: ab.label, onClick: () => sendAction({ type: 'activate_ability', object_id: card.id, ability_index: ab.index }) }];
  }
  // Fallback: intrinsic mana ability — use tap_land
  return [{ label: 'Tap for mana', onClick: () => sendAction({ type: 'tap_land', object_id: card.id }) }];
}
```

- [ ] **Step 2: Extend `handleCardClick` to handle battlefield lands**

Add after the hand-card section in `handleCardClick`:

```javascript
  // Battlefield lands
  const land = playerData.lands.find(c => c.id === cardId);
  if (land) {
    const actions = getBattlefieldLandActions(land);
    if (actions.length === 1) { actions[0].onClick(); return; }
    openPopup(actions, event.currentTarget, 'Tap for mana');
    return;
  }
```

- [ ] **Step 3: Verify in browser**

Tap a basic Forest in P1's lands — green mana should appear in the mana pool immediately (no popup). If you have a dual land in the deck config, tapping it should open a popup with two colour options. Clicking either option should add the corresponding mana.

- [ ] **Step 4: Commit**

```bash
git add src/serve.html
git commit -m "feat: battlefield land clicks — tap for mana with popup for dual lands"
```

---

### Task 8: Combat card clicks — attacker toggling and blocker assignment

**Files:**
- Modify: `src/serve.html`

During DeclareAttackers, clicking a creature with `can_attack` toggles it in the local `attackersSelected` array. During DeclareBlockers, clicking a creature with `can_block` opens a popup to assign it to an attacker.

- [ ] **Step 1: Add `getBattlefieldCreatureActions(card, s, pid, anchorEl)` helper**

```javascript
// Returns an array of action items, or null if the action was handled inline
// (attacker direct-toggle or blocker popup).
function getBattlefieldCreatureActions(card, s, pid, anchorEl) {
  const oppData = pid === 0 ? s.p2 : s.p1;

  // DeclareBlockers: always show attacker-assignment popup (takes priority over abilities)
  if (s.step === 'DeclareBlockers' && card.can_block) {
    const attackers = oppData.creatures.filter(c => c.is_attacking);
    const currentAssignment = blockersAssignment[card.id];
    const items = attackers.map(atk => ({
      label: `Block ${atk.name}`,
      active: currentAssignment === atk.id,
      onClick: () => {
        if (blockersAssignment[card.id] === atk.id) {
          delete blockersAssignment[card.id]; // deselect if already assigned here
        } else {
          blockersAssignment[card.id] = atk.id;
        }
        render(currentState);
      },
    }));
    openPopup(items, anchorEl, 'Assign blocker');
    return null; // handled inline
  }

  // Build action list for all other steps
  const actions = [];

  // DeclareAttackers: "Declare as attacker" toggle is offered as an action
  if (s.step === 'DeclareAttackers' && card.can_attack) {
    const isSelected = attackersSelected.includes(card.id);
    actions.push({
      label: isSelected ? '✓ Attacking — click to remove' : 'Declare as attacker',
      onClick: () => {
        if (isSelected) attackersSelected.splice(attackersSelected.indexOf(card.id), 1);
        else attackersSelected.push(card.id);
        render(currentState);
      },
    });
  }

  // Activated abilities
  if (card.activated_abilities) {
    card.activated_abilities.forEach(ab => {
      actions.push({
        label: ab.label,
        onClick: () => sendAction({ type: 'activate_ability', object_id: card.id, ability_index: ab.index }),
      });
    });
  }

  // If the only action is the attacker toggle, fire it directly (no popup needed)
  if (actions.length === 1 && s.step === 'DeclareAttackers' && card.can_attack) {
    actions[0].onClick();
    return null; // handled inline
  }

  return actions; // empty array = no-op; 1+ items = caller opens popup or fires directly
}
```

- [ ] **Step 2: Extend `handleCardClick` to handle battlefield creatures**

Add after the battlefield-lands section in `handleCardClick`:

```javascript
  // Battlefield creatures
  const creature = playerData.creatures.find(c => c.id === cardId);
  if (creature) {
    const actions = getBattlefieldCreatureActions(creature, s, pid, event.currentTarget);
    if (actions === null) return; // handled inline (toggle or popup opened)
    if (actions.length === 0) return; // no actions — click is a no-op
    if (actions.length === 1) { actions[0].onClick(); return; }
    openPopup(actions, event.currentTarget, 'Actions');
    return;
  }
```

- [ ] **Step 3: Verify partial combat flow in browser**

Navigate to DeclareAttackers (press Space 4 times to pass through PreCombatMain and BeginningOfCombat). Click a P1 creature with `can_attack` — its border should turn gold. Click it again — gold border goes away. Build a selection.

The Confirm button and Enter key are wired in Task 9. To advance past DeclareAttackers now, call `confirmAttackers()` in the browser console. For DeclareBlockers: click a P2 creature with `can_block` — a popup listing each attacking creature by name should appear. Click one to assign (gold border on blocker). Click the blocker again to reassign or deselect.

- [ ] **Step 4: Commit**

```bash
git add src/serve.html
git commit -m "feat: combat card clicks — attacker toggle and blocker assignment popup"
```

---

### Task 9: Action bar — combat mode with Confirm button and Enter key

**Files:**
- Modify: `src/serve.html`

Implement `renderActionBarCombat` so the bar shows combat context and the Confirm button during DeclareAttackers/DeclareBlockers. Wire the Enter key to confirm.

- [ ] **Step 1: Replace the stub `renderActionBarCombat` with the real implementation**

Find the stub:
```javascript
function renderActionBarCombat(s, bar) {
  renderActionBarNormal(s, bar);
}
```

Replace with:

```javascript
function renderActionBarCombat(s, bar) {
  bar.className = 'combat';
  const isDeclareAttackers = s.step === 'DeclareAttackers';
  const label = isDeclareAttackers ? 'Declare Attackers' : 'Declare Blockers';
  const count = isDeclareAttackers
    ? attackersSelected.length
    : Object.keys(blockersAssignment).length;
  const noun = isDeclareAttackers ? 'Attackers' : 'Blockers';
  const confirmFn = isDeclareAttackers ? 'confirmAttackers()' : 'confirmBlockers()';

  bar.innerHTML =
    `<span style="color:#888">${label}</span>` +
    `<span style="color:#333;margin:0 4px">·</span>` +
    `<span style="color:#b8b840">${count} selected</span>` +
    `<span style="color:#555;font-size:10px;margin-left:4px">· click creatures to toggle · Enter to confirm</span>` +
    `<div class="bar-right">` +
      `<button class="bar-btn bar-btn-confirm" onclick="${confirmFn}">Confirm ${noun} ✓</button>` +
      `<button class="bar-btn-log" onclick="toggleLog()">Log</button>` +
    `</div>`;
}
```

- [ ] **Step 2: Add Enter key handler for combat confirm**

Update the `keydown` listener (already modified in Task 4) to add Enter handling:

```javascript
document.addEventListener('keydown', e => {
  if (e.code === 'Escape') {
    closePopup();
    return;
  }
  if (e.code === 'Enter' && !e.target.closest('input, textarea, button')) {
    const s = currentState;
    if (!s) return;
    if (s.step === 'DeclareAttackers' && !s.attackers_declared) { confirmAttackers(); return; }
    if (s.step === 'DeclareBlockers'  && !s.blockers_declared)  { confirmBlockers();  return; }
  }
  if (e.code === 'Space' && !e.target.closest('input, textarea, button')) {
    e.preventDefault();
    sendAction({ type: 'advance_step' });
  }
});
```

- [ ] **Step 3: Verify full combat flow in browser**

Run through a complete combat turn:
1. Navigate to DeclareAttackers (4x Space to pass).
2. Click a P1 creature — gold border appears, bar shows "1 selected".
3. Press Enter (or click Confirm Attackers) — attackers declared, bar stays in combat mode for P2 to continue.
4. Space twice to reach DeclareBlockers.
5. Click a P2 creature — blocker popup appears.
6. Assign it to the attacker.
7. Press Enter (or click Confirm Blockers) — blockers declared, combat proceeds.

- [ ] **Step 4: Commit**

```bash
git add src/serve.html
git commit -m "feat: combat mode action bar — confirm strip replaces step chips"
```

---

### Task 10: Make mana pool clickable for reset

**Files:**
- Modify: `src/serve.html`

When `can_reset_mana` is true, the mana pool display gains a border, a `↩` suffix, and a click handler that sends `reset_mana`.

- [ ] **Step 1: Add mana pool CSS**

Add to the `<style>` block:

```css
.mana-pool-wrap { display: inline-flex; align-items: center; gap: 3px; }
.mana-pool-wrap.resettable {
  cursor: pointer; padding: 1px 4px; border-radius: 3px;
  border: 1px solid #2a3a2a; background: #0d140d;
}
.mana-pool-wrap.resettable:hover { border-color: #4a6a4a; }
.mana-reset-hint { font-size: 9px; color: #4a7a4a; margin-left: 2px; }
```

- [ ] **Step 2: Update `renderMana` to accept `canReset` and wrap accordingly**

Current signature:
```javascript
function renderMana(elId, pool) {
```

Change to:
```javascript
function renderMana(elId, pool, canReset) {
```

Change the inner HTML assignment. Currently:
```javascript
  document.getElementById(elId).innerHTML = colors
    .filter(([, n]) => n > 0)
    .flatMap(([c, n]) => Array(n).fill(`<span class="pip pip-${c}">${c}</span>`))
    .join('');
```

Replace with:
```javascript
  const pips = colors
    .filter(([, n]) => n > 0)
    .flatMap(([c, n]) => Array(n).fill(`<span class="pip pip-${c}">${c}</span>`))
    .join('');
  const wrapClass = 'mana-pool-wrap' + (canReset ? ' resettable' : '');
  const hint = canReset ? '<span class="mana-reset-hint">↩</span>' : '';
  const clickAttr = canReset ? ' onclick="sendAction({type:\'reset_mana\'})" title="Click to undo mana taps"' : '';
  document.getElementById(elId).innerHTML =
    `<span class="${wrapClass}"${clickAttr}>${pips}${hint}</span>`;
```

- [ ] **Step 3: Update `renderMana` call sites in `render(s)` to pass `canReset`**

Find:
```javascript
  renderMana('p1-mana', s.p1.mana_pool);
  renderMana('p2-mana', s.p2.mana_pool);
```

Replace with:
```javascript
  renderMana('p1-mana', s.p1.mana_pool, s.active_player === 0 && s.can_reset_mana);
  renderMana('p2-mana', s.p2.mana_pool, s.active_player === 1 && s.can_reset_mana);
```

- [ ] **Step 4: Verify in browser**

Tap a land for mana. The mana pool should show the pip(s) with a faint border and a `↩` icon. Clicking the mana pool should untap the land and clear the pool. When no mana is tapped (no checkpoint), the mana pool should show no border and no icon.

- [ ] **Step 5: Commit**

```bash
git add src/serve.html
git commit -m "feat: mana pool click resets mana when checkpoint exists"
```

---

### Task 11: Cleanup and full smoke test

**Files:**
- Modify: `src/serve.html`

Remove any remaining dead code, verify all existing cargo tests pass, and run through a complete game to check for regressions.

- [ ] **Step 1: Check for remaining dead references**

Search `src/serve.html` for:
- `renderPanes` — should not appear
- `actions-p1`, `actions-p2`, `badge-p1`, `badge-p2`, `pass-p1`, `pass-p2` — should not appear
- `pane-`, `action-group`, `action-btn` — should not appear in JS (only in CSS comments if any)

Remove any dead code found.

- [ ] **Step 2: Run existing cargo tests**

```bash
cargo test 2>&1 | grep -E "^test result|FAILED|error\["
```

Expected: `test result: ok. N passed; 0 failed` — no Rust changes were made so all existing tests should still pass.

- [ ] **Step 3: Full smoke test in browser**

Run through this checklist:

1. **Main phase actions:** P1 plays a land from hand → land appears on battlefield. P1 taps it → mana pip appears. P1 casts a creature (if enough mana) → creature appears.
2. **Toasts:** On P2's cards during P1's priority, click a P2 land → toast appears ("cannot tap: not your land" or similar).
3. **Log:** Click Log button → log drawer opens. Click again → closes.
4. **Full combat turn:** Declare attackers (click creatures → gold border → Enter), declare blockers (click blocker → popup → assign → Enter), combat damage resolves.
5. **Graveyard:** Click GY pile → modal opens with card list.
6. **Tooltips:** Hover a card → tooltip appears, positioned correctly, never obscured by the action bar.
7. **Game over:** Reduce a player to 0 life (use a deck that can do this quickly). Game over state renders correctly, Space no longer passes.
8. **Server disconnect:** Stop the server → disconnect overlay appears. Restart → reconnects.

- [ ] **Step 4: Update `docs/todo.md`**

The following items from the `## Actions` subsection of `# UI Issues` are resolved by this change and should be deleted:

- "Player 1 can only use mana abilities on their turn..." — resolved: mana abilities available to priority player via card clicks
- "Spells should be a visible option for casting even if there's not the available mana." — resolved: all spells in hand are clickable; server rejects if invalid

Also delete the `## Actions` subsection header if it's now empty.

- [ ] **Step 5: Final commit**

```bash
git add src/serve.html docs/todo.md
git commit -m "feat: card-click UI revamp — remove sidebar, click-to-act on all cards"
```
