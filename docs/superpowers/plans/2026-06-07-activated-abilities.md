# Activated Abilities Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Parse `{cost}: effect` oracle text into a structured `ActivatedAbility` AST, execute those abilities in the engine, and expose them as sidebar actions in the UI.

**Architecture:** Parser detects `{cost}: effect` paragraphs via a new depth-zero colon search (before the existing comma-split), emitting `Parsed(Activated(...))` spans. A new `src/engine/activated.rs` module executes abilities by checking then paying costs, then applying effects. The UI adds `activated_abilities: Vec<ActivatedAbilityView>` to `CardView` and renders them in the sidebar.

**Tech Stack:** Rust (existing), axum (existing), vanilla JS sidebar (existing)

---

## File Map

| File | Change |
|---|---|
| `src/types/mana.rs` | Add `Eq` to `ManaCost` and `ManaPool` derives |
| `src/types/ability.rs` | Replace `ActivatedAbility` stub; add `ActivationCost`, `AbilityEffect`, `CostComponent`, `EffectStep`, `PermanentFilter`, `CardFilter` |
| `src/types/mod.rs` | Re-export new public types |
| `tests/fixtures/oracle_cards_test.json` | Add Llanowar Elves |
| `src/parser/oracle.rs` | New private helpers; colon check in `parse_oracle_text` |
| `src/engine/mod.rs` | Add `AbilityIndexOutOfRange`; `pub mod activated` |
| `src/engine/activated.rs` | New — `activate_ability`, `can_pay_cost` |
| `src/serve.rs` | `ActivatedAbilityView`; format helpers; `CardView` field; oracle_text arm; `ActivateAbility` action |
| `src/serve.html` | Sidebar items for activated abilities |

---

## Task 1: Add `Eq` to `ManaCost` and `ManaPool`

**Files:**
- Modify: `src/types/mana.rs:11-12`, `src/types/mana.rs:32-33`

`CostComponent::Mana(ManaCost)` and `EffectStep::AddMana(ManaPool)` need `Eq` so the enclosing enums can derive `PartialEq + Eq`. Both types have only `u32` fields so `Eq` is trivially derivable.

- [ ] **Step 1: Add `Eq` to both types**

In `src/types/mana.rs`, change:
```rust
#[derive(Debug, Clone, PartialEq, Default)]
pub struct ManaCost {
```
to:
```rust
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct ManaCost {
```

And:
```rust
#[derive(Debug, Clone, PartialEq, Default)]
pub struct ManaPool {
```
to:
```rust
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct ManaPool {
```

- [ ] **Step 2: Confirm tests still pass**

```bash
cargo test 2>&1 | grep -E "^test result|FAILED|error\["
```
Expected: `test result: ok` — no failures (existing tests unaffected).

- [ ] **Step 3: Commit**

```bash
git add src/types/mana.rs
git commit -m "feat: derive Eq for ManaCost and ManaPool"
```

---

## Task 2: Expand Data Model

**Files:**
- Modify: `src/types/ability.rs`
- Modify: `src/types/mod.rs`

Replace the unit-struct `ActivatedAbility` stub with the full type hierarchy.

- [ ] **Step 1: Write a failing test for the new types**

At the bottom of the `#[cfg(test)]` block in `src/types/ability.rs`, add:

```rust
#[test]
fn activated_ability_construction() {
    use super::super::mana::{ManaCost, ManaPool};
    let ability = ActivatedAbility {
        cost: vec![CostComponent::Tap],
        effect: vec![EffectStep::AddMana(ManaPool { green: 1, ..Default::default() })],
    };
    assert_eq!(ability.cost.len(), 1);
    assert_eq!(ability.effect.len(), 1);
    assert!(matches!(ability.cost[0], CostComponent::Tap));
}

#[test]
fn cost_component_unimplemented_round_trips() {
    let c = CostComponent::Unimplemented("Sacrifice a creature".to_string());
    assert!(matches!(c, CostComponent::Unimplemented(_)));
}
```

- [ ] **Step 2: Run to confirm failure**

```bash
cargo test 2>&1 | grep -E "^test result|FAILED|error\["
```
Expected: compile error — `ActivatedAbility`, `CostComponent`, `EffectStep` not defined.

- [ ] **Step 3: Replace the stub in `src/types/ability.rs`**

Add the import at the top of the file (after existing imports):
```rust
use super::mana::{ManaCost, ManaPool};
```

Replace the existing stub:
```rust
/// An ability paid for with a cost. Phase 2+ adds cost + effect fields.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ActivatedAbility;
```

with:

```rust
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ActivatedAbility {
    pub cost: ActivationCost,
    pub effect: AbilityEffect,
}

pub type ActivationCost = Vec<CostComponent>;
pub type AbilityEffect = Vec<EffectStep>;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CostComponent {
    Tap,
    Mana(ManaCost),
    PayLife(u32),
    Sacrifice(u32, PermanentFilter),
    Discard(u32, CardFilter),
    Unimplemented(String),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EffectStep {
    AddMana(ManaPool),
    Mill(u32),
    DrawCard(u32),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PermanentFilter;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CardFilter;
```

- [ ] **Step 4: Update `src/types/mod.rs` re-exports**

Change the `ability` re-export line from:
```rust
pub use ability::{
    AbilityAST, ActivatedAbility, IgnoredKind, OracleSpan, StaticAbility, TriggerEvent,
    TriggeredAbility,
};
```
to:
```rust
pub use ability::{
    AbilityAST, ActivatedAbility, ActivationCost, AbilityEffect,
    CardFilter, CostComponent, EffectStep, IgnoredKind, OracleSpan,
    PermanentFilter, StaticAbility, TriggerEvent, TriggeredAbility,
};
```

- [ ] **Step 5: Run tests to confirm pass**

```bash
cargo test 2>&1 | grep -E "^test result|FAILED|error\["
```
Expected: `test result: ok`.

- [ ] **Step 6: Commit**

```bash
git add src/types/ability.rs src/types/mod.rs
git commit -m "feat: expand ActivatedAbility with cost and effect types"
```

---

## Task 3: Add Llanowar Elves to Test Fixture

**Files:**
- Modify: `tests/fixtures/oracle_cards_test.json`

This fixture is loaded by `CardDatabase::from_path` in all engine and serve tests. Adding Llanowar Elves here makes it available for parser and engine tests without a mock.

- [ ] **Step 1: Write a failing test in `src/cards/mod.rs`**

In the existing `#[cfg(test)]` block in `src/cards/mod.rs`, add:

