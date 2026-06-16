// ── State ────────────────────────────────────────────────────────────────────

let currentState = null;
let attackersSelected = [];
let blockersAssignment = {}; // blocker_id (number) -> attacker_id (number)
let gyData = { 1: [], 2: [] };
let toastTimer = null;
let popupDismissHandler = null;
let paymentContext = null; // null when no payment is in progress


// ── Card color helpers ──────────────────────────────────────────────────────

const MANA_HEX = {};
['w', 'u', 'b', 'r', 'g', 'c', 'gold'].forEach(k => {
  MANA_HEX[k] = getComputedStyle(document.documentElement)
    .getPropertyValue(`--mana-${k}-bg`).trim();
});

// colors: array of single-letter color codes (e.g. ["W"], ["U","B"], []) as sent by
// the server's display_colors() (src/serve.rs) — already resolved to "what should this
// render as", so no land/non-land distinction is needed here.
function cardColorBackground(colors) {
  if (!colors || colors.length === 0) return MANA_HEX.c;
  if (colors.length === 1) return MANA_HEX[colors[0].toLowerCase()];
  if (colors.length === 2) {
    const [a, b] = colors.map(c => MANA_HEX[c.toLowerCase()]);
    return `linear-gradient(to right, ${a}, ${b})`; // colors[0] left, colors[1] right
  }
  return MANA_HEX.gold;
}

function bestTextColor(hex) {
  const n = parseInt(hex.replace('#', ''), 16);
  const r = (n >> 16) & 255, g = (n >> 8) & 255, b = n & 255;
  const luminance = (0.299 * r + 0.587 * g + 0.114 * b) / 255;
  return luminance > 0.6 ? '#1a1a1a' : '#ddd';
}


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
    case 'pay_pending_cost':    return `<span class="who">P${currentState.priority_player + 1}</span> paid cost`;
    case 'decline_pending_cost': return `<span class="who">P${currentState.priority_player + 1}</span> declined — spell countered`;
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
  maybeEnterPendingPaymentContext(s);
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

function tooltipHTML({ name, manaCost, typeLine, oracleHtml, pt, tags, extraSections }) {
  return `
    <div class="tooltip">
      <div class="tooltip-name">${esc(name)}</div>
      ${manaCost ? `<div class="tooltip-cost">${esc(manaCost)}</div>` : ''}
      <div class="tooltip-type">${esc(typeLine)}</div>
      ${oracleHtml ? `<div class="tooltip-text">${oracleHtml}</div>` : ''}
      ${pt ? `<div class="tooltip-pt">${pt}</div>` : ''}
      ${tags && tags.length ? `<div class="tooltip-tags">${tags.join('')}</div>` : ''}
      ${extraSections && extraSections.length ? extraSections.join('') : ''}
    </div>`;
}

function targetsSectionHTML(targets) {
  if (!targets || !targets.length) return '';
  return `<div class="tooltip-targets">
    <div class="tooltip-targets-label">Targets:</div>
    ${targets.map(t => `<div class="tooltip-target">${esc(t)}</div>`).join('')}
  </div>`;
}

