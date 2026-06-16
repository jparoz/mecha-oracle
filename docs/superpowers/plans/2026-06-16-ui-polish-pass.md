# UI Polish Pass Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Fix tooltip positioning/stacking and add mechanical-color card backgrounds and icon-rendered mana symbols to the web UI, per `docs/superpowers/specs/2026-06-16-ui-polish-pass-design.md`.

**Architecture:** Presentation-only change across three files: `src/serve.rs` (adds a `display_colors()` helper and a `source_colors` field, switches mana-cost serialization to braced notation), `src/serve.css` (CSS custom properties for the mana palette, new pip variants, one z-index fix), `src/serve.js` (background-color helpers, a unified `{token}` → icon renderer, and a rewritten tooltip-positioning listener). No engine/rules logic changes.

**Tech Stack:** Rust (axum-style serve layer, `cargo test`/`cargo clippy`), vanilla JS/CSS (no bundler, no JS test runner — `node --check` is used for syntax verification on JS-only tasks, and the final task does a manual browser check).

---

## Before you start

Run this once to confirm a clean baseline:

```bash
cargo test 2>&1 | grep -E "^test result|FAILED|error\["
```

Expected: all `test result: ok` lines, no `FAILED`/`error[`.

---

### Task 1: `display_colors()` helper in `src/serve.rs`

**Files:**
- Modify: `src/serve.rs` (add helper near `format_mana_cost`, currently at `src/serve.rs:229`)
- Test: `src/serve.rs` (inside `mod tests`, currently starting at `src/serve.rs:1161`)

- [ ] **Step 1: Write the failing tests**

Add these to the bottom of `mod tests` in `src/serve.rs` (just above the closing `}` of the module, currently line 2396):

```rust
    #[test]
    fn display_colors_uses_printed_colors_when_present() {
        use mecha_oracle::types::card::{CardDefinition, CardType, TypeLine};
        use mecha_oracle::types::mana::ManaColor;
        let def = CardDefinition {
            name: "Test Creature".into(),
            mana_cost: None,
            type_line: TypeLine {
                supertypes: vec![],
                card_types: vec![CardType::Creature],
                subtypes: vec![],
            },
            oracle_text: String::new(),
            abilities: vec![],
            text_annotations: vec![],
            power: None,
            toughness: None,
            colors: vec![ManaColor::Blue],
        };
        assert_eq!(display_colors(&def), vec![ManaColor::Blue]);
    }

    #[test]
    fn display_colors_colorless_nonland_is_empty() {
        use mecha_oracle::types::card::{CardDefinition, CardType, TypeLine};
        let def = CardDefinition {
            name: "Test Artifact".into(),
            mana_cost: None,
            type_line: TypeLine {
                supertypes: vec![],
                card_types: vec![CardType::Artifact],
                subtypes: vec![],
            },
            oracle_text: "Tap: Add {C}.".into(),
            abilities: vec![],
            text_annotations: vec![],
            power: None,
            toughness: None,
            colors: vec![],
        };
        assert_eq!(display_colors(&def), vec![]);
    }

    #[test]
    fn display_colors_land_from_single_basic_subtype() {
        use mecha_oracle::types::card::{CardDefinition, CardType, Supertype, TypeLine};
        use mecha_oracle::types::mana::ManaColor;
        let def = CardDefinition {
            name: "Test Plains".into(),
            mana_cost: None,
            type_line: TypeLine {
                supertypes: vec![Supertype::Basic],
                card_types: vec![CardType::Land],
                subtypes: vec!["Plains".into()],
            },
            oracle_text: "({T}: Add {W}.)".into(),
            abilities: vec![],
            text_annotations: vec![],
            power: None,
            toughness: None,
            colors: vec![],
        };
        assert_eq!(display_colors(&def), vec![ManaColor::White]);
    }

    #[test]
    fn display_colors_land_unions_dual_subtypes_in_wubrg_order() {
        use mecha_oracle::types::card::{CardDefinition, CardType, Supertype, TypeLine};
        use mecha_oracle::types::mana::ManaColor;
        let def = CardDefinition {
            name: "Test Swamp Forest".into(),
            mana_cost: None,
            type_line: TypeLine {
                supertypes: vec![Supertype::Basic],
                card_types: vec![CardType::Land],
                subtypes: vec!["Swamp".into(), "Forest".into()],
            },
            oracle_text: String::new(),
            abilities: vec![],
            text_annotations: vec![],
            power: None,
            toughness: None,
            colors: vec![],
        };
        assert_eq!(
            display_colors(&def),
            vec![ManaColor::Black, ManaColor::Green]
        );
    }

    #[test]
    fn display_colors_land_from_oracle_text_mana_symbol() {
        use mecha_oracle::types::card::{CardDefinition, CardType, TypeLine};
        use mecha_oracle::types::mana::ManaColor;
        let def = CardDefinition {
            name: "Test Utility Land".into(),
            mana_cost: None,
            type_line: TypeLine {
                supertypes: vec![],
                card_types: vec![CardType::Land],
                subtypes: vec!["Gate".into()],
            },
            oracle_text: "{T}: Add {U}.".into(),
            abilities: vec![],
            text_annotations: vec![],
            power: None,
            toughness: None,
            colors: vec![],
        };
        assert_eq!(display_colors(&def), vec![ManaColor::Blue]);
    }

    #[test]
    fn display_colors_land_dedupes_subtype_and_text_match() {
        use mecha_oracle::types::card::{CardDefinition, CardType, Supertype, TypeLine};
        use mecha_oracle::types::mana::ManaColor;
        let def = CardDefinition {
            name: "Test Island".into(),
            mana_cost: None,
            type_line: TypeLine {
                supertypes: vec![Supertype::Basic],
                card_types: vec![CardType::Land],
                subtypes: vec!["Island".into()],
            },
            oracle_text: "({T}: Add {U}.)".into(),
            abilities: vec![],
            text_annotations: vec![],
            power: None,
            toughness: None,
            colors: vec![],
        };
        assert_eq!(display_colors(&def), vec![ManaColor::Blue]);
    }

    #[test]
    fn display_colors_land_with_no_recognized_color_is_empty() {
        use mecha_oracle::types::card::{CardDefinition, CardType, TypeLine};
        let def = CardDefinition {
            name: "Test Wastes".into(),
            mana_cost: None,
            type_line: TypeLine {
                supertypes: vec![],
                card_types: vec![CardType::Land],
                subtypes: vec![],
            },
            oracle_text: "({T}: Add {C}.)".into(),
            abilities: vec![],
            text_annotations: vec![],
            power: None,
            toughness: None,
            colors: vec![],
        };
        assert_eq!(display_colors(&def), vec![]);
    }
```