```rust
#[test]
fn llanowar_elves_loads_with_activated_ability() {
    use crate::types::{AbilityAST, OracleSpan};
    let db = test_db();
    let card = db.get("Llanowar Elves").expect("Llanowar Elves not in fixture");
    assert!(card.abilities.iter().any(|s| {
        matches!(s, OracleSpan::Parsed(AbilityAST::Activated(_)))
    }));
}
```

- [ ] **Step 2: Run to confirm failure**

```bash
cargo test llanowar_elves_loads 2>&1 | grep -E "^test result|FAILED|error\[|panicked"
```
Expected: panics with `Llanowar Elves not in fixture`.

- [ ] **Step 3: Add the card to the fixture**

Open `tests/fixtures/oracle_cards_test.json`. Before the final `]`, add a comma after the last entry and then insert:

```json
  ,
  {
    "object": "card",
    "name": "Llanowar Elves",
    "mana_cost": "{G}",
    "type_line": "Creature — Elf Druid",
    "oracle_text": "{T}: Add {G}.",
    "power": "1",
    "toughness": "1"
  }
```

(The test still fails because the parser doesn't emit `Parsed(Activated(...))` yet — that's Task 5. Keep the test for now; it drives Tasks 4 and 5.)

- [ ] **Step 4: Verify the fixture is valid JSON**

```bash
python3 -c "import json,sys; json.load(open('tests/fixtures/oracle_cards_test.json'))" && echo "valid"
```
Expected: `valid`

- [ ] **Step 5: Commit**

```bash
git add tests/fixtures/oracle_cards_test.json
git commit -m "test: add Llanowar Elves fixture for activated ability tests"
```

---

## Task 4: Parser — Private Helper Functions

**Files:**
- Modify: `src/parser/oracle.rs`

Add seven private helpers. All tests go in the existing `#[cfg(test)]` block; implementations go in the private helpers section near the top of the file.

- [ ] **Step 1: Write failing tests for `find_colon_at_depth_zero`**

In `src/parser/oracle.rs` tests, add:

```rust
#[test]
fn find_colon_none_for_no_colon() {
    assert_eq!(super::find_colon_at_depth_zero("Flying"), None);
}

#[test]
fn find_colon_skips_inside_parens() {
    assert_eq!(super::find_colon_at_depth_zero("({T}: Add {G}.)"), None);
}

#[test]
fn find_colon_skips_inside_braces() {
    // hypothetical, but verifies brace depth tracking
    assert_eq!(super::find_colon_at_depth_zero("{T}: Add {G}."), Some(3));
}

#[test]
fn find_colon_at_depth_zero_comma_cost() {
    // "{2}, {T}: Add {G}." — colon is at index 8
    assert_eq!(super::find_colon_at_depth_zero("{2}, {T}: Add {G}."), Some(8));
}
```

- [ ] **Step 2: Run to confirm failure**

```bash
cargo test find_colon 2>&1 | grep -E "^test result|FAILED|error\["
```
Expected: compile error — function not found.

- [ ] **Step 3: Implement `find_colon_at_depth_zero`**

Add to the private helpers section in `src/parser/oracle.rs`:

```rust
/// Returns the byte offset of the first `:` at depth 0,
/// tracking both `{`/`}` and `(`/`)` as nesting delimiters.
fn find_colon_at_depth_zero(text: &str) -> Option<usize> {
    let mut depth = 0usize;
    for (i, c) in text.char_indices() {
        match c {
            '(' | '{' => depth += 1,
            ')' | '}' => depth = depth.saturating_sub(1),
            ':' if depth == 0 => return Some(i),
            _ => {}
        }
    }
    None
}
```

- [ ] **Step 4: Write failing tests for mana/pool/number helpers**

```rust
#[test]
fn try_parse_mana_cost_single_color() {
    use crate::types::mana::ManaCost;
    let c = super::try_parse_mana_cost("{G}").unwrap();
    assert_eq!(c, ManaCost { green: 1, ..Default::default() });
}

#[test]
fn try_parse_mana_cost_generic_and_color() {
    use crate::types::mana::ManaCost;
    let c = super::try_parse_mana_cost("{2}{G}").unwrap();
    assert_eq!(c, ManaCost { generic: 2, green: 1, ..Default::default() });
}

#[test]
fn try_parse_mana_cost_tap_symbol_is_none() {
    assert!(super::try_parse_mana_cost("{T}").is_none());
}

#[test]
fn try_parse_mana_cost_non_symbol_text_is_none() {
    assert!(super::try_parse_mana_cost("Sacrifice a creature").is_none());
}

#[test]
fn try_parse_mana_pool_green() {
    use crate::types::mana::ManaPool;
    let p = super::try_parse_mana_pool("{G}").unwrap();
    assert_eq!(p, ManaPool { green: 1, ..Default::default() });
}

#[test]
fn try_parse_mana_pool_two_colors() {
    use crate::types::mana::ManaPool;
    let p = super::try_parse_mana_pool("{G}{W}").unwrap();
    assert_eq!(p, ManaPool { green: 1, white: 1, ..Default::default() });
}

#[test]
fn try_parse_mana_pool_generic_is_none() {
    assert!(super::try_parse_mana_pool("{2}").is_none());
}

#[test]
fn parse_number_word_digits_and_words() {
    assert_eq!(super::parse_number_word("2"), Some(2));
    assert_eq!(super::parse_number_word("two"), Some(2));
    assert_eq!(super::parse_number_word("three"), Some(3));
    assert_eq!(super::parse_number_word("banana"), None);
}
```

- [ ] **Step 5: Run to confirm failure**

```bash
cargo test "try_parse_mana|parse_number" 2>&1 | grep -E "^test result|FAILED|error\["
```
Expected: compile error.

- [ ] **Step 6: Implement `try_parse_mana_cost`, `try_parse_mana_pool`, `parse_number_word`**

Add to `src/parser/oracle.rs` private helpers:

