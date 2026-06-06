# Oracle Span Parsing — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Replace the fail-fast oracle text parser with a fault-tolerant span-annotated parser that loads all cards and annotates oracle text for UI rendering.

**Architecture:** `parse_oracle_text` becomes infallible, returning `Vec<OracleSpan>` — typed spans the engine queries for abilities (`Parsed`) and the UI renders per-kind (`Ignored` = italics, `Unparsed` = red+underline). `CardDefinition.abilities` changes from `Vec<AbilityAST>` to `Vec<OracleSpan>`. `ParseError` is removed.

**Tech Stack:** Rust 2024 edition (rustc ≥ 1.85), serde 1.x with derive (already a dependency).

---

## File Map

| File | Change |
|---|---|
| `src/types/ability.rs` | Add `IgnoredKind` (with `Serialize`), `OracleSpan`, `StaticAbility::display_name` |
| `src/types/mod.rs` | Re-export `OracleSpan`, `IgnoredKind` |
| `src/types/card.rs` | `CardDefinition.abilities` type; `has_unparsed()` |
| `src/types/card_object.rs` | Update `has_keyword` |
| `src/parser/mod.rs` | Remove `ParseError`; update re-exports |
| `src/parser/oracle.rs` | Full rewrite: infallible `Vec<OracleSpan>` |
| `src/cards/scryfall.rs` | Remove `.map_err()` on oracle parse |
| `src/cards/mod.rs` | `unparsed_count()`; update load-time tracing |
| `src/serve.rs` | `SpanKind`, `OracleSpanView`, update `CardView` + mapping |
| `src/serve.html` | `renderOracleText()`; replace all oracle text renders |
| `tests/fixtures/oracle_cards_test.json` | Add ability-word card |

---

## Task 1: `OracleSpan` and `IgnoredKind` types

**Files:**
- Modify: `src/types/ability.rs`
- Modify: `src/types/mod.rs`

- [ ] **Step 1: Write a failing test**

Add a `#[cfg(test)]` block at the bottom of `src/types/ability.rs`:

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn oracle_span_variants_are_comparable() {
        let a = OracleSpan::Parsed(AbilityAST::Static(StaticAbility::Flying));
        let b = OracleSpan::Parsed(AbilityAST::Static(StaticAbility::Flying));
        assert_eq!(a, b);

        let c = OracleSpan::Ignored(IgnoredKind::ReminderText, "(reminder)".into());
        let d = OracleSpan::Ignored(IgnoredKind::ReminderText, "(reminder)".into());
        assert_eq!(c, d);

        let e = OracleSpan::Unparsed("When this enters".into());
        assert_ne!(a, e);
    }
}
```

- [ ] **Step 2: Run to confirm failure**

```bash
cargo test types::ability
```

Expected: compile error — `OracleSpan` and `IgnoredKind` not defined.

- [ ] **Step 3: Add the new types to `src/types/ability.rs`**

Append after the existing `AbilityAST` enum:

```rust
/// Classifies oracle text that has no rules effect and is rendered in italics.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "snake_case")]
pub enum IgnoredKind {
    /// Parenthetical reminder text, e.g. "(This creature can't block.)".
    ReminderText,
    /// Ability words (CR 207.2c) and flavour words (CR 207.2d) that precede an em-dash,
    /// e.g. "Landfall \u{2014}" or "Cumulative upkeep\u{2014}".
    AbilityWord,
}

