# Annotation Completeness & Yellow Gap Colour Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Ensure every byte of oracle text has an explicit `TextAnnotation` so the frontend can colour implemented text normally, unimplemented text cyan, and truly-missed text yellow.

**Architecture:** Add `AnnotationKind::Active` to the existing annotation enum, then emit it in the three parser paths (`parse_permanent` keyword/ability/trigger branches, and `parse_instant_or_sorcery` spell steps). A fourth change in `serve.js` colours unannotated text yellow and handles the new `active` kind.

**Tech Stack:** Rust (stable nightly features already enabled via `Cargo.toml`), vanilla JS in `src/serve.js`.

## Global Constraints

- Run `cargo test 2>&1 | grep -E "^test result|FAILED|error\["` — must show `test result: ok` with zero failures before each commit.
- Run `cargo clippy --all-targets` — must be warning/error-free before each commit.
- Do not change any public API surface that would break existing tests except where a task explicitly says to update those tests.
- All new test functions live in the existing `#[cfg(test)] mod tests` block in the same file as the code under test.
- CR reference format: `(NNN.MMx)` — verify against `docs/CR.txt` with `grep '^NNN\\.MM' docs/CR.txt` before adding any new reference.

---

### Task 1: Add `AnnotationKind::Active` and wire `serve.js`

**Files:**
- Modify: `src/types/ability.rs` — add `Active` variant to `AnnotationKind`
- Modify: `src/serve.js` — handle `'active'` in `annStyle`, colour unannotated text yellow

**Interfaces:**
- Produces: `AnnotationKind::Active` variant, serialised as `"active"` via the existing `#[serde(rename_all = "snake_case")]` derive.

- [ ] **Step 1: Write the failing test**

In `src/types/ability.rs`, inside `mod tests`:

```rust
#[test]
fn annotation_kind_active_serializes_as_active() {
    assert_eq!(
        serde_json::to_string(&AnnotationKind::Active).unwrap(),
        r#""active""#
    );
}
```

- [ ] **Step 2: Run test to verify it fails**

```bash
cargo test annotation_kind_active_serializes_as_active 2>&1 | grep -E "^test result|FAILED|error\["
```

Expected: compile error — `Active` variant does not exist yet.

- [ ] **Step 3: Add `Active` variant to `AnnotationKind`**

In `src/types/ability.rs`, edit the `AnnotationKind` enum:

```rust
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "snake_case")]
pub enum AnnotationKind {
    ReminderText,
    AbilityWord,
    ParsedUnimplemented,
    Unparsed,
    Active, // ← ADD: parsed and engine-implemented oracle text
}
```

- [ ] **Step 4: Run test to verify it passes**

```bash
cargo test annotation_kind_active_serializes_as_active 2>&1 | grep -E "^test result|FAILED|error\["
```

Expected: `test result: ok. 1 passed`

- [ ] **Step 5: Update `annStyle` in `serve.js`**

In `src/serve.js`, find `function annStyle(kind)` and add the `active` branch:

```js
function annStyle(kind) {
  if (kind === 'reminder_text' || kind === 'ability_word') return 'font-style:italic';
  if (kind === 'parsed_unimplemented') return 'color:#4dd9d9;text-decoration:underline';
  if (kind === 'unparsed') return 'color:red;text-decoration:underline';
  if (kind === 'active') return '';   // inherits default tooltip colour — explicitly handled
  return '';
}
```

- [ ] **Step 6: Colour unannotated text yellow in `renderOracleText`**

In `src/serve.js`, find `function renderOracleText(card)`. Replace the two plain-push lines with yellow-wrapped versions:

Current:
```js
  for (const ann of annotations) {
    if (ann.start > pos) parts.push(renderManaSymbols(text.slice(pos, ann.start)));
    const style = annStyle(ann.kind);
    const content = renderManaSymbols(text.slice(ann.start, ann.end));
    parts.push(style ? `<span style="${style}">${content}</span>` : content);
    pos = ann.end;
  }
  if (pos < text.length) parts.push(renderManaSymbols(text.slice(pos)));
```

Replacement:
```js
  for (const ann of annotations) {
    if (ann.start > pos) {
      const gap = renderManaSymbols(text.slice(pos, ann.start));
      parts.push(`<span style="color:#c8b820">${gap}</span>`);
    }
    const style = annStyle(ann.kind);
    const content = renderManaSymbols(text.slice(ann.start, ann.end));
    parts.push(style ? `<span style="${style}">${content}</span>` : content);
    pos = ann.end;
  }
  if (pos < text.length) {
    const gap = renderManaSymbols(text.slice(pos));
    parts.push(`<span style="color:#c8b820">${gap}</span>`);
  }
```

- [ ] **Step 7: Run full test suite and clippy**