- [ ] **Step 2: Run the tests to verify they fail**

```bash
cargo test display_colors 2>&1 | grep -E "^test result|FAILED|error\["
```

Expected: `error[E0425]: cannot find function `display_colors` in this scope` (or similar) — the function doesn't exist yet.

- [ ] **Step 3: Implement `display_colors()`**

Add this function in `src/serve.rs` directly above `fn format_mana_cost` (currently `src/serve.rs:229`):

```rust
/// CR 105.2a — lands are colorless by definition, but their *display* color (for the UI
/// only) is derived from basic land subtypes and any colored mana symbols printed in
/// their rules text, e.g. "Swamp Forest" → [Black, Green]. Non-land cards with no
/// printed colors stay colorless (CR 105.2 remains authoritative for everything else,
/// e.g. protection-from-color targeting at `legal_targets`).
fn display_colors(
    def: &mecha_oracle::types::card::CardDefinition,
) -> Vec<mecha_oracle::types::mana::ManaColor> {
    use mecha_oracle::types::mana::ManaColor;
    if !def.colors.is_empty() {
        return def.colors.clone();
    }
    if !def.type_line.is_land() {
        return vec![];
    }
    let mut colors: Vec<ManaColor> = Vec::new();
    let mut push = |c: ManaColor| {
        if !colors.contains(&c) {
            colors.push(c);
        }
    };
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
        ("{W}", ManaColor::White),
        ("{U}", ManaColor::Blue),
        ("{B}", ManaColor::Black),
        ("{R}", ManaColor::Red),
        ("{G}", ManaColor::Green),
    ] {
        if def.oracle_text.contains(needle) {
            push(color);
        }
    }
    colors
}
```

- [ ] **Step 4: Run the tests to verify they pass**

```bash
cargo test display_colors 2>&1 | grep -E "^test result|FAILED|error\["
```

Expected: 7 passing tests (`display_colors_uses_printed_colors_when_present`, `display_colors_colorless_nonland_is_empty`, `display_colors_land_from_single_basic_subtype`, `display_colors_land_unions_dual_subtypes_in_wubrg_order`, `display_colors_land_from_oracle_text_mana_symbol`, `display_colors_land_dedupes_subtype_and_text_match`, `display_colors_land_with_no_recognized_color_is_empty`), no `FAILED`.

- [ ] **Step 5: Clippy check and commit**

```bash
cargo clippy --all-targets 2>&1 | grep -E "error|warning"
```

Expected: no output (clean). Fix anything that appears (a common one here: clippy may suggest `colors.contains(&c)` is fine, but flag the closure capturing `colors` — if so, follow its suggestion).

```bash
git add src/serve.rs
git commit -m "feat: add display_colors() helper for UI card-color derivation"
```

---

### Task 2: Wire `display_colors()` into `CardView.colors`