/// A typed span of oracle text.
/// The ordered sequence of spans represents the full oracle text.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum OracleSpan {
    /// A recognised ability the engine can act on.
    Parsed(AbilityAST),
    /// Non-rules text — displayed in italics in the UI.
    Ignored(IgnoredKind, String),
    /// Text the parser could not interpret — displayed red+underline in the UI.
    Unparsed(String),
}
```

- [ ] **Step 4: Update `src/types/mod.rs`**

Find the existing `pub use ability::{...}` line and add `IgnoredKind` and `OracleSpan`:

```rust
pub use ability::{
    AbilityAST, ActivatedAbility, IgnoredKind, OracleSpan,
    StaticAbility, TriggerEvent, TriggeredAbility,
};
```

- [ ] **Step 5: Run the test**

```bash
cargo test types::ability
```

Expected: `oracle_span_variants_are_comparable` passes.

- [ ] **Step 6: Verify no regressions**

```bash
cargo test 2>&1 | grep -E "^(test result|FAILED)"
```

Expected: all tests pass.

- [ ] **Step 7: Commit**

```bash
git add src/types/ability.rs src/types/mod.rs
git commit -m "feat: add OracleSpan and IgnoredKind types"
```

---

## Task 2: Rewrite `parse_oracle_text`

**Files:**
- Modify: `src/parser/oracle.rs`
- Modify: `src/parser/mod.rs`

- [ ] **Step 1: Replace the entire contents of `src/parser/oracle.rs` with the new implementation and tests**

```rust
use crate::types::{AbilityAST, IgnoredKind, OracleSpan, ability::StaticAbility};

// ── Private helpers ──────────────────────────────────────────────────────────

/// Returns the byte offset of the first `target` char at parenthetical depth 0,
/// or `None` if not found.
fn find_at_depth_zero(text: &str, target: char) -> Option<usize> {
    let mut depth = 0usize;
    for (i, c) in text.char_indices() {
        match c {
            '(' => depth += 1,
            ')' => depth = depth.saturating_sub(1),
            c if c == target && depth == 0 => return Some(i),
            _ => {}
        }
    }
    None
}

/// Splits `text` on `sep` characters at parenthetical depth 0.
fn split_at_depth_zero<'a>(text: &'a str, sep: char) -> Vec<&'a str> {
    let mut result = Vec::new();
    let mut depth = 0usize;
    let mut start = 0usize;
    for (i, c) in text.char_indices() {
        match c {
            '(' => depth += 1,
            ')' => depth = depth.saturating_sub(1),
            c if c == sep && depth == 0 => {
                result.push(&text[start..i]);
                start = i + sep.len_utf8();
            }
            _ => {}
        }
    }
    result.push(&text[start..]);
    result
}

/// True if `s` (already lowercased) is one of the eleven evergreen keywords.
fn is_known_keyword(s: &str) -> bool {
    matches!(
        s,
        "flying" | "reach" | "trample" | "first strike" | "double strike"
            | "vigilance" | "haste" | "lifelink" | "deathtouch" | "menace"
            | "indestructible"
    )
}

/// Emits spans for a single comma-separated token (no top-level em-dash).
/// Extracts any `(…)` reminder text inline, in source order.
fn emit_token_spans(token: &str, spans: &mut Vec<OracleSpan>) {
    // Partition the token into alternating non-paren and paren segments.
    let mut segments: Vec<(bool, &str)> = Vec::new();
    let mut depth = 0usize;
    let mut seg_start = 0usize;

    for (i, c) in token.char_indices() {
        match c {
            '(' if depth == 0 => {
                if i > seg_start {
                    segments.push((false, &token[seg_start..i]));
                }
                seg_start = i;
                depth = 1;
            }
            '(' => depth += 1,
            ')' if depth == 1 => {
                depth = 0;
                let end = i + ')'.len_utf8();
                segments.push((true, &token[seg_start..end]));
                seg_start = end;
            }
            ')' if depth > 0 => depth -= 1,
            _ => {}
        }
    }
    if seg_start < token.len() {
        segments.push((false, &token[seg_start..]));
    }

    // Emit spans in source order; accumulate non-paren text for keyword matching.
    let mut accumulated = String::new();
    for (is_paren, text) in segments {
        if is_paren {
            let kw = accumulated.trim();
            if !kw.is_empty() {
                spans.push(match_keyword(kw));
            }
            accumulated.clear();
            spans.push(OracleSpan::Ignored(IgnoredKind::ReminderText, text.to_string()));
        } else {
            accumulated.push_str(text);
        }
    }
    let kw = accumulated.trim();
    if !kw.is_empty() {
        spans.push(match_keyword(kw));
    }
}

