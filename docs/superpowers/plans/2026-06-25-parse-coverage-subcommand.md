# Parse Coverage Subcommand Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add a `parse-coverage` subcommand that iterates the full card database and prints a human-readable report classifying how many cards are fully parsed, partially parsed, or opaque, with breakdown tables showing which unimplemented patterns appear most often.

**Architecture:** Add `CardDatabase::iter()`, implement card classification and frequency counting in a new `run_parse_coverage()` function in `main.rs`, then wire it into the `Command` enum via clap. The report is plain text to stdout — no new dependencies.

**Tech Stack:** Rust, clap (already a dependency), standard library `BTreeMap` for sorted output.

## Global Constraints

- Run `cargo test 2>&1 | grep -E "^test result|FAILED|error\["` — must be clean before each commit.
- Run `cargo clippy --all-targets` — must be warning/error-free before each commit.
- Do not touch `parser/oracle.rs`, `types/ability.rs`, or `serve.js` — those are modified by the parallel annotation-completeness plan.
- All new test functions live in the existing `#[cfg(test)] mod tests` block in the same file as the code under test.

---

### Task 1: Add `CardDatabase::iter()`

**Files:**
- Modify: `src/cards/mod.rs`

**Interfaces:**
- Produces: `pub fn iter(&self) -> impl Iterator<Item = &CardDefinition>` on `CardDatabase`

- [ ] **Step 1: Write the failing test**

In `src/cards/mod.rs`, inside `mod tests`:

```rust
#[test]
fn iter_returns_all_non_token_cards() {
    let db = test_db();
    // The test fixture has a known number of regular cards — iter should return them.
    // We just verify it is non-empty and doesn't include tokens.
    let cards: Vec<_> = db.iter().collect();
    assert!(!cards.is_empty(), "iter() must return at least one card");
    // Every result must have a name (basic sanity)
    assert!(cards.iter().all(|c| !c.name.is_empty()));
}
```

- [ ] **Step 2: Run test to verify it fails**

```bash
cargo test iter_returns_all_non_token_cards 2>&1 | grep -E "^test result|FAILED|error\["
```

Expected: compile error — `iter` method does not exist yet.

- [ ] **Step 3: Add `iter()` to `CardDatabase`**

In `src/cards/mod.rs`, add after `get_token`:

```rust
/// Iterates all non-token card definitions in the database.
/// Order is unspecified (hash-map iteration).
pub fn iter(&self) -> impl Iterator<Item = &CardDefinition> {
    self.inner.values()
}
```

- [ ] **Step 4: Run test to verify it passes**

```bash
cargo test iter_returns_all_non_token_cards 2>&1 | grep -E "^test result|FAILED|error\["
```

Expected: `test result: ok. 1 passed`

- [ ] **Step 5: Run full suite and clippy**

```bash
cargo test 2>&1 | grep -E "^test result|FAILED|error\["
cargo clippy --all-targets 2>&1 | grep -E "^error|^warning"
```

Both must be clean.

- [ ] **Step 6: Commit**

```bash
git add src/cards/mod.rs
git commit -m "feat: add CardDatabase::iter() for full-database traversal"
```

---

### Task 2: Implement `parse-coverage` subcommand

**Files:**
- Modify: `src/main.rs` — add `Command::ParseCoverage`, `run_parse_coverage()`

**Interfaces:**
- Consumes: `CardDatabase::iter()` (Task 1), existing `RulesText`, `Rule`, `EffectStep`, `CostComponent` from `mecha_oracle::types`.

**Card classification logic:**

A card is scanned by walking `card.rules_text`. Define these helpers (internal to `run_parse_coverage`):

```
has_active(card)          → card.rules_text.iter().any(|s| matches!(s, RulesText::Active(_)))
has_unimplemented(card)   → any of:
    RulesText::Unparsed(_)
    RulesText::ParsedUnimplemented(_)
    RulesText::Active(Rule::Activated(ab)) where ab.cost has CostComponent::Unimplemented(_)
    RulesText::Active(Rule::Activated(ab)) where ab.effect has EffectStep::Unimplemented(_)
    RulesText::Active(Rule::SpellAbility(sa)) where sa.steps has EffectStep::Unimplemented(_)
    RulesText::Active(Rule::Triggered(ta)) where ta.effect has EffectStep::Unimplemented(_)
```

Then:
- **Clean**: `!has_unimplemented(card)`
- **Opaque**: `!has_active(card) && has_unimplemented(card)`
- **Partial**: `has_active(card) && has_unimplemented(card)`

**Frequency counting logic:**

