# Parse Coverage: Annotation Completeness & Coverage Report

**Date:** 2026-06-25  
**Status:** Approved for implementation

---

## Problem

Three gaps currently hide the real parse state of cards in the engine:

1. **`CostComponent::Unimplemented` is invisible.** When an activated ability has an unrecognised cost (e.g. "Sacrifice a creature:"), `parse_activation_cost` still succeeds and the ability is stored as `Active(Rule::Activated(...))`. No `TextAnnotation` is emitted, so the UI shows the card as fully parsed.

2. **`EffectStep::Unimplemented` in spell steps is mostly invisible.** `parse_spell_effect` is lenient and produces `EffectStep::Unimplemented` for unrecognised sentences. Only a short hard-coded list of keyword actions (Scry, Surveil, etc.) in `annotate_spell_paragraph` currently produces an annotation; everything else silently disappears.

3. **No tool to measure coverage.** The only signal is a log line on startup showing a rough `partially_parsed` count that is based on `has_unparsed()`, which misses both of the above gaps.

---

## Goals

- Every byte of oracle text has exactly one `TextAnnotation`, so the UI can show distinct states for implemented, recognised-but-unimplemented, and genuinely-missed text.
- A new `parse-coverage` subcommand prints a human-readable report on how much of the card database the engine can handle, with enough breakdown detail to guide future implementation priorities.

---

## Feature 1: Fill annotation gaps

### New `AnnotationKind::Active`

Add `AnnotationKind::Active` to `types/ability.rs`. Rendered in `serve.js` as normal text (inherits default tooltip color — no special style). This allows the yellow gap-color to mean "truly missed" rather than "implemented but not tagged."

### `parse_permanent` — activated ability cost gap (oracle.rs)

Current path when cost parses but contains `CostComponent::Unimplemented`:
```
cost contains Unimplemented, effect parses OK → Active(Activated) [no annotation] ← BUG
```

Fix: after building cost and effect, if `cost.iter().any(|c| matches!(c, CostComponent::Unimplemented(_)))`, emit a `ParsedUnimplemented` annotation spanning the whole paragraph instead of an `Active` annotation. The `Active(Rule::Activated(...))` span is still stored (the engine handles what it can), but the UI marks the whole line as partially unimplemented.

### `parse_instant_or_sorcery` — spell step gap (oracle.rs)

Current path: `annotate_spell_paragraph` only annotates reminder text and a short keyword list. `EffectStep::Unimplemented("Destroy target creature")` gets no annotation.

Fix: in `parse_instant_or_sorcery`, after calling `parse_spell_paragraph`, iterate the resulting steps. For each `EffectStep::Unimplemented(s)`, locate `s` as a substring within the paragraph (it is always present since it came from there) and emit a `ParsedUnimplemented` annotation at that byte range. Deduplication: before returning, sort annotations by `start` and remove entries with identical `(start, end)` pairs (handles overlap with `annotate_spell_paragraph`'s existing keyword-action annotations).

### Emit `Active` annotations for all parsed text (oracle.rs)

To support the yellow gap-colour, every parsed region must be tagged. Emit `AnnotationKind::Active` annotations in:

- **`emit_token_spans`**: for any token that returns `Active(Rule::...)` from `match_keyword`. Already knows the byte range.
- **Activated ability paragraphs**: if cost is clean and effect parses — emit `Active` for the whole paragraph.
- **ETB/triggered ability paragraphs**: if `try_parse_etb_trigger` returns `Active(...)` — emit `Active` for the paragraph.
- **Continuous P/T effect paragraphs**: if `try_parse_continuous_pt_effect` returns `Some(Active(...))` — emit `Active` for the paragraph.
- **Spell effect paragraphs (per-sentence)**: for each sentence/step that produces an implemented `EffectStep`, emit `Active` at the sentence's byte range. For `EffectStep::Unimplemented`, emit `ParsedUnimplemented` (handled by the gap fix above).

### `serve.js` rendering changes

`annStyle` additions:
```js
if (kind === 'active') return '';   // default tooltip colour
```

Unannotated text (before first annotation, between annotations, after last) wraps in:
```html
<span style="color:#c8b820">…</span>
```

This yellow (`#c8b820`) is not currently used in the palette. The full colour meaning becomes:
| Colour / style | Meaning |
|---|---|
| `#bbb` (default via `active`) | Parsed and implemented |
| `#4dd9d9` cyan + underline | Recognised, unimplemented |
| `red` + underline | Totally unrecognised |
| italic | Reminder / ability word |
| `#c8b820` yellow | Not annotated — annotation system missed this |

---

## Feature 2: `parse-coverage` subcommand

### `CardDatabase::iter()`

Add `pub fn iter(&self) -> impl Iterator<Item = &CardDefinition>` exposing all non-token card definitions.

### Card classification (three buckets)

| Bucket | Condition |
|---|---|
| **Clean** | No `Unparsed`, `ParsedUnimplemented`, `CostComponent::Unimplemented`, or `EffectStep::Unimplemented` anywhere in the card |
| **Opaque** | Has zero `RulesText::Active` spans AND at least one unimplemented signal |
| **Partial** | Has at least one `RulesText::Active` span AND at least one unimplemented signal |

Vanilla creatures with no rules text count as Clean.

### Breakdown tables (top 15 per category, sorted by descending count)

1. **Top unimplemented effect steps** — exact `EffectStep::Unimplemented` text, case-folded.
2. **Top unimplemented keywords** — from `ParsedUnimplemented`, first space-delimited token only (normalises `"flashback {2}{U}"` → `"flashback"`).
3. **Top unimplemented activation costs** — exact `CostComponent::Unimplemented` text, case-folded.
4. **Top unparsed paragraphs** — exact `Unparsed` text; top 10 (these are typically all distinct).

### Output format (plain text to stdout)

```
=== Parse Coverage Report ===
Total cards:          28,347

Clean (fully parsed): 4,102  (14.5%)
Partial:             21,855  (77.1%)
Opaque (no Active):   2,390   (8.4%)

=== Top unimplemented effect steps (top 15) ===
  1. "destroy target creature"              ×  1,234
  2. "create a 1/1 white soldier token"     ×    987
  …

=== Top unimplemented keywords (top 15) ===
  1. cascade                                ×    456
  2. flashback                              ×    321
  …

=== Top unimplemented activation costs (top 15) ===
  1. "sacrifice a creature"                 ×    234
  …

=== Top unparsed paragraphs (top 10) ===
  1. "Whenever a land enters under your control, you may gain 1 life."   ×  89
  …
```

No flags required for the initial implementation; a future `--json` flag could be added later.

---

## Out of scope

- Fixing any of the unimplemented mechanics (this spec is about visibility and measurement only).
- Changing which mechanics are classified as `ParsedUnimplemented` vs `Unparsed`.
- JSON output for the coverage report.
- Annotating `Rule::Kicker`, `Rule::Multikicker`, `Rule::Dash`, `Rule::Evoke` separately (they each occupy a paragraph and will get an `Active` annotation from the activated-ability or keyword path as appropriate).

---

## Testing

- Existing `oracle.rs` tests must continue to pass; the annotation additions are additive.
- New unit tests for:
  - `CostComponent::Unimplemented` in activated ability cost → `ParsedUnimplemented` annotation emitted
  - `EffectStep::Unimplemented` in spell step → `ParsedUnimplemented` annotation emitted
  - Implemented keyword token → `Active` annotation emitted
  - `CardDatabase::iter()` returns expected count
- Integration: `cargo test 2>&1 | grep -E "^test result|FAILED|error\["` must be clean.
- `cargo clippy --all-targets` must be clean before completion.