fn match_keyword(kw: &str) -> OracleSpan {
    match kw.to_lowercase().as_str() {
        "flying"         => OracleSpan::Parsed(AbilityAST::Static(StaticAbility::Flying)),
        "reach"          => OracleSpan::Parsed(AbilityAST::Static(StaticAbility::Reach)),
        "trample"        => OracleSpan::Parsed(AbilityAST::Static(StaticAbility::Trample)),
        "first strike"   => OracleSpan::Parsed(AbilityAST::Static(StaticAbility::FirstStrike)),
        "double strike"  => OracleSpan::Parsed(AbilityAST::Static(StaticAbility::DoubleStrike)),
        "vigilance"      => OracleSpan::Parsed(AbilityAST::Static(StaticAbility::Vigilance)),
        "haste"          => OracleSpan::Parsed(AbilityAST::Static(StaticAbility::Haste)),
        "lifelink"       => OracleSpan::Parsed(AbilityAST::Static(StaticAbility::Lifelink)),
        "deathtouch"     => OracleSpan::Parsed(AbilityAST::Static(StaticAbility::Deathtouch)),
        "menace"         => OracleSpan::Parsed(AbilityAST::Static(StaticAbility::Menace)),
        "indestructible" => OracleSpan::Parsed(AbilityAST::Static(StaticAbility::Indestructible)),
        _                => OracleSpan::Unparsed(kw.to_string()),
    }
}

// ── Public API ───────────────────────────────────────────────────────────────

