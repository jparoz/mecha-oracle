# Counter Display Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Render hexagon counter badges below each card on the battlefield and poison counters next to the player life total, with detailed counter breakdowns in card tooltips.

**Architecture:** The server serialises `PermanentState.counters` and `Player.counters` into a new `CounterView` type that pre-computes display labels and kind strings so the JS client is purely presentational. A new `card-counters` div sits below the card face inside the existing `.card-wrap`, and a `countersSectionHTML()` helper feeds the tooltip's `extraSections` slot.

**Tech Stack:** Rust / axum (`src/serve.rs`), vanilla JS (`src/serve.js`), HTML (`src/serve.html`), CSS (`src/serve.css`)

**Spec:** `docs/superpowers/specs/2026-06-18-counter-display-design.md`

---

## File map

| File | Change |
|---|---|
| `src/serve.rs` | Add `CounterView`, `counter_to_view()`; extend `CardView` + `PlayerView`; populate in `to_card_view` and `build_player_view`; add `CounterKind` to imports; add `counters: vec![]` to stack `CardView` construction |
| `src/serve.html` | Add `#p1-poison` and `#p2-poison` spans in player headers |
| `src/serve.css` | Add `.hex-counter`, `.hex-{kind}`, `.card-counters`, `.poison-counter`, `.tooltip-counters`, `.counter-row`, `.counter-label`, `.counter-sublabel` rules |
| `src/serve.js` | Add `countersSectionHTML()`; update `tooltipHTML()` to render `extraSections` before `tags`; update `cardHTML()` for badge row and tooltip; update `render()` for poison counter spans |

---

## Task 1: Add `CounterView` and `counter_to_view` to serve.rs

**Files:**
- Modify: `src/serve.rs`

- [ ] **Step 1: Write failing unit tests for `counter_to_view`**

Add inside the `#[cfg(test)] mod tests` block at the bottom of `src/serve.rs`:

```rust
#[test]
fn counter_to_view_plus_modifier() {
    let v = counter_to_view(&CounterKind::PtModifier { power: 1, toughness: 1 }, 3);
    assert_eq!(v.label, "+1/+1");
    assert_eq!(v.kind, "plus");
    assert_eq!(v.count, 3);
    assert_eq!(v.sublabel.as_deref(), Some("+3/+3 to P/T"));
}

#[test]
fn counter_to_view_minus_modifier() {
    let v = counter_to_view(&CounterKind::PtModifier { power: -1, toughness: -1 }, 2);
    assert_eq!(v.label, "-1/-1");
    assert_eq!(v.kind, "minus");
    assert_eq!(v.count, 2);
    assert_eq!(v.sublabel.as_deref(), Some("-2/-2 to P/T"));
}

#[test]
fn counter_to_view_mixed_modifier() {
    let v = counter_to_view(&CounterKind::PtModifier { power: 2, toughness: -1 }, 3);
    assert_eq!(v.label, "+2/-1");
    assert_eq!(v.kind, "mixed");
    assert_eq!(v.sublabel.as_deref(), Some("+6/-3 to P/T"));
}

#[test]
fn counter_to_view_poison() {
    let v = counter_to_view(&CounterKind::Poison, 5);
    assert_eq!(v.label, "Poison");
    assert_eq!(v.kind, "poison");
    assert_eq!(v.count, 5);
    assert!(v.sublabel.is_none());
}

#[test]
fn counter_to_view_named_capitalizes_first_letter() {
    let v = counter_to_view(&CounterKind::Named("charge".to_string()), 4);
    assert_eq!(v.label, "Charge");
    assert_eq!(v.kind, "named");
    assert!(v.sublabel.is_none());
}

#[test]
fn counter_to_view_named_already_capitalized() {
    let v = counter_to_view(&CounterKind::Named("Time".to_string()), 1);
    assert_eq!(v.label, "Time");
}
```

- [ ] **Step 2: Run tests — expect compile errors (functions not yet defined)**

```bash
cargo test 2>&1 | grep -E "^test result|FAILED|error\["
```

Expected: compile error on `counter_to_view` not found.

- [ ] **Step 3: Add `CounterView` struct and `counter_to_view` to serve.rs**

Add `CounterKind` to the existing use statement near the top of `src/serve.rs`:

```rust
use mecha_oracle::types::{CardObject, CounterKind, GameState, ObjectId, Player, PlayerId, Step, Zone};
```

Add the struct and function in the "View model" section, after the existing view structs:

```rust
#[derive(Serialize)]
struct CounterView {
    label: String,
    kind: String,
    count: u32,
    sublabel: Option<String>,
}

fn counter_to_view(kind: &CounterKind, count: u32) -> CounterView {
    match kind {
        CounterKind::PtModifier { power, toughness } => {
            let label = format!("{:+}/{:+}", power, toughness);
            let kind_str = if *power > 0 && *toughness > 0 {
                "plus"
            } else if *power < 0 && *toughness < 0 {
                "minus"
            } else {
                "mixed"
            };
            let net_p = *power * count as i32;
            let net_t = *toughness * count as i32;
            CounterView {
                label,
                kind: kind_str.to_string(),
                count,
                sublabel: Some(format!("{:+}/{:+} to P/T", net_p, net_t)),
            }
        }
        CounterKind::Poison => CounterView {
            label: "Poison".to_string(),
            kind: "poison".to_string(),
            count,
            sublabel: None,
        },
        CounterKind::Named(name) => {
            let label = {
                let mut chars = name.chars();
                match chars.next() {
                    None => String::new(),
                    Some(first) => first.to_uppercase().collect::<String>() + chars.as_str(),
                }
            };
            CounterView {
                label,
                kind: "named".to_string(),
                count,
                sublabel: None,
            }
        }
    }
}
```

- [ ] **Step 4: Run tests — expect pass**

```bash
cargo test 2>&1 | grep -E "^test result|FAILED|error\["
```

Expected: `test result: ok`

- [ ] **Step 5: Commit**

```bash
git add src/serve.rs
git commit -m "feat: add CounterView and counter_to_view to serve.rs"
```

---

## Task 2: Extend CardView and PlayerView with counter data

**Files:**
- Modify: `src/serve.rs`

- [ ] **Step 1: Write failing integration tests**

Add to the `#[cfg(test)] mod tests` block:

```rust
#[test]
fn card_view_includes_counters_for_permanent() {
    use mecha_oracle::types::{CardObject, CounterKind, Zone};
    let db = test_db();
    let config = vec![
        (0..10).map(|_| "Forest".to_string()).collect(),
        (0..10).map(|_| "Forest".to_string()).collect(),
    ];
    let mut gs = build_game_state(config, &db, false).unwrap();

    let id = gs.alloc_id();
    let obj = CardObject::new(
        id,
        db.get("Grizzly Bears").unwrap().clone(),
        PlayerId(0),
        Zone::Battlefield,
    );
    let mut perm = PermanentState::new(&obj.definition);
    perm.add_counters(CounterKind::PtModifier { power: 1, toughness: 1 }, 3);
    perm.add_counters(CounterKind::Named("charge".to_string()), 2);
    gs.battlefield.insert(id, perm);
    gs.add_object(obj);

    let view = build_game_view(&gs);
    let card = view.p1.creatures.iter().find(|c| c.id == id).unwrap();
    assert_eq!(card.counters.len(), 2);

    let plus = card.counters.iter().find(|c| c.kind == "plus").unwrap();
    assert_eq!(plus.label, "+1/+1");
    assert_eq!(plus.count, 3);
    assert_eq!(plus.sublabel.as_deref(), Some("+3/+3 to P/T"));

    let named = card.counters.iter().find(|c| c.kind == "named").unwrap();
    assert_eq!(named.label, "Charge");
    assert_eq!(named.count, 2);
}

#[test]
fn card_view_empty_counters_when_none() {
    use mecha_oracle::types::{CardObject, Zone};
    let db = test_db();
    let config = vec![
        (0..10).map(|_| "Forest".to_string()).collect(),
        (0..10).map(|_| "Forest".to_string()).collect(),
    ];
    let mut gs = build_game_state(config, &db, false).unwrap();

    let id = gs.alloc_id();
    let obj = CardObject::new(
        id,
        db.get("Grizzly Bears").unwrap().clone(),
        PlayerId(0),
        Zone::Battlefield,
    );
    let perm = PermanentState::new(&obj.definition);
    gs.battlefield.insert(id, perm);
    gs.add_object(obj);

    let view = build_game_view(&gs);
    let card = view.p1.creatures.iter().find(|c| c.id == id).unwrap();
    assert!(card.counters.is_empty());
}

#[test]
fn player_view_includes_poison_counters() {
    use mecha_oracle::types::CounterKind;
    let db = test_db();
    let config = vec![
        (0..10).map(|_| "Forest".to_string()).collect(),
        (0..10).map(|_| "Forest".to_string()).collect(),
    ];
    let mut gs = build_game_state(config, &db, false).unwrap();
    gs.get_player_mut(PlayerId(0)).unwrap().add_counters(CounterKind::Poison, 4);

    let view = build_game_view(&gs);
    assert_eq!(view.p1.poison_counters, 4);
    assert_eq!(view.p2.poison_counters, 0);
}
```

