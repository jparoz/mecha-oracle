# UI Polish Pass Design

**Date:** 2026-06-16
**Scope:** Four independent presentation-layer fixes to the web UI: (1) zone-aware, viewport-clamped tooltip positioning, (2) tooltip z-index relative to the right-click context menu, (3) card background colors driven by mechanical color, (4) mana symbols rendered as icons everywhere a cost string is shown, including `{T}`/`{Q}`. No engine/rules changes — this only touches `src/serve.rs` (view serialization) and `src/serve.js`/`src/serve.css` (presentation).

---

## 1. Tooltip positioning

### Background

The single `mouseover` listener at `src/serve.js:671-695` positions every `.tooltip` the same way regardless of zone: prefer right of the card (flip left if it overflows), prefer top-aligned with the card (flip to bottom-aligned using a **hardcoded** `TH = 260` guess if it would overflow). This guess is the root cause of both reported bugs:
- When the real tooltip is much shorter than 260px (P1 hand, near the bottom of the viewport), the flip-to-bottom-aligned math leaves a large, disconnected gap above the card.
- When the real tooltip is taller than 260px (long oracle text), the overflow check based on the wrong guess doesn't flip, and the tooltip runs off the bottom of the viewport.

### Fix

Replace the guess with the tooltip's **real** rendered size, and make the preferred placement zone-specific:

```js
document.addEventListener('mouseover', e => {
  const wrap = e.target.closest('.card-wrap');
  if (!wrap) return;
  const tooltip = wrap.querySelector('.tooltip') || wrap._tooltipEl;
  if (!tooltip) return;
  if (wrap._tooltipEl) tooltip.style.display = 'block';

  const cardRect = wrap.getBoundingClientRect();
  const tw = tooltip.offsetWidth;
  const th = tooltip.offsetHeight;
  const GAP = 6;

  let left, top;
  if (wrap.classList.contains('stack-card')) {
    // Stack: prefer left of the card, vertical centers aligned.
    left = cardRect.left - tw - GAP;
    if (left < 8) left = cardRect.right + GAP; // flip right only if no room on the left
    top = cardRect.top + cardRect.height / 2 - th / 2;
  } else {
    // Hand / battlefield / graveyard-modal: prefer above/below, horizontal centers aligned.
    left = cardRect.left + cardRect.width / 2 - tw / 2;
    const spaceAbove = cardRect.top;
    const spaceBelow = window.innerHeight - cardRect.bottom;
    if (spaceAbove >= th + GAP || spaceAbove >= spaceBelow) {
      top = cardRect.top - th - GAP;
    } else {
      top = cardRect.bottom + GAP;
    }
  }

  // Universal viewport clamp — applies no matter which branch above ran.
  left = Math.max(8, Math.min(left, window.innerWidth - tw - 8));
  top  = Math.max(8, Math.min(top,  window.innerHeight - th - 8));
  tooltip.style.left = left + 'px';
  tooltip.style.top  = top  + 'px';
});
```

`tooltip.offsetWidth`/`offsetHeight` must be read while the tooltip is visible, which already holds here: CSS-hover-triggered tooltips are visible by the time `mouseover` fires because the `:hover` pseudo-class is active for the whole event; detached stack tooltips are forced visible one line above via `tooltip.style.display = 'block'`.

This satisfies all three zone rules from the request:
- Hand/battlefield: above/below, horizontal centers aligned, whichever side has more room wins.
- Stack: left, vertical centers aligned, flips right only when there's no room on the left.
- Graveyard-modal cards reuse `cardHTML()` (see below) and fall into the same "hand/battlefield" branch — reasonable default since they're laid out in a similar grid.
- The final clamp is unconditional, so no branch above can ever place a tooltip outside the viewport.

No changes to `renderStack()`'s use of `_tooltipEl` or the `mouseout` listener (`src/serve.js:697-702`).

---

## 2. Tooltip vs. context menu stacking

`.tooltip`'s `z-index: 9999` (`src/serve.css:82`) drops to `350` — below `#popup`'s `z-index: 400` (`src/serve.css:273`, the right-click action menu), but still above `.payment-panel` (`250`) and `#gy-modal` (`200`). One-line CSS change.

---

## 3. Card background colors by mechanical color

### Server: a single authoritative "display colors" helper

Cards' printed `colors` (CR 105.2) already drive correctness elsewhere (e.g. protection-from-color targeting at `src/serve.rs:466`) and must not change. But lands are colorless by definition (CR 105.2a) and currently render with no useful color signal. Add one new helper in `src/serve.rs`, near `format_mana_cost` (`src/serve.rs:229`):