/// Parse Oracle text into a sequence of typed spans.
///
/// Always succeeds. Separators (`\n`, `,`) are consumed; each logical token
/// becomes one span. See `OracleSpan` for rendering intent.
pub fn parse_oracle_text(text: &str) -> Vec<OracleSpan> {
    const EM_DASH: char = '\u{2014}';
    let mut spans = Vec::new();

    for paragraph in text.split('\n') {
        let paragraph = paragraph.trim();
        if paragraph.is_empty() {
            continue;
        }

        // Em-dash at depth 0 → ability/flavour word line.
        if let Some(dash_pos) = find_at_depth_zero(paragraph, EM_DASH) {
            let left = paragraph[..dash_pos].trim();
            let right = paragraph[dash_pos + EM_DASH.len_utf8()..].trim();

            if !is_known_keyword(&left.to_lowercase()) {
                // Preserve the raw label text up to and including the em-dash.
                let label = paragraph[..dash_pos + EM_DASH.len_utf8()].to_string();
                spans.push(OracleSpan::Ignored(IgnoredKind::AbilityWord, label));
                if !right.is_empty() {
                    spans.push(OracleSpan::Unparsed(right.to_string()));
                }
                continue;
            }
        }

        // Split on commas at depth 0; classify each token.
        for token in split_at_depth_zero(paragraph, ',') {
            let token = token.trim();
            if !token.is_empty() {
                emit_token_spans(token, &mut spans);
            }
        }
    }

    spans
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::ability::StaticAbility;

    fn parsed(kw: StaticAbility) -> OracleSpan {
        OracleSpan::Parsed(AbilityAST::Static(kw))
    }
    fn reminder(text: &str) -> OracleSpan {
        OracleSpan::Ignored(IgnoredKind::ReminderText, text.to_string())
    }
    fn ability_word(text: &str) -> OracleSpan {
        OracleSpan::Ignored(IgnoredKind::AbilityWord, text.to_string())
    }
    fn unparsed(text: &str) -> OracleSpan {
        OracleSpan::Unparsed(text.to_string())
    }

    #[test]
    fn empty_text_returns_empty_vec() {
        assert_eq!(parse_oracle_text(""), vec![]);
    }

    #[test]
    fn blank_lines_skipped() {
        assert_eq!(parse_oracle_text("\n\n"), vec![]);
    }

    #[test]
    fn reminder_text_only() {
        assert_eq!(
            parse_oracle_text("({T}: Add {G}.)"),
            vec![reminder("({T}: Add {G}.)")]
        );
    }

    #[test]
    fn single_keyword() {
        assert_eq!(parse_oracle_text("Flying"), vec![parsed(StaticAbility::Flying)]);
    }

    #[test]
    fn comma_separated_keywords() {
        assert_eq!(
            parse_oracle_text("Flying, vigilance"),
            vec![parsed(StaticAbility::Flying), parsed(StaticAbility::Vigilance)]
        );
    }

    #[test]
    fn multiline_keywords() {
        assert_eq!(
            parse_oracle_text("Trample\nLifelink"),
            vec![parsed(StaticAbility::Trample), parsed(StaticAbility::Lifelink)]
        );
    }

    #[test]
    fn two_word_keyword() {
        assert_eq!(
            parse_oracle_text("First strike"),
            vec![parsed(StaticAbility::FirstStrike)]
        );
    }

    #[test]
    fn keyword_with_reminder_text() {
        assert_eq!(
            parse_oracle_text(
                "Deathtouch (Any amount of damage this deals to a creature is enough to destroy it.)"
            ),
            vec![
                parsed(StaticAbility::Deathtouch),
                reminder("(Any amount of damage this deals to a creature is enough to destroy it.)"),
            ]
        );
    }

    #[test]
    fn ability_word_line_splits_at_em_dash() {
        let result = parse_oracle_text(
            "Landfall \u{2014} Whenever a land you control enters, you gain 1 life."
        );
        assert_eq!(result, vec![
            ability_word("Landfall \u{2014}"),
            unparsed("Whenever a land you control enters, you gain 1 life."),
        ]);
    }

    #[test]
    fn cumulative_upkeep_style_no_spaces() {
        let result = parse_oracle_text("Cumulative upkeep\u{2014}Add {R}.");
        assert_eq!(result, vec![
            ability_word("Cumulative upkeep\u{2014}"),
            unparsed("Add {R}."),
        ]);
    }

    #[test]
    fn triggered_ability_becomes_unparsed() {
        assert_eq!(
            parse_oracle_text("When this creature enters, draw a card."),
            vec![unparsed("When this creature enters, draw a card.")]
        );
    }

    #[test]
    fn em_dash_inside_parens_not_split() {
        assert_eq!(
            parse_oracle_text("(Choose one \u{2014} do A; or do B.)"),
            vec![reminder("(Choose one \u{2014} do A; or do B.)")]
        );
    }

    #[test]
    fn all_eleven_keywords_parse() {
        let text = "Flying\nReach\nTrample\nFirst strike\nDouble strike\nVigilance\nHaste\nLifelink\nDeathtouch\nMenace\nIndestructible";
        let result = parse_oracle_text(text);
        assert_eq!(result.len(), 11);
        assert!(result.iter().all(|s| matches!(s, OracleSpan::Parsed(_))));
    }

    #[test]
    fn keyword_and_ability_word_on_separate_lines() {
        let text = "Flying\nLandfall \u{2014} Whenever a land you control enters, you gain 1 life.";
        let result = parse_oracle_text(text);
        assert_eq!(result, vec![
            parsed(StaticAbility::Flying),
            ability_word("Landfall \u{2014}"),
            unparsed("Whenever a land you control enters, you gain 1 life."),
        ]);
    }
}
```

- [ ] **Step 2: Run to confirm tests fail**

```bash
cargo test parser::oracle
```

Expected: compile errors — `OracleSpan`/`IgnoredKind` not yet in scope from `super`, and old `ParseError` import fails.

- [ ] **Step 3: Replace `src/parser/mod.rs`**

```rust
mod oracle;
pub use oracle::parse_oracle_text;
```

- [ ] **Step 4: Run parser tests**

```bash
cargo test parser::oracle
```

Expected: all 13 tests pass.

- [ ] **Step 5: Check what else breaks**

```bash
cargo check 2>&1 | grep "^error"
```

Expected: errors in `src/cards/scryfall.rs` (`.map_err()` on infallible return) and `src/types/card.rs` (type mismatch). Fix those in Tasks 3 and 4.

- [ ] **Step 6: Commit**

```bash
git add src/parser/oracle.rs src/parser/mod.rs
git commit -m "feat: rewrite parse_oracle_text as infallible Vec<OracleSpan>"
```

---

## Task 3: Update `CardDefinition`, `has_keyword`, and `scryfall.rs`

**Files:**
- Modify: `src/types/card.rs`
- Modify: `src/types/card_object.rs`
- Modify: `src/cards/scryfall.rs`
- Modify: test helpers in `src/engine/combat.rs`, `src/engine/state_based_actions.rs`

- [ ] **Step 1: Update `src/types/card.rs`**

Change the import and `abilities` field type, then add `has_unparsed`:

```rust
use super::ability::{AbilityAST, OracleSpan};  // replace `use super::ability::AbilityAST;`
use super::mana::ManaCost;

