# Stack Card Hover Tooltips Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Stack items (spells, triggered abilities, activated abilities) show the same hover tooltip as cards elsewhere in the UI, plus a "Targets" line and (for non-spell stack items) a "Source" line.

**Architecture:** Backend (`src/serve.rs`) adds `targets: Vec<String>` and `source_name: Option<String>` to the `StackItemView` JSON view, resolved via a new shared helper `target_display_name`. Frontend (`src/serve.js`, `src/serve.css`) factors the existing tooltip markup out of `cardHTML()` into a reusable `tooltipHTML()` function, reuses it for stack cards by tagging each `.stack-card` element with the `card-wrap` class (no new DOM node), and removes the `pointer-events: none` that currently blocks hover.

**Tech Stack:** Rust (axum) backend, vanilla JS/CSS frontend, no JS test framework in this repo — frontend changes are verified manually in a browser.

**Spec:** `docs/superpowers/specs/2026-06-16-stack-card-tooltips-design.md`

---

### Task 1: Extract `target_display_name` helper in `src/serve.rs`

**Files:**
- Modify: `src/serve.rs:460-499` (existing inline target-name match, inside the per-target action-building loop)
- Test: `src/serve.rs` (inside `#[cfg(test)] mod tests` at the bottom of the file)

This is a pure refactor — same behavior, now reusable from the stack-view code in Task 2.

- [ ] **Step 1: Write the failing test**

Add to the `mod tests` block at the bottom of `src/serve.rs` (after the existing tests, before the closing `}` of `mod tests`):

```rust
#[test]
fn target_display_name_resolves_each_target_kind() {
    use mecha_oracle::types::stack::{StackObject, StackPayload};

    let config = vec![
        (0..10).map(|_| "Forest".to_string()).collect(),
        (0..10).map(|_| "Forest".to_string()).collect(),
    ];
    let db = test_db();
    let mut gs = build_game_state(config, &db, false).unwrap();

    // Object target: a creature on the battlefield
    let creature_id = gs.alloc_id();
    let creature = CardObject::new(
        creature_id,
        db.get("Grizzly Bears").unwrap().clone(),
        PlayerId(0),
        Zone::Battlefield,
    );
    let perm = PermanentState::new(&creature.definition);
    gs.battlefield.insert(creature_id, perm);
    gs.add_object(creature);

    // StackObject target: a spell already on the stack
    let spell_card_id = gs.alloc_id();
    let spell_card = CardObject::new(
        spell_card_id,
        db.get("Lightning Bolt").unwrap().clone(),
        PlayerId(1),
        Zone::Stack,
    );
    gs.add_object(spell_card);
    let spell_stack_id = gs.alloc_stack_id();
    gs.stack.push(spell_stack_id);
    gs.stack_objects.insert(
        spell_stack_id,
        StackObject {
            id: spell_stack_id,
            payload: StackPayload::Spell {
                card_id: spell_card_id,
            },
            controller: PlayerId(1),
            targets: vec![],
            x_value: None,
        },
    );

    assert_eq!(
        target_display_name(&gs, &EffectTarget::Object { id: creature_id }),
        "Grizzly Bears"
    );
    assert_eq!(
        target_display_name(&gs, &EffectTarget::Player { id: PlayerId(1) }),
        "Player 2"
    );
    assert_eq!(
        target_display_name(&gs, &EffectTarget::StackObject { id: spell_stack_id }),
        "Lightning Bolt"
    );
}
```

- [ ] **Step 2: Run test to verify it fails to compile**

Run: `cargo test target_display_name_resolves_each_target_kind 2>&1 | grep -E "^test result|FAILED|error\["`

Expected: compile error, `target_display_name` not found.

- [ ] **Step 3: Add the helper function and use it at the existing call site**

Add this function above `fn build_game_view` in `src/serve.rs` (i.e. just before line 725, the `fn build_game_view` definition):

