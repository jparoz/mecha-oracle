# Oracle Text Annotation Design

**Date:** 2026-06-12  
**Status:** Approved

## Problem

Cards in the UI currently display *reconstructed* oracle text — the server rebuilds human-readable strings from parsed ability structs (e.g. `format_activated_ability`, `kw.display_name()`). This loses original formatting (capitalisation, spacing, phrasing) and is error-prone. The goal is to show the verbatim original oracle text on the card, with the existing visual formatting (italic reminder/ability-word text, cyan underline for ParsedUnimplemented, red underline for Unparsed) preserved via a span-annotation system.

## Approach

`CardDefinition` already stores `oracle_text: String` (the raw original). A new `Vec<TextAnnotation>` is added alongside `abilities: Vec<OracleSpan>`. The parser produces both in one pass. The server serialises raw text + annotation ranges. The frontend renders the raw text with `white-space: pre-wrap` and wraps annotated byte ranges in styled `<span>` elements.

The `OracleSpan`/engine types are unchanged. The separation is:
- `abilities` — for the rules engine
- `text_annotations` — for the UI renderer

## Data Types

In `src/types/ability.rs`:

```rust
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "snake_case")]
pub enum AnnotationKind {
    ReminderText,        // italic
    AbilityWord,         // italic
    ParsedUnimplemented, // cyan underline
    Unparsed,            // red underline
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct TextAnnotation {
    pub start: usize, // byte offset in oracle_text (inclusive)
    pub end: usize,   // byte offset in oracle_text (exclusive)
    pub kind: AnnotationKind,
}
```

In `src/types/card.rs`:

```rust
pub struct CardDefinition {
    pub name: String,
    pub mana_cost: Option<ManaCost>,
    pub type_line: TypeLine,
    pub oracle_text: String,
    pub abilities: Vec<OracleSpan>,
    pub text_annotations: Vec<TextAnnotation>, // NEW
    pub power: Option<i32>,
    pub toughness: Option<i32>,
}
```

## Parser Changes (`src/parser/oracle.rs`)

### Helper function

```rust
/// Returns the byte offset of `sub` within `whole`.
/// Panics in debug if `sub` is not a subslice of `whole`.
fn subslice_offset(whole: &str, sub: &str) -> usize {
    debug_assert!(
        sub.as_ptr() >= whole.as_ptr()
            && sub.as_ptr() as usize + sub.len() <= whole.as_ptr() as usize + whole.len()
    );
    sub.as_ptr() as usize - whole.as_ptr() as usize
}
```

### Signature changes

Both public parser functions gain an `original: &str` parameter and return a `Vec<TextAnnotation>` alongside `Vec<OracleSpan>`:

```rust
pub fn parse_permanent(text: &str, card_name: &str) -> (Vec<OracleSpan>, Vec<TextAnnotation>)
pub fn parse_instant_or_sorcery(text: &str, card_name: &str) -> (Vec<OracleSpan>, Vec<TextAnnotation>)
```

The `original` parameter is the full unmodified oracle text (same string passed as `text`). Internal helpers that emit spans are extended to also push the corresponding annotation. The annotation range is computed via `subslice_offset(original, sub)` where `sub` is the subslice being classified.

### Annotation mapping

| OracleSpan emitted | Annotation emitted |
|---|---|
| `Parsed(_)` | none |
| `Ignored(ReminderText, s)` | `AnnotationKind::ReminderText` over range of `s` in original |
| `Ignored(AbilityWord, s)` | `AnnotationKind::AbilityWord` over range of `s` in original |
| `ParsedUnimplemented(s)` | `AnnotationKind::ParsedUnimplemented` over range of `s` in original |
| `Unparsed(s)` | `AnnotationKind::Unparsed` over range of `s` in original |

### Internal helpers

- `emit_token_spans` gains `(original: &str, annotations: &mut Vec<TextAnnotation>)` parameters
- `parse_permanent` and `parse_instant_or_sorcery` pass `text` as `original` throughout, accumulating annotations into a local `Vec<TextAnnotation>` that is returned at the end

### Test impact

All existing parser tests assert on the `Vec<OracleSpan>` output. They must be updated to unpack the tuple `(spans, _annotations)`. No new annotation-specific tests are strictly required at this stage, though a few basic ones are welcome.

## Call Site (`src/cards/scryfall.rs`)

`parse_entry` unpacks the tuple:

```rust
let (abilities, text_annotations) = if is_instant_or_sorcery {
    parse_instant_or_sorcery(&oracle_text, &name)
} else {
    parse_permanent(&oracle_text, &name)
};

let def = CardDefinition {
    ...,
    abilities,
    text_annotations,
    ...,
};
```

## Server Changes (`src/serve.rs`)

### `CardView`

`oracle_text` changes from `Vec<OracleSpanView>` to `String`. New field `text_annotations`:

```rust
#[derive(Serialize)]
struct CardView {
    id: ObjectId,
    name: String,
    type_line: String,
    oracle_text: String,               // raw original (was Vec<OracleSpanView>)
    text_annotations: Vec<TextAnnotationView>,  // NEW
    mana_cost: Option<String>,
    ...
}

#[derive(Serialize)]
struct TextAnnotationView {
    start: usize,
    end: usize,
    kind: mecha_oracle::types::ability::AnnotationKind,
}
```

### Deleted code

The following functions are removed entirely (no longer needed):
- `format_activated_ability`
- `format_triggered_ability`
- `format_spell_effect`
- `format_mana_pool`
- `SpanKind` enum
- `OracleSpanView` struct

`format_mana_cost` and `format_mana_cost_braced` are kept — they're still used for `mana_cost` display and action labels.

### `build_player_view`

The `oracle_text` closure changes from the current span-mapping logic to:

```rust
oracle_text: obj.definition.oracle_text.clone(),
text_annotations: obj.definition.text_annotations.iter().map(|a| TextAnnotationView {
    start: a.start,
    end: a.end,
    kind: a.kind.clone(),
}).collect(),
```

## Frontend Changes (`src/serve.js`)

### `renderOracleText(card)`

Rewritten to take the full card object (or `{ oracle_text, text_annotations }`). 

Algorithm:
1. Sort annotations by `start` (should already be ordered, but sort defensively).
2. Walk through `oracle_text` splitting at annotation boundaries.
3. Emit plain text for unannotated ranges; emit styled `<span>` for annotated ranges.
4. Wrap the entire result in a `<div style="white-space: pre-wrap">`.

Style map:
- `reminder_text` → `font-style: italic`
- `ability_word` → `font-style: italic`
- `parsed_unimplemented` → `color: #4dd9d9; text-decoration: underline`
- `unparsed` → `color: red; text-decoration: underline`

### Call site

The existing `renderOracleText(card.oracle_text)` call in the card tooltip builder changes to `renderOracleText(card)` (or pass both fields explicitly).

## Non-goals

- Mana symbol rendering (`{T}`, `{G}` etc.) in oracle text — symbols remain as plain text.
- Annotation overlap handling — annotations are non-overlapping by construction.
- Backwards-compatibility for the old `OracleSpanView` JSON shape — the API is internal.
