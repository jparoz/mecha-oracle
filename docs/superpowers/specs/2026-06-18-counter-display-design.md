# Counter Display — Design Spec
_2026-06-18_

## Goal

Surface counter state visually in the serve.rs UI: hexagon badges on each card that carries counters, and poison counters next to the player's life total. The tooltip provides detailed breakdowns of every counter type.

---

## 1. Server-side data model (`serve.rs`)

### 1.1 New type: `CounterView`

```rust
#[derive(Serialize)]
struct CounterView {
    label:   String,         // "+1/+1", "-1/-1", "+2/-1", "Poison", "Charge", …
    kind:    String,         // "plus" | "minus" | "mixed" | "poison" | "named"
    count:   u32,
    sublabel: Option<String>, // e.g. "+3/+3 to P/T" for PtModifier kinds; None otherwise
}
```

`sublabel` is computed server-side as `format!("+{}/{} to P/T", count * power, count * toughness)` (with appropriate sign handling). This avoids JS having to parse the label string.

**`kind` mapping from `CounterKind`:**

| `CounterKind` | `kind` | color intent |
|---|---|---|
| `PtModifier { power > 0, toughness > 0 }` | `"plus"` | green |
| `PtModifier { power < 0, toughness < 0 }` | `"minus"` | red |
| `PtModifier` (signs differ) | `"mixed"` | teal/cyan |
| `Poison` | `"poison"` | purple |
| `Named(_)` | `"named"` | olive |

**`label` formatting:**
- `PtModifier { power: p, toughness: t }` → `"{+/-}{|p|}/{+/-}{|t|}"`, e.g. `"+1/+1"`, `"-1/-1"`, `"+2/-1"`.
- `Poison` → `"Poison"`.
- `Named(s)` → capitalize the first character of `s` (e.g. `"charge"` → `"Charge"`).

### 1.2 Changes to `CardView`

Add:
```rust
counters: Vec<CounterView>,  // empty when no counters present
```

Populated in `to_card_view` from `perm.counters` (only for battlefield objects that have a `PermanentState`). Hand/graveyard cards always emit an empty `Vec`.

### 1.3 Changes to `PlayerView`

Add:
```rust
poison_counters: u32,
```

Derived from `player.counter_count(&CounterKind::Poison)`. Zero when none (UI hides it when zero).

---

## 2. Client-side rendering (`serve.js`)

### 2.1 Card badge row

`cardHTML()` appends a `<div class="card-counters">` directly after the closing `</div>` of the card face element, still inside the `.card-wrap`:

```html
<div class="card-wrap">
  <div class="card ..."> ... </div>
  <div class="card-counters">
    <span class="hex-counter hex-plus" title="+1/+1">3</span>
    <span class="hex-counter hex-named" title="Charge">2</span>
  </div>
  <div class="tooltip"> ... </div>
</div>
```

- One badge per `CounterView` entry.
- Badge text content is the `count`.
- `title` attribute on each badge equals the `label` (native browser tooltip as a fallback).
- `card-counters` is a `display:flex` row; collapses to zero height when the array is empty (no layout disruption for counter-free cards).

### 2.2 Tooltip counter section

Passed as an entry in the `extraSections` array to the existing `tooltipHTML()` helper. A new `countersSectionHTML(counters)` function (parallel to `targetsSectionHTML`) builds the HTML string and returns `''` when the array is empty. Only rendered when `counters.length > 0`.

```html
<div class="tooltip-counters">
  <div class="tooltip-counters-label">Counters</div>
  <!-- one row per counter type -->
  <div class="counter-row">
    <span class="hex-counter hex-plus" style="width:16px;height:16px;font-size:8px">3</span>
    <span class="counter-label">
      <b>3× +1/+1</b>
      <span class="counter-sublabel">+3/+3 to P/T</span>
    </span>
  </div>
</div>
```

The sub-label is rendered from `counter.sublabel` when present (only set for `"plus"`, `"minus"`, `"mixed"` kinds). Omitted for `"poison"` and `"named"`.

### 2.3 Player poison counter

In `render()`, after setting the life text:

```js
const poisonEl = document.getElementById('p1-poison');  // new span in HTML
poisonEl.textContent = s.p1.poison_counters > 0 ? `☠ ${s.p1.poison_counters}` : '';
```

The `☠ N` span sits immediately after the life total in the player header. Hidden (empty text / zero width) when `poison_counters === 0`.

---

## 3. HTML changes (`serve.html`)

Add poison counter spans to both player headers:

```html
<!-- P1 header -->
<span class="life p1" id="p1-life">♥ 20</span>
<span class="poison-counter" id="p1-poison"></span>

<!-- P2 header -->
<span class="life p2" id="p2-life">♥ 20</span>
<span class="poison-counter" id="p2-poison"></span>
```

---

## 4. CSS additions (`serve.css`)

```css
/* Counter badge row below card face */
.card-counters {
  display: flex; gap: 2px; flex-wrap: wrap; justify-content: center;
}
.hex-counter {
  width: 14px; height: 14px;
  clip-path: polygon(50% 0%, 100% 25%, 100% 75%, 50% 100%, 0% 75%, 0% 25%);
  display: inline-flex; align-items: center; justify-content: center;
  font-size: 7px; font-weight: bold; color: #fff; flex-shrink: 0;
}
.hex-plus   { background: #2a6a2a; }
.hex-minus  { background: #6a1a1a; }
.hex-mixed  { background: #1a5a5a; }
.hex-poison { background: #4a1a5a; }
.hex-named  { background: #3a3a0a; }

/* Poison counter in player header */
.poison-counter { font-size: 14px; font-weight: bold; color: #c06fc0; margin-left: 4px; }

/* Tooltip counter section */
.tooltip-counters { margin-top: 6px; border-top: 1px solid #2a3a4a; padding-top: 6px; }
.tooltip-counters-label { font-size: 10px; color: #666; text-transform: uppercase; letter-spacing: 0.5px; margin-bottom: 4px; }
.counter-row { display: flex; align-items: flex-start; gap: 6px; margin-bottom: 4px; }
.counter-label { font-size: 11px; color: #ccc; display: flex; flex-direction: column; }
.counter-sublabel { font-size: 10px; color: #666; }
```

---

## 5. Out of scope

- Tapped-wrap sizing: the `tapped-wrap` class already accommodates extra height for the rotated card; the counter badge row below the card face adds a few pixels of height to the wrap regardless of tap state — acceptable.
- Counter manipulation actions (adding/removing counters via UI) — counters are currently only set by the engine; no UI actions for them.
- Named counter icons/emoji — plain colored hexagon with count is sufficient for now.