// ... (Supertype, CardType, TypeLine unchanged) ...

/// The static Oracle data for a card — shared across all copies.
#[derive(Debug, Clone)]
pub struct CardDefinition {
    pub name: String,
    pub mana_cost: Option<ManaCost>,
    pub type_line: TypeLine,
    pub oracle_text: String,
    pub abilities: Vec<OracleSpan>,  // was Vec<AbilityAST>
    pub power: Option<i32>,
    pub toughness: Option<i32>,
}

impl CardDefinition {
    pub fn has_unparsed(&self) -> bool {
        self.abilities.iter().any(|s| matches!(s, OracleSpan::Unparsed(_)))
    }
}
```

Note: `AbilityAST` is still needed transitively (it's embedded in `OracleSpan::Parsed`), but `card.rs` itself no longer needs to name it directly. The import can drop `AbilityAST` if `OracleSpan` is sufficient — `cargo check` will tell you.

- [ ] **Step 2: Update `has_keyword` in `src/types/card_object.rs`**

Change the import to add `OracleSpan`:

```rust
use super::ability::{AbilityAST, OracleSpan, StaticAbility};
```

Replace the `has_keyword` method body:

```rust
pub fn has_keyword(&self, kw: StaticAbility) -> bool {
    self.definition.abilities.iter().any(|span| {
        matches!(span, OracleSpan::Parsed(AbilityAST::Static(k)) if *k == kw)
    })
}
```

- [ ] **Step 3: Fix `src/cards/scryfall.rs`**

Change the oracle text line from:

```rust
let abilities = parse_oracle_text(&oracle_text).map_err(|e| e.to_string())?;
```

to:

```rust
let abilities = parse_oracle_text(&oracle_text);
```

- [ ] **Step 4: Fix `abilities` construction in inline test helpers**

Run:

```bash
grep -rn "AbilityAST::Static" src/ tests/
```

Every `abilities:` field that contains `AbilityAST::Static(...)` needs it wrapped in `OracleSpan::Parsed(...)`. The affected locations are:

- `keyword_creature` helper in `src/engine/combat.rs`
- `keyword_creature_on_battlefield` helper in `src/engine/state_based_actions.rs`
- Two tests in `src/types/card_object.rs` that set `def.abilities = vec![AbilityAST::Static(...)]`
- `trample_excess_kills_player` in `tests/scripted_game.rs`

Change:

```rust
// Before (in both files):
abilities: keywords.into_iter()
    .map(|k| AbilityAST::Static(k))
    .collect(),

// After:
abilities: keywords.into_iter()
    .map(|k| OracleSpan::Parsed(AbilityAST::Static(k)))
    .collect(),
```

Also add `OracleSpan` to the `use crate::types::{...}` import in each of those engine test modules:

```rust
use crate::types::{AbilityAST, OracleSpan, CardDefinition, card::{CardType, TypeLine}};
```

In `src/types/card_object.rs` tests, change the two `def.abilities` assignments:

```rust
// Before (appears twice, with different keywords):
def.abilities = vec![AbilityAST::Static(StaticAbility::Flying)];
def.abilities = vec![AbilityAST::Static(StaticAbility::Haste)];