```rust
fn target_display_name(state: &GameState, target: &EffectTarget) -> String {
    match target {
        EffectTarget::Object { id } => state
            .objects
            .get(id)
            .map(|o| o.definition.name.clone())
            .unwrap_or_default(),
        EffectTarget::Player { id } => state
            .get_player(*id)
            .map(|p| p.name.clone())
            .unwrap_or_default(),
        EffectTarget::StackObject { id } => state
            .stack_objects
            .get(id)
            .and_then(|obj| match &obj.payload {
                StackPayload::Spell { card_id } => {
                    state.objects.get(card_id).map(|c| c.definition.name.clone())
                }
                _ => None,
            })
            .unwrap_or_default(),
    }
}
```

Then in `src/serve.rs`, replace the existing inline match (currently lines 474-498):

```rust
                    let target_name = match &target {
                        EffectTarget::Object { id } => state
                            .objects
                            .get(id)
                            .map(|o| o.definition.name.clone())
                            .unwrap_or_default(),
                        EffectTarget::Player { id } => state
                            .get_player(*id)
                            .map(|p| p.name.clone())
                            .unwrap_or_default(),
                        EffectTarget::StackObject { id } => state
                            .stack_objects
                            .get(id)
                            .and_then(|obj| {
                                if let StackPayload::Spell { card_id } = &obj.payload {
                                    state
                                        .objects
                                        .get(card_id)
                                        .map(|c| c.definition.name.clone())
                                } else {
                                    None
                                }
                            })
                            .unwrap_or_default(),
                    };
```

with:

```rust
                    let target_name = target_display_name(state, &target);
```

- [ ] **Step 4: Run test to verify it passes, and run the full suite to check for regressions**

Run: `cargo test 2>&1 | grep -E "^test result|FAILED|error\["`

Expected: `target_display_name_resolves_each_target_kind` passes; no other test changes status (this is a pure refactor of existing logic).

- [ ] **Step 5: Commit**

```bash
git add src/serve.rs
git commit -m "refactor: extract target_display_name helper in serve.rs"
```

---

### Task 2: Add `targets` and `source_name` to `StackItemView`

**Files:**
- Modify: `src/serve.rs:141-150` (`StackItemView` struct)
- Modify: `src/serve.rs:725-780` (`build_game_view`, the `stack` construction)
- Test: `src/serve.rs` (`mod tests`)

- [ ] **Step 1: Write the failing tests**

Add to `mod tests` in `src/serve.rs`:

```rust
#[test]
fn stack_item_view_includes_targets_and_source_name_for_ability() {
    use mecha_oracle::types::effect::EffectStep;
    use mecha_oracle::types::stack::{StackObject, StackPayload};

    let config = vec![
        (0..10).map(|_| "Forest".to_string()).collect(),
        (0..10).map(|_| "Forest".to_string()).collect(),
    ];
    let db = test_db();
    let mut gs = build_game_state(config, &db, false).unwrap();

    let source_id = gs.alloc_id();
    let source = CardObject::new(
        source_id,
        db.get("Grizzly Bears").unwrap().clone(),
        PlayerId(0),
        Zone::Battlefield,
    );
    let perm = PermanentState::new(&source.definition);
    gs.battlefield.insert(source_id, perm);
    gs.add_object(source);

    let stack_id = gs.alloc_stack_id();
    let stack_obj = StackObject {
        id: stack_id,
        payload: StackPayload::ActivatedAbility {
            source_id,
            effect: vec![EffectStep::DealDamage(2)],
            label: "Grizzly Bears: activated ability".into(),
        },
        controller: PlayerId(0),
        targets: vec![EffectTarget::Player { id: PlayerId(1) }],
        x_value: None,
    };
    gs.stack.push(stack_id);
    gs.stack_objects.insert(stack_id, stack_obj);

    let view = build_game_view(&gs);
    assert_eq!(view.stack.len(), 1);
    let item = &view.stack[0];
    assert_eq!(item.targets, vec!["Player 2".to_string()]);
    assert_eq!(item.source_name, Some("Grizzly Bears".to_string()));
}

#[test]
fn stack_item_view_includes_source_name_for_triggered_ability() {
    use mecha_oracle::types::effect::EffectStep;
    use mecha_oracle::types::stack::{StackObject, StackPayload};

    let config = vec![
        (0..10).map(|_| "Forest".to_string()).collect(),
        (0..10).map(|_| "Forest".to_string()).collect(),
    ];
    let db = test_db();
    let mut gs = build_game_state(config, &db, false).unwrap();

    let source_id = gs.alloc_id();
    let source = CardObject::new(
        source_id,
        db.get("Serra Angel").unwrap().clone(),
        PlayerId(0),
        Zone::Battlefield,
    );
    let perm = PermanentState::new(&source.definition);
    gs.battlefield.insert(source_id, perm);
    gs.add_object(source);

    let stack_id = gs.alloc_stack_id();
    let stack_obj = StackObject {
        id: stack_id,
        payload: StackPayload::TriggeredAbility {
            source_id,
            effect: vec![EffectStep::DealDamage(1)],
            label: "Prowess".into(),
        },
        controller: PlayerId(0),
        targets: vec![],
        x_value: None,
    };
    gs.stack.push(stack_id);
    gs.stack_objects.insert(stack_id, stack_obj);

    let view = build_game_view(&gs);
    let item = &view.stack[0];
    assert!(item.targets.is_empty());
    assert_eq!(item.source_name, Some("Serra Angel".to_string()));
}

#[test]
fn stack_item_view_spell_has_no_source_name() {
    use mecha_oracle::types::stack::{StackObject, StackPayload};

    let config = vec![
        (0..10).map(|_| "Forest".to_string()).collect(),
        (0..10).map(|_| "Forest".to_string()).collect(),
    ];
    let db = test_db();
    let mut gs = build_game_state(config, &db, false).unwrap();

    let card_id = gs.alloc_id();
    let card = CardObject::new(
        card_id,
        db.get("Lightning Bolt").unwrap().clone(),
        PlayerId(0),
        Zone::Stack,
    );
    gs.add_object(card);
    let stack_id = gs.alloc_stack_id();
    let stack_obj = StackObject {
        id: stack_id,
        payload: StackPayload::Spell { card_id },
        controller: PlayerId(0),
        targets: vec![],
        x_value: None,
    };
    gs.stack.push(stack_id);
    gs.stack_objects.insert(stack_id, stack_obj);

    let view = build_game_view(&gs);
    let item = &view.stack[0];
    assert!(item.targets.is_empty());
    assert_eq!(item.source_name, None);
}
```

- [ ] **Step 2: Run tests to verify they fail to compile**

Run: `cargo test stack_item_view 2>&1 | grep -E "^test result|FAILED|error\["`

Expected: compile error — `targets`/`source_name` fields don't exist on `StackItemView`.

- [ ] **Step 3: Add the fields to `StackItemView`**