```rust
use crate::types::mana::{ManaCost, ManaPool};

fn try_parse_mana_cost(s: &str) -> Option<ManaCost> {
    let mut cost = ManaCost::default();
    let mut chars = s.chars().peekable();
    let mut saw_symbol = false;
    while let Some(c) = chars.next() {
        if c != '{' {
            return None;
        }
        let mut token = String::new();
        for inner in chars.by_ref() {
            if inner == '}' { break; }
            token.push(inner);
        }
        match token.as_str() {
            "W" => cost.white += 1,
            "U" => cost.blue += 1,
            "B" => cost.black += 1,
            "R" => cost.red += 1,
            "G" => cost.green += 1,
            "C" => cost.colorless += 1,
            n => {
                if let Ok(v) = n.parse::<u32>() {
                    cost.generic += v;
                } else {
                    return None; // unknown symbol (includes {T})
                }
            }
        }
        saw_symbol = true;
    }
    if saw_symbol { Some(cost) } else { None }
}

fn try_parse_mana_pool(s: &str) -> Option<ManaPool> {
    let mut pool = ManaPool::default();
    let mut chars = s.chars().peekable();
    let mut saw_symbol = false;
    while let Some(c) = chars.next() {
        if c != '{' {
            return None;
        }
        let mut token = String::new();
        for inner in chars.by_ref() {
            if inner == '}' { break; }
            token.push(inner);
        }
        match token.as_str() {
            "W" => pool.white += 1,
            "U" => pool.blue += 1,
            "B" => pool.black += 1,
            "R" => pool.red += 1,
            "G" => pool.green += 1,
            "C" => pool.colorless += 1,
            _ => return None, // no generic mana in add effects
        }
        saw_symbol = true;
    }
    if saw_symbol { Some(pool) } else { None }
}

fn parse_number_word(s: &str) -> Option<u32> {
    match s {
        "one"   | "1"  => Some(1),
        "two"   | "2"  => Some(2),
        "three" | "3"  => Some(3),
        "four"  | "4"  => Some(4),
        "five"  | "5"  => Some(5),
        "six"   | "6"  => Some(6),
        "seven" | "7"  => Some(7),
        "eight" | "8"  => Some(8),
        "nine"  | "9"  => Some(9),
        "ten"   | "10" => Some(10),
        _ => s.parse().ok(),
    }
}
```

- [ ] **Step 7: Write failing tests for `parse_activation_cost` and `parse_ability_effect`**

```rust
#[test]
fn parse_activation_cost_tap_only() {
    use crate::types::ability::CostComponent;
    let cost = super::parse_activation_cost("{T}");
    assert_eq!(cost, vec![CostComponent::Tap]);
}

#[test]
fn parse_activation_cost_mana_and_tap() {
    use crate::types::ability::{CostComponent};
    use crate::types::mana::ManaCost;
    let cost = super::parse_activation_cost("{2}, {T}");
    assert_eq!(cost, vec![
        CostComponent::Mana(ManaCost { generic: 2, ..Default::default() }),
        CostComponent::Tap,
    ]);
}

#[test]
fn parse_activation_cost_unrecognised_becomes_unimplemented() {
    use crate::types::ability::CostComponent;
    let cost = super::parse_activation_cost("Sacrifice a creature");
    assert_eq!(cost, vec![CostComponent::Unimplemented("Sacrifice a creature".to_string())]);
}

#[test]
fn parse_ability_effect_add_mana() {
    use crate::types::ability::EffectStep;
    use crate::types::mana::ManaPool;
    let effect = super::parse_ability_effect("Add {G}.").unwrap();
    assert_eq!(effect, vec![EffectStep::AddMana(ManaPool { green: 1, ..Default::default() })]);
}

#[test]
fn parse_ability_effect_draw_a_card() {
    use crate::types::ability::EffectStep;
    let effect = super::parse_ability_effect("Draw a card.").unwrap();
    assert_eq!(effect, vec![EffectStep::DrawCard(1)]);
}

#[test]
fn parse_ability_effect_mill_two() {
    use crate::types::ability::EffectStep;
    let effect = super::parse_ability_effect("Mill 2.").unwrap();
    assert_eq!(effect, vec![EffectStep::Mill(2)]);
}

#[test]
fn parse_ability_effect_multi_step() {
    use crate::types::ability::EffectStep;
    use crate::types::mana::ManaPool;
    let effect = super::parse_ability_effect("Mill 2. Draw a card.").unwrap();
    assert_eq!(effect, vec![
        EffectStep::Mill(2),
        EffectStep::DrawCard(1),
    ]);
}

#[test]
fn parse_ability_effect_unknown_returns_none() {
    assert!(super::parse_ability_effect("Create a 1/1 token.").is_none());
}
```

- [ ] **Step 8: Run to confirm failure**

```bash
cargo test "parse_activation_cost|parse_ability_effect" 2>&1 | grep -E "^test result|FAILED|error\["
```
Expected: compile error.

- [ ] **Step 9: Implement `parse_activation_cost`, `try_parse_effect_step`, `parse_ability_effect`**

```rust
use crate::types::ability::{AbilityEffect, ActivationCost, CostComponent, EffectStep};

fn parse_activation_cost(s: &str) -> ActivationCost {
    s.split(',')
        .map(|t| t.trim())
        .filter(|t| !t.is_empty())
        .map(|token| {
            if token == "{T}" {
                CostComponent::Tap
            } else if let Some(cost) = try_parse_mana_cost(token) {
                CostComponent::Mana(cost)
            } else {
                CostComponent::Unimplemented(token.to_string())
            }
        })
        .collect()
}

fn try_parse_effect_step(s: &str) -> Option<EffectStep> {
    let lower = s.to_lowercase();
    if lower.starts_with("add ") {
        let mana_str = s["add ".len()..].trim();
        return try_parse_mana_pool(mana_str).map(EffectStep::AddMana);
    }
    if lower == "draw a card" {
        return Some(EffectStep::DrawCard(1));
    }
    if lower.starts_with("draw ") && lower.ends_with(" cards") {
        let middle = &lower["draw ".len()..lower.len() - " cards".len()];
        if let Some(n) = parse_number_word(middle) {
            return Some(EffectStep::DrawCard(n));
        }
    }
    if lower.starts_with("mill ") {
        let rest = lower["mill ".len()..].trim_end_matches(" cards");
        if let Some(n) = parse_number_word(rest.trim()) {
            return Some(EffectStep::Mill(n));
        }
    }
    None
}

fn parse_ability_effect(s: &str) -> Option<AbilityEffect> {
    let s = s.trim_end_matches('.');
    s.split(". ")
        .filter(|step| !step.is_empty())
        .map(|step| try_parse_effect_step(step.trim()))
        .collect()
}
```

- [ ] **Step 10: Run tests to confirm pass**

```bash
cargo test 2>&1 | grep -E "^test result|FAILED|error\["
```
Expected: `test result: ok`.

- [ ] **Step 11: Commit**

```bash
git add src/parser/oracle.rs
git commit -m "feat: add activated ability parser helpers"
```

---

## Task 5: Parser Integration