function sourceSectionHTML(sourceName) {
  if (!sourceName) return '';
  return `<div class="tooltip-source">Source: ${esc(sourceName)}</div>`;
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

  const tooltip = tooltipHTML({
    name: card.name,
    manaCost: card.mana_cost,
    typeLine: card.type_line,
    oracleHtml: card.oracle_text ? renderOracleText(card) : '',
    pt: card.power != null ? `${card.power} / ${card.toughness}` : null,
    tags,
  });

  const pt = card.power != null
    ? `<span class="card-pt${card.damage_marked > 0 ? ' damaged' : ''}">${card.power}/${card.toughness}</span>`
    : '';

  const bg = cardColorBackground(card.colors);
  const fg = bestTextColor(card.colors && card.colors.length === 1 ? MANA_HEX[card.colors[0].toLowerCase()] : '#000000');
  const cardStyle = `style="background:${bg};color:${fg}"`;

  return `<div class="${wrap}"><div class="${classes}" data-id="${card.id}" ${clickAttr} ${cardStyle}>
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
        if (t === 'cast_spell' || t === 'cycle_card' ||
            (t === 'activate_ability' && !item.action.mana_ability)) {
            const kind = t === 'activate_ability' ? 'activate' : t === 'cycle_card' ? 'cycle' : 'cast';
            enterPaymentContext(kind, item.label, item.action.cost_label || '', item.action, false, null);
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

// kind: "cast" | "activate" | "cycle" | "pending"
// actionLabel: human-readable description (e.g. "Cast", "Pay {2}")
// costLabel: pure cost string (e.g. "{U}{U}", "{T}, {G}", "Pay 2 life")
function enterPaymentContext(kind, actionLabel, costLabel, confirmAction, declineable, declineAction) {
  paymentContext = { kind, actionLabel, costLabel, confirmAction, declineable, declineAction };
  document.getElementById('payment-x-input').value = 0;
  renderPaymentPanel();
}

function canPayCost(costLabel, pool, xValue = 0) {
  if (!costLabel) return true;
  const pips = costLabel.match(/\{([^}]+)\}/g) || [];
  let generic = 0;
  const colored = { W: 0, U: 0, B: 0, R: 0, G: 0, C: 0 };
  for (const pip of pips) {
    const inner = pip.slice(1, -1);
    if (inner === 'T' || inner === 'Q') continue; // tap/untap: structural only
    const n = parseInt(inner, 10);
    if (!isNaN(n)) { generic += n; continue; }
    if (inner === 'X') { generic += xValue; continue; } // each X pip costs xValue
    if (inner.includes('/')) continue; // hybrid/phyrexian: skip
    const col = inner.toUpperCase();
    if (col in colored) colored[col]++;
    else generic++;
  }
  const lifeMatch = costLabel.match(/Pay (\d+) life/);
  if (lifeMatch) {
    const myPid = currentState.priority_player;
    const myPlayer = myPid === 0 ? currentState.p1 : currentState.p2;
    if ((myPlayer?.life || 0) < parseInt(lifeMatch[1], 10)) return false;
  }
  if ((pool.w || 0) < colored.W) return false;
  if ((pool.u || 0) < colored.U) return false;
  if ((pool.b || 0) < colored.B) return false;
  if ((pool.r || 0) < colored.R) return false;
  if ((pool.g || 0) < colored.G) return false;
  if ((pool.c || 0) < colored.C) return false;
  const poolTotal = (pool.w||0)+(pool.u||0)+(pool.b||0)+(pool.r||0)+(pool.g||0)+(pool.c||0);
  const coloredUsed = colored.W+colored.U+colored.B+colored.R+colored.G+colored.C;
  return poolTotal - coloredUsed >= generic;
}

function renderPaymentPanel() {
  const panel = document.getElementById('payment-panel');
  if (!paymentContext || !currentState) {
    panel.style.display = 'none';
    return;
  }
  panel.style.display = '';
  document.getElementById('payment-title').textContent = paymentContext.actionLabel || 'Pay cost';
  document.getElementById('payment-cost').textContent = paymentContext.costLabel || '(no cost)';
  document.getElementById('payment-pool').textContent = '';

  const myPid = currentState.priority_player;
  const myPlayer = myPid === 0 ? currentState.p1 : currentState.p2;
  const pool = myPlayer ? myPlayer.mana_pool : {};

  const hasX = !!(paymentContext.costLabel && paymentContext.costLabel.includes('{X}'));
  const xRow = document.getElementById('payment-x-row');
  const xInput = document.getElementById('payment-x-input');

  if (hasX) {
    // Compute max X: pool total minus fixed (non-X) pip requirements
    const pips = (paymentContext.costLabel.match(/\{([^}]+)\}/g) || []);
    let fixedGeneric = 0;
    const fixedColored = { W: 0, U: 0, B: 0, R: 0, G: 0, C: 0 };
    let xCount = 0;
    for (const pip of pips) {
      const inner = pip.slice(1, -1);
      if (inner === 'T' || inner === 'Q') continue;
      if (inner === 'X') { xCount++; continue; }
      if (inner.includes('/')) continue;
      const n = parseInt(inner, 10);
      if (!isNaN(n)) { fixedGeneric += n; continue; }
      const col = inner.toUpperCase();
      if (col in fixedColored) fixedColored[col]++;
      else fixedGeneric++;
    }
    const fixedColoredTotal = Object.values(fixedColored).reduce((a, b) => a + b, 0);
    const poolTotal = (pool.w||0)+(pool.u||0)+(pool.b||0)+(pool.r||0)+(pool.g||0)+(pool.c||0);
    const budgetForX = Math.max(0, poolTotal - fixedColoredTotal - fixedGeneric);
    xInput.max = Math.floor(budgetForX / (xCount || 1));
    xRow.style.display = '';
  } else {
    xInput.value = 0;
    xRow.style.display = 'none';
  }

  const xValue = hasX ? parseInt(xInput.value || '0', 10) : 0;
  document.getElementById('payment-confirm').disabled = !canPayCost(paymentContext.costLabel, pool, xValue);
  document.getElementById('payment-cancel').style.display  = paymentContext.declineable ? 'none' : '';
  document.getElementById('payment-decline').style.display = paymentContext.declineable ? '' : 'none';
}

function confirmPayment() {
  if (!paymentContext) return;
  const action = { ...paymentContext.confirmAction };
  if (paymentContext.costLabel && paymentContext.costLabel.includes('{X}')) {
    action.x_value = parseInt(document.getElementById('payment-x-input').value || '0', 10);
  }
  paymentContext = null;
  renderPaymentPanel();
  sendAction(action);
}

function cancelPayment() {
  if (!paymentContext || paymentContext.declineable) return;
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

function maybeEnterPendingPaymentContext(s) {
  if (paymentContext !== null) return;
  if (!s.pending_payment) return;
  const pp = s.pending_payment;
  enterPaymentContext(
    'pending',
    `Pay ${pp.cost_label}`,
    pp.cost_label || '',
    { type: 'pay_pending_cost' },
    true,
    { type: 'decline_pending_cost' }
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
  // Stack cards carry an inline `transform` for their slide animation, which makes the
  // card the containing block for `position: fixed` descendants (CSS Transforms spec) —
  // a nested `.tooltip` would then be positioned relative to the card, not the viewport.
  // So stack-card tooltips live detached in #stack-items (see renderStack) and are
  // tracked via `wrap._tooltipEl` instead of being found by querySelector.
  const tooltip = wrap.querySelector('.tooltip') || wrap._tooltipEl;
  if (!tooltip) return;
  if (wrap._tooltipEl) tooltip.style.display = 'block';
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

document.addEventListener('mouseout', e => {
  const wrap = e.target.closest('.card-wrap');
  if (!wrap || !wrap._tooltipEl) return;
  if (wrap.contains(e.relatedTarget)) return; // still inside the card
  wrap._tooltipEl.style.display = 'none';
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
      setTimeout(() => {
        el.remove();
        el._tooltipEl?.remove();
      }, 400); // past both 250ms opacity and 350ms transform transitions
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
      el.className       = 'card-wrap stack-card ' + (item.controller === 0 ? 'p1' : 'p2');
      el.dataset.stackId = idStr;
      const tooltipHtml = item.card
        ? tooltipHTML({
            name: item.card.name,
            manaCost: item.card.mana_cost,
            typeLine: item.card.type_line,
            oracleHtml: item.card.oracle_text ? renderOracleText(item.card) : '',
            pt: item.card.power != null ? `${item.card.power} / ${item.card.toughness}` : null,
            extraSections: [targetsSectionHTML(item.targets)],
          })
        : tooltipHTML({
            name: item.label,
            typeLine: item.kind === 'activated_ability' ? 'Activated Ability' : 'Triggered Ability',
            extraSections: [sourceSectionHTML(item.source_name), targetsSectionHTML(item.targets)],
          });
      el.innerHTML =
        `<span class="stack-card-name">${esc(item.label)}</span>` +
        `<span class="stack-kind">${kindLabel}</span>`;
      // The tooltip is NOT nested inside `el`: `el` carries an inline `transform` for its
      // slide animation, which would become the containing block for the tooltip's
      // `position: fixed`, breaking viewport-relative positioning. Instead it's appended
      // as a sibling in #stack-items and tracked via `el._tooltipEl` (see mouseover/mouseout
      // listeners above) so CSS `.card-wrap:hover .tooltip` can't reach it — shown/hidden via JS.
      const tooltipHost = document.createElement('div');
      tooltipHost.innerHTML = tooltipHtml;
      const tooltip = tooltipHost.firstElementChild;
      container.appendChild(tooltip);
      el._tooltipEl = tooltip;
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