```rust
fn display_colors(def: &mecha_oracle::types::card::CardDefinition) -> Vec<mecha_oracle::types::mana::ManaColor> {
    use mecha_oracle::types::mana::ManaColor;
    if !def.colors.is_empty() {
        return def.colors.clone();
    }
    if !def.type_line.is_land() {
        return vec![];
    }
    // CR 105.2a: lands are colorless by definition, but a land's *display* color is
    // useful UI signal — derive it from basic land subtypes and any colored mana
    // symbols printed in its rules text (e.g. "Swamp Forest" → [Black, Green]).
    let mut colors = Vec::new();
    let mut push = |c: ManaColor| if !colors.contains(&c) { colors.push(c) };
    // WUBRG canonical order
    for subtype in &def.type_line.subtypes {
        match subtype.as_str() {
            "Plains" => push(ManaColor::White),
            "Island" => push(ManaColor::Blue),
            "Swamp" => push(ManaColor::Black),
            "Mountain" => push(ManaColor::Red),
            "Forest" => push(ManaColor::Green),
            _ => {}
        }
    }
    for (needle, color) in [
        ("{W}", ManaColor::White), ("{U}", ManaColor::Blue), ("{B}", ManaColor::Black),
        ("{R}", ManaColor::Red),   ("{G}", ManaColor::Green),
    ] {
        if def.oracle_text.contains(needle) {
            push(color);
        }
    }
    colors
}
```

(`push` as a closure capturing `colors` by `&mut` needs `colors` declared `mut` and the closure marked `mut` — standard Rust borrow pattern, not shown in pseudo-detail above but trivial.) No new dependency: plain `str::contains` substring checks, no regex needed (this project has no `regex` crate dependency today).

### `CardView.colors` (`src/serve.rs:184`) — reinterpreted, not renamed

Both `CardView` construction sites switch from raw printed colors to `display_colors`:
- `to_card_view` closure in `build_player_view` (`src/serve.rs:633-638`): `colors: display_colors(&obj.definition).iter().map(|c| c.to_string()).collect()`.
- Stack spell's inline `CardView` in `build_game_view` (`src/serve.rs:761`): same pattern against `c.definition`.

This is a value-only change — the field name, type, and every other reader of `obj.definition.colors` directly (targeting, etc.) is untouched.

### `StackItemView` — new `source_colors` field for non-spell stack items

Triggered/activated abilities on the stack have no `card`, so there's nothing for the client to read a color from. Add one field (`src/serve.rs:142-154`):

```rust
#[serde(skip_serializing_if = "Vec::is_empty")]
source_colors: Vec<String>,
```

Populated in `build_game_view` (`src/serve.rs:774-803`):
- `Spell` arm: not applicable (`card` already carries `colors`) — omit/empty.
- `TriggeredAbility`/`ActivatedAbility` arms: `state.objects.get(source_id).map(|o| display_colors(&o.definition)).unwrap_or_default().iter().map(|c| c.to_string()).collect()`.

### Client: one function, used everywhere a card-shaped thing renders

```js
const MANA_HEX = {}; // populated from CSS custom properties — see CSS section
['w','u','b','r','g','c','gold'].forEach(k => {
  MANA_HEX[k] = getComputedStyle(document.documentElement)
    .getPropertyValue(`--mana-${k}-bg`).trim();
});

function cardColorBackground(colors) {
  if (!colors || colors.length === 0) return MANA_HEX.c;
  if (colors.length === 1) return MANA_HEX[colors[0].toLowerCase()];
  if (colors.length === 2) {
    const [a, b] = colors.map(c => MANA_HEX[c.toLowerCase()]);
    return `linear-gradient(to right, ${a}, ${b})`; // colors[0] left, colors[1] right — order preserved
  }
  return MANA_HEX.gold;
}

function bestTextColor(hex) {
  // Relative luminance check — only matters if a background swatch turns out light.
  const n = parseInt(hex.replace('#', ''), 16);
  const r = (n >> 16) & 255, g = (n >> 8) & 255, b = n & 255;
  const luminance = (0.299 * r + 0.587 * g + 0.114 * b) / 255;
  return luminance > 0.6 ? '#1a1a1a' : '#ddd';
}
```

`cardHTML()` (`src/serve.js:275-325`) computes `background-image/background-color` + `color` once per card and applies them as an inline `style` on the `.card` div (alongside the existing `classes` string) — colors are per-card data, not expressible as a static CSS class. Since every muted swatch in this palette is dark by construction (see CSS section), `bestTextColor` in practice always returns `#ddd`, matching the existing text color — it's there to honor "change foreground if needed" defensively, not because today's palette requires it.

`renderStack()` (`src/serve.js:709-810`) applies the same `cardColorBackground(item.card ? item.card.colors : item.source_colors)` to `.stack-card`'s inline style. The existing `.stack-card.p1`/`.stack-card.p2` border-color rules are untouched — only `background` is overridden inline, so the controller border cue survives.

