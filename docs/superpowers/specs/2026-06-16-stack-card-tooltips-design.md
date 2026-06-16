# Stack Card Hover Tooltips Design

**Date:** 2026-06-16
**Scope:** Make stack items (spells, triggered abilities, activated abilities) show the same hover-tooltip pattern already used for cards in hand/battlefield/graveyard, plus a new "Targets" section, plus a "Source" line for non-spell stack items. No new interactivity (click) is added to stack cards — hover only.

---

## Background

Cards in hand, on the battlefield, and in the graveyard modal all render via `cardHTML()` (`src/serve.js`), which nests a `.tooltip` div inside a `.card-wrap` element. A global `mouseover` listener (`src/serve.js:646-664`) positions the `.tooltip` next to whichever `.card-wrap` is hovered, and `.card-wrap:hover .tooltip { display: block; }` (`src/serve.css:86`) shows it.

Stack items render via `renderStack()` (`src/serve.js:671-741`) into `.stack-card` elements. These currently have `pointer-events: none` (`src/serve.css:238`), so hover never fires, and they carry no tooltip markup at all. `StackObject` (`src/types/stack.rs:31-38`) already tracks `targets: Vec<EffectTarget>`, but `StackItemView` (`src/serve.rs:142-150`), the JSON view sent to the frontend, has no field for it.

---

## Data Model — no changes

`StackObject.targets` and `EffectTarget` are unchanged. `StackPayload::TriggeredAbility`/`ActivatedAbility` already carry `source_id: ObjectId` (`src/types/stack.rs:16,22`), which is enough to resolve a source card name.

---

## Serve Layer (`src/serve.rs`)

### Shared target-name resolver

Extract the inline match currently duplicated at `src/serve.rs:474-498` (used when labeling per-target cast actions) into:

```rust
fn target_display_name(state: &GameState, target: &EffectTarget) -> String {
    match target {
        EffectTarget::Object { id } => state.objects.get(id)
            .map(|o| o.definition.name.clone()).unwrap_or_default(),
        EffectTarget::Player { id } => state.get_player(*id)
            .map(|p| p.name.clone()).unwrap_or_default(),
        EffectTarget::StackObject { id } => state.stack_objects.get(id)
            .and_then(|obj| match &obj.payload {
                StackPayload::Spell { card_id } => state.objects.get(card_id)
                    .map(|c| c.definition.name.clone()),
                _ => None,
            })
            .unwrap_or_default(),
    }
}
```

The existing call site at `src/serve.rs:474-498` is rewritten to call this helper instead of inlining the match.

### `StackItemView` (`src/serve.rs:142-150`) — two new fields

```rust
#[derive(Serialize)]
struct StackItemView {
    id: u64,
    kind: String,
    label: String,
    controller: PlayerId,
    card: Option<CardView>,
    #[serde(skip_serializing_if = "Option::is_none")]
    cost_label: Option<String>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    targets: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    source_name: Option<String>,
}
```

- `targets`: `obj.targets.iter().map(|t| target_display_name(state, t)).collect()`. Same computation for all three `StackItemView` match arms in `build_game_view` (`src/serve.rs:726-780`).
- `source_name`: `None` for `StackPayload::Spell` (the `card` field already identifies it). For `TriggeredAbility { source_id, .. }` and `ActivatedAbility { source_id, .. }`, resolve `state.objects.get(source_id).map(|o| o.definition.name.clone())`.

`cost_label` stays as-is (already always `None`, untouched by this change — out of scope).

---

## Frontend (`src/serve.js`)

### Shared tooltip builder

Factor the tooltip markup currently inlined in `cardHTML()` (`src/serve.js:280-288`) into a standalone function:

```js
function tooltipHTML({ name, manaCost, typeLine, oracleHtml, pt, tags, extraSections }) {
  return `
    <div class="tooltip">
      <div class="tooltip-name">${esc(name)}</div>
      ${manaCost ? `<div class="tooltip-cost">${esc(manaCost)}</div>` : ''}
      <div class="tooltip-type">${esc(typeLine)}</div>
      ${oracleHtml ? `<div class="tooltip-text">${oracleHtml}</div>` : ''}
      ${pt != null ? `<div class="tooltip-pt">${pt}</div>` : ''}
      ${tags && tags.length ? `<div class="tooltip-tags">${tags.join('')}</div>` : ''}
      ${extraSections ? extraSections.join('') : ''}
    </div>`;
}
```

`cardHTML()` calls this with its existing fields (`extraSections` omitted/undefined). This is the "consistent card display" cleanup the request asked for — one tooltip code path for every card-shaped thing in the UI.

### Targets / source extra sections

```js
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
```

### `renderStack()` (`src/serve.js:671-741`)

When building a new stack card element (currently `src/serve.js:713-718`):

1. Add `card-wrap` to `el.className` alongside `stack-card p1`/`p2` (no extra wrapper DOM node — the element keeps its existing absolute-position/transform animation untouched).
2. Build the tooltip:
   - **Spell** (`item.card` present): call `tooltipHTML()` with the same fields `cardHTML()` would use for `item.card` (name, mana_cost, type_line, oracle via `renderOracleText(item.card)`, power/toughness), plus `extraSections: [targetsSectionHTML(item.targets)]`. (No `sourceSectionHTML` for spells — `source_name` is always absent.)
   - **Ability** (`item.card` is null): call `tooltipHTML()` with `name: item.label`, no `manaCost`, `typeLine: item.kind === 'activated_ability' ? 'Activated Ability' : 'Triggered Ability'`, no `oracleHtml`, no `pt`, no `tags`, `extraSections: [sourceSectionHTML(item.source_name), targetsSectionHTML(item.targets)]`.
3. Append the resulting tooltip HTML string to `el.innerHTML` alongside the existing name/kind spans (same nesting pattern as `cardHTML()`'s `${tooltip}` inside the wrap).

No change to the staggered positioning/animation logic (`src/serve.js:699-740`) — the global `mouseover` listener already works against `.card-wrap` regardless of its other classes or transform state.

---

## CSS (`src/serve.css`)

- Remove `pointer-events: none;` from `.stack-card` (`src/serve.css:238`) — this is the only thing currently preventing hover from firing. `user-select: none` stays.
- Add styles for the new tooltip sections, visually consistent with the existing `.tooltip-pt`/`.tooltip-tags` block:
  ```css
  .tooltip-source { font-size: 10px; color: #888; margin-bottom: 6px; }
  .tooltip-targets-label { font-size: 10px; color: #888; margin-bottom: 2px; }
  .tooltip-target { font-size: 11px; color: #ddd; padding-left: 8px; }
  ```

---

## Out of scope

- No click/interactivity added to stack cards — hover-only, matching the request.
- `x_value` (CR 107.4) is not surfaced in any tooltip — not shown anywhere in the UI today; a separate feature if wanted later.
- `cost_label` on `StackItemView` is untouched (already dead/always `None`).

---

## Testing

This is a presentation-layer change with no engine/rules logic. Verification is manual + `cargo clippy --all-targets` clean:

- Start the server, cast a targeted spell (e.g. Lightning Bolt at a creature), confirm hovering the stack card shows name/cost/type/oracle text and a "Targets:" line with the creature's name.
- Cast an untargeted spell, confirm no "Targets:" section appears.
- Trigger a triggered ability (e.g. Prowess) and confirm the tooltip shows "Source: <card name>" and the ability kind, with no oracle text/mana cost.
- Activate an activated ability with a target, confirm "Source:" and "Targets:" both appear.
- Confirm tooltip positioning still works correctly for stack cards near screen edges (reuses existing positioning logic, but worth a visual spot-check since stack cards sit center-screen).
