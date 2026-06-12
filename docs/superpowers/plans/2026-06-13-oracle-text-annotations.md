# Oracle Text Annotations Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Replace reconstructed oracle text in the card UI with the original Scryfall oracle text, using a `Vec<TextAnnotation>` (byte-range + kind pairs) to preserve italic/underline formatting.

**Architecture:** The oracle parser already works on subslices of the original text string — byte offsets are derived via pointer arithmetic in a single `subslice_offset` helper. `CardDefinition` gains a `text_annotations: Vec<TextAnnotation>` field produced in the same parsing pass as `abilities`. `serve.rs` converts byte offsets to Unicode codepoint offsets (safe for MTG oracle text, which is BMP-only) in `TextAnnotationView` before JSON serialisation. The frontend renders the raw `oracle_text` string with `white-space: pre-wrap` and wraps annotated char ranges in styled `<span>` elements.

**Tech Stack:** Rust (axum, serde), JavaScript (vanilla)

---

### Task 1: Add `AnnotationKind` and `TextAnnotation` types

**Files:**
- Modify: `src/types/ability.rs`

- [ ] **Step 1: Add the types after the `IgnoredKind` enum**

In `src/types/ability.rs`, after the `IgnoredKind` enum block, insert:

```rust
/// Describes the visual style to apply to an annotated range of oracle text in the UI.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "snake_case")]
pub enum AnnotationKind {
    ReminderText,
    AbilityWord,
    ParsedUnimplemented,
    Unparsed,
}

/// A styled byte-range annotation over a `CardDefinition`'s `oracle_text` field.
/// `start` and `end` are byte offsets (UTF-8) into `oracle_text`, exclusive of `end`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TextAnnotation {
    pub start: usize,
    pub end: usize,
    pub kind: AnnotationKind,
}
```

- [ ] **Step 2: Add tests for the new types**

In the `#[cfg(test)] mod tests` block at the bottom of `src/types/ability.rs`, add:

```rust
#[test]
fn annotation_kind_serialises_to_snake_case() {
    assert_eq!(serde_json::to_string(&AnnotationKind::ReminderText).unwrap(), r#""reminder_text""#);
    assert_eq!(serde_json::to_string(&AnnotationKind::ParsedUnimplemented).unwrap(), r#""parsed_unimplemented""#);
}

#[test]
fn text_annotation_construction() {
    let ann = TextAnnotation { start: 3, end: 10, kind: AnnotationKind::Unparsed };
    assert_eq!(ann.start, 3);
    assert_eq!(ann.end, 10);
    assert_eq!(ann.kind, AnnotationKind::Unparsed);
}
```

- [ ] **Step 3: Run tests**

```bash
cargo test 2>&1 | grep -E "^test result|FAILED|error\["
```

Expected: `test result: ok`.

- [ ] **Step 4: Commit**

```bash
git add src/types/ability.rs
git commit -m "feat: add AnnotationKind and TextAnnotation types"
```

---

### Task 2: Add `text_annotations` field to `CardDefinition`; fix all direct constructors

**Files:**
- Modify: `src/types/card.rs`
- Modify: `src/serve.rs` (tests that construct `CardDefinition` directly)

- [ ] **Step 1: Update the import and struct in `src/types/card.rs`**

Line 1 currently reads:
```rust
use super::ability::OracleSpan;
```
Change to:
```rust
use super::ability::{OracleSpan, TextAnnotation};
```

Add the new field to `CardDefinition` (after `abilities`):
```rust
pub struct CardDefinition {
    pub name: String,
    pub mana_cost: Option<ManaCost>,
    pub type_line: TypeLine,
    pub oracle_text: String,
    pub abilities: Vec<OracleSpan>,
    pub text_annotations: Vec<TextAnnotation>,
    pub power: Option<i32>,
    pub toughness: Option<i32>,
}
```

- [ ] **Step 2: Try to compile; note all `E0063` missing-field errors**