```bash
cargo test 2>&1 | grep -E "^test result|FAILED|error\["
cargo clippy --all-targets 2>&1 | grep -E "^error|^warning"
```

Both must be clean.

- [ ] **Step 8: Commit**

```bash
git add src/types/ability.rs src/serve.js
git commit -m "feat: add AnnotationKind::Active; colour unannotated oracle text yellow"
```

---

### Task 2: Emit `Active` annotations in `parse_permanent`

This task adds `Active` annotations for every parsed paragraph in `parse_permanent`, and adds a `ParsedUnimplemented` annotation for the new gap: activated abilities whose cost contains `CostComponent::Unimplemented`.

**Files:**
- Modify: `src/parser/oracle.rs`

**Interfaces:**
- Consumes: `AnnotationKind::Active` (from Task 1)
- Produces: all keyword, activated-ability, ETB-trigger, and continuous-PT-effect paragraphs annotated

**Background — how `push_keyword_annotation` works:**

```rust
fn push_keyword_annotation(
    span: &RulesText,
    raw_start: usize,
    raw_end: usize,
    original: &str,
    annotations: &mut Vec<TextAnnotation>,
) {
    let kind = match span {
        RulesText::Unparsed(_) => AnnotationKind::Unparsed,
        RulesText::ParsedUnimplemented(_) => AnnotationKind::ParsedUnimplemented,
        _ => return,  // ← currently silently skips Active and Ignored spans
    };
    ...
}
```

The `raw_start..raw_end` range already covers the correct byte range of the token.

- [ ] **Step 1: Write the failing tests**

Add to `mod tests` in `src/parser/oracle.rs`:

```rust
#[test]
fn flying_emits_active_annotation() {
    let (_, anns) = parse_permanent("Flying", "Test");
    assert!(
        anns.iter().any(|a| a.kind == AnnotationKind::Active),
        "expected Active annotation for implemented keyword"
    );
}

#[test]
fn unimplemented_activation_cost_emits_parsed_unimplemented_annotation() {
    // "Sacrifice a creature" is an unimplemented cost component
    let (_, anns) = parse_permanent("Sacrifice a creature: Draw a card.", "Test");
    assert!(
        anns.iter().any(|a| a.kind == AnnotationKind::ParsedUnimplemented),
        "expected ParsedUnimplemented annotation for unimplemented activation cost"
    );
}

#[test]
fn clean_activated_ability_emits_active_annotation() {
    let (_, anns) = parse_permanent("{T}: Add {G}.", "Test");
    assert!(
        anns.iter().any(|a| a.kind == AnnotationKind::Active),
        "expected Active annotation for fully-parsed activated ability"
    );
}

#[test]
fn etb_trigger_emits_active_annotation() {
    let (_, anns) = parse_permanent("When this enters, you gain 1 life.", "Test");
    assert!(
        anns.iter().any(|a| a.kind == AnnotationKind::Active),
        "expected Active annotation for parsed ETB trigger"
    );
}

#[test]
fn continuous_pt_effect_emits_active_annotation() {
    let (_, anns) = parse_permanent("Creatures you control get +1/+1.", "Test");
    assert!(
        anns.iter().any(|a| a.kind == AnnotationKind::Active),
        "expected Active annotation for continuous PT effect"
    );
}
```

- [ ] **Step 2: Run tests to verify they fail**

```bash
cargo test "flying_emits_active\|unimplemented_activation_cost\|clean_activated_ability_emits\|etb_trigger_emits\|continuous_pt_effect_emits" 2>&1 | grep -E "^test result|FAILED|error\["
```

Expected: failures (no `Active` annotations emitted yet).

- [ ] **Step 3: Extend `push_keyword_annotation` to emit `Active`**

In `src/parser/oracle.rs`, update the match arm in `push_keyword_annotation`:

```rust
fn push_keyword_annotation(
    span: &RulesText,
    raw_start: usize,
    raw_end: usize,
    original: &str,
    annotations: &mut Vec<TextAnnotation>,
) {
    let kind = match span {
        RulesText::Unparsed(_) => AnnotationKind::Unparsed,
        RulesText::ParsedUnimplemented(_) => AnnotationKind::ParsedUnimplemented,
        RulesText::Active(_) => AnnotationKind::Active,
        _ => return,
    };
    let raw_slice = &original[raw_start..raw_end];
    let trimmed = raw_slice.trim();
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

- [ ] **Step 4: Annotate activated abilities in `parse_permanent`**

Locate the activated-ability branch in `parse_permanent` (the `find_colon_at_depth_zero` block). Replace it:

```rust
// Colon check: activated ability ({cost}: effect).
if let Some(colon_pos) = find_colon_at_depth_zero(paragraph) {
    let cost_str = paragraph[..colon_pos].trim();
    let effect_str = paragraph[colon_pos + 1..].trim();
    let cost = parse_activation_cost(cost_str);
    if !cost.is_empty() {
        let has_unimplemented_cost = cost
            .iter()
            .any(|c| matches!(c, CostComponent::Unimplemented(_)));
        if let Some(effect) = parse_ability_effect(effect_str) {
            let para_start = subslice_offset(text, paragraph);
            let ann_kind = if has_unimplemented_cost {
                AnnotationKind::ParsedUnimplemented
            } else {
                AnnotationKind::Active
            };
            annotations.push(TextAnnotation {
                start: para_start,
                end: para_start + paragraph.len(),
                kind: ann_kind,
            });
            spans.push(RulesText::Active(Rule::Activated(ActivatedAbility {
                cost,
                target_requirements: vec![],
                effect,
            })));
        } else {
            let para_start = subslice_offset(text, paragraph);
            annotations.push(TextAnnotation {
                start: para_start,
                end: para_start + paragraph.len(),
                kind: AnnotationKind::ParsedUnimplemented,
            });
            spans.push(RulesText::ParsedUnimplemented(paragraph.to_string()));
        }
        continue;
    }
}
```

- [ ] **Step 5: Annotate ETB triggers in `parse_permanent`**

Locate the ETB trigger block and update it to also emit `Active` on success:

```rust
if let Some(span) = try_parse_etb_trigger(paragraph, card_name) {
    let para_start = subslice_offset(text, paragraph);
    let ann_kind = match &span {
        RulesText::ParsedUnimplemented(_) => AnnotationKind::ParsedUnimplemented,
        _ => AnnotationKind::Active,
    };
    annotations.push(TextAnnotation {
        start: para_start,
        end: para_start + paragraph.len(),
        kind: ann_kind,
    });
    spans.push(span);
    continue;
}
```

- [ ] **Step 6: Annotate continuous P/T effects in `parse_permanent`**

Locate `try_parse_continuous_pt_effect` and update:

```rust
if let Some(span) = try_parse_continuous_pt_effect(paragraph) {
    let para_start = subslice_offset(text, paragraph);
    annotations.push(TextAnnotation {
        start: para_start,
        end: para_start + paragraph.len(),
        kind: AnnotationKind::Active,
    });
    spans.push(span);
    continue;
}
```

- [ ] **Step 7: Run the failing tests**

```bash
cargo test "flying_emits_active\|unimplemented_activation_cost\|clean_activated_ability_emits\|etb_trigger_emits\|continuous_pt_effect_emits" 2>&1 | grep -E "^test result|FAILED|error\["
```

Expected: all 5 pass.

- [ ] **Step 8: Run full suite and clippy**

```bash
cargo test 2>&1 | grep -E "^test result|FAILED|error\["
cargo clippy --all-targets 2>&1 | grep -E "^error|^warning"
```

Both must be clean.

- [ ] **Step 9: Commit**

```bash
git add src/parser/oracle.rs
git commit -m "feat: emit Active/ParsedUnimplemented annotations in parse_permanent for keywords, abilities, triggers"
```

---

### Task 3: Fill the spell-step annotation gap

`parse_instant_or_sorcery` currently only annotates reminder text and a hardcoded list of keyword actions. `EffectStep::Unimplemented` steps produced by the lenient `parse_spell_effect` get no annotation. This task emits `ParsedUnimplemented` for those steps by locating the source text within the paragraph, and deduplicates any overlapping annotations that the existing `annotate_spell_paragraph` already produced.

**Files:**
- Modify: `src/parser/oracle.rs` — `parse_instant_or_sorcery` only

**Interfaces:**
- Consumes: `AnnotationKind::ParsedUnimplemented` (existing), `EffectStep::Unimplemented` (existing)
- Produces: `ParsedUnimplemented` annotations for unrecognised spell-effect sentences

**Note on scope:** Per-sentence `Active` annotations for spell steps that *do* parse are deferred — tracking which byte range produced each `EffectStep` would require threading source strings through `parse_spell_paragraph` and `parse_spell_effect`, a significant refactor. Implemented spell sentences will remain unannotated (yellow) in this pass.

- [ ] **Step 1: Write the failing tests**

Add to `mod tests` in `src/parser/oracle.rs`:

```rust
#[test]
fn unimplemented_spell_step_emits_parsed_unimplemented_annotation() {
    // "Create a 1/1 token." is not handled by try_parse_effect_step
    let (_, anns) = parse_instant_or_sorcery("Create a 1/1 white Soldier token.", "Test");
    assert!(
        anns.iter().any(|a| a.kind == AnnotationKind::ParsedUnimplemented),
        "expected ParsedUnimplemented annotation for unimplemented spell step"
    );
}