Walk every card collecting into four `HashMap<String, usize>`:
1. `effect_step_counts` — `EffectStep::Unimplemented(s)` → lowercase `s` as key
2. `keyword_counts` — `ParsedUnimplemented(s)` → first space-delimited token of lowercase `s` as key (so `"flashback {2}{u}"` → `"flashback"`)
3. `cost_counts` — `CostComponent::Unimplemented(s)` → lowercase `s` as key
4. `unparsed_counts` — `Unparsed(s)` → lowercase trimmed `s` as key

- [ ] **Step 1: Write a failing test**

In `src/main.rs`, inside `mod tests` (add a new test below the existing CLI tests):

```rust
#[test]
fn parse_coverage_classification_clean_card() {
    use mecha_oracle::types::RulesText;
    // Use the test fixture database (does not require a production database download).
    // Serra Angel is in tests/fixtures/oracle_cards_test.json and has only implemented keywords.
    let db = mecha_oracle::cards::test_helpers::test_db();
    let card = db.get("Serra Angel").expect("Serra Angel not found in test fixture");
    let has_active = card.rules_text.iter().any(|s| matches!(s, RulesText::Active(_)));
    let has_unimpl = card_has_unimplemented(card);
    assert!(has_active, "Serra Angel should have Active spans");
    assert!(!has_unimpl, "Serra Angel should have no unimplemented signals");
}
```

This test references `card_has_unimplemented` which doesn't exist yet — it will be added in the next step as a free function in `main.rs`.

- [ ] **Step 2: Run test to verify it fails**

```bash
cargo test parse_coverage_classification_clean_card 2>&1 | grep -E "^test result|FAILED|error\["
```

Expected: compile error.

- [ ] **Step 3: Implement `card_has_unimplemented` and wire the subcommand**

In `src/main.rs`, add the following (not inside `mod serve` — at module level).

`CostComponent`, `EffectStep`, `Rule`, and `RulesText` are all re-exported from `mecha_oracle::types` (see `src/types/mod.rs`), so use the top-level path:

```rust
use mecha_oracle::types::{CardDefinition, CostComponent, EffectStep, Rule, RulesText};

/// Returns true if `card` contains any unimplemented signal: an Unparsed span,
/// a ParsedUnimplemented span, or an Active span whose cost/effect/steps include
/// any Unimplemented components.
fn card_has_unimplemented(card: &CardDefinition) -> bool {
    card.rules_text.iter().any(|span| match span {
        RulesText::Unparsed(_) => true,
        RulesText::ParsedUnimplemented(_) => true,
        RulesText::Active(Rule::Activated(ab)) => {
            ab.cost.iter().any(|c| matches!(c, CostComponent::Unimplemented(_)))
                || ab.effect.iter().any(|s| matches!(s, EffectStep::Unimplemented(_)))
        }
        RulesText::Active(Rule::SpellAbility(sa)) => {
            sa.steps.iter().any(|s| matches!(s, EffectStep::Unimplemented(_)))
        }
        RulesText::Active(Rule::Triggered(ta)) => {
            ta.effect.iter().any(|s| matches!(s, EffectStep::Unimplemented(_)))
        }
        _ => false,
    })
}
```

Add `Command::ParseCoverage` to the `Command` enum:

```rust
/// Parse every card in the database and print a coverage report.
ParseCoverage,
```

Add the match arm in `main`:

```rust
Command::ParseCoverage => run_parse_coverage(),
```

Implement `run_parse_coverage`:

```rust
fn run_parse_coverage() {
    use std::collections::HashMap;
    use mecha_oracle::cards::CardDatabase;

    let db = CardDatabase::open()
        .expect("Card database not found — run `mecha-oracle update-cards` first");

    let mut clean = 0usize;
    let mut partial = 0usize;
    let mut opaque = 0usize;

    let mut effect_step_counts: HashMap<String, usize> = HashMap::new();
    let mut keyword_counts: HashMap<String, usize> = HashMap::new();
    let mut cost_counts: HashMap<String, usize> = HashMap::new();
    let mut unparsed_counts: HashMap<String, usize> = HashMap::new();

    for card in db.iter() {
        let has_active = card.rules_text.iter().any(|s| matches!(s, RulesText::Active(_)));
        let has_unimpl = card_has_unimplemented(card);

        if !has_unimpl {
            clean += 1;
        } else if !has_active {
            opaque += 1;
        } else {
            partial += 1;
        }

        // Collect breakdown data
        for span in &card.rules_text {
            match span {
                RulesText::Unparsed(s) => {
                    *unparsed_counts
                        .entry(s.trim().to_lowercase())
                        .or_insert(0) += 1;
                }
                RulesText::ParsedUnimplemented(s) => {
                    let key = s
                        .split_whitespace()
                        .next()
                        .unwrap_or(s)
                        .to_lowercase();
                    *keyword_counts.entry(key).or_insert(0) += 1;
                }
                RulesText::Active(Rule::Activated(ab)) => {
                    for c in &ab.cost {
                        if let CostComponent::Unimplemented(s) = c {
                            *cost_counts
                                .entry(s.to_lowercase())
                                .or_insert(0) += 1;
                        }
                    }
                    for s in &ab.effect {
                        if let EffectStep::Unimplemented(text) = s {
                            *effect_step_counts
                                .entry(text.to_lowercase())
                                .or_insert(0) += 1;
                        }
                    }
                }
                RulesText::Active(Rule::SpellAbility(sa)) => {
                    for s in &sa.steps {
                        if let EffectStep::Unimplemented(text) = s {
                            *effect_step_counts
                                .entry(text.to_lowercase())
                                .or_insert(0) += 1;
                        }
                    }
                }
                RulesText::Active(Rule::Triggered(ta)) => {
                    for s in &ta.effect {
                        if let EffectStep::Unimplemented(text) = s {
                            *effect_step_counts
                                .entry(text.to_lowercase())
                                .or_insert(0) += 1;
                        }
                    }
                }
                _ => {}
            }
        }
    }

    let total = clean + partial + opaque;
    let pct = |n: usize| if total > 0 { n as f64 * 100.0 / total as f64 } else { 0.0 };

    println!("=== Parse Coverage Report ===");
    println!("Total cards:          {total:>7}");
    println!();
    println!("Clean (fully parsed): {clean:>7}  ({:.1}%)", pct(clean));
    println!("Partial:              {partial:>7}  ({:.1}%)", pct(partial));
    println!("Opaque (no Active):   {opaque:>7}  ({:.1}%)", pct(opaque));

    print_top_15("Top unimplemented effect steps", &effect_step_counts, 15);
    print_top_15("Top unimplemented keywords", &keyword_counts, 15);
    print_top_15("Top unimplemented activation costs", &cost_counts, 15);
    print_top_15("Top unparsed paragraphs", &unparsed_counts, 10);
}

fn print_top_15(header: &str, counts: &std::collections::HashMap<String, usize>, n: usize) {
    if counts.is_empty() {
        return;
    }
    let mut entries: Vec<_> = counts.iter().collect();
    entries.sort_by(|a, b| b.1.cmp(a.1).then(a.0.cmp(b.0)));
    println!();
    println!("=== {header} (top {n}) ===");
    for (i, (text, count)) in entries.iter().take(n).enumerate() {
        println!("  {:>2}. {:.<60}  × {:>6}", i + 1, format!("\"{text}\""), count);
    }
}
```

Note: `mecha_oracle::types::ability` and `mecha_oracle::types::effect` re-exports — verify the import paths compile; if `CostComponent` or `EffectStep` are not re-exported from `mecha_oracle::types` directly, use the full module path `mecha_oracle::types::ability::CostComponent` etc. Check `src/types/mod.rs` for what is `pub use`-d.

- [ ] **Step 4: Run the test to verify it passes**

```bash
cargo test parse_coverage_classification_clean_card 2>&1 | grep -E "^test result|FAILED|error\["
```

Expected: `test result: ok. 1 passed`

- [ ] **Step 5: Add a CLI test for the new subcommand**

In `mod tests` in `src/main.rs`:

```rust
#[test]
fn cli_parse_coverage_subcommand() {
    let cli = Cli::try_parse_from(["mecha-oracle", "parse-coverage"]).unwrap();
    assert!(matches!(cli.command, Command::ParseCoverage));
}
```

Run:

```bash
cargo test cli_parse_coverage_subcommand 2>&1 | grep -E "^test result|FAILED|error\["
```

Expected: `test result: ok. 1 passed`

- [ ] **Step 6: Run full suite and clippy**

```bash
cargo test 2>&1 | grep -E "^test result|FAILED|error\["
cargo clippy --all-targets 2>&1 | grep -E "^error|^warning"
```

Both must be clean. Common clippy issue: unused `pct` lambda — either remove it or use it.

- [ ] **Step 7: Smoke-test the subcommand**

```bash
cargo run -- parse-coverage 2>&1 | head -30
```

Verify the report prints without panicking and shows sensible numbers. The total should be in the range of 20,000–30,000 cards if the database is populated.

- [ ] **Step 8: Commit**

```bash
git add src/main.rs
git commit -m "feat: add parse-coverage subcommand with classification and breakdown tables"
```