// After:
def.abilities = vec![OracleSpan::Parsed(AbilityAST::Static(StaticAbility::Flying))];
def.abilities = vec![OracleSpan::Parsed(AbilityAST::Static(StaticAbility::Haste))];
```

Add `OracleSpan` to the `use crate::types::{...}` import in the `card_object.rs` test module:

```rust
use crate::types::{AbilityAST, OracleSpan, ability::StaticAbility};
```

In `tests/scripted_game.rs`, the inline `trampler_def` construction uses `AbilityAST::Static(...)`:

```rust
// Before:
abilities: vec![AbilityAST::Static(StaticAbility::Trample)],

// After:
abilities: vec![OracleSpan::Parsed(AbilityAST::Static(StaticAbility::Trample))],
```

Add `OracleSpan` to the local `use` statement inside that test:

```rust
use mecha_oracle::types::{AbilityAST, OracleSpan, ability::StaticAbility, CardDefinition, card::{CardType, TypeLine}};
```

- [ ] **Step 5: Run the full test suite**

```bash
cargo test 2>&1 | grep -E "^(test result|FAILED)"
```

Expected: all tests pass.

- [ ] **Step 6: Commit**

```bash
git add src/types/card.rs src/types/card_object.rs src/cards/scryfall.rs \
        src/engine/combat.rs src/engine/state_based_actions.rs tests/scripted_game.rs
git commit -m "feat: CardDefinition.abilities is Vec<OracleSpan>; update has_keyword"
```

---

## Task 4: `CardDatabase` unparsed tracking

**Files:**
- Modify: `src/cards/mod.rs`

- [ ] **Step 1: Write a failing test**

Add to the `#[cfg(test)] pub mod test_helpers` block in `src/cards/mod.rs`:

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use test_helpers::test_db;

    #[test]
    fn unparsed_count_zero_for_current_fixtures() {
        // All current fixture cards have only keywords or empty oracle text.
        // This will change in Task 8 when a Landfall card is added.
        let db = test_db();
        assert_eq!(db.unparsed_count(), 0);
    }
}
```

- [ ] **Step 2: Run to confirm failure**

```bash
cargo test cards::tests
```

Expected: compile error — `unparsed_count` not found.

- [ ] **Step 3: Add `unparsed_count` and update load tracing**

Add to `impl CardDatabase`:

```rust
/// Number of loaded cards that contain at least one `OracleSpan::Unparsed` span.
pub fn unparsed_count(&self) -> usize {
    self.inner.values().filter(|def| def.has_unparsed()).count()
}
```

In `from_path`, add an `unparsed` counter and update the tracing call:

```rust
pub fn from_path(path: &Path) -> Result<Self, String> {
    let content = std::fs::read_to_string(path)
        .map_err(|e| format!("Cannot read {}: {e}", path.display()))?;
    let cards: Vec<serde_json::Value> = serde_json::from_str(&content)
        .map_err(|e| format!("Invalid JSON in {}: {e}", path.display()))?;

    let mut inner = HashMap::new();
    let mut loaded = 0usize;
    let mut skipped = 0usize;
    let mut unparsed = 0usize;
    for v in &cards {
        match scryfall::parse_card(v) {
            Ok(def) => {
                if def.has_unparsed() { unparsed += 1; }
                inner.insert(def.name.to_lowercase(), def);
                loaded += 1;
            }
            Err(e) => {
                let name = v["name"].as_str().unwrap_or("<unknown>");
                tracing::debug!(card = name, error = %e, "skipped card");
                skipped += 1;
            }
        }
    }
    tracing::info!(loaded, skipped, unparsed, "card database loaded");

    Ok(Self { inner })
}
```

- [ ] **Step 4: Run tests**

```bash
cargo test cards::
```

Expected: `unparsed_count_zero_for_current_fixtures` passes.

- [ ] **Step 5: Commit**

```bash
git add src/cards/mod.rs
git commit -m "feat: CardDatabase::unparsed_count and load-time tracing"
```

---

## Task 5: `StaticAbility::display_name`

**Files:**
- Modify: `src/types/ability.rs`

- [ ] **Step 1: Write a failing test**

Add to the `#[cfg(test)]` block in `src/types/ability.rs`:

```rust
#[test]
fn display_name_canonical_casing() {
    assert_eq!(StaticAbility::Flying.display_name(), "Flying");
    assert_eq!(StaticAbility::FirstStrike.display_name(), "First strike");
    assert_eq!(StaticAbility::DoubleStrike.display_name(), "Double strike");
    assert_eq!(StaticAbility::Indestructible.display_name(), "Indestructible");
}
```

- [ ] **Step 2: Run to confirm failure**

```bash
cargo test types::ability::tests::display_name
```

Expected: compile error — `display_name` not found.

- [ ] **Step 3: Add `impl StaticAbility`**

```rust
impl StaticAbility {
    pub fn display_name(&self) -> &'static str {
        match self {
            Self::Flying         => "Flying",
            Self::Reach          => "Reach",
            Self::Trample        => "Trample",
            Self::FirstStrike    => "First strike",
            Self::DoubleStrike   => "Double strike",
            Self::Vigilance      => "Vigilance",
            Self::Haste          => "Haste",
            Self::Lifelink       => "Lifelink",
            Self::Deathtouch     => "Deathtouch",
            Self::Menace         => "Menace",
            Self::Indestructible => "Indestructible",
        }
    }
}
```

- [ ] **Step 4: Run tests**

```bash
cargo test types::ability
```

Expected: all ability tests pass.

- [ ] **Step 5: Commit**

```bash
git add src/types/ability.rs
git commit -m "feat: StaticAbility::display_name"
```

---

## Task 6: `serve.rs` view model

**Files:**
- Modify: `src/serve.rs`

- [ ] **Step 1: Add `SpanKind` and `OracleSpanView` after `ManaPoolView`**

```rust
#[derive(Serialize)]
#[serde(rename_all = "snake_case")]
enum SpanKind {
    Parsed,
    Ignored,
    Unparsed,
}

#[derive(Serialize)]
struct OracleSpanView {
    kind: SpanKind,
    text: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    ignored_kind: Option<mecha_oracle::types::IgnoredKind>,
}
```

- [ ] **Step 2: Change `oracle_text` field in `CardView`**

```rust
// Before:
oracle_text: String,

// After:
oracle_text: Vec<OracleSpanView>,
```

- [ ] **Step 3: Replace the `oracle_text` assignment in `to_card_view`**

Find:
```rust
oracle_text: obj.definition.oracle_text.clone(),
```

Replace with:

```rust
oracle_text: {
    use mecha_oracle::types::{AbilityAST, IgnoredKind, OracleSpan};
    obj.definition.abilities.iter().map(|span| match span {
        OracleSpan::Parsed(AbilityAST::Static(kw)) => OracleSpanView {
            kind: SpanKind::Parsed,
            text: kw.display_name().to_string(),
            ignored_kind: None,
        },
        OracleSpan::Ignored(kind, t) => OracleSpanView {
            kind: SpanKind::Ignored,
            text: t.clone(),
            ignored_kind: Some(kind.clone()),
        },
        OracleSpan::Unparsed(t) => OracleSpanView {
            kind: SpanKind::Unparsed,
            text: t.clone(),
            ignored_kind: None,
        },
        _ => OracleSpanView {
            kind: SpanKind::Unparsed,
            text: format!("{span:?}"),
            ignored_kind: None,
        },
    }).collect()
},
```

- [ ] **Step 4: Verify compile**

```bash
cargo build 2>&1 | grep "^error"
```

Expected: no errors.

- [ ] **Step 5: Commit**

```bash
git add src/serve.rs
git commit -m "feat: CardView oracle_text as Vec<OracleSpanView>"
```

---

## Task 7: Frontend rendering

**Files:**
- Modify: `src/serve.html`

- [ ] **Step 1: Add `renderOracleText` to the `<script>` block**

Find the `<script>` section. Add this function near the top, before it is called:

```js
function renderOracleText(spans) {
    if (!spans || spans.length === 0) return '';
    return spans.map(span => {
        const t = String(span.text)
            .replace(/&/g, '&amp;')
            .replace(/</g, '&lt;')
            .replace(/>/g, '&gt;');
        if (span.kind === 'parsed')   return `<span>${t}</span>`;
        if (span.kind === 'ignored')  return `<span style="font-style:italic">${t}</span>`;
        if (span.kind === 'unparsed') return `<span style="color:red;text-decoration:underline">${t}</span>`;
        return t;
    }).join('<br>');
}
```