```bash
cargo test 2>&1 | grep -E "error\[E0063\]"
```

There are three direct `CardDefinition { ... }` constructors in `src/serve.rs` tests:
- `can_cast_true_for_instant_in_hand_with_mana_and_priority`
- `autoskips_declare_blockers_when_no_valid_blocker_for_any_attacker`
- `blocker_ui_only_shows_valid_pairings` (two constructors here)

- [ ] **Step 3: Add `text_annotations: vec![]` to each direct constructor in `src/serve.rs`**

Each `CardDefinition { ... }` literal in the tests needs:
```rust
text_annotations: vec![],
```
added alongside `abilities: vec![...]`.

- [ ] **Step 4: Run tests**

```bash
cargo test 2>&1 | grep -E "^test result|FAILED|error\["
```

Expected: `test result: ok`.

- [ ] **Step 5: Commit**

```bash
git add src/types/card.rs src/serve.rs
git commit -m "feat: add text_annotations field to CardDefinition"
```

---

### Task 3: Change parser return types; update all call sites

This task changes `parse_permanent` and `parse_instant_or_sorcery` to return `(Vec<OracleSpan>, Vec<TextAnnotation>)`. Annotations are empty for now — emission is added in Tasks 4–5. All call sites are updated to compile and pass.

**Files:**
- Modify: `src/parser/oracle.rs`
- Modify: `src/cards/scryfall.rs`

- [ ] **Step 1: Add imports to `src/parser/oracle.rs`**

At the top of `src/parser/oracle.rs`, add alongside the existing `use crate::types::{...}` imports:

```rust
use crate::types::ability::{AnnotationKind, TextAnnotation};
```