The graveyard-pile preview (`renderGYPile`, `src/serve.js:235-247`) applies the same helper to the top graveyard card's `colors` for its `#prefix-gy-top` element (the one real-card box; the two decorative stacked-shadow `.gy-card` siblings are left as-is — they carry no card data). The graveyard **modal** already renders through `cardHTML()` (`src/serve.js:650-652`), so it's covered automatically.

---

## 4. Mana symbols as icons

### Wire format: one consistent brace notation

`CardView.mana_cost` switches from the plain `format_mana_cost` ("3G") to the already-existing `format_mana_cost_braced` ("{3}{G}") at both call sites (`src/serve.rs:630`, `src/serve.rs:758`). `Cycle`'s action label (`src/serve.rs:503`) switches from `format_mana_cost` to `format_mana_cost_braced` too, so `"Cycle (3G)"` becomes `"Cycle ({3}{G})"`. Every other cost-bearing string sent to the client (`cost_label`, `format_ability_cost_label`, `format_activated_ability`, `format_mana_pool`) already uses brace notation — no change needed there. `format_mana_cost` (unbraced) keeps its two direct unit tests (`src/serve.rs:1307`, `1316`) but no longer has any non-test caller — left in place since the function itself, not its callers, is what's tested.

### CSS: custom properties as the single source of truth for color hexes

Add mana-related custom properties to `:root` (`src/serve.css:2-5`), then point the existing `.pip-W`...`.pip-C` rules at them instead of their current hardcoded hex literals, plus new variants:

```css
:root {
  --p1-color: #51cf66;
  --p2-color: #ff7b7b;
  --mana-w-bg: #4d4a1a; --mana-w-border: #9a932d; --mana-w-fg: #e8e06f;
  --mana-u-bg: #1a2a4d; --mana-u-border: #2d4a9a; --mana-u-fg: #6f8ee8;
  --mana-b-bg: #2a1a2a; --mana-b-border: #6a3a6a; --mana-b-fg: #c06fc0;
  --mana-r-bg: #4d1a1a; --mana-r-border: #9a2d2d; --mana-r-fg: #e86f6f;
  --mana-g-bg: #1a4d1a; --mana-g-border: #2d7a2d; --mana-g-fg: #6fd86f;
  --mana-c-bg: #2a2a2a; --mana-c-border: #666;    --mana-c-fg: #ccc;
  --mana-gold-bg: #4a3a10; --mana-gold-border: #9a7a20; --mana-gold-fg: #e8c860;
  --mana-x-bg: #161616; --mana-x-border: #444; --mana-x-fg: #eee;
  --mana-s-bg: #1c2c34; --mana-s-border: #4a7a90; --mana-s-fg: #bfe6f5;
  --mana-p-bg: #161616; /* phyrexian segment in split pips — shares X's dark tone */
}
.pip-W { background: var(--mana-w-bg); border-color: var(--mana-w-border); color: var(--mana-w-fg); }
/* ...same pattern for U/B/R/G/C... */
.pip-generic { background: var(--mana-c-bg); border-color: var(--mana-c-border); color: var(--mana-c-fg); }
.pip-X { background: var(--mana-x-bg); border-color: var(--mana-x-border); color: var(--mana-x-fg); }
.pip-S { background: var(--mana-s-bg); border-color: var(--mana-s-border); color: var(--mana-s-fg); }
.pip-split { font-size: 6px; } /* background set inline per-token; label is 2-3 chars */
.pip-tap svg { display: block; }
```

Reading hexes from these custom properties (rather than duplicating literals in JS) keeps the palette in exactly one place; JS reads them once via `getComputedStyle` (shown in section 3).

### JS: one parser/renderer for every `{token}`