- [ ] **Step 2: Find every place oracle text is rendered**

```bash
grep -n "oracle_text" src/serve.html
```

Each occurrence will be either `card.oracle_text` or inside a template literal like `` `${card.oracle_text}` ``.

- [ ] **Step 3: Replace each occurrence**

For every occurrence found in Step 2, replace the string expression with a call to `renderOracleText`. Examples:

```js
// Before:
card.oracle_text

// After:
renderOracleText(card.oracle_text)
```

```js
// Before (inside innerHTML template):
<div class="oracle">${card.oracle_text}</div>

// After:
<div class="oracle">${renderOracleText(card.oracle_text)}</div>
```

- [ ] **Step 4: Start the server and verify visually**

```bash
cargo run -- docs/test-decks/basic.json
```

Open the printed URL in a browser. Verify:
- Cards with keywords show them in normal text.
- Forest (reminder text only) shows its oracle text in italics.
- No JS errors in the browser console (open DevTools → Console).

- [ ] **Step 5: Commit**

```bash
git add src/serve.html
git commit -m "feat: render oracle text spans with per-kind formatting"
```

---

## Task 8: Fixture card and final verification

**Files:**
- Modify: `tests/fixtures/oracle_cards_test.json`
- Modify: `src/cards/mod.rs`

- [ ] **Step 1: Add an ability-word card to `tests/fixtures/oracle_cards_test.json`**

Append before the closing `]`:

```json
  ,
  {
    "object": "card",
    "name": "Grazing Gladehart",
    "mana_cost": "{2}{G}",
    "type_line": "Creature — Elk",
    "oracle_text": "Landfall — Whenever a land you control enters, you gain 2 life.",
    "power": "2",
    "toughness": "2"
  }
```

- [ ] **Step 2: Write tests for span structure and updated `unparsed_count`**

Replace the test added in Task 4 (`unparsed_count_zero_for_current_fixtures`) with these four tests:

```rust
#[test]
fn landfall_card_loads_successfully() {
    let db = test_db();
    assert!(db.get("Grazing Gladehart").is_some());
}

#[test]
fn landfall_card_has_unparsed_span() {
    let db = test_db();
    let card = db.get("Grazing Gladehart").unwrap();
    assert!(card.has_unparsed());
}

#[test]
fn landfall_card_span_structure() {
    use mecha_oracle::types::{IgnoredKind, OracleSpan};
    let db = test_db();
    let card = db.get("Grazing Gladehart").unwrap();
    assert_eq!(card.abilities.len(), 2);
    assert!(matches!(&card.abilities[0], OracleSpan::Ignored(IgnoredKind::AbilityWord, _)));
    assert!(matches!(&card.abilities[1], OracleSpan::Unparsed(_)));
}

#[test]
fn keyword_only_card_has_no_unparsed_spans() {
    use mecha_oracle::types::OracleSpan;
    let db = test_db();
    let card = db.get("Serra Angel").unwrap();
    assert!(!card.has_unparsed());
    assert!(card.abilities.iter().all(|s| matches!(s, OracleSpan::Parsed(_))));
}

#[test]
fn unparsed_count_reflects_landfall_card() {
    let db = test_db();
    assert_eq!(db.unparsed_count(), 1);
}
```

- [ ] **Step 3: Run new tests**

```bash
cargo test cards::tests
```

Expected: all 5 tests pass.

- [ ] **Step 4: Run the full suite**

```bash
cargo test 2>&1 | grep -E "^(test result|FAILED)"
```

Expected: all tests pass.

- [ ] **Step 5: Check for warnings**

```bash
cargo build 2>&1 | grep "^warning" | wc -l
```

Expected: same count as before or lower (removing `ParseError` may reduce the count).

- [ ] **Step 6: Commit**

```bash
git add tests/fixtures/oracle_cards_test.json src/cards/mod.rs
git commit -m "feat: Landfall fixture; verify span structure and unparsed_count"
```