In `src/serve.rs`, replace the struct at line 141-150:

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
}
```

with:

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

- [ ] **Step 4: Populate the new fields in `build_game_view`**

In `src/serve.rs`, the `stack` construction (lines 726-780) builds one `StackItemView` per match arm. Replace the whole `let stack: Vec<StackItemView> = ...` block with:

```rust
    let stack: Vec<StackItemView> = state
        .stack
        .iter()
        .map(|&sid| {
            let obj = &state.stack_objects[&sid];
            let targets: Vec<String> = obj
                .targets
                .iter()
                .map(|t| target_display_name(state, t))
                .collect();
            match &obj.payload {
                StackPayload::Spell { card_id } => {
                    let card = state.objects.get(card_id);
                    StackItemView {
                        id: sid.0,
                        kind: "spell".into(),
                        label: card.map(|c| c.definition.name.clone()).unwrap_or_default(),
                        controller: obj.controller,
                        card: card.map(|c| CardView {
                            id: c.id,
                            name: c.definition.name.clone(),
                            type_line: format_type_line(&c.definition.type_line),
                            oracle_text: c.definition.oracle_text.clone(),
                            text_annotations: annotation_views(
                                &c.definition.oracle_text,
                                &c.definition.text_annotations,
                            ),
                            mana_cost: c.definition.mana_cost.as_ref().map(format_mana_cost),
                            power: c.definition.power,
                            toughness: c.definition.toughness,
                            colors: c.definition.colors.iter().map(|c| c.to_string()).collect(),
                            tapped: false,
                            summoning_sick: false,
                            damage_marked: 0,
                            is_attacking: false,
                            is_blocking: false,
                            actions: vec![],
                        }),
                        cost_label: None,
                        targets,
                        source_name: None,
                    }
                }
                StackPayload::TriggeredAbility { label, source_id, .. } => StackItemView {
                    id: sid.0,
                    kind: "triggered_ability".into(),
                    label: label.clone(),
                    controller: obj.controller,
                    card: None,
                    cost_label: None,
                    targets,
                    source_name: state
                        .objects
                        .get(source_id)
                        .map(|o| o.definition.name.clone()),
                },
                StackPayload::ActivatedAbility { label, source_id, .. } => StackItemView {
                    id: sid.0,
                    kind: "activated_ability".into(),
                    label: label.clone(),
                    controller: obj.controller,
                    card: None,
                    cost_label: None,
                    targets,
                    source_name: state
                        .objects
                        .get(source_id)
                        .map(|o| o.definition.name.clone()),
                },
            }
        })
        .collect();