**Files:**
- Modify: `src/parser/oracle.rs` (`parse_oracle_text` function)

Wire the colon check into `parse_oracle_text` as Step 2 of the paragraph-processing loop (after em-dash, before comma-split).

- [ ] **Step 1: Write failing integration tests**

Add to `src/parser/oracle.rs` tests:

```rust
#[test]
fn tap_add_green_parses_as_activated() {
    use crate::types::ability::{ActivatedAbility, CostComponent, EffectStep};
    use crate::types::mana::ManaPool;
    let result = parse_oracle_text("{T}: Add {G}.");
    assert_eq!(result.len(), 1);
    assert!(matches!(
        &result[0],
        OracleSpan::Parsed(AbilityAST::Activated(ActivatedAbility {
            cost,
            effect,
        })) if cost == &vec![CostComponent::Tap]
            && effect == &vec![EffectStep::AddMana(ManaPool { green: 1, ..Default::default() })]
    ));
}

#[test]
fn two_tap_add_two_green_parses_as_activated() {
    use crate::types::ability::{ActivatedAbility, CostComponent, EffectStep};
    use crate::types::mana::{ManaCost, ManaPool};
    let result = parse_oracle_text("{2}, {T}: Add {G}{G}.");
    assert_eq!(result.len(), 1);
    assert!(matches!(
        &result[0],
        OracleSpan::Parsed(AbilityAST::Activated(ActivatedAbility { cost, effect }))
        if cost == &vec![
            CostComponent::Mana(ManaCost { generic: 2, ..Default::default() }),
            CostComponent::Tap,
        ]
        && effect == &vec![EffectStep::AddMana(ManaPool { green: 2, ..Default::default() })]
    ));
}

#[test]
fn one_draw_a_card_parses_as_activated() {
    use crate::types::ability::{ActivatedAbility, CostComponent, EffectStep};
    use crate::types::mana::ManaCost;
    let result = parse_oracle_text("{1}: Draw a card.");
    assert_eq!(result.len(), 1);
    assert!(matches!(
        &result[0],
        OracleSpan::Parsed(AbilityAST::Activated(ActivatedAbility { cost, effect }))
        if cost == &vec![CostComponent::Mana(ManaCost { generic: 1, ..Default::default() })]
        && effect == &vec![EffectStep::DrawCard(1)]
    ));
}

#[test]
fn tap_mill_two_parses_as_activated() {
    use crate::types::ability::{ActivatedAbility, CostComponent, EffectStep};
    let result = parse_oracle_text("{T}: Mill 2.");
    assert_eq!(result.len(), 1);
    assert!(matches!(
        &result[0],
        OracleSpan::Parsed(AbilityAST::Activated(ActivatedAbility { cost, effect }))
        if cost == &vec![CostComponent::Tap]
        && effect == &vec![EffectStep::Mill(2)]
    ));
}

#[test]
fn reminder_text_colon_not_treated_as_activated() {
    // ({T}: Add {G}.) is reminder text — not an activated ability
    let result = parse_oracle_text("({T}: Add {G}.)");
    assert_eq!(result.len(), 1);
    assert!(matches!(&result[0], OracleSpan::Ignored(IgnoredKind::ReminderText, _)));
}

#[test]
fn sacrifice_cost_becomes_unimplemented_in_cost_activated_parsed() {
    use crate::types::ability::{ActivatedAbility, CostComponent, EffectStep};
    use crate::types::mana::ManaPool;
    let result = parse_oracle_text("Sacrifice a creature: Add {G}{G}.");
    assert_eq!(result.len(), 1);
    assert!(matches!(
        &result[0],
        OracleSpan::Parsed(AbilityAST::Activated(ActivatedAbility { cost, effect }))
        if cost == &vec![CostComponent::Unimplemented("Sacrifice a creature".to_string())]
        && effect == &vec![EffectStep::AddMana(ManaPool { green: 2, ..Default::default() })]
    ));
}

#[test]
fn unknown_effect_becomes_parsed_unimplemented() {
    let result = parse_oracle_text("{T}: Create a 1/1 token.");
    assert_eq!(result.len(), 1);
    assert!(matches!(&result[0], OracleSpan::ParsedUnimplemented(_)));
}
```

- [ ] **Step 2: Run to confirm failure**