- [ ] **Step 2: Run tests — expect compile error on missing fields**

```bash
cargo test 2>&1 | grep -E "^test result|FAILED|error\["
```

Expected: compile errors about missing `counters` and `poison_counters` fields.

- [ ] **Step 3: Add `counters` to `CardView` and `poison_counters` to `PlayerView`**

In the `CardView` struct, add after `actions`:

```rust
counters: Vec<CounterView>,
```

In the `PlayerView` struct, add after `graveyard`:

```rust
poison_counters: u32,
```

- [ ] **Step 4: Populate `counters` in `to_card_view` and `poison_counters` in `build_player_view`**

In the `to_card_view` closure inside `build_player_view`, add after `actions: compute_actions(...)`:

```rust
counters: perm
    .map(|p| {
        p.counters
            .iter()
            .map(|(kind, &count)| counter_to_view(kind, count))
            .collect()
    })
    .unwrap_or_default(),
```

In `build_player_view`, add `poison_counters` to the `PlayerView { .. }` construction after `graveyard`:

```rust
poison_counters: player.counter_count(&CounterKind::Poison),
```

- [ ] **Step 5: Add `counters: vec![]` to the stack spell `CardView` construction**

In `build_game_view`, inside the `StackPayload::Spell` arm, find the `CardView { .. }` literal (around line 776) and add:

```rust
counters: vec![],
```

after `actions: vec![]`.

- [ ] **Step 6: Run tests — expect pass**

```bash
cargo test 2>&1 | grep -E "^test result|FAILED|error\["
```

Expected: `test result: ok`

- [ ] **Step 7: Clippy clean**

```bash
cargo clippy --all-targets 2>&1 | grep -E "^error|^warning"
```

Fix any warnings. Run `cargo clippy --fix` first if there are straightforward mechanical fixes.

- [ ] **Step 8: Commit**

```bash
git add src/serve.rs
git commit -m "feat: add counters to CardView and poison_counters to PlayerView"
```

---

## Task 3: HTML and CSS changes

**Files:**
- Modify: `src/serve.html`
- Modify: `src/serve.css`

- [ ] **Step 1: Add poison counter spans to `serve.html`**

In the P2 player header (around line 15), add a `poison-counter` span immediately after `id="p2-life"`:

```html
<span class="life p2" id="p2-life">♥ 20</span>
<span class="poison-counter" id="p2-poison"></span>
```

In the P1 player header (around line 43), add the same after `id="p1-life"`:

```html
<span class="life p1" id="p1-life">♥ 20</span>
<span class="poison-counter" id="p1-poison"></span>
```

- [ ] **Step 2: Add CSS rules to `serve.css`**

Append to the end of `src/serve.css`:

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

- [ ] **Step 3: Commit**

```bash
git add src/serve.html src/serve.css
git commit -m "feat: add counter badge HTML and CSS"
```

---

## Task 4: JS — counter badge row, tooltip section, and poison counter rendering

**Files:**
- Modify: `src/serve.js`

- [ ] **Step 1: Add `countersSectionHTML` function**

Add after the existing `sourceSectionHTML` function (around line 312):

```js
function countersSectionHTML(counters) {
  if (!counters || !counters.length) return '';
  const rows = counters.map(c => {
    const sub = c.sublabel
      ? `<span class="counter-sublabel">${esc(c.sublabel)}</span>`
      : '';
    return `<div class="counter-row">` +
      `<span class="hex-counter hex-${esc(c.kind)}" style="width:16px;height:16px;font-size:8px">${esc(String(c.count))}</span>` +
      `<span class="counter-label"><b>${esc(String(c.count))}× ${esc(c.label)}</b>${sub}</span>` +
      `</div>`;
  }).join('');
  return `<div class="tooltip-counters">` +
    `<div class="tooltip-counters-label">Counters</div>${rows}</div>`;
}
```