```js
function manaComponentStyle(part) {
  const p = part.toUpperCase();
  if (p === 'P') return { bg: 'var(--mana-p-bg)', label: 'P' };
  if (p === 'X') return { bg: 'var(--mana-x-bg)', label: 'X' };
  if (p === 'S') return { bg: 'var(--mana-s-bg)', label: 'S' };
  if (/^\d+$/.test(p)) return { bg: 'var(--mana-c-bg)', label: p };
  if ('WUBRGC'.includes(p)) return { bg: `var(--mana-${p.toLowerCase()}-bg)`, label: p };
  return { bg: 'var(--mana-c-bg)', label: p }; // unrecognized — fall back to generic
}

function manaPipHTML(parts) {
  if (parts.length === 1) {
    const { label } = manaComponentStyle(parts[0]);
    const cls = /^\d+$/.test(parts[0]) ? 'pip-generic' : `pip-${label}`;
    return `<span class="pip ${cls}">${esc(label)}</span>`;
  }
  const comps = parts.map(manaComponentStyle);
  const n = comps.length;
  const stops = comps.map((c, i) =>
    `${c.bg} ${(i / n * 100).toFixed(2)}%, ${c.bg} ${((i + 1) / n * 100).toFixed(2)}%`
  ).join(', ');
  const label = comps.map(c => c.label).join('');
  return `<span class="pip pip-split" style="background:linear-gradient(to right, ${stops})">${esc(label)}</span>`;
}

function tapPipHTML(untap) {
  const circle = untap ? '#1a1a1a' : '#cfcfcf';
  const arrow  = untap ? '#fff'    : '#1a1a1a';
  const rotate = untap ? ' transform="rotate(180 12 12)"' : '';
  return `<span class="pip pip-tap">` +
    `<svg viewBox="0 0 24 24" width="12" height="12"><g${rotate}>` +
    `<circle cx="12" cy="12" r="11" fill="${circle}" stroke="#555" stroke-width="1"/>` +
    `<path d="M12 4.5 A7.5 7.5 0 1 1 5.0 8.8" fill="none" stroke="${arrow}" stroke-width="2.2" stroke-linecap="round"/>` +
    `<path d="M5.0 8.8 L3.4 5.2 L7.6 6.4 Z" fill="${arrow}"/>` +
    `</g></svg></span>`;
}

function renderManaSymbols(str) {
  if (str == null) return '';
  const s = String(str);
  const re = /\{([^}]+)\}/g;
  let out = '', last = 0, m;
  while ((m = re.exec(s))) {
    out += esc(s.slice(last, m.index));
    out += m[1] === 'T' ? tapPipHTML(false)
         : m[1] === 'Q' ? tapPipHTML(true)
         : manaPipHTML(m[1].split('/'));
    last = re.lastIndex;
  }
  return out + esc(s.slice(last));
}
```

Both `{T}` and `{Q}` share one SVG path (circle + ~290° clockwise arc + arrowhead); `{Q}` just rotates the whole `<g>` 180° and swaps the two fill colors, per the request ("the same shape ... rotated 180 degrees").

### Call sites switched from `esc(...)` / `textContent` to `renderManaSymbols(...)`

| Location | Before | After |
|---|---|---|
| `cardHTML()` card-cost span (`src/serve.js:321`) | `esc(card.mana_cost)` | `renderManaSymbols(card.mana_cost)` |
| `tooltipHTML()` cost line (`src/serve.js:253`) | `esc(manaCost)` | `renderManaSymbols(manaCost)` |
| `openPopup()` item label (`src/serve.js:35`) | `esc(item.label)` | `renderManaSymbols(item.label)` |
| `renderPaymentPanel()` title (`src/serve.js:527`) | `.textContent = paymentContext.actionLabel` | `.innerHTML = renderManaSymbols(paymentContext.actionLabel \|\| 'Pay cost')` |
| `renderPaymentPanel()` cost (`src/serve.js:528`) | `.textContent = paymentContext.costLabel` | `.innerHTML = renderManaSymbols(paymentContext.costLabel \|\| '(no cost)')` |

All other readers of these same raw strings (`canPayCost`, the X-detection regexes in `renderPaymentPanel`/`confirmPayment`, `maybeEnterPendingPaymentContext`) keep parsing the original `{...}` string directly — unaffected, since the string's shape doesn't change, only how it's displayed.

`stack-card-name` (`item.label` in `renderStack()`) is unchanged — ability/spell names on the stack don't contain cost tokens.

---

## Out of scope

- No engine/rules logic changes — `display_colors` is purely a presentation derivation, not used by any rules check.
- Hybrid/Phyrexian/generic-hybrid pips with 3 components render a 3-character label in a 14px circle — acceptable for a rare case (`HybridPhyrexian`); no card in the current test decks uses one.
- Graveyard pile's two decorative shadow cards (non-top) stay a fixed shade — no per-card data to color them with.

---

## Testing

Presentation-only change. `cargo clippy --all-targets` clean, plus manual verification:
- Hover cards in P1 hand, P2 hand, P1/P2 battlefield, and the stack — tooltip appears on the correct side, centers aligned, never clipped by the viewport (test with a card with long oracle text in P1's hand specifically, since that's the reported bug).
- Right-click a card while hovering another card's tooltip — confirm the context menu draws on top.
- Check a mono-color card, a two-color card (gradient direction matches `colors` order), a 3+ color card (gold), an artifact (grey), a basic land of each type, and a dual-type land (if one exists in test decks) for correct backgrounds.
- Trigger an ability from a land/permanent and confirm the stack card picks up `source_colors`.
- Cast a spell with a generic+colored cost, activate a mana ability (tap-for-mana), and open the payment panel for an `{X}` cost — confirm all mana/tap symbols render as icons, not raw text.
