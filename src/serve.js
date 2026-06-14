// ── State ────────────────────────────────────────────────────────────────────

let currentState = null;
let attackersSelected = [];
let blockersAssignment = {}; // blocker_id (number) -> attacker_id (number)
let gyData = { 1: [], 2: [] };
let toastTimer = null;
let popupDismissHandler = null;
let paymentContext = null; // null when no payment is in progress


// ── Toast ────────────────────────────────────────────────────────────────────

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

// ── Popup disambiguation menu ─────────────────────────────────────────────────

// items: [{ label, onClick, active }]   active = true highlights the item (used for blocker reassignment)
// header: optional string shown above items
function openPopup(items, anchorEl, header) {
  const popup = document.getElementById('popup');
  popup.innerHTML =
    (header ? `<div class="popup-header">${esc(header)}</div>` : '') +
    items.map((item, i) =>
      `<button class="popup-item${item.active ? ' active' : ''}${item.disabled ? ' disabled' : ''}" data-idx="${i}">${esc(item.label)}</button>`
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
      if (items[i].disabled) return;
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

// ── Fetch / send ─────────────────────────────────────────────────────────────

let serverDisconnected = false;

function showServerOverlay() {
  if (!serverDisconnected) {
    serverDisconnected = true;
    document.getElementById('server-overlay').classList.add('open');
  }
}
function hideServerOverlay() {
  if (serverDisconnected) {
    serverDisconnected = false;
    document.getElementById('server-overlay').classList.remove('open');
  }
}

async function fetchState() {
  try {
    const res = await fetch('/state');
    if (!res.ok) throw new Error('server error');
    currentState = await res.json();
    hideServerOverlay();
    render(currentState);
  } catch {
    showServerOverlay();
    setTimeout(fetchState, 2000);
  }
}

async function sendAction(action) {
  let res;
  try {
    res = await fetch('/action', {
      method: 'POST',
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify(action),
    });
  } catch {
    showServerOverlay();
    setTimeout(fetchState, 2000);
    return;
  }
  const data = await res.json();
  currentState = data.state;
  if (data.ok) {
    appendLog(describeAction(action), actionLogClass(action, currentState));
    attackersSelected = [];
    blockersAssignment = {};
  } else {
    appendLog('Engine: ' + data.error, 'log-error');
    showToast(data.error);
  }
  render(currentState);
}

function describeAction(action) {
  const ap = currentState.active_player; // 0 or 1
  const apLabel = ap + 1;               // display: 1 or 2
  switch (action.type) {
    case 'tap_land':       return `<span class="who">P${apLabel}</span> tapped a land for mana`;
    case 'play_land':      return `<span class="who">P${apLabel}</span> played a land`;
    case 'cast_spell': {
      // Instants and Flash creatures can be cast by any player with priority
      const casterLabel = currentState.priority_player + 1;
      if (action.targets && action.targets.length > 0) {
        return `<span class="who">P${casterLabel}</span> cast a targeted spell`;
      }
      return `<span class="who">P${casterLabel}</span> cast a spell`;
    }
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
    case 'pay_cost':    return `<span class="who">P${currentState.priority_player + 1}</span> paid ward cost`;
    case 'decline_cost': return `<span class="who">P${currentState.priority_player + 1}</span> declined ward — spell countered`;
    default: return JSON.stringify(action);
  }
}

function actionLogClass(action) {
  if (action.type === 'advance_step') return 'log-engine';
  if (action.type === 'declare_blockers') {
    return currentState.active_player === 0 ? 'log-p2' : 'log-p1';
  }
  return currentState.active_player === 0 ? 'log-p1' : 'log-p2';
}

// ── Rendering ─────────────────────────────────────────────────────────────────

const STEP_ORDER = [
  'Untap','Upkeep','Draw','PreCombatMain',
  'BeginningOfCombat','DeclareAttackers','DeclareBlockers','CombatDamage','EndOfCombat',
  'PostCombatMain','End','Cleanup'
];
const STEP_LABELS = {
  Untap:'Untap', Upkeep:'Upkeep', Draw:'Draw', PreCombatMain:'Main 1',
  BeginningOfCombat:'Begin Combat', DeclareAttackers:'Attackers',
  DeclareBlockers:'Blockers', CombatDamage:'Damage', EndOfCombat:'End Combat',
  PostCombatMain:'Main 2', End:'End', Cleanup:'Cleanup'
};
const STEP_INITIALS = {
  Untap:'UT', Upkeep:'UP', Draw:'DR', PreCombatMain:'M1',
  BeginningOfCombat:'BC', DeclareAttackers:'ATK',
  DeclareBlockers:'BLK', CombatDamage:'DMG', EndOfCombat:'EC',
  PostCombatMain:'M2', End:'END', Cleanup:'CL'
};

function render(s) {
  gyData = { 1: s.p1.graveyard, 2: s.p2.graveyard };

  document.getElementById('p1-life').textContent = '♥ ' + s.p1.life;
  document.getElementById('p2-life').textContent = '♥ ' + s.p2.life;

  renderMana('p1-mana', s.p1.mana_pool, s.active_player === 0 && s.can_reset_mana);
  renderMana('p2-mana', s.p2.mana_pool, s.active_player === 1 && s.can_reset_mana);

  document.querySelector('#p1-lib .pile-count').textContent = s.p1.library_count;
  document.querySelector('#p2-lib .pile-count').textContent = s.p2.library_count;

  renderGYPile('p1', s.p1.graveyard);
  renderGYPile('p2', s.p2.graveyard);

  document.getElementById('p1-hand').innerHTML      = s.p1.hand.map(c => cardHTML(c, s, 0, 'hand')).join('');
  document.getElementById('p2-hand').innerHTML      = s.p2.hand.map(c => cardHTML(c, s, 1, 'hand')).join('');
  document.getElementById('p1-lands').innerHTML     = s.p1.lands.map(c => cardHTML(c, s, 0, 'bf')).join('');
  document.getElementById('p2-lands').innerHTML     = s.p2.lands.map(c => cardHTML(c, s, 1, 'bf')).join('');
  document.getElementById('p1-creatures').innerHTML = s.p1.creatures.map(c => cardHTML(c, s, 0, 'bf')).join('');
  document.getElementById('p2-creatures').innerHTML = s.p2.creatures.map(c => cardHTML(c, s, 1, 'bf')).join('');

  renderActionBar(s);
  renderStack(s.stack ?? []);
  maybeEnterWardContext(s);
  renderPaymentPanel();
}

function renderMana(elId, pool, canReset) {
  const colors = [
    ['W', pool.w], ['U', pool.u], ['B', pool.b],
    ['R', pool.r], ['G', pool.g], ['C', pool.c],
  ];
  const pips = colors
    .filter(([, n]) => n > 0)
    .flatMap(([c, n]) => Array(n).fill(`<span class="pip pip-${c}">${c}</span>`))
    .join('');
  const wrapClass = 'mana-pool-wrap' + (canReset ? ' resettable' : '');
  const hint = canReset ? '<span class="mana-reset-hint">↩</span>' : '';
  const clickAttr = canReset ? ' onclick="sendAction({type:\'reset_mana\'})" title="Click to undo mana taps"' : '';
  document.getElementById(elId).innerHTML =
    `<span class="${wrapClass}"${clickAttr}>${pips}${hint}</span>`;
}

function renderGYPile(prefix, graveyard) {
  const top = graveyard[graveyard.length - 1];
  const label = document.getElementById(prefix + '-gy-label');
  const topEl = document.getElementById(prefix + '-gy-top');
  label.textContent = `GY (${graveyard.length})`;
  if (top) {
    topEl.innerHTML = `<span class="gy-card-name">${top.name}</span><span class="gy-card-type">${top.type_line}</span>` +
      (top.power != null ? `<span class="gy-card-pt">${top.power}/${top.toughness}</span>` : '');
  } else {
    topEl.innerHTML = '<span style="font-size:10px;color:#442222;text-align:center;width:100%">empty</span>';
  }
  document.getElementById(prefix + '-gy-wrap').style.cursor = graveyard.length > 0 ? 'pointer' : 'default';
}

function cardHTML(card, s, pid, zone) {
  const isLand = card.type_line.includes('Land');
  const isSelected = attackersSelected.includes(card.id) ||
    Object.keys(blockersAssignment).map(Number).includes(card.id);

  let classes = 'card';
  if (isLand) classes += ' land';
  if (card.tapped) classes += ' tapped';
  if (card.is_attacking) classes += ' attacking';
  if (card.is_blocking) classes += ' blocking';
  if (isSelected) classes += ' selected';

  if (!isSelected && !card.is_attacking && !card.is_blocking) {
    if (card.actions && card.actions.length > 0) {
      classes += ' actionable';
    } else {
      classes += ' dim';
    }
  }

  const wrap = card.tapped ? 'card-wrap tapped-wrap' : 'card-wrap';
  const pid_ = pid !== undefined ? pid : -1;
  const clickAttr = `onclick="handleCardClick(${card.id}, ${pid_}, event, true)" oncontextmenu="handleCardClick(${card.id}, ${pid_}, event, false)"`;

  const tags = [];
  if (card.tapped)         tags.push('<span class="tag tag-tapped">Tapped</span>');
  if (card.summoning_sick) tags.push('<span class="tag tag-sick">Summoning sickness</span>');
  if (card.damage_marked > 0) tags.push(`<span class="tag tag-damage">${card.damage_marked} damage</span>`);
  if (card.is_attacking)   tags.push('<span class="tag tag-attack">Attacking</span>');
  if (card.is_blocking)    tags.push('<span class="tag tag-block">Blocking</span>');

  const tooltip = `
    <div class="tooltip">
      <div class="tooltip-name">${esc(card.name)}</div>
      ${card.mana_cost ? `<div class="tooltip-cost">${esc(card.mana_cost)}</div>` : ''}
      <div class="tooltip-type">${esc(card.type_line)}</div>
      ${card.oracle_text ? `<div class="tooltip-text">${renderOracleText(card)}</div>` : ''}
      ${card.power != null ? `<div class="tooltip-pt">${card.power} / ${card.toughness}</div>` : ''}
      ${tags.length ? `<div class="tooltip-tags">${tags.join('')}</div>` : ''}
    </div>`;

  const pt = card.power != null
    ? `<span class="card-pt${card.damage_marked > 0 ? ' damaged' : ''}">${card.power}/${card.toughness}</span>`
    : '';

  return `<div class="${wrap}"><div class="${classes}" data-id="${card.id}" ${clickAttr}>
    <span class="card-name">${esc(card.name)}</span>
    ${card.mana_cost ? `<span class="card-cost">${esc(card.mana_cost)}</span>` : ''}
    <span class="card-type">${esc(card.type_line)}</span>
    ${pt}
  </div>${tooltip}</div>`;
}

function esc(s) {
  return String(s).replace(/&/g,'&amp;').replace(/</g,'&lt;').replace(/>/g,'&gt;').replace(/"/g,'&quot;');
}

function annStyle(kind) {
  if (kind === 'reminder_text' || kind === 'ability_word') return 'font-style:italic';
  if (kind === 'parsed_unimplemented') return 'color:#4dd9d9;text-decoration:underline';
  if (kind === 'unparsed') return 'color:red;text-decoration:underline';
  return '';
}

function renderOracleText(card) {
  const text = card.oracle_text || '';
  if (!text) return '';
  const annotations = (card.text_annotations || []).slice().sort((a, b) => a.start - b.start);
  const parts = [];
  let pos = 0;
  for (const ann of annotations) {
    if (ann.start > pos) parts.push(esc(text.slice(pos, ann.start)));
    const style = annStyle(ann.kind);
    const content = esc(text.slice(ann.start, ann.end));
    parts.push(style ? `<span style="${style}">${content}</span>` : content);
    pos = ann.end;
  }
  if (pos < text.length) parts.push(esc(text.slice(pos)));
  return `<div style="white-space:pre-wrap">${parts.join('')}</div>`;
}

function renderActionBar(s) {
  const bar = document.getElementById('action-bar');
  const isCombat =
    (s.step === 'DeclareAttackers' && !s.attackers_declared) ||
    (s.step === 'DeclareBlockers'  && !s.blockers_declared);

  if (isCombat) {
    renderActionBarCombat(s, bar);
  } else {
    renderActionBarNormal(s, bar);
  }
}

function renderActionBarNormal(s, bar) {
  if (s.game_over) {
    bar.className = 'normal';
    const winner = s.winner != null ? `Player ${s.winner + 1} wins!` : 'Draw!';
    bar.innerHTML =
      `<span style="color:#ffcc44;font-weight:bold">Game Over — ${winner}</span>`;
    return;
  }

  bar.className = 'normal';
  const cur = STEP_ORDER.indexOf(s.step);
  // bar.clientWidth is available synchronously; right cluster ≈ 210px, turn chip ≈ 80px
  const compact = bar.clientWidth > 0 && bar.clientWidth < 900;
  const stepMap = compact ? STEP_INITIALS : STEP_LABELS;
  const p1Priority = s.priority_player === 0;
  const chips = STEP_ORDER.map((step, i) => {
    const cls = i < cur ? 'done' : i === cur ? 'active' : 'upcoming';
    if (cls === 'active') {
      // CSS triangle chevron: below chip for P1 priority, above for P2
      const chevron = p1Priority
        ? `<span style="position:absolute;left:50%;transform:translateX(-50%);top:calc(100% + 3px);width:0;height:0;border-left:4px solid transparent;border-right:4px solid transparent;border-top:5px solid #ffd700;"></span>`
        : `<span style="position:absolute;left:50%;transform:translateX(-50%);bottom:calc(100% + 3px);width:0;height:0;border-left:4px solid transparent;border-right:4px solid transparent;border-bottom:5px solid #ffd700;"></span>`;
      return `<span class="step-chip active" style="position:relative">${stepMap[step]}${chevron}</span>`;
    }
    return `<span class="step-chip ${cls}">${stepMap[step]}</span>`;
  }).join('<span class="step-sep">·</span>');

  const pp = s.priority_player === 0 ? 'P1' : 'P2';
  const ppColor = s.priority_player === 0 ? 'var(--p1-color)' : 'var(--p2-color)';
  const apColor = s.active_player   === 0 ? 'var(--p1-color)' : 'var(--p2-color)';
  const apLabel = s.active_player   === 0 ? 'P1' : 'P2';

  bar.innerHTML =
    `<span style="background:#1a1a1a;border:1px solid #333;border-radius:3px;padding:1px 6px;margin-right:6px;flex-shrink:0;color:#888">` +
      `Turn ${s.turn}: <span style="color:${apColor};font-weight:bold">${apLabel}</span>` +
    `</span>${chips}` +
    `<div class="bar-right">` +
      `<span class="bar-hint"><span style="color:${ppColor};font-weight:bold">${pp}</span> priority · Space to pass</span>` +
      `<button class="bar-btn bar-btn-pass" onclick="sendAction({type:'advance_step'})">Pass Priority →</button>` +
      `<button class="bar-btn-log" onclick="toggleLog()">Log</button>` +
    `</div>`;
}

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
    `<div class="bar-right">` +
      `<span class="bar-hint">click to toggle · Enter to confirm</span>` +
      `<button class="bar-btn bar-btn-confirm" onclick="${confirmFn}">Confirm ${noun} ✓</button>` +
      `<button class="bar-btn-log" onclick="toggleLog()">Log</button>` +
    `</div>`;
}

function toggleLog() {
  document.getElementById('log-drawer').classList.toggle('open');
}


// ── Card click dispatch ───────────────────────────────────────────────────────

function findCard(cardId, pid) {
    const p = pid === 0 ? currentState.p1 : currentState.p2;
    return p.hand.find(c => c.id === cardId)
        || p.lands.find(c => c.id === cardId)
        || p.creatures.find(c => c.id === cardId);
}

function dispatchAction(item) {
    if (item.kind === 'server') {
        const t = item.action.type;
        if (t === 'cast_spell' || t === 'activate_ability' || t === 'cycle_card') {
            const kind = t === 'activate_ability' ? 'activate' : 'cast';
            const costLabel = item.label;
            enterPaymentContext(kind, costLabel, item.action, false, null);
            return;
        }
        sendAction(item.action);
    } else if (item.kind === 'toggle_attacker') {
        const idx = attackersSelected.indexOf(item.object_id);
        if (idx >= 0) attackersSelected.splice(idx, 1);
        else attackersSelected.push(item.object_id);
        render(currentState);
    } else if (item.kind === 'assign_blocker') {
        if (blockersAssignment[item.blocker_id] === item.attacker_id)
            delete blockersAssignment[item.blocker_id];
        else
            blockersAssignment[item.blocker_id] = item.attacker_id;
        render(currentState);
    }
}

function buildPopupItems(actions) {
    return actions.map(a => ({
        label: a.label,
        disabled: false,
        onClick: () => dispatchAction(a),
    }));
}

// kind: "cast" | "activate" | "ward"
function enterPaymentContext(kind, costLabel, confirmAction, declineable, declineAction) {
  paymentContext = { kind, costLabel, confirmAction, declineable, declineAction };
  renderPaymentPanel();
}

function renderPaymentPanel() {
  const panel = document.getElementById('payment-panel');
  if (!paymentContext || !currentState) {
    panel.style.display = 'none';
    return;
  }
  panel.style.display = '';
  document.getElementById('payment-title').textContent =
    paymentContext.kind === 'ward'     ? 'Ward — pay to protect your spell'
    : paymentContext.kind === 'cast'   ? 'Cast — pay cost'
    : 'Activate — pay cost';
  document.getElementById('payment-cost').textContent = paymentContext.costLabel || '(no cost)';

  // Mana pool from current state — server sends per-player with keys w/u/b/r/g/c
  const myPid = currentState.priority_player;
  const myPlayer = myPid === 0 ? currentState.p1 : currentState.p2;
  const pool = myPlayer ? myPlayer.mana_pool : {};
  const poolParts = [];
  if (pool.w) poolParts.push(`W\xd7${pool.w}`);
  if (pool.u) poolParts.push(`U\xd7${pool.u}`);
  if (pool.b) poolParts.push(`B\xd7${pool.b}`);
  if (pool.r) poolParts.push(`R\xd7${pool.r}`);
  if (pool.g) poolParts.push(`G\xd7${pool.g}`);
  if (pool.c) poolParts.push(`C\xd7${pool.c}`);
  document.getElementById('payment-pool').textContent =
    'Pool: ' + (poolParts.length ? poolParts.join(' ') : 'empty');

  document.getElementById('payment-remaining').textContent = '';

  document.getElementById('payment-confirm').disabled = false;
  document.getElementById('payment-cancel').style.display  = paymentContext.declineable ? 'none' : '';
  document.getElementById('payment-decline').style.display = paymentContext.declineable ? '' : 'none';
}

function confirmPayment() {
  if (!paymentContext) return;
  const action = paymentContext.confirmAction;
  paymentContext = null;
  renderPaymentPanel();
  sendAction(action);
}

function cancelPayment() {
  if (!paymentContext) return;
  const needsReset = currentState && currentState.can_reset_mana;
  paymentContext = null;
  renderPaymentPanel();
  if (needsReset) sendAction({ type: 'reset_mana' });
}

function declinePayment() {
  if (!paymentContext || !paymentContext.declineable) return;
  const action = paymentContext.declineAction;
  paymentContext = null;
  renderPaymentPanel();
  sendAction(action);
}

function maybeEnterWardContext(s) {
  if (paymentContext !== null) return;
  if (!s.stack || s.stack.length === 0) return;
  const top = s.stack[s.stack.length - 1];
  if (top.kind !== 'ward_trigger') return;
  enterPaymentContext(
    'ward',
    top.cost_label || 'unknown cost',
    { type: 'pay_cost', stack_id: top.id },
    true,
    { type: 'decline_cost', stack_id: top.id }
  );
}

// autoDispatchIfSingle=true for left-click; false for right-click (always show popup)
function handleCardClick(cardId, pid, event, autoDispatchIfSingle) {
    if (!autoDispatchIfSingle) event.preventDefault();
    if (!currentState) return;
    closePopup();
    const card = pid >= 0 ? findCard(cardId, pid) : null;
    const actions = card ? card.actions : [];

    if (autoDispatchIfSingle) {
        if (actions.length === 1) { dispatchAction(actions[0]); return; }
        if (actions.length === 0) return;
    }

    const items = actions.length > 0
        ? buildPopupItems(actions)
        : [{ label: 'No valid actions', onClick: () => {} }];
    openPopup(items, event.target, 'Actions');
}

// ── Attacker / Blocker selection ──────────────────────────────────────────────

function confirmAttackers() {
  sendAction({ type: 'declare_attackers', attacker_ids: attackersSelected });
}

function confirmBlockers() {
  const blocks = Object.entries(blockersAssignment)
    .map(([b, a]) => [parseInt(b), parseInt(a)]);
  sendAction({ type: 'declare_blockers', blocks });
}

// ── Graveyard modal ───────────────────────────────────────────────────────────

function openGY(player) {
  const cards = gyData[player];
  if (cards.length === 0) return;
  document.getElementById('gy-modal-title').textContent = `Player ${player} — Graveyard`;
  document.getElementById('gy-modal-cards').innerHTML =
    `<div class="gy-cards-grid">${cards.map(c => cardHTML(c, null, -1, 'gy')).join('')}</div>`;
  document.getElementById('gy-modal').classList.add('open');
}
function closeGY() { document.getElementById('gy-modal').classList.remove('open'); }
document.getElementById('gy-modal').addEventListener('click', e => { if (e.target === e.currentTarget) closeGY(); });

// ── Log ───────────────────────────────────────────────────────────────────────

function appendLog(html, cls) {
  const log = document.getElementById('log-entries');
  if (!log) return;
  const entry = document.createElement('div');
  entry.className = 'log-entry ' + (cls || '');
  entry.innerHTML = html;
  log.appendChild(entry);
  log.scrollTop = log.scrollHeight;
}

// ── Tooltip positioning ───────────────────────────────────────────────────────

document.addEventListener('mouseover', e => {
  const wrap = e.target.closest('.card-wrap');
  if (!wrap) return;
  const tooltip = wrap.querySelector('.tooltip');
  if (!tooltip) return;
  const rect = wrap.getBoundingClientRect();
  const TW = 208; // tooltip width (200) + small buffer
  const TH = 260; // conservative max tooltip height
  // Horizontal: prefer right of card; flip left if it would overflow
  let left = rect.right + 4;
  if (left + TW > window.innerWidth - 8) left = rect.left - TW - 4;
  left = Math.max(8, left);
  // Vertical: prefer aligned to card top; flip above if it would overflow
  let top = rect.top;
  if (top + TH > window.innerHeight - 8) top = rect.bottom - TH;
  top = Math.max(8, top);
  tooltip.style.left = left + 'px';
  tooltip.style.top  = top  + 'px';
});

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
  container.querySelectorAll('.stack-card:not([data-leaving])').forEach(el => {
    existing[el.dataset.stackId] = el;
  });

  // Remove cards no longer in the stack (leaving animation)
  const incomingIds = new Set(stack.map(item => String(item.id)));
  for (const [id, el] of Object.entries(existing)) {
    if (!incomingIds.has(id)) {
      const savedX = el._stackX || 0;
      const savedY = el._stackY || 0;
      el.dataset.leaving = '1';
      el.style.opacity = '0';
      el.style.transform = `translate(calc(-50% + ${savedX}px), calc(-50% + ${savedY - 12}px))`;
      setTimeout(() => el.remove(), 400); // past both 250ms opacity and 350ms transform transitions
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

      requestAnimationFrame(() =>
        requestAnimationFrame(() => {
          el.style.opacity   = '1';
          el.style.transform = `translate(calc(-50% + ${staggerX}px), calc(-50% + ${offsetY}px))`;
        })
      );
    } else {
      // Existing card — slide to new position
      el._stackX = staggerX;
      el._stackY = offsetY;
      el.style.zIndex    = zIndex;
      el.style.transform = `translate(calc(-50% + ${staggerX}px), calc(-50% + ${offsetY}px))`;
    }
  });
}

// ── Boot ──────────────────────────────────────────────────────────────────────

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

fetchState();