- [ ] **Step 2: Update `tooltipHTML` to render `extraSections` before `tags`**

Find the `tooltipHTML` function. Change the template so `extraSections` renders before `tags`:

```js
function tooltipHTML({ name, manaCost, typeLine, oracleHtml, pt, tags, extraSections }) {
  return `
    <div class="tooltip">
      <div class="tooltip-name">${esc(name)}</div>
      ${manaCost ? `<div class="tooltip-cost">${renderManaSymbols(manaCost)}</div>` : ''}
      <div class="tooltip-type">${esc(typeLine)}</div>
      ${oracleHtml ? `<div class="tooltip-text">${oracleHtml}</div>` : ''}
      ${pt ? `<div class="tooltip-pt">${pt}</div>` : ''}
      ${extraSections && extraSections.length ? extraSections.join('') : ''}
      ${tags && tags.length ? `<div class="tooltip-tags">${tags.join('')}</div>` : ''}
    </div>`;
}
```

(Only the last two `${...}` lines swap order compared to the original.)

- [ ] **Step 3: Update `cardHTML` — add counter badge row and pass counters to tooltip**

Find the `tooltip` variable construction in `cardHTML`. Change it to pass `countersSectionHTML`:

```js
  const tooltip = tooltipHTML({
    name: card.name,
    manaCost: card.mana_cost,
    typeLine: card.type_line,
    oracleHtml: card.oracle_text ? renderOracleText(card) : '',
    pt: card.power != null ? `${card.power} / ${card.toughness}` : null,
    tags,
    extraSections: [countersSectionHTML(card.counters || [])],
  });
```

Then update the return statement to insert `.card-counters` between the card face and the tooltip:

```js
  const counterBadges = (card.counters && card.counters.length)
    ? `<div class="card-counters">${card.counters.map(c =>
        `<span class="hex-counter hex-${esc(c.kind)}" title="${esc(c.label)}">${esc(String(c.count))}</span>`
      ).join('')}</div>`
    : '';

  return `<div class="${wrap}"><div class="${classes}" data-id="${card.id}" ${clickAttr} ${cardStyle}>
    <span class="card-name">${esc(card.name)}</span>
    ${card.mana_cost ? `<span class="card-cost">${renderManaSymbols(card.mana_cost)}</span>` : ''}
    <span class="card-type">${esc(card.type_line)}</span>
    ${pt}
  </div>${counterBadges}${tooltip}</div>`;
```

- [ ] **Step 4: Update `render()` to populate poison counter spans**

In the `render(s)` function, add after the life total lines (`document.getElementById('p1-life')...`):

```js
  const p1Poison = document.getElementById('p1-poison');
  const p2Poison = document.getElementById('p2-poison');
  if (p1Poison) p1Poison.textContent = s.p1.poison_counters > 0 ? `☠ ${s.p1.poison_counters}` : '';
  if (p2Poison) p2Poison.textContent = s.p2.poison_counters > 0 ? `☠ ${s.p2.poison_counters}` : '';
```

- [ ] **Step 5: Commit**

```bash
git add src/serve.js
git commit -m "feat: render counter badges and poison counter in UI"
```

---

## Task 5: Verify end-to-end

**Files:** none changed

- [ ] **Step 1: Run full test suite**

```bash
cargo test 2>&1 | grep -E "^test result|FAILED|error\["
```

Expected: `test result: ok`

- [ ] **Step 2: Run clippy**

```bash
cargo clippy --all-targets 2>&1 | grep -E "^error|^warning"
```

Expected: no warnings.

- [ ] **Step 3: Start the server and manually verify counter display**

```bash
cargo run -- --deck docs/test-decks/green_abilities.json 2>/dev/null &
```

Open http://localhost:3000 and verify:
- Cards with counters (if any in the test deck) show hexagon badges below the card face
- Hovering a card with counters shows a "Counters" section in the tooltip
- The poison counter span stays empty for both players at game start (no noise in normal gameplay)

Kill the server after verification: `kill %1`