```bash
cargo test "tap_add_green|two_tap_add|one_draw|tap_mill|reminder_text_colon|sacrifice_cost|unknown_effect" 2>&1 | grep -E "^test result|FAILED|error\["
```
Expected: failures (parser doesn't have colon check yet).

- [ ] **Step 3: Add colon check to `parse_oracle_text`**

In `src/parser/oracle.rs`, inside the `parse_oracle_text` function, add the colon check **after** the em-dash check and **before** the comma-split loop. Replace:

```rust
        // Split on commas at depth 0; classify each token.
        for token in split_at_depth_zero(paragraph, ',') {
```

with:

```rust
        // Colon check: activated ability ({cost}: effect).
        if let Some(colon_pos) = find_colon_at_depth_zero(paragraph) {
            let cost_str = paragraph[..colon_pos].trim();
            let effect_str = paragraph[colon_pos + 1..].trim();
            let cost = parse_activation_cost(cost_str);
            if !cost.is_empty() {
                if let Some(effect) = parse_ability_effect(effect_str) {
                    spans.push(OracleSpan::Parsed(AbilityAST::Activated(ActivatedAbility {
                        cost,
                        effect,
                    })));
                } else {
                    spans.push(OracleSpan::ParsedUnimplemented(paragraph.to_string()));
                }
                continue;
            }
        }

        // Split on commas at depth 0; classify each token.
        for token in split_at_depth_zero(paragraph, ',') {
```

Also add `ActivatedAbility` to the use at the top of `src/parser/oracle.rs`:
```rust
use crate::types::{AbilityAST, IgnoredKind, OracleSpan, ability::StaticAbility};
```
becomes:
```rust
use crate::types::{AbilityAST, IgnoredKind, OracleSpan, ability::{ActivatedAbility, StaticAbility}};
```

- [ ] **Step 4: Run all tests**

```bash
cargo test 2>&1 | grep -E "^test result|FAILED|error\["
```
Expected: `test result: ok`. The `llanowar_elves_loads_with_activated_ability` test from Task 3 should now pass too.

- [ ] **Step 5: Commit**

```bash
git add src/parser/oracle.rs
git commit -m "feat: parse {cost}: effect as ActivatedAbility spans"
```

---

## Task 6: Engine — `activate_ability`

**Files:**
- Modify: `src/engine/mod.rs`
- Create: `src/engine/activated.rs`

- [ ] **Step 1: Write failing engine tests in `src/engine/activated.rs`**

Create `src/engine/activated.rs` with just the tests (no implementation yet):

```rust
use super::EngineError;
use crate::types::{CardObject, GameState, ObjectId, PlayerId, Player, Zone};
use crate::types::card::{CardDefinition, CardType, TypeLine};
use crate::types::ability::{AbilityAST, ActivatedAbility, CostComponent, EffectStep, OracleSpan};
use crate::types::mana::{ManaCost, ManaPool};

pub fn activate_ability(
    _state: GameState,
    _object_id: ObjectId,
    _ability_index: usize,
    _activating_player: PlayerId,
) -> Result<GameState, EngineError> {
    unimplemented!()
}

pub fn can_pay_cost(
    _state: &GameState,
    _object_id: ObjectId,
    _ability: &ActivatedAbility,
    _player: PlayerId,
) -> bool {
    unimplemented!()
}

// ── Test helpers ──────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    fn two_player_state() -> GameState {
        GameState::new(vec![
            Player::new(PlayerId(0), "Alice"),
            Player::new(PlayerId(1), "Bob"),
        ])
    }

    fn make_tap_green_def() -> CardDefinition {
        // {T}: Add {G}.
        CardDefinition {
            name: "Llanowar Elves".into(),
            mana_cost: Some(ManaCost { green: 1, ..Default::default() }),
            type_line: TypeLine {
                supertypes: vec![],
                card_types: vec![CardType::Creature],
                subtypes: vec!["Elf".into(), "Druid".into()],
            },
            oracle_text: "{T}: Add {G}.".into(),
            abilities: vec![OracleSpan::Parsed(AbilityAST::Activated(ActivatedAbility {
                cost: vec![CostComponent::Tap],
                effect: vec![EffectStep::AddMana(ManaPool { green: 1, ..Default::default() })],
            }))],
            power: Some(1),
            toughness: Some(1),
        }
    }

    fn make_mill_def() -> CardDefinition {
        // {T}: Mill 2.
        CardDefinition {
            name: "Mill Thingy".into(),
            mana_cost: None,
            type_line: TypeLine {
                supertypes: vec![],
                card_types: vec![CardType::Artifact],
                subtypes: vec![],
            },
            oracle_text: "{T}: Mill 2.".into(),
            abilities: vec![OracleSpan::Parsed(AbilityAST::Activated(ActivatedAbility {
                cost: vec![CostComponent::Tap],
                effect: vec![EffectStep::Mill(2)],
            }))],
            power: None,
            toughness: None,
        }
    }

    fn make_draw_def() -> CardDefinition {
        // {1}: Draw a card.
        CardDefinition {
            name: "Staff".into(),
            mana_cost: None,
            type_line: TypeLine {
                supertypes: vec![],
                card_types: vec![CardType::Artifact],
                subtypes: vec![],
            },
            oracle_text: "{1}: Draw a card.".into(),
            abilities: vec![OracleSpan::Parsed(AbilityAST::Activated(ActivatedAbility {
                cost: vec![CostComponent::Mana(ManaCost { generic: 1, ..Default::default() })],
                effect: vec![EffectStep::DrawCard(1)],
            }))],
            power: None,
            toughness: None,
        }
    }

    fn place_on_battlefield(state: &mut GameState, def: CardDefinition, owner: PlayerId) -> ObjectId {
        let id = state.alloc_id();
        let mut obj = CardObject::new(id, def, owner, Zone::Battlefield);
        obj.summoning_sick = false;
        state.battlefield.push(id);
        state.add_object(obj);
        id
    }

    fn put_in_library(state: &mut GameState, owner: PlayerId) -> ObjectId {
        use crate::types::card::{CardType, TypeLine};
        let def = CardDefinition {
            name: "Dummy".into(),
            mana_cost: None,
            type_line: TypeLine { supertypes: vec![], card_types: vec![CardType::Creature], subtypes: vec![] },
            oracle_text: String::new(),
            abilities: vec![],
            power: Some(1),
            toughness: Some(1),
        };
        let id = state.alloc_id();
        let obj = CardObject::new(id, def, owner, Zone::Library);
        state.libraries.get_mut(&owner).unwrap().push(id);
        state.add_object(obj);
        id
    }

    #[test]
    fn tap_mana_ability_taps_and_adds_mana() {
        let mut gs = two_player_state();
        let id = place_on_battlefield(&mut gs, make_tap_green_def(), PlayerId(0));

        let gs = activate_ability(gs, id, 0, PlayerId(0)).unwrap();

        assert!(gs.objects[&id].tapped);
        assert_eq!(gs.get_player(PlayerId(0)).unwrap().mana_pool.green, 1);
    }

    #[test]
    fn tap_mana_ability_creates_checkpoint() {
        let mut gs = two_player_state();
        let id = place_on_battlefield(&mut gs, make_tap_green_def(), PlayerId(0));

        let gs = activate_ability(gs, id, 0, PlayerId(0)).unwrap();

        assert!(gs.mana_checkpoint.is_some());
        assert_eq!(gs.mana_checkpoint.as_ref().unwrap().tapped_lands, vec![id]);
    }

    #[test]
    fn already_tapped_returns_error() {
        let mut gs = two_player_state();
        let id = place_on_battlefield(&mut gs, make_tap_green_def(), PlayerId(0));
        gs.objects.get_mut(&id).unwrap().tapped = true;

        assert!(matches!(
            activate_ability(gs, id, 0, PlayerId(0)),
            Err(EngineError::AlreadyTapped)
        ));
    }

    #[test]
    fn summoning_sick_creature_with_tap_cost_returns_error() {
        let mut gs = two_player_state();
        let id = place_on_battlefield(&mut gs, make_tap_green_def(), PlayerId(0));
        gs.objects.get_mut(&id).unwrap().summoning_sick = true;

        assert!(matches!(
            activate_ability(gs, id, 0, PlayerId(0)),
            Err(EngineError::SummoningSick)
        ));
    }

    #[test]
    fn insufficient_mana_returns_error() {
        let mut gs = two_player_state();
        let id = place_on_battlefield(&mut gs, make_draw_def(), PlayerId(0));
        // pool is empty

        assert!(matches!(
            activate_ability(gs, id, 0, PlayerId(0)),
            Err(EngineError::InsufficientMana)
        ));
    }

    #[test]
    fn mana_cost_ability_deducts_mana() {
        let mut gs = two_player_state();
        let id = place_on_battlefield(&mut gs, make_draw_def(), PlayerId(0));
        gs.get_player_mut(PlayerId(0)).unwrap().mana_pool.colorless = 1;
        put_in_library(&mut gs, PlayerId(0));

        let gs = activate_ability(gs, id, 0, PlayerId(0)).unwrap();

        assert!(gs.get_player(PlayerId(0)).unwrap().mana_pool.is_empty());
        assert_eq!(gs.hands[&PlayerId(0)].len(), 1);
    }

    #[test]
    fn mill_two_moves_top_two_to_graveyard() {
        let mut gs = two_player_state();
        let id = place_on_battlefield(&mut gs, make_mill_def(), PlayerId(0));
        let card1 = put_in_library(&mut gs, PlayerId(0));
        let card2 = put_in_library(&mut gs, PlayerId(0));

        let gs = activate_ability(gs, id, 0, PlayerId(0)).unwrap();

        assert!(gs.libraries[&PlayerId(0)].is_empty());
        assert!(gs.graveyards[&PlayerId(0)].contains(&card1));
        assert!(gs.graveyards[&PlayerId(0)].contains(&card2));
    }

    #[test]
    fn mill_with_fewer_cards_than_n_mills_all_without_error() {
        let mut gs = two_player_state();
        let id = place_on_battlefield(&mut gs, make_mill_def(), PlayerId(0));
        let card1 = put_in_library(&mut gs, PlayerId(0));
        // only one card in library, ability mills 2

        let gs = activate_ability(gs, id, 0, PlayerId(0)).unwrap();

        assert!(gs.libraries[&PlayerId(0)].is_empty());
        assert!(gs.graveyards[&PlayerId(0)].contains(&card1));
    }

    #[test]
    fn ability_index_out_of_range_returns_error() {
        let mut gs = two_player_state();
        let id = place_on_battlefield(&mut gs, make_tap_green_def(), PlayerId(0));

        assert!(matches!(
            activate_ability(gs, id, 99, PlayerId(0)),
            Err(EngineError::AbilityIndexOutOfRange)
        ));
    }

    #[test]
    fn unimplemented_cost_component_is_skipped() {
        // Sacrifice cost: not enforced, ability still fires
        let def = CardDefinition {
            name: "Free Mill".into(),
            mana_cost: None,
            type_line: TypeLine {
                supertypes: vec![],
                card_types: vec![CardType::Artifact],
                subtypes: vec![],
            },
            oracle_text: "Sacrifice a creature: Mill 2.".into(),
            abilities: vec![OracleSpan::Parsed(AbilityAST::Activated(ActivatedAbility {
                cost: vec![CostComponent::Unimplemented("Sacrifice a creature".into())],
                effect: vec![EffectStep::Mill(2)],
            }))],
            power: None,
            toughness: None,
        };
        let mut gs = two_player_state();
        let id = place_on_battlefield(&mut gs, def, PlayerId(0));
        put_in_library(&mut gs, PlayerId(0));
        put_in_library(&mut gs, PlayerId(0));

        let gs = activate_ability(gs, id, 0, PlayerId(0)).unwrap();
        assert!(gs.libraries[&PlayerId(0)].is_empty());
    }

    #[test]
    fn can_pay_cost_true_when_untapped_and_mana_sufficient() {
        let mut gs = two_player_state();
        let id = place_on_battlefield(&mut gs, make_tap_green_def(), PlayerId(0));
        let ability = ActivatedAbility {
            cost: vec![CostComponent::Tap],
            effect: vec![EffectStep::AddMana(ManaPool::default())],
        };
        assert!(can_pay_cost(&gs, id, &ability, PlayerId(0)));
    }

    #[test]
    fn can_pay_cost_false_when_tapped() {
        let mut gs = two_player_state();
        let id = place_on_battlefield(&mut gs, make_tap_green_def(), PlayerId(0));
        gs.objects.get_mut(&id).unwrap().tapped = true;
        let ability = ActivatedAbility {
            cost: vec![CostComponent::Tap],
            effect: vec![],
        };
        assert!(!can_pay_cost(&gs, id, &ability, PlayerId(0)));
    }
}
```

- [ ] **Step 2: Add `AbilityIndexOutOfRange` and `pub mod activated` to engine**

In `src/engine/mod.rs`:

```rust
pub mod casting;
pub mod combat;
pub mod mana;
pub mod state_based_actions;
pub mod turn;
pub mod activated;   // ← add this line

#[derive(Debug, Clone, PartialEq)]
pub enum EngineError {
    CardNotFound,
    CardNotInHand,
    CardNotOnBattlefield,
    AlreadyTapped,
    InsufficientMana,
    CannotCastNow,
    LandLimitReached,
    NotALand,
    NotACreature,
    NotYourCard,
    SummoningSick,
    CreatureTapped,
    InvalidBlocker,
    MenaceRequiresTwoBlockers,
    NoManaCheckpoint,
    AbilityIndexOutOfRange,   // ← add this line
}
```

- [ ] **Step 3: Run to confirm tests compile and fail on `unimplemented!()`**

```bash
cargo test "activated::" 2>&1 | grep -E "^test result|FAILED|error\[|panicked"
```
Expected: tests run but panic on `unimplemented!()`.

- [ ] **Step 4: Implement `activate_ability` and `can_pay_cost`**

Replace the stub implementations in `src/engine/activated.rs` with:

```rust
use super::EngineError;
use crate::engine::mana::pay_mana_cost;
use crate::engine::turn::draw_card;
use crate::types::ability::{AbilityAST, ActivatedAbility, CostComponent, EffectStep, OracleSpan};
use crate::types::ability::StaticAbility;
use crate::types::{GameState, ManaCheckpoint, ObjectId, PlayerId, Zone};

pub fn activate_ability(
    mut state: GameState,
    object_id: ObjectId,
    ability_index: usize,
    activating_player: PlayerId,
) -> Result<GameState, EngineError> {
    // Validate object
    {
        let obj = state.objects.get(&object_id).ok_or(EngineError::CardNotFound)?;
        if obj.zone != Zone::Battlefield {
            return Err(EngineError::CardNotOnBattlefield);
        }
        if obj.controller != activating_player {
            return Err(EngineError::NotYourCard);
        }
    }

    // Get the ability at index
    let ability: ActivatedAbility = state
        .objects
        .get(&object_id)
        .unwrap()
        .definition
        .abilities
        .iter()
        .filter_map(|span| match span {
            OracleSpan::Parsed(AbilityAST::Activated(a)) => Some(a.clone()),
            _ => None,
        })
        .nth(ability_index)
        .ok_or(EngineError::AbilityIndexOutOfRange)?;

    // Check costs (read-only)
    for component in &ability.cost {
        match component {
            CostComponent::Tap => {
                let obj = state.objects.get(&object_id).unwrap();
                if obj.tapped {
                    return Err(EngineError::AlreadyTapped);
                }
                if obj.is_creature()
                    && obj.summoning_sick
                    && !obj.has_keyword(StaticAbility::Haste)
                {
                    return Err(EngineError::SummoningSick);
                }
            }
            CostComponent::Mana(cost) => {
                let pool = &state
                    .get_player(activating_player)
                    .ok_or(EngineError::CardNotFound)?
                    .mana_pool;
                if pool.white < cost.white
                    || pool.blue < cost.blue
                    || pool.black < cost.black
                    || pool.red < cost.red
                    || pool.green < cost.green
                    || pool.colorless < cost.colorless
                {
                    return Err(EngineError::InsufficientMana);
                }
                let after_colored = pool.total() - cost.total_colored();
                if after_colored < cost.generic {
                    return Err(EngineError::InsufficientMana);
                }
            }
            _ => {} // Unimplemented, PayLife, Sacrifice, Discard — not enforced
        }
    }

    // If this is a mana ability, create checkpoint before paying anything
    let produces_mana = ability.effect.iter().any(|e| matches!(e, EffectStep::AddMana(_)));
    if produces_mana && state.mana_checkpoint.is_none() {
        let pools = state
            .players
            .iter()
            .map(|p| (p.id, p.mana_pool.clone()))
            .collect();
        state.mana_checkpoint = Some(ManaCheckpoint { pools, tapped_lands: vec![] });
    }

    // Pay costs
    for component in ability.cost.clone().iter() {
        match component {
            CostComponent::Tap => {
                if produces_mana {
                    state
                        .mana_checkpoint
                        .as_mut()
                        .unwrap()
                        .tapped_lands
                        .push(object_id);
                }
                state.objects.get_mut(&object_id).unwrap().tapped = true;
            }
            CostComponent::Mana(cost) => {
                state = pay_mana_cost(state, activating_player, cost)?;
            }
            _ => {}
        }
    }

    // Apply effects
    for step in &ability.effect {
        match step {
            EffectStep::AddMana(pool) => {
                let player = state.get_player_mut(activating_player).unwrap();
                player.mana_pool.white += pool.white;
                player.mana_pool.blue += pool.blue;
                player.mana_pool.black += pool.black;
                player.mana_pool.red += pool.red;
                player.mana_pool.green += pool.green;
                player.mana_pool.colorless += pool.colorless;
            }
            EffectStep::Mill(n) => {
                let to_mill = (*n as usize).min(
                    state.libraries.get(&activating_player).map_or(0, |l| l.len()),
                );
                for _ in 0..to_mill {
                    if let Some(card_id) = state
                        .libraries
                        .get_mut(&activating_player)
                        .filter(|l| !l.is_empty())
                        .map(|l| l.remove(0))
                    {
                        state
                            .graveyards
                            .get_mut(&activating_player)
                            .unwrap()
                            .push(card_id);
                        if let Some(obj) = state.objects.get_mut(&card_id) {
                            obj.zone = Zone::Graveyard;
                        }
                    }
                }
            }
            EffectStep::DrawCard(n) => {
                for _ in 0..*n {
                    state = draw_card(state, activating_player);
                }
            }
        }
    }

    Ok(state)
}

pub fn can_pay_cost(
    state: &GameState,
    object_id: ObjectId,
    ability: &ActivatedAbility,
    player: PlayerId,
) -> bool {
    for component in &ability.cost {
        match component {
            CostComponent::Tap => {
                let obj = match state.objects.get(&object_id) {
                    Some(o) if o.zone == Zone::Battlefield => o,
                    _ => return false,
                };
                if obj.tapped {
                    return false;
                }
                if obj.is_creature()
                    && obj.summoning_sick
                    && !obj.has_keyword(StaticAbility::Haste)
                {
                    return false;
                }
            }
            CostComponent::Mana(cost) => {
                let pool = match state.get_player(player) {
                    Some(p) => &p.mana_pool,
                    None => return false,
                };
                if pool.white < cost.white
                    || pool.blue < cost.blue
                    || pool.black < cost.black
                    || pool.red < cost.red
                    || pool.green < cost.green
                    || pool.colorless < cost.colorless
                {
                    return false;
                }
                let after_colored = pool.total() - cost.total_colored();
                if after_colored < cost.generic {
                    return false;
                }
            }
            _ => {}
        }
    }
    true
}
```

- [ ] **Step 5: Run all tests**

```bash
cargo test 2>&1 | grep -E "^test result|FAILED|error\["
```
Expected: `test result: ok`.

- [ ] **Step 6: Commit**

```bash
git add src/engine/mod.rs src/engine/activated.rs
git commit -m "feat: implement activate_ability and can_pay_cost engine functions"
```

---

## Task 7: `serve.rs` — View Types, Endpoint, Oracle Text Arm

**Files:**
- Modify: `src/serve.rs`

- [ ] **Step 1: Add imports and formatting helpers**

Add to the imports section at the top of `src/serve.rs`:
```rust
use mecha_oracle::engine::activated::{activate_ability, can_pay_cost};
use mecha_oracle::types::ability::{AbilityAST, ActivatedAbility, CostComponent, EffectStep, OracleSpan};
```

Add these private functions after the existing `format_mana_cost` and `format_type_line` helpers:

```rust
fn format_mana_cost_braced(cost: &mecha_oracle::types::mana::ManaCost) -> String {
    let mut s = String::new();
    if cost.generic > 0 { s.push_str(&format!("{{{}}}", cost.generic)); }
    for _ in 0..cost.white { s.push_str("{W}"); }
    for _ in 0..cost.blue { s.push_str("{U}"); }
    for _ in 0..cost.black { s.push_str("{B}"); }
    for _ in 0..cost.red { s.push_str("{R}"); }
    for _ in 0..cost.green { s.push_str("{G}"); }
    for _ in 0..cost.colorless { s.push_str("{C}"); }
    s
}

fn format_mana_pool(pool: &mecha_oracle::types::mana::ManaPool) -> String {
    let mut s = String::new();
    for _ in 0..pool.white { s.push_str("{W}"); }
    for _ in 0..pool.blue { s.push_str("{U}"); }
    for _ in 0..pool.black { s.push_str("{B}"); }
    for _ in 0..pool.red { s.push_str("{R}"); }
    for _ in 0..pool.green { s.push_str("{G}"); }
    for _ in 0..pool.colorless { s.push_str("{C}"); }
    s
}

fn format_activated_ability(ability: &ActivatedAbility) -> String {
    let cost_parts: Vec<String> = ability
        .cost
        .iter()
        .map(|c| match c {
            CostComponent::Tap => "{T}".to_string(),
            CostComponent::Mana(m) => format_mana_cost_braced(m),
            CostComponent::PayLife(n) => format!("Pay {n} life"),
            CostComponent::Sacrifice(n, _) => format!("Sacrifice {n}"),
            CostComponent::Discard(n, _) => format!("Discard {n}"),
            CostComponent::Unimplemented(s) => s.clone(),
        })
        .collect();
    let effect_parts: Vec<String> = ability
        .effect
        .iter()
        .map(|e| match e {
            EffectStep::AddMana(pool) => format!("Add {}", format_mana_pool(pool)),
            EffectStep::Mill(n) => format!("Mill {n}"),
            EffectStep::DrawCard(n) => {
                if *n == 1 { "Draw a card".to_string() } else { format!("Draw {n} cards") }
            }
        })
        .collect();
    format!("{}: {}", cost_parts.join(", "), effect_parts.join(". "))
}
```

- [ ] **Step 2: Add `ActivatedAbilityView` and update `CardView`**

Add after the existing `OracleSpanView` struct:

```rust
#[derive(Serialize)]
struct ActivatedAbilityView {
    index: usize,
    label: String,
    can_activate: bool,
}
```

Add `activated_abilities: Vec<ActivatedAbilityView>` to `CardView`:

```rust
struct CardView {
    id: ObjectId,
    name: String,
    type_line: String,
    oracle_text: Vec<OracleSpanView>,
    mana_cost: Option<String>,
    power: Option<i32>,
    toughness: Option<i32>,
    tapped: bool,
    summoning_sick: bool,
    damage_marked: u32,
    is_attacking: bool,
    is_blocking: bool,
    can_attack: bool,
    can_block: bool,
    activated_abilities: Vec<ActivatedAbilityView>,   // ← add
}
```

- [ ] **Step 3: Update `to_card_view` — oracle_text arm and activated_abilities field**

In `build_player_view`, inside the `to_card_view` closure, add a match arm for `Parsed(Activated(_))` **before** the existing catch-all `_ =>` arm:

```rust
OracleSpan::Parsed(AbilityAST::Activated(a)) => OracleSpanView {
    kind: SpanKind::Parsed,
    text: format_activated_ability(a),
    ignored_kind: None,
},
```

Add the `activated_abilities` field to the `CardView { ... }` construction:

```rust
activated_abilities: obj
    .definition
    .abilities
    .iter()
    .filter_map(|span| match span {
        OracleSpan::Parsed(AbilityAST::Activated(a)) => Some(a),
        _ => None,
    })
    .enumerate()
    .map(|(i, ability)| ActivatedAbilityView {
        index: i,
        label: format_activated_ability(ability),
        can_activate: can_pay_cost(state, obj.id, ability, pid),
    })
    .collect(),
```

- [ ] **Step 4: Add `ActivateAbility` to `ActionRequest` and `dispatch_action`**

Add variant to `ActionRequest`:
```rust
ActivateAbility { object_id: u64, ability_index: usize },
```

Add arm to `dispatch_action`:
```rust
ActionRequest::ActivateAbility { object_id, ability_index } => {
    let player = state.priority_player;
    activate_ability(state, ObjectId(object_id), ability_index, player)
        .map_err(|e| format!("{e:?}"))
}
```

- [ ] **Step 5: Build to confirm no compile errors**

```bash
cargo build 2>&1 | grep -E "error\["
```
Expected: no errors.

- [ ] **Step 6: Run all tests**

```bash
cargo test 2>&1 | grep -E "^test result|FAILED|error\["
```
Expected: `test result: ok`.

- [ ] **Step 7: Commit**

```bash
git add src/serve.rs
git commit -m "feat: expose activated abilities in CardView and add ActivateAbility action"
```

---

## Task 8: Frontend — Sidebar Items for Activated Abilities

**Files:**
- Modify: `src/serve.html`

- [ ] **Step 1: Add activated abilities to the sidebar in `renderPanes`**

In `src/serve.html`, inside the `renderPanes` function, find the "Reset mana" block:

```js
    // Reset mana (AP only, when checkpoint exists)
    if (iAmAP && s.can_reset_mana) {
      html += group('Mana', [`<button class="action-btn" onclick="sendAction({type:'reset_mana'})">↩ Reset mana</button>`]);
    }
```

**Before** the reset-mana block, add:

```js
    // Activated abilities: show for priority player during main phases (any activatable)
    if (pid === s.priority_player && (s.step === 'PreCombatMain' || s.step === 'PostCombatMain')) {
      const allCards = [...myData.creatures, ...myData.lands];
      const activatables = [];
      for (const card of allCards) {
        if (!card.activated_abilities) continue;
        for (const ab of card.activated_abilities) {
          activatables.push({ card, ab });
        }
      }
      if (activatables.length > 0) {
        const btns = activatables.map(({ card, ab }) => {
          const disabled = !ab.can_activate ? ' disabled' : '';
          return `<button class="action-btn"${disabled} onclick="sendAction({type:'activate_ability',object_id:${card.id},ability_index:${ab.index}})">${esc(card.name)} — ${esc(ab.label)}</button>`;
        });
        html += group('Activated abilities', btns);
      }
    }
```

- [ ] **Step 2: Build and do a quick smoke-test**

```bash
cargo build 2>&1 | grep -E "error\["
```
Expected: no errors.

Start the server with the basic deck to verify the UI renders without crashing (Ctrl-C after confirming the page loads):
```bash
cargo run -- serve --deck docs/test-decks/basic.json 2>&1 &
sleep 2 && curl -s http://localhost:3000/state | python3 -m json.tool | grep -A3 "activated_abilities" | head -20
kill %1
```
Expected: JSON output showing `activated_abilities` arrays on cards. (They'll be empty for Forest/Grizzly Bears which have no activated abilities.)

- [ ] **Step 3: Run all tests**

```bash
cargo test 2>&1 | grep -E "^test result|FAILED|error\["
```
Expected: `test result: ok`.

- [ ] **Step 4: Commit**

```bash
git add src/serve.html
git commit -m "feat: render activated ability buttons in sidebar"
```