**Files:**
- Modify: `src/serve.rs:633-638` (the `to_card_view` closure in `build_player_view`)
- Modify: `src/serve.rs:761` (the stack spell's inline `CardView` in `build_game_view`)
- Test: `src/serve.rs` (`mod tests`)

- [ ] **Step 1: Write the failing test**

Add to `mod tests`:

```rust
    #[test]
    fn build_game_view_land_colors_derived_from_subtype() {
        let config = vec![
            (0..10).map(|_| "Forest".to_string()).collect(),
            (0..10).map(|_| "Forest".to_string()).collect(),
        ];
        let db = test_db();
        let gs = build_game_state(config, &db, false).unwrap();
        let view = build_game_view(&gs);
        let forest = view.p1.hand.iter().find(|c| c.name == "Forest").unwrap();
        assert_eq!(forest.colors, vec!["G".to_string()]);
    }
```

- [ ] **Step 2: Run the test to verify it fails**

```bash
cargo test build_game_view_land_colors_derived_from_subtype 2>&1 | grep -E "^test result|FAILED|error\["
```

Expected: `FAILED` — `forest.colors` is currently `[]` (Forest has no printed colors, and the field is still wired to raw `obj.definition.colors`).

- [ ] **Step 3: Wire `display_colors()` in**

In `src/serve.rs`, find the `to_card_view` closure (around line 619-648) and change:

```rust
            colors: obj
                .definition
                .colors
                .iter()
                .map(|c| c.to_string())
                .collect(),
```

to:

```rust
            colors: display_colors(&obj.definition)
                .iter()
                .map(|c| c.to_string())
                .collect(),
```

Then find the stack spell's `CardView` construction (around line 749-768) and change:

```rust
                            colors: c.definition.colors.iter().map(|c| c.to_string()).collect(),
```

to:

```rust
                            colors: display_colors(&c.definition).iter().map(|c| c.to_string()).collect(),
```

- [ ] **Step 4: Run the test to verify it passes**

```bash
cargo test build_game_view_land_colors_derived_from_subtype 2>&1 | grep -E "^test result|FAILED|error\["
```

Expected: `test result: ok`.

- [ ] **Step 5: Run the full test suite and clippy, then commit**

```bash
cargo test 2>&1 | grep -E "^test result|FAILED|error\["
cargo clippy --all-targets 2>&1 | grep -E "error|warning"
```

Expected: all `test result: ok`, no clippy output. (This confirms the existing color-related tests — e.g. any test asserting a Grizzly Bears card's `colors == ["G"]` — still pass, since printed colors take priority unchanged.)

```bash
git add src/serve.rs
git commit -m "feat: derive CardView.colors via display_colors() for lands"
```

---

### Task 3: Switch mana-cost serialization to braced notation

**Files:**
- Modify: `src/serve.rs:630` (hand/battlefield `to_card_view`)
- Modify: `src/serve.rs:758` (stack spell `CardView`)
- Modify: `src/serve.rs:503` (Cycling action label)
- Test: `src/serve.rs` (`mod tests`)

- [ ] **Step 1: Write the failing tests**

Add to `mod tests`:

```rust
    #[test]
    fn build_game_view_mana_cost_is_braced() {
        use mecha_oracle::types::{CardObject, Zone};
        let config = vec![
            (0..10).map(|_| "Forest".to_string()).collect(),
            (0..10).map(|_| "Forest".to_string()).collect(),
        ];
        let db = test_db();
        let mut gs = build_game_state(config, &db, false).unwrap();
        let id = gs.alloc_id();
        let obj = CardObject::new(
            id,
            db.get("Grizzly Bears").unwrap().clone(),
            PlayerId(0),
            Zone::Hand,
        );
        gs.hands.get_mut(&PlayerId(0)).unwrap().push(id);
        gs.add_object(obj);

        let view = build_game_view(&gs);
        let bears = view.p1.hand.iter().find(|c| c.name == "Grizzly Bears").unwrap();
        assert_eq!(bears.mana_cost, Some("{1}{G}".to_string()));
    }

    #[test]
    fn cycle_action_label_is_braced() {
        use mecha_oracle::types::card::{CardDefinition, CardType, TypeLine};
        use mecha_oracle::types::mana::{ManaColor, ManaCost, ManaPip};
        use mecha_oracle::types::{Ability, CardObject, OracleSpan, Zone};

        let config = vec![
            (0..10).map(|_| "Forest".to_string()).collect(),
            (0..10).map(|_| "Forest".to_string()).collect(),
        ];
        let db = test_db();
        let mut gs = build_game_state(config, &db, false).unwrap();
        let def = CardDefinition {
            name: "Test Cycler".into(),
            mana_cost: Some(ManaCost {
                pips: vec![ManaPip::Generic(1), ManaPip::Blue],
            }),
            type_line: TypeLine {
                supertypes: vec![],
                card_types: vec![CardType::Creature],
                subtypes: vec![],
            },
            oracle_text: "Cycling {2}".into(),
            abilities: vec![OracleSpan::Parsed(Ability::Cycling(ManaCost {
                pips: vec![ManaPip::Generic(2)],
            }))],
            text_annotations: vec![],
            power: Some(1),
            toughness: Some(1),
            colors: vec![ManaColor::Blue],
        };
        let id = gs.alloc_id();
        let obj = CardObject::new(id, def, PlayerId(0), Zone::Hand);
        gs.hands.get_mut(&PlayerId(0)).unwrap().push(id);
        gs.add_object(obj);

        let view = build_game_view(&gs);
        let card = view.p1.hand.iter().find(|c| c.name == "Test Cycler").unwrap();
        let cycle_action = card
            .actions
            .iter()
            .find(|a| a.label.starts_with("Cycle"))
            .expect("expected a Cycle action");
        assert_eq!(cycle_action.label, "Cycle ({2})");
    }
```

- [ ] **Step 2: Run the tests to verify they fail**

```bash
cargo test build_game_view_mana_cost_is_braced cycle_action_label_is_braced 2>&1 | grep -E "^test result|FAILED|error\["
```

Expected: both `FAILED` — current output is unbraced (`"1G"` and `"Cycle (2)"`).

- [ ] **Step 3: Switch to braced format**

In `to_card_view` (around `src/serve.rs:630`), change:

```rust
            mana_cost: obj.definition.mana_cost.as_ref().map(format_mana_cost),
```

to:

```rust
            mana_cost: obj
                .definition
                .mana_cost
                .as_ref()
                .map(format_mana_cost_braced),
```

In the stack spell `CardView` (around `src/serve.rs:758`), change:

```rust
                            mana_cost: c.definition.mana_cost.as_ref().map(format_mana_cost),
```

to:

```rust
                            mana_cost: c.definition.mana_cost.as_ref().map(format_mana_cost_braced),
```

In the Cycling action (around `src/serve.rs:503`), change:

```rust
                label: format!("Cycle ({})", format_mana_cost(cost)),
```

to:

```rust
                label: format!("Cycle ({})", format_mana_cost_braced(cost)),
```

- [ ] **Step 4: Run the tests to verify they pass**

```bash
cargo test build_game_view_mana_cost_is_braced cycle_action_label_is_braced 2>&1 | grep -E "^test result|FAILED|error\["
```

Expected: both `test result: ok`.

- [ ] **Step 5: Run the full suite, clippy, and commit**

```bash
cargo test 2>&1 | grep -E "^test result|FAILED|error\["
cargo clippy --all-targets 2>&1 | grep -E "error|warning"
```

Expected: all green. `format_mana_cost` (unbraced) keeps its own two direct unit tests (`format_mana_cost_green_green`, `format_mana_cost_generic_and_color`) — those still pass since the function itself is unchanged, only its callers moved to the braced variant. Clippy may now warn `format_mana_cost` is unused outside tests — if so, that's expected and fine (it's still exercised directly by its own tests in the same module); if clippy actually errors on dead code (it shouldn't, since `#[cfg(test)]` callers count), re-check before suppressing anything.

**Deviation (recorded during implementation):** clippy did flag `format_mana_cost` as `dead_code` under `--all-targets` once all three callers moved to `format_mana_cost_braced`, since none of its remaining callers fell outside its own `#[cfg(test)]` block. Per repo-wide grep confirming zero callers anywhere outside that one function's own tests, and per this project's "delete code that's truly unused rather than suppress with `#[allow(dead_code)]`" convention, `format_mana_cost` and its two direct unit tests were deleted entirely in a follow-up commit (`3b33d9b`) rather than kept per this section's original text. No later task in this plan needs the unbraced format.

```bash
git add src/serve.rs
git commit -m "feat: serialize mana costs in braced notation for icon rendering"
```

---

### Task 4: `source_colors` on `StackItemView`

**Files:**
- Modify: `src/serve.rs:141-154` (`StackItemView` struct)
- Modify: `src/serve.rs:730-806` (`build_game_view`, the three `StackItemView` match arms)
- Test: `src/serve.rs` (`mod tests`)

- [ ] **Step 1: Write the failing test**

Add to `mod tests` (model this on the existing `stack_item_view_includes_targets_and_source_name_for_ability` test at `src/serve.rs:2272`):

```rust
    #[test]
    fn stack_item_view_includes_source_colors_for_ability() {
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
            targets: vec![],
            x_value: None,
        };
        gs.stack.push(stack_id);
        gs.stack_objects.insert(stack_id, stack_obj);

        let view = build_game_view(&gs);
        let item = &view.stack[0];
        assert_eq!(item.source_colors, vec!["G".to_string()]);
    }
```

- [ ] **Step 2: Run the test to verify it fails**

```bash
cargo test stack_item_view_includes_source_colors_for_ability 2>&1 | grep -E "^test result|FAILED|error\["
```

Expected: `error[E0609]: no field `source_colors` on type `StackItemView`` — the field doesn't exist yet.

- [ ] **Step 3: Add the field and wire it**

In `src/serve.rs`, change the `StackItemView` struct (`src/serve.rs:141-154`) from:

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

to:

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
    #[serde(skip_serializing_if = "Vec::is_empty")]
    source_colors: Vec<String>,
}
```

Then in `build_game_view` (`src/serve.rs:730-806`), each of the three `StackItemView` literals needs a `source_colors` field:

- The `StackPayload::Spell` arm (around line 744-773): add `source_colors: vec![],` (the `card` field already carries colors).
- The `StackPayload::TriggeredAbility` arm (around line 774-788): add, right after the existing `source_name:` field:

```rust
                    source_colors: state
                        .objects
                        .get(source_id)
                        .map(|o| display_colors(&o.definition))
                        .unwrap_or_default()
                        .iter()
                        .map(|c| c.to_string())
                        .collect(),
```

- The `StackPayload::ActivatedAbility` arm (around line 789-803): add the same `source_colors:` block as the `TriggeredAbility` arm above.

- [ ] **Step 4: Run the test to verify it passes**

```bash
cargo test stack_item_view_includes_source_colors_for_ability 2>&1 | grep -E "^test result|FAILED|error\["
```

Expected: `test result: ok`.

- [ ] **Step 5: Run the full suite, clippy, and commit**

```bash
cargo test 2>&1 | grep -E "^test result|FAILED|error\["
cargo clippy --all-targets 2>&1 | grep -E "error|warning"
```

Expected: all green (this also confirms `stack_item_view_spell_has_no_source_name` and the triggered-ability test still pass with the new field defaulting to `vec![]` and being skipped in serialization).

```bash
git add src/serve.rs
git commit -m "feat: add source_colors to StackItemView for non-spell stack items"
```

---

### Task 5: CSS — mana custom properties, new pip variants, tooltip z-index

**Files:**
- Modify: `src/serve.css:2-5` (`:root`)
- Modify: `src/serve.css:34-40` (`.pip-W` through `.pip-C`)
- Modify: `src/serve.css:79-95` (`.tooltip` block)

No `cargo test`/`node --check` applies to a CSS-only file; verification here is "the file parses as valid CSS and the project still builds" (CSS isn't compiled by `cargo build`, but a syntax slip would be visually obvious later, so a quick brace-balance sanity check substitutes for a real linter).

- [ ] **Step 1: Add mana custom properties to `:root`**

In `src/serve.css`, change:

```css
:root {
  --p1-color: #51cf66;
  --p2-color: #ff7b7b;
}
```

to:

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
  --mana-p-bg: #161616;
}
```

- [ ] **Step 2: Point the existing pip rules at the new variables and add new variants**

Change:

```css
.pip { width: 14px; height: 14px; border-radius: 50%; border: 1px solid #555; display: inline-flex; align-items: center; justify-content: center; font-size: 8px; font-weight: bold; }
.pip-W { background: #4d4a1a; border-color: #9a932d; color: #e8e06f; }
.pip-U { background: #1a2a4d; border-color: #2d4a9a; color: #6f8ee8; }
.pip-B { background: #2a1a2a; border-color: #6a3a6a; color: #c06fc0; }
.pip-R { background: #4d1a1a; border-color: #9a2d2d; color: #e86f6f; }
.pip-G { background: #1a4d1a; border-color: #2d7a2d; color: #6fd86f; }
.pip-C { background: #2a2a2a; border-color: #666; color: #ccc; }
```

to:

```css
.pip { width: 14px; height: 14px; border-radius: 50%; border: 1px solid #555; display: inline-flex; align-items: center; justify-content: center; font-size: 8px; font-weight: bold; }
.pip-W { background: var(--mana-w-bg); border-color: var(--mana-w-border); color: var(--mana-w-fg); }
.pip-U { background: var(--mana-u-bg); border-color: var(--mana-u-border); color: var(--mana-u-fg); }
.pip-B { background: var(--mana-b-bg); border-color: var(--mana-b-border); color: var(--mana-b-fg); }
.pip-R { background: var(--mana-r-bg); border-color: var(--mana-r-border); color: var(--mana-r-fg); }
.pip-G { background: var(--mana-g-bg); border-color: var(--mana-g-border); color: var(--mana-g-fg); }
.pip-C { background: var(--mana-c-bg); border-color: var(--mana-c-border); color: var(--mana-c-fg); }
.pip-generic { background: var(--mana-c-bg); border-color: var(--mana-c-border); color: var(--mana-c-fg); }
.pip-X { background: var(--mana-x-bg); border-color: var(--mana-x-border); color: var(--mana-x-fg); }
.pip-S { background: var(--mana-s-bg); border-color: var(--mana-s-border); color: var(--mana-s-fg); }
.pip-split { font-size: 6px; }
.pip-tap svg { display: block; }
```

- [ ] **Step 3: Fix the tooltip z-index**

Change (`src/serve.css:79-85`):

```css
.tooltip {
  display: none; position: fixed; left: 0; top: 0;
  width: 200px; background: #1e2430; border: 1px solid #4a6a9a;
  border-radius: 6px; padding: 10px; z-index: 9999;
  font-size: 11px; color: #ccc; pointer-events: none;
  box-shadow: 0 4px 16px rgba(0,0,0,0.6); line-height: 1.5;
}
```

to (only the `z-index` value changes):

```css
.tooltip {
  display: none; position: fixed; left: 0; top: 0;
  width: 200px; background: #1e2430; border: 1px solid #4a6a9a;
  border-radius: 6px; padding: 10px; z-index: 350;
  font-size: 11px; color: #ccc; pointer-events: none;
  box-shadow: 0 4px 16px rgba(0,0,0,0.6); line-height: 1.5;
}
```

- [ ] **Step 4: Sanity-check the file**

```bash
python3 -c "
s = open('src/serve.css').read()
assert s.count('{') == s.count('}'), 'unbalanced braces'
print('OK', s.count('{'), 'rules')
"
```

Expected: `OK <some number> rules`, no `AssertionError`.

- [ ] **Step 5: Commit**

```bash
git add src/serve.css
git commit -m "feat: add mana color custom properties, new pip variants, fix tooltip z-index"
```

---

### Task 6: JS — card-color background helpers, applied in `cardHTML()`

**Files:**
- Modify: `src/serve.js` (add helpers near the top, after the `let`/`const` declarations at `src/serve.js:1-9`)
- Modify: `src/serve.js:275-325` (`cardHTML()`)

- [ ] **Step 1: Add the color helpers**

In `src/serve.js`, right after the existing state declarations (after `let paymentContext = null;` at line 9), add:

```js
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
```

- [ ] **Step 2: Apply it in `cardHTML()`**

In `src/serve.js`, find the return statement of `cardHTML()` (around line 319-324):

```js
  return `<div class="${wrap}"><div class="${classes}" data-id="${card.id}" ${clickAttr}>
    <span class="card-name">${esc(card.name)}</span>
    ${card.mana_cost ? `<span class="card-cost">${esc(card.mana_cost)}</span>` : ''}
    <span class="card-type">${esc(card.type_line)}</span>
    ${pt}
  </div>${tooltip}</div>`;
```

Change it to compute and apply a background style (note: the `card-cost` line's `esc(...)` → `renderManaSymbols(...)` swap happens in Task 9, not here — leave it as `esc` for now):

```js
  const bg = cardColorBackground(card.colors);
  const fg = bestTextColor(card.colors && card.colors.length === 1 ? MANA_HEX[card.colors[0].toLowerCase()] : '#000000');
  const cardStyle = `style="background:${bg};color:${fg}"`;

  return `<div class="${wrap}"><div class="${classes}" data-id="${card.id}" ${clickAttr} ${cardStyle}>
    <span class="card-name">${esc(card.name)}</span>
    ${card.mana_cost ? `<span class="card-cost">${esc(card.mana_cost)}</span>` : ''}
    <span class="card-type">${esc(card.type_line)}</span>
    ${pt}
  </div>${tooltip}</div>`;
```

(The `fg` luminance check is only meaningful for the single-color case in this palette — a 2-color gradient or the always-dark gold/grey swatches don't need it, so passing `'#000000'` — a dark fallback that always yields the light `#ddd` text — for those cases keeps the call simple without changing visible behavior.)

- [ ] **Step 3: Syntax-check**

```bash
node --check src/serve.js
```

Expected: no output (exit code 0).

- [ ] **Step 4: Visual smoke test**

```bash
cargo build 2>&1 | tail -5
```

Expected: `Compiling mecha-oracle ...` then a clean finish (no errors — this task didn't touch any `.rs` file, so this just confirms the workspace is still healthy before moving on).

- [ ] **Step 5: Commit**

```bash
git add src/serve.js
git commit -m "feat: color card backgrounds by mechanical color in cardHTML()"
```

---

### Task 7: JS — apply card-color backgrounds to stack cards and the graveyard preview

**Files:**
- Modify: `src/serve.js:709-790` (`renderStack()`, the "new card" branch)
- Modify: `src/serve.js:235-247` (`renderGYPile()`)

- [ ] **Step 1: Apply in `renderStack()`**

In `src/serve.js`, find where the new `.stack-card` element's class is set (around line 754-756):

```js
      el = document.createElement('div');
      el.className       = 'card-wrap stack-card ' + (item.controller === 0 ? 'p1' : 'p2');
      el.dataset.stackId = idStr;
```

Add a background style line right after:

```js
      el = document.createElement('div');
      el.className       = 'card-wrap stack-card ' + (item.controller === 0 ? 'p1' : 'p2');
      el.dataset.stackId = idStr;
      const stackColors = item.card ? item.card.colors : item.source_colors;
      el.style.backgroundImage  = cardColorBackground(stackColors).startsWith('linear-gradient') ? cardColorBackground(stackColors) : 'none';
      el.style.backgroundColor  = cardColorBackground(stackColors).startsWith('linear-gradient') ? '' : cardColorBackground(stackColors);
```

(`.stack-card`'s border-color stays controlled by the existing `.p1`/`.p2` CSS classes — only `background` is overridden inline, so the controller cue from the border survives alongside the new mechanical-color fill.)

- [ ] **Step 2: Apply in `renderGYPile()`**

In `src/serve.js`, find `renderGYPile()` (lines 235-247):

```js
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
```

Change to apply (and clear, when the pile is empty) the background:

```js
function renderGYPile(prefix, graveyard) {
  const top = graveyard[graveyard.length - 1];
  const label = document.getElementById(prefix + '-gy-label');
  const topEl = document.getElementById(prefix + '-gy-top');
  label.textContent = `GY (${graveyard.length})`;
  if (top) {
    topEl.innerHTML = `<span class="gy-card-name">${top.name}</span><span class="gy-card-type">${top.type_line}</span>` +
      (top.power != null ? `<span class="gy-card-pt">${top.power}/${top.toughness}</span>` : '');
    const bg = cardColorBackground(top.colors);
    if (bg.startsWith('linear-gradient')) {
      topEl.style.backgroundImage = bg;
      topEl.style.backgroundColor = '';
    } else {
      topEl.style.backgroundImage = 'none';
      topEl.style.backgroundColor = bg;
    }
  } else {
    topEl.innerHTML = '<span style="font-size:10px;color:#442222;text-align:center;width:100%">empty</span>';
    topEl.style.backgroundImage = 'none';
    topEl.style.backgroundColor = '';
  }
  document.getElementById(prefix + '-gy-wrap').style.cursor = graveyard.length > 0 ? 'pointer' : 'default';
}
```

- [ ] **Step 3: Syntax-check**

```bash
node --check src/serve.js
```

Expected: no output.

- [ ] **Step 4: Commit**

```bash
git add src/serve.js
git commit -m "feat: color stack cards and graveyard preview by mechanical color"
```

---

### Task 8: JS — mana symbol renderer (`renderManaSymbols`)

**Files:**
- Modify: `src/serve.js` (add new functions near `esc()`, currently at `src/serve.js:327`)

- [ ] **Step 1: Add the renderer functions**

In `src/serve.js`, right after `function esc(s) { ... }` (currently lines 327-329), add:

```js
// ── Mana symbol rendering ───────────────────────────────────────────────────
// Renders every {token} in a string as an icon (mana pip or tap/untap symbol),
// matching the brace notation emitted by format_mana_cost_braced / format_ability_cost_label
// / format_mana_pool in src/serve.rs. Plain text outside {tokens} is HTML-escaped as usual.

function manaComponentStyle(part) {
  const p = part.toUpperCase();
  if (p === 'P') return { bg: 'var(--mana-p-bg)', label: 'P' };
  if (p === 'X') return { bg: 'var(--mana-x-bg)', label: 'X' };
  if (p === 'S') return { bg: 'var(--mana-s-bg)', label: 'S' };
  if (/^\d+$/.test(p)) return { bg: 'var(--mana-c-bg)', label: p };
  if ('WUBRGC'.includes(p)) return { bg: `var(--mana-${p.toLowerCase()}-bg)`, label: p };
  return { bg: 'var(--mana-c-bg)', label: p };
}

function manaPipHTML(parts) {
  if (parts.length === 1) {
    const part = parts[0];
    const { label } = manaComponentStyle(part);
    let cls;
    if (/^\d+$/.test(part)) cls = 'pip-generic';
    else if ('WUBRGC'.includes(part.toUpperCase())) cls = `pip-${part.toUpperCase()}`;
    else if (part.toUpperCase() === 'X') cls = 'pip-X';
    else if (part.toUpperCase() === 'S') cls = 'pip-S';
    else cls = 'pip-generic';
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
    if (m[1] === 'T') out += tapPipHTML(false);
    else if (m[1] === 'Q') out += tapPipHTML(true);
    else out += manaPipHTML(m[1].split('/'));
    last = re.lastIndex;
  }
  return out + esc(s.slice(last));
}
```

- [ ] **Step 2: Syntax-check**

```bash
node --check src/serve.js
```

Expected: no output.

- [ ] **Step 3: Manual function check via Node**

```bash
node -e "
$(sed -n '/^function esc/,/^function manaComponentStyle/p' src/serve.js | head -n -1)
$(sed -n '/^function manaComponentStyle/,/^function renderManaSymbols/p' src/serve.js)
$(sed -n '/^function renderManaSymbols/,/^}/p' src/serve.js)
console.log(renderManaSymbols('Cast {2}{W}, {T}, Sac a {X}/{P} thing {Q}'));
"
```

Expected: a line of HTML containing `<span class="pip pip-generic">2</span>`, `<span class="pip pip-W">W</span>`, a `pip-tap` span with an `<svg>`, a `pip-split` span for `X/P`, and another `pip-tap` span (the untapped one) with `transform="rotate(180 12 12)"` — i.e. no thrown exception and recognizable icon markup in the output.

- [ ] **Step 4: Commit**

```bash
git add src/serve.js
git commit -m "feat: add renderManaSymbols() icon renderer for {token} mana/tap symbols"
```

---

### Task 9: JS — wire `renderManaSymbols()` into the display call sites

**Files:**
- Modify: `src/serve.js:253` (`tooltipHTML()` cost line)
- Modify: `src/serve.js:321` (`cardHTML()` cost span)
- Modify: `src/serve.js:35` (`openPopup()` item label)
- Modify: `src/serve.js:527-528` (`renderPaymentPanel()` title/cost)

- [ ] **Step 1: `tooltipHTML()`**

Change (`src/serve.js:249-260`):

```js
function tooltipHTML({ name, manaCost, typeLine, oracleHtml, pt, tags, extraSections }) {
  return `
    <div class="tooltip">
      <div class="tooltip-name">${esc(name)}</div>
      ${manaCost ? `<div class="tooltip-cost">${esc(manaCost)}</div>` : ''}
```

to:

```js
function tooltipHTML({ name, manaCost, typeLine, oracleHtml, pt, tags, extraSections }) {
  return `
    <div class="tooltip">
      <div class="tooltip-name">${esc(name)}</div>
      ${manaCost ? `<div class="tooltip-cost">${renderManaSymbols(manaCost)}</div>` : ''}
```

(rest of the function body unchanged).

- [ ] **Step 2: `cardHTML()`**

Change the card-cost line introduced/touched in Task 6:

```js
    ${card.mana_cost ? `<span class="card-cost">${esc(card.mana_cost)}</span>` : ''}
```

to:

```js
    ${card.mana_cost ? `<span class="card-cost">${renderManaSymbols(card.mana_cost)}</span>` : ''}
```

- [ ] **Step 3: `openPopup()`**

Change (`src/serve.js:30-36`):

```js
function openPopup(items, anchorEl, header) {
  const popup = document.getElementById('popup');
  popup.innerHTML =
    (header ? `<div class="popup-header">${esc(header)}</div>` : '') +
    items.map((item, i) =>
      `<button class="popup-item${item.active ? ' active' : ''}${item.disabled ? ' disabled' : ''}" data-idx="${i}">${esc(item.label)}</button>`
    ).join('');
```

to:

```js
function openPopup(items, anchorEl, header) {
  const popup = document.getElementById('popup');
  popup.innerHTML =
    (header ? `<div class="popup-header">${esc(header)}</div>` : '') +
    items.map((item, i) =>
      `<button class="popup-item${item.active ? ' active' : ''}${item.disabled ? ' disabled' : ''}" data-idx="${i}">${renderManaSymbols(item.label)}</button>`
    ).join('');
```

- [ ] **Step 4: `renderPaymentPanel()`**

Change (`src/serve.js:520-528`):

```js
function renderPaymentPanel() {
  const panel = document.getElementById('payment-panel');
  if (!paymentContext || !currentState) {
    panel.style.display = 'none';
    return;
  }
  panel.style.display = '';
  document.getElementById('payment-title').textContent = paymentContext.actionLabel || 'Pay cost';
  document.getElementById('payment-cost').textContent = paymentContext.costLabel || '(no cost)';
```

to:

```js
function renderPaymentPanel() {
  const panel = document.getElementById('payment-panel');
  if (!paymentContext || !currentState) {
    panel.style.display = 'none';
    return;
  }
  panel.style.display = '';
  document.getElementById('payment-title').innerHTML = renderManaSymbols(paymentContext.actionLabel || 'Pay cost');
  document.getElementById('payment-cost').innerHTML = renderManaSymbols(paymentContext.costLabel || '(no cost)');
```

- [ ] **Step 5: Syntax-check and commit**

```bash
node --check src/serve.js
```

Expected: no output.

```bash
git add src/serve.js
git commit -m "feat: render mana symbols as icons in card cost, tooltip, popup, and payment panel"
```

---

### Task 10: JS — zone-aware, viewport-clamped tooltip positioning

**Files:**
- Modify: `src/serve.js:671-695` (the `mouseover` listener)

- [ ] **Step 1: Replace the positioning logic**

Change (`src/serve.js:671-695`):

```js
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
```

to:

```js
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

  const cardRect = wrap.getBoundingClientRect();
  // Real rendered size, not a guess — the tooltip is visible by this point (either via
  // the active :hover pseudo-class, or forced visible above for detached stack tooltips),
  // so offsetWidth/offsetHeight reflect its actual content (including long oracle text).
  const tw = tooltip.offsetWidth;
  const th = tooltip.offsetHeight;
  const GAP = 6;

  let left, top;
  if (wrap.classList.contains('stack-card')) {
    // Stack: prefer left of the card, vertical centers aligned.
    left = cardRect.left - tw - GAP;
    if (left < 8) left = cardRect.right + GAP; // flip right only if there's no room on the left
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

  // Universal viewport clamp — applies no matter which branch above ran, so a tooltip
  // can never be positioned outside the viewport regardless of zone or content length.
  left = Math.max(8, Math.min(left, window.innerWidth - tw - 8));
  top  = Math.max(8, Math.min(top,  window.innerHeight - th - 8));
  tooltip.style.left = left + 'px';
  tooltip.style.top  = top  + 'px';
});
```

- [ ] **Step 2: Syntax-check**

```bash
node --check src/serve.js
```

Expected: no output.

- [ ] **Step 3: Commit**

```bash
git add src/serve.js
git commit -m "fix: position tooltips per-zone using real rendered size, clamp to viewport"
```

---

### Task 11: Final verification

**Files:** none (verification only)

- [ ] **Step 1: Full backend check**

```bash
cargo test 2>&1 | grep -E "^test result|FAILED|error\["
cargo clippy --all-targets 2>&1 | grep -E "error|warning"
```

Expected: all `test result: ok`, no `FAILED`, no clippy output. If clippy flags anything, run `cargo clippy --fix` first, re-check, then hand-fix anything that remains (per `CLAUDE.md`'s linter-clean requirement).

- [ ] **Step 2: Launch the dev server**

```bash
cargo run -- serve docs/test-decks/blue_abilities.json
```

Leave it running (or run with `run_in_background` if using the agent harness) and open the printed local URL in a browser.

- [ ] **Step 3: Manually verify all four fixes**

- Hover a card in P1's hand with long oracle text — tooltip appears above or below the card (whichever fits), horizontally centered on it, fully inside the viewport (no clipping at the bottom).
- Hover a card in P2's hand — same correctness, tooltip on the appropriate side given P2's position near the top of the screen.
- Hover a card on the stack — tooltip appears to the left, vertically centered on the card.
- Right-click a card while another card's tooltip happens to be visible — confirm the right-click context menu renders on top of the tooltip, not behind it.
- Check a mono-color card's background (solid muted color), a land of each basic type (colored to match), an artifact/colorless card (grey), and — if available in the loaded deck — a multicolor card (gradient, or gold for 3+).
- Activate a mana ability (tap a land) and open the resulting action menu — confirm the cost (e.g. `{T}: Add {G}`) renders as a tap icon followed by a colored mana pip, not raw `{T}{G}` text.
- Cast a spell with a generic + colored cost and confirm the payment panel shows icons, not text, for both the cost line and (if applicable) an `{X}` pip.

- [ ] **Step 4: Stop the dev server**

```bash
# Ctrl-C the foregrounded `cargo run`, or stop the background task if run_in_background was used.
```

No commit for this task — it's verification only. If any manual check fails, fix the relevant earlier task's code, re-run that task's `node --check`/`cargo test`, and re-verify here before considering the plan complete.