(`AnnotationKind` is imported now so it's available for Tasks 4–5 without another edit.)

- [ ] **Step 2: Update `parse_permanent` signature and return**

Change:
```rust
pub fn parse_permanent(text: &str, card_name: &str) -> Vec<OracleSpan> {
    const EM_DASH: char = '\u{2014}';
    let mut spans = Vec::new();
```
to:
```rust
pub fn parse_permanent(text: &str, card_name: &str) -> (Vec<OracleSpan>, Vec<TextAnnotation>) {
    const EM_DASH: char = '\u{2014}';
    let mut spans = Vec::new();
    let mut annotations: Vec<TextAnnotation> = Vec::new();
```

Change the final `spans` return to:
```rust
    (spans, annotations)
```

- [ ] **Step 3: Update `parse_instant_or_sorcery` signature and return**

Change:
```rust
pub fn parse_instant_or_sorcery(text: &str, card_name: &str) -> Vec<OracleSpan> {
    use crate::types::ability::Ability;
    let mut spans = Vec::new();
```
to:
```rust
pub fn parse_instant_or_sorcery(text: &str, card_name: &str) -> (Vec<OracleSpan>, Vec<TextAnnotation>) {
    use crate::types::ability::Ability;
    let mut spans = Vec::new();
```

Change the final `spans` return to:
```rust
    (spans, vec![])
```

(This function always emits `Parsed` spans; no annotations ever apply.)

- [ ] **Step 4: Add `parse_perm` and `parse_spell` helpers in the test module**

At the very top of the `#[cfg(test)] mod tests { ... }` block in `src/parser/oracle.rs`, add:

```rust
fn parse_perm(text: &str, name: &str) -> Vec<OracleSpan> {
    parse_permanent(text, name).0
}
fn parse_spell(text: &str, name: &str) -> Vec<OracleSpan> {
    parse_instant_or_sorcery(text, name).0
}
```

- [ ] **Step 5: Replace all `parse_permanent(` with `parse_perm(` in the test module**

Inside `mod tests`, replace every call `parse_permanent(` with `parse_perm(` and every `parse_instant_or_sorcery(` with `parse_spell(`. This is a find-replace scoped to the `tests` module only. The existing test assertions remain unchanged since `parse_perm` returns `Vec<OracleSpan>`.

- [ ] **Step 6: Update `src/cards/scryfall.rs` to unpack the tuple**

Change the abilities assignment:
```rust
let abilities = if type_line
    .card_types
    .iter()
    .any(|t| matches!(t, CardType::Instant | CardType::Sorcery))
{
    parse_instant_or_sorcery(&oracle_text, &name)
} else {
    parse_permanent(&oracle_text, &name)
};
```
to:
```rust
let (abilities, text_annotations) = if type_line
    .card_types
    .iter()
    .any(|t| matches!(t, CardType::Instant | CardType::Sorcery))
{
    parse_instant_or_sorcery(&oracle_text, &name)
} else {
    parse_permanent(&oracle_text, &name)
};
```

Update the `CardDefinition` struct literal to include the new field:
```rust
let def = CardDefinition {
    name,
    mana_cost,
    type_line,
    oracle_text,
    abilities,
    text_annotations,
    power,
    toughness,
};
```

- [ ] **Step 7: Run all tests**

```bash
cargo test 2>&1 | grep -E "^test result|FAILED|error\["
```

Expected: `test result: ok`. Fix any remaining compile errors before proceeding.

- [ ] **Step 8: Commit**

```bash
git add src/parser/oracle.rs src/cards/scryfall.rs
git commit -m "refactor: parser returns (Vec<OracleSpan>, Vec<TextAnnotation>); annotations empty for now"
```

---

### Task 4: Add `subslice_offset` + `push_keyword_annotation`; update `emit_token_spans` to emit annotations

**Files:**
- Modify: `src/parser/oracle.rs`

- [ ] **Step 1: Write failing annotation tests**

In `mod tests`, add:

```rust
#[test]
fn reminder_text_emits_annotation() {
    let text = "Deathtouch (Any amount of damage this deals to a creature is enough to destroy it.)";
    let (_, annotations) = parse_permanent(text, "");
    assert_eq!(annotations.len(), 1);
    assert_eq!(annotations[0].kind, AnnotationKind::ReminderText);
    let expected_start = text.find('(').unwrap();
    assert_eq!(annotations[0].start, expected_start);
    assert_eq!(annotations[0].end, text.len());
}

#[test]
fn parsed_keyword_emits_no_annotation() {
    let (_, annotations) = parse_permanent("Flying", "");
    assert!(annotations.is_empty());
}

#[test]
fn parsed_unimplemented_keyword_emits_annotation() {
    let text = "Storm";
    let (_, annotations) = parse_permanent(text, "");
    assert_eq!(annotations.len(), 1);
    assert_eq!(annotations[0].kind, AnnotationKind::ParsedUnimplemented);
    assert_eq!(annotations[0].start, 0);
    assert_eq!(annotations[0].end, text.len());
}

#[test]
fn unparsed_text_emits_annotation() {
    let text = "Whenever a land you control enters, you gain 1 life.";
    let (_, annotations) = parse_permanent(text, "");
    assert_eq!(annotations.len(), 1);
    assert_eq!(annotations[0].kind, AnnotationKind::Unparsed);
    assert_eq!(annotations[0].start, 0);
    assert_eq!(annotations[0].end, text.len());
}
```

- [ ] **Step 2: Run to confirm the new tests fail**

```bash
cargo test 2>&1 | grep -E "^test result|FAILED|error\["
```

Expected: the four new tests fail (annotation lengths are 0, not 1).

- [ ] **Step 3: Add `subslice_offset` in the private helpers section**

In `src/parser/oracle.rs`, at the top of the `// ── Private helpers ──` section, add:

```rust
/// Returns the byte offset of `sub` within `whole`.
/// Panics in debug builds if `sub` is not a subslice of `whole`.
fn subslice_offset(whole: &str, sub: &str) -> usize {
    debug_assert!(
        sub.as_ptr() as usize >= whole.as_ptr() as usize
            && sub.as_ptr() as usize + sub.len() <= whole.as_ptr() as usize + whole.len(),
        "sub is not a subslice of whole"
    );
    sub.as_ptr() as usize - whole.as_ptr() as usize
}
```

- [ ] **Step 4: Add `push_keyword_annotation` before `emit_token_spans`**

```rust
/// Pushes a `TextAnnotation` for a keyword span if the span kind warrants one.
/// `raw_start..raw_end` is the byte range of the *untrimmed* non-paren text in `original`.
fn push_keyword_annotation(
    span: &OracleSpan,
    raw_start: usize,
    raw_end: usize,
    original: &str,
    annotations: &mut Vec<TextAnnotation>,
) {
    let kind = match span {
        OracleSpan::Unparsed(_) => AnnotationKind::Unparsed,
        OracleSpan::ParsedUnimplemented(_) => AnnotationKind::ParsedUnimplemented,
        _ => return,
    };
    let raw_slice = &original[raw_start..raw_end];
    let trimmed = raw_slice.trim(); // str::trim returns a subslice; safe for subslice_offset
    if trimmed.is_empty() {
        return;
    }
    let trim_start = subslice_offset(original, trimmed);
    annotations.push(TextAnnotation {
        start: trim_start,
        end: trim_start + trimmed.len(),
        kind,
    });
}
```

- [ ] **Step 5: Replace `emit_token_spans` with the annotation-aware version**

Replace the entire `emit_token_spans` function with:

```rust
fn emit_token_spans(token: &str, original: &str, spans: &mut Vec<OracleSpan>, annotations: &mut Vec<TextAnnotation>) {
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

    // Track byte range of the current non-paren accumulation in `original`.
    let mut acc_start: Option<usize> = None;
    let mut acc_end: usize = 0;
    let mut accumulated = String::new();

    for (is_paren, text) in &segments {
        if *is_paren {
            // Flush accumulated keyword.
            let kw = accumulated.trim();
            if !kw.is_empty() {
                let span = match_keyword(kw);
                push_keyword_annotation(&span, acc_start.unwrap(), acc_end, original, annotations);
                spans.push(span);
            }
            accumulated.clear();
            acc_start = None;

            // Emit reminder text annotation and span.
            let off = subslice_offset(original, text);
            annotations.push(TextAnnotation {
                start: off,
                end: off + text.len(),
                kind: AnnotationKind::ReminderText,
            });
            spans.push(OracleSpan::Ignored(IgnoredKind::ReminderText, text.to_string()));
        } else {
            let off = subslice_offset(original, text);
            if acc_start.is_none() {
                acc_start = Some(off);
            }
            acc_end = off + text.len();
            accumulated.push_str(text);
        }
    }

    // Flush remaining keyword.
    let kw = accumulated.trim();
    if !kw.is_empty() {
        let span = match_keyword(kw);
        push_keyword_annotation(&span, acc_start.unwrap(), acc_end, original, annotations);
        spans.push(span);
    }
}
```

- [ ] **Step 6: Update all `emit_token_spans` call sites in `parse_permanent`**

In `parse_permanent`, change:
```rust
emit_token_spans(token, &mut spans);
```
to:
```rust
emit_token_spans(token, text, &mut spans, &mut annotations);
```

- [ ] **Step 7: Run all tests**

```bash
cargo test 2>&1 | grep -E "^test result|FAILED|error\["
```

Expected: `test result: ok`.

- [ ] **Step 8: Commit**

```bash
git add src/parser/oracle.rs
git commit -m "feat: emit token-level annotations (reminder text, keywords) in emit_token_spans"
```

---

### Task 5: Emit paragraph-level annotations in `parse_permanent`

Covers the em-dash branch (ability word / ParsedUnimplemented), the colon branch (ParsedUnimplemented on unknown effect), and the ETB trigger branch (ParsedUnimplemented).

**Files:**
- Modify: `src/parser/oracle.rs`

- [ ] **Step 1: Write failing tests**

In `mod tests`, add:

```rust
#[test]
fn ability_word_emits_ability_word_and_unparsed_annotations() {
    let text = "Landfall \u{2014} Whenever a land you control enters, you gain 1 life.";
    let (_, annotations) = parse_permanent(text, "");
    assert_eq!(annotations.len(), 2);
    assert_eq!(annotations[0].kind, AnnotationKind::AbilityWord);
    // label = "Landfall —" (em-dash is 3 bytes)
    let em_dash = '\u{2014}';
    let label = format!("Landfall {em_dash}");
    assert_eq!(annotations[0].start, 0);
    assert_eq!(annotations[0].end, label.len()); // 9 + 3 = 12
    assert_eq!(annotations[1].kind, AnnotationKind::Unparsed);
    let right = "Whenever a land you control enters, you gain 1 life.";
    let right_start = text.find(right).unwrap();
    assert_eq!(annotations[1].start, right_start);
    assert_eq!(annotations[1].end, text.len());
}

#[test]
fn em_dash_cr702_keyword_emits_parsed_unimplemented_annotation() {
    let text = "Cumulative upkeep\u{2014}Add {R}.";
    let (_, annotations) = parse_permanent(text, "");
    assert_eq!(annotations.len(), 1);
    assert_eq!(annotations[0].kind, AnnotationKind::ParsedUnimplemented);
    assert_eq!(annotations[0].start, 0);
    assert_eq!(annotations[0].end, text.len());
}

#[test]
fn activated_with_unknown_effect_emits_parsed_unimplemented_annotation() {
    let text = "{T}: Create a 1/1 token.";
    let (_, annotations) = parse_permanent(text, "");
    assert_eq!(annotations.len(), 1);
    assert_eq!(annotations[0].kind, AnnotationKind::ParsedUnimplemented);
    assert_eq!(annotations[0].start, 0);
    assert_eq!(annotations[0].end, text.len());
}

#[test]
fn etb_with_unknown_effect_emits_parsed_unimplemented_annotation() {
    let text = "When this enters, create a 1/1 token.";
    let (_, annotations) = parse_permanent(text, "");
    assert_eq!(annotations.len(), 1);
    assert_eq!(annotations[0].kind, AnnotationKind::ParsedUnimplemented);
    assert_eq!(annotations[0].start, 0);
    assert_eq!(annotations[0].end, text.len());
}

#[test]
fn fully_parsed_spans_emit_no_annotations() {
    let (_, a1) = parse_permanent("When this enters, draw a card.", "");
    assert!(a1.is_empty());
    let (_, a2) = parse_permanent("{T}: Add {G}.", "");
    assert!(a2.is_empty());
    let (_, a3) = parse_permanent("Flying", "");
    assert!(a3.is_empty());
}
```

- [ ] **Step 2: Run to confirm the new tests fail**

```bash
cargo test 2>&1 | grep -E "^test result|FAILED|error\["
```

Expected: the five new tests fail.

- [ ] **Step 3: Add annotation to the `ParsedUnimplemented` arm of the em-dash branch**

In `parse_permanent`, inside the `match match_keyword(left)` block, the `OracleSpan::ParsedUnimplemented(_)` arm currently reads:

```rust
OracleSpan::ParsedUnimplemented(_) => {
    spans.push(ParsedUnimplemented(paragraph.to_string()));
    continue;
}
```

Replace with:

```rust
OracleSpan::ParsedUnimplemented(_) => {
    let para_start = subslice_offset(text, paragraph);
    annotations.push(TextAnnotation {
        start: para_start,
        end: para_start + paragraph.len(),
        kind: AnnotationKind::ParsedUnimplemented,
    });
    spans.push(ParsedUnimplemented(paragraph.to_string()));
    continue;
}
```

- [ ] **Step 4: Add annotations to the ability-word `_` arm of the em-dash branch**

The `_` arm currently reads:

```rust
_ => {
    let label = paragraph[..dash_pos + EM_DASH.len_utf8()].to_string();
    spans.push(OracleSpan::Ignored(IgnoredKind::AbilityWord, label));
    if !right.is_empty() {
        spans.push(OracleSpan::Unparsed(right.to_string()));
    }
    continue;
}
```

Replace with:

```rust
_ => {
    let label_slice = &paragraph[..dash_pos + EM_DASH.len_utf8()];
    let label_start = subslice_offset(text, label_slice);
    annotations.push(TextAnnotation {
        start: label_start,
        end: label_start + label_slice.len(),
        kind: AnnotationKind::AbilityWord,
    });
    spans.push(OracleSpan::Ignored(IgnoredKind::AbilityWord, label_slice.to_string()));
    if !right.is_empty() {
        let right_start = subslice_offset(text, right);
        annotations.push(TextAnnotation {
            start: right_start,
            end: right_start + right.len(),
            kind: AnnotationKind::Unparsed,
        });
        spans.push(OracleSpan::Unparsed(right.to_string()));
    }
    continue;
}
```

- [ ] **Step 5: Add annotation to the colon branch (ParsedUnimplemented on unknown effect)**

The colon branch currently ends with:

```rust
} else {
    spans.push(OracleSpan::ParsedUnimplemented(paragraph.to_string()));
}
continue;
```

Change the `else` block to:

```rust
} else {
    let para_start = subslice_offset(text, paragraph);
    annotations.push(TextAnnotation {
        start: para_start,
        end: para_start + paragraph.len(),
        kind: AnnotationKind::ParsedUnimplemented,
    });
    spans.push(OracleSpan::ParsedUnimplemented(paragraph.to_string()));
}
continue;
```

- [ ] **Step 6: Add annotation to the ETB trigger branch**

The ETB check currently reads:

```rust
if let Some(span) = try_parse_etb_trigger(paragraph, card_name) {
    spans.push(span);
    continue;
}
```

Replace with:

```rust
if let Some(span) = try_parse_etb_trigger(paragraph, card_name) {
    if let OracleSpan::ParsedUnimplemented(_) = &span {
        let para_start = subslice_offset(text, paragraph);
        annotations.push(TextAnnotation {
            start: para_start,
            end: para_start + paragraph.len(),
            kind: AnnotationKind::ParsedUnimplemented,
        });
    }
    spans.push(span);
    continue;
}
```

- [ ] **Step 7: Run all tests**

```bash
cargo test 2>&1 | grep -E "^test result|FAILED|error\["
```

Expected: `test result: ok`.

- [ ] **Step 8: Commit**

```bash
git add src/parser/oracle.rs
git commit -m "feat: emit paragraph-level annotations in parse_permanent"
```

---

### Task 6: Update `src/serve.rs` — swap `Vec<OracleSpanView>` for raw text + annotation view

**Files:**
- Modify: `src/serve.rs`

- [ ] **Step 1: Add `TextAnnotationView` struct and `byte_to_char` helper**

After the `ManaPoolView` struct, add:

```rust
#[derive(Serialize)]
struct TextAnnotationView {
    start: usize,
    end: usize,
    kind: mecha_oracle::types::ability::AnnotationKind,
}

/// Converts a UTF-8 byte offset to a Unicode codepoint offset.
/// MTG oracle text is BMP-only, so codepoint offsets equal JS string char indices.
fn byte_to_char(s: &str, byte_offset: usize) -> usize {
    s[..byte_offset].chars().count()
}
```

- [ ] **Step 2: Update `CardView`**

Change `oracle_text: Vec<OracleSpanView>` to `oracle_text: String` and add `text_annotations`:

```rust
#[derive(Serialize)]
struct CardView {
    id: ObjectId,
    name: String,
    type_line: String,
    oracle_text: String,
    text_annotations: Vec<TextAnnotationView>,
    mana_cost: Option<String>,
    power: Option<i32>,
    toughness: Option<i32>,
    tapped: bool,
    summoning_sick: bool,
    damage_marked: u32,
    is_attacking: bool,
    is_blocking: bool,
    actions: Vec<ActionItemView>,
}
```

- [ ] **Step 3: Delete dead code**

Remove the following items entirely from `src/serve.rs`:
- `SpanKind` enum
- `OracleSpanView` struct
- `format_activated_ability` function
- `format_triggered_ability` function
- `format_spell_effect` function
- `format_mana_pool` function

Keep `format_mana_cost`, `format_mana_cost_braced`, `format_type_line` — they are still used.

- [ ] **Step 4: Update the `oracle_text` block inside `to_card_view` in `build_player_view`**

The closure currently has a large `oracle_text: { obj.definition.abilities.iter().map(...).collect() }` block. Replace the entire `oracle_text` and any related fields with:

```rust
oracle_text: obj.definition.oracle_text.clone(),
text_annotations: obj.definition.text_annotations.iter().map(|a| TextAnnotationView {
    start: byte_to_char(&obj.definition.oracle_text, a.start),
    end: byte_to_char(&obj.definition.oracle_text, a.end),
    kind: a.kind.clone(),
}).collect(),
```

- [ ] **Step 5: Fix the stack spell `CardView` in `build_game_view`**

In the stack `Spell` match arm, the inline `CardView` has `oracle_text: vec![]`. Change to:

```rust
oracle_text: String::new(),
text_annotations: vec![],
```

- [ ] **Step 6: Remove now-unused imports**

The `to_card_view` closure no longer pattern-matches on `OracleSpan` variants. Remove any imports that Rust now flags as unused. Specifically, check whether these are still needed (they may be used elsewhere in `compute_hand_actions` / `compute_battlefield_actions`):

```rust
use mecha_oracle::types::ability::{
    Ability, ActivatedAbility, CostComponent, OracleSpan, StaticAbility, TriggeredAbility,
};
```

`OracleSpan`, `Ability`, `ActivatedAbility`, `StaticAbility` are still used in `compute_hand_actions` and `compute_battlefield_actions`. Remove only what Rust tells you is unused.

- [ ] **Step 7: Run tests**

```bash
cargo test 2>&1 | grep -E "^test result|FAILED|error\["
```

Expected: `test result: ok`.

- [ ] **Step 8: Run clippy and fix warnings**

```bash
cargo clippy --all-targets 2>&1 | grep -E "^error|warning\[" | head -30
```

Fix all warnings.

- [ ] **Step 9: Commit**

```bash
git add src/serve.rs
git commit -m "feat: CardView sends raw oracle_text + TextAnnotationView list; remove reconstructed spans"
```

---

### Task 7: Rewrite `renderOracleText` in `src/serve.js`

**Files:**
- Modify: `src/serve.js`

- [ ] **Step 1: Replace `renderOracleText` with an annotation-aware version**

The function currently lives around line 302. Replace it (and keep the `esc` helper it uses) with:

```js
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
```

- [ ] **Step 2: Update the call site**

Find the lines near the tooltip construction that read approximately:

```js
const oracle = card.oracle_text;
...
${oracle.length > 0 ? `<div class="tooltip-text">${renderOracleText(oracle)}</div>` : ''}
```

Change to (removing the `const oracle` line if it is no longer used elsewhere in the function):

```js
${card.oracle_text ? `<div class="tooltip-text">${renderOracleText(card)}</div>` : ''}
```

- [ ] **Step 3: Run Rust tests (JS changes don't affect them, but verify nothing broke)**

```bash
cargo test 2>&1 | grep -E "^test result|FAILED|error\["
```

Expected: `test result: ok`.

- [ ] **Step 4: Run clippy**

```bash
cargo clippy --all-targets 2>&1 | grep -E "^error|warning\[" | head -20
```

- [ ] **Step 5: Commit**

```bash
git add src/serve.js
git commit -m "feat: render original oracle text with annotation ranges and white-space:pre-wrap"
```