```

- [ ] **Step 5: Run tests to verify they pass**

Run: `cargo test 2>&1 | grep -E "^test result|FAILED|error\["`

Expected: all 3 new tests pass, no regressions.

- [ ] **Step 6: Commit**

```bash
git add src/serve.rs
git commit -m "feat: surface stack item targets and ability source name to UI"
```

---

### Task 3: Frontend — shared tooltip builder and hoverable stack cards

**Files:**
- Modify: `src/serve.js:249-300` (`cardHTML`, tooltip block)
- Modify: `src/serve.js:671-741` (`renderStack`)
- Modify: `src/serve.css:223-256` (`.stack-card`)
- Modify: `src/serve.css:78-98` (tooltip section, append new rules)

No JS test framework exists in this repo — verification for this task is manual (Task 4 covers it). This task is implementation only.

- [ ] **Step 1: Add `tooltipHTML`, `targetsSectionHTML`, `sourceSectionHTML` helpers**

In `src/serve.js`, add these three functions directly above `function cardHTML(card, s, pid, zone) {` (currently line 249):

```js
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
```

- [ ] **Step 2: Make `cardHTML` use `tooltipHTML`**

In `src/serve.js`, replace the inline tooltip block (currently lines 280-288):

```js
  const tooltip = `
    <div class="tooltip">
      <div class="tooltip-name">${esc(card.name)}</div>
      ${card.mana_cost ? `<div class="tooltip-cost">${esc(card.mana_cost)}</div>` : ''}
      <div class="tooltip-type">${esc(card.type_line)}</div>
      ${card.oracle_text ? `<div class="tooltip-text">${renderOracleText(card)}</div>` : ''}
      ${card.power != null ? `<div class="tooltip-pt">${card.power} / ${card.toughness}</div>` : ''}
      ${tags.length ? `<div class="tooltip-tags">${tags.join('')}</div>` : ''}
    </div>`;
```

with:

```js
  const tooltip = tooltipHTML({
    name: card.name,
    manaCost: card.mana_cost,
    typeLine: card.type_line,
    oracleHtml: card.oracle_text ? renderOracleText(card) : '',
    pt: card.power != null ? `${card.power} / ${card.toughness}` : null,
    tags,
  });
```

- [ ] **Step 3: Start the dev server and confirm hand/battlefield tooltips still render identically**

Run: `cargo run` (or use the project's `run` skill) to start the server, open it in a browser, and hover a card in hand. Confirm the tooltip looks unchanged from before this refactor (name, cost, type, oracle text, P/T, tags).

- [ ] **Step 4: Commit the refactor**

```bash
git add src/serve.js
git commit -m "refactor: factor tooltip markup out of cardHTML into tooltipHTML"
```

- [ ] **Step 5: Make stack cards hoverable with tooltips, in `renderStack`**

In `src/serve.js`, inside `renderStack`, replace the "new card" branch (currently lines 708-732):

```js
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
```

with:

```js
    if (!el) {
      // New card — create at entering position, then animate to final
      const kindLabel = item.kind === 'spell'              ? 'SPELL'
                      : item.kind === 'activated_ability' ? 'ACT'
                      : 'TRIG'; // triggered_ability
      el = document.createElement('div');
      el.className       = 'card-wrap stack-card ' + (item.controller === 0 ? 'p1' : 'p2');
      el.dataset.stackId = idStr;
      const tooltip = item.card
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
        `<span class="stack-kind">${kindLabel}</span>` +
        tooltip;
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
```

- [ ] **Step 6: Remove `pointer-events: none` from `.stack-card` in `src/serve.css`**

Replace (currently lines 224-240):

```css
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
```

with:

```css
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
  user-select: none;
}
```

- [ ] **Step 7: Add CSS for the new tooltip sections**

In `src/serve.css`, directly below the existing `.tooltip-tags` rule (currently line 92, `.tooltip-tags { display: flex; flex-wrap: wrap; gap: 3px; }`), add:

```css
.tooltip-source { font-size: 10px; color: #888; margin-bottom: 6px; }
.tooltip-targets-label { font-size: 10px; color: #888; margin-bottom: 2px; }
.tooltip-target { font-size: 11px; color: #ddd; padding-left: 8px; }
```

- [ ] **Step 8: Commit**

```bash
git add src/serve.js src/serve.css
git commit -m "feat: hoverable stack card tooltips with targets and source"
```

---

### Task 4: Manual verification and lint cleanup

**Files:** none (verification only)

- [ ] **Step 1: Run clippy**

Run: `cargo clippy --all-targets 2>&1 | grep -E "error|warning"`

Expected: clean (no output). If anything appears, run `cargo clippy --fix` first, then fix anything remaining by hand.

- [ ] **Step 2: Run the full test suite**

Run: `cargo test 2>&1 | grep -E "^test result|FAILED|error\["`

Expected: all tests pass, including the 4 new tests from Tasks 1-2.

- [ ] **Step 3: Manual browser verification**

Start the server with `cargo run` (the `serve` module is wired up as the default binary in `src/main.rs`), or use the project's `run` skill, then in a browser:

1. Cast a targeted spell (e.g. Lightning Bolt at a creature). Hover the stack card. Confirm: name, mana cost, type line, oracle text, and a "Targets:" line naming the creature.
2. Cast an untargeted spell. Hover its stack card. Confirm no "Targets:" section appears.
3. Get a triggered ability onto the stack (e.g. attack with a Prowess creature after casting a noncreature spell, or any other available trigger). Hover it. Confirm "Source: <card name>" appears, ability kind shown as "Triggered Ability", no oracle text/mana cost shown.
4. Activate an activated ability that has a target. Hover it. Confirm both "Source:" and "Targets:" appear.
5. Hover stack cards near the left/right/top/bottom edges of the window and confirm the tooltip still repositions itself on-screen (reuses the existing global positioning logic — should need no new code, but worth a visual spot check since stack cards sit center-screen rather than in a side panel).
6. Hover a card in hand/battlefield/graveyard one more time to confirm the Task 3 refactor didn't change their appearance.

- [ ] **Step 4: Update `docs/todo.md` if any items were resolved**

Read `docs/todo.md`. None of the current entries reference stack tooltips or targets, so no deletions are expected — but check before finishing, since the file may have changed since this plan was written.

- [ ] **Step 5: Final commit if Step 3 turned up fixes**

If manual verification in Step 3 required code changes, commit them separately with a message describing what was fixed. If no changes were needed, this step is a no-op.