#[test]
fn implemented_spell_step_does_not_emit_parsed_unimplemented_annotation() {
    // "Draw a card." parses successfully — should produce no ParsedUnimplemented annotation
    let (_, anns) = parse_instant_or_sorcery("Draw a card.", "Test");
    assert!(
        !anns.iter().any(|a| a.kind == AnnotationKind::ParsedUnimplemented),
        "DrawCard step should not produce ParsedUnimplemented annotation"
    );
}

#[test]
fn mixed_spell_text_emits_annotation_only_for_unimplemented_part() {
    // "Mill 2." parses; "Create a token." does not
    let (_, anns) = parse_instant_or_sorcery("Mill 2. Create a 1/1 Soldier token.", "Test");
    let unimpl_count = anns
        .iter()
        .filter(|a| a.kind == AnnotationKind::ParsedUnimplemented)
        .count();
    assert_eq!(unimpl_count, 1, "exactly one ParsedUnimplemented annotation expected");
}

#[test]
fn spell_annotations_have_no_duplicate_ranges() {
    // Scry is in SPELL_KEYWORD_ACTIONS (annotated by annotate_spell_paragraph)
    // AND would be caught as EffectStep::Unimplemented — verify no duplicate
    let (_, anns) = parse_instant_or_sorcery("Scry 2.", "Test");
    let scry_anns: Vec<_> = anns
        .iter()
        .filter(|a| a.kind == AnnotationKind::ParsedUnimplemented)
        .collect();
    // There should be exactly one annotation for "Scry 2" (deduplication)
    assert_eq!(scry_anns.len(), 1);
}
```

- [ ] **Step 2: Run tests to verify they fail**

```bash
cargo test "unimplemented_spell_step_emits\|implemented_spell_step_does_not\|mixed_spell_text_emits\|spell_annotations_have_no_duplicate" 2>&1 | grep -E "^test result|FAILED|error\["
```

Expected: failures.

- [ ] **Step 3: Update `parse_instant_or_sorcery` to annotate unimplemented steps**

In `src/parser/oracle.rs`, update `parse_instant_or_sorcery`:

```rust
pub fn parse_instant_or_sorcery(
    text: &str,
    card_name: &str,
) -> (Vec<RulesText>, Vec<TextAnnotation>) {
    use crate::types::ability::Rule;
    let mut spans = Vec::new();
    let mut annotations = Vec::new();
    for paragraph in text.split('\n') {
        let paragraph = paragraph.trim();
        if paragraph.is_empty() {
            continue;
        }
        let spell_ability = parse_spell_paragraph(paragraph, card_name);

        // Emit ParsedUnimplemented annotations for unimplemented effect steps.
        // EffectStep::Unimplemented(s) stores the source text trimmed from the
        // paragraph, so it is always a substring of `paragraph`.
        for step in &spell_ability.steps {
            if let EffectStep::Unimplemented(s) = step {
                if let Some(pos) = paragraph.find(s.as_str()) {
                    let start = subslice_offset(text, &paragraph[pos..pos + s.len()]);
                    annotations.push(TextAnnotation {
                        start,
                        end: start + s.len(),
                        kind: AnnotationKind::ParsedUnimplemented,
                    });
                }
            }
        }

        spans.push(RulesText::Active(Rule::SpellAbility(spell_ability)));
        annotate_spell_paragraph(paragraph, text, &mut annotations);
    }

    // Sort then deduplicate: annotate_spell_paragraph may produce the same range
    // for keyword actions (e.g. "Scry 2") that the step loop above also catches.
    annotations.sort_by_key(|a| (a.start, a.end));
    annotations.dedup_by(|a, b| a.start == b.start && a.end == b.end);

    (spans, annotations)
}
```

Note: `dedup_by` removes consecutive duplicates after the sort, which covers the overlap case.

- [ ] **Step 4: Run the new tests**

```bash
cargo test "unimplemented_spell_step_emits\|implemented_spell_step_does_not\|mixed_spell_text_emits\|spell_annotations_have_no_duplicate" 2>&1 | grep -E "^test result|FAILED|error\["
```

Expected: all 4 pass.

- [ ] **Step 5: Run the full suite**

```bash
cargo test 2>&1 | grep -E "^test result|FAILED|error\["
```

Expected: `test result: ok` with zero failures. If any pre-existing tests fail due to annotation ordering changes, check whether they assert on annotation count or kind — the deduplication may reduce counts where duplicates existed before.

- [ ] **Step 6: Clippy**

```bash
cargo clippy --all-targets 2>&1 | grep -E "^error|^warning"
```

Fix any warnings with `cargo clippy --fix --all-targets` then re-run.

- [ ] **Step 7: Commit**

```bash
git add src/parser/oracle.rs
git commit -m "feat: emit ParsedUnimplemented annotations for EffectStep::Unimplemented in spell effects"
```
