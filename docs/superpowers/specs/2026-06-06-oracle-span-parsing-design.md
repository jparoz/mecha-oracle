# Oracle Span Parsing — Design Spec

**Date:** 2026-06-06
**Status:** Approved

---

## Goal

Replace the current fail-fast oracle text parser with a fault-tolerant span-annotated parser. Every card in the database loads successfully; oracle text is represented as a sequence of typed spans that the engine can query for abilities and the UI can render with appropriate formatting.

This is Phase A of a three-phase parsing expansion:
- **Phase A (this spec):** Ability words, flavour words, reminder text — all ignored text handled gracefully; unparsed text surfaced as a distinct span type.
- **Phase B (future):** Activated abilities (`{cost}: effect` syntax).
- **Phase C (future):** Triggered abilities (`When/Whenever/At…` syntax).

---

## Background

The current parser (`parse_oracle_text`) returns `Err(ParseError::UnknownKeyword)` for any token that is not one of the eleven evergreen static keywords. `CardDatabase` logs and skips cards that error. This means any card with an ability word, flavour word, or non-keyword ability text is silently dropped from the database.

---

## Section 1: Data Model

### New types in `src/types/ability.rs`

```rust
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum IgnoredKind {
    ReminderText,  // parenthetical: (This creature can't block.)
    AbilityWord,   // ability words (CR 207.2c) and flavour words (CR 207.2d): "Landfall — ", "Cumulative upkeep—", etc.
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum OracleSpan {
    Parsed(AbilityAST),             // recognised ability — engine uses this
    Ignored(IgnoredKind, String),   // non-rules text — UI: italics
    Unparsed(String),               // not yet parseable — UI: red + underline
}
```

`IgnoredKind` derives `Serialize` with `rename_all = "snake_case"` so it can be used directly in the view model without a separate mirror type. If that causes a dependency issue, a `serve.rs`-local `IgnoredKindView` mirror is the fallback.

### `CardDefinition.abilities` type change

`abilities: Vec<AbilityAST>` → `abilities: Vec<OracleSpan>`

No other field changes on `CardDefinition`. The `oracle_text: String` field is kept as-is (raw source string for reference).

### `ParseError` removed

`src/parser/mod.rs` no longer exports `ParseError`. `parse_oracle_text` becomes infallible and `scryfall::parse_card` no longer has an oracle text error path.

### `CardDefinition::has_unparsed`

New helper for tracking parse coverage:

```rust
pub fn has_unparsed(&self) -> bool {
    self.abilities.iter().any(|s| matches!(s, OracleSpan::Unparsed(_)))
}
```

### `CardObject::has_keyword` update

```rust
pub fn has_keyword(&self, kw: StaticAbility) -> bool {
    self.definition.abilities.iter().any(|span| {
        matches!(span, OracleSpan::Parsed(AbilityAST::Static(k)) if *k == kw)
    })
}
```

All engine call sites are unchanged — they call `has_keyword` and remain blind to `Ignored` and `Unparsed` spans.

---

## Section 2: Parser Algorithm

### Signature

```rust
pub fn parse_oracle_text(text: &str) -> Vec<OracleSpan>
```

Infallible. Always returns a `Vec`; never panics or errors.

### Whitespace normalisation

Separators (`\n` and `,`) between tokens are consumed. Each logical token becomes one span. The UI renders spans separated by `<br>` — so `Flying, vigilance` and `Flying\nVigilance` both display as two lines. This is the only normalisation applied.

### Algorithm

Process the oracle text one paragraph at a time (split on `\n`). For each paragraph:

**Step 1 — Em-dash check (ability/flavour words)**

Scan for the Unicode em-dash `\u{2014}` at parenthetical depth 0 (i.e. not inside `(…)`). If found and the text to its left (trimmed) is not a recognised keyword:

- Emit `OracleSpan::Ignored(IgnoredKind::AbilityWord, "<left including dash> ")` — includes the em-dash and any trailing space, exactly as it appears.
- Emit `OracleSpan::Unparsed("<right trimmed>")` for the body.
- Stop processing this paragraph.

If the text to the left of the em-dash *is* a recognised keyword (e.g. a future parameterised keyword), fall through to Step 2 and treat the whole line as a single token.

**Step 2 — Comma-split and token classification**

Split on commas at depth 0 (respecting parenthetical nesting — do not split on commas inside `(…)`). For each trimmed, non-empty token:

1. If the entire token is parenthetical (starts with `(`, ends with `)`): emit `Ignored(ReminderText, token)`.
2. Otherwise: extract any `(…)` substrings as `Ignored(ReminderText, …)` spans. Take the non-parenthetical remainder, trim it, and:
   - Match case-insensitively against the known keyword table → `Parsed(AbilityAST::Static(…))`
   - No match → `Unparsed(remainder)`

### Examples

| Input | Spans emitted |
|---|---|
| `"Flying"` | `Parsed(Flying)` |
| `"Flying, vigilance"` | `Parsed(Flying)`, `Parsed(Vigilance)` |
| `"Deathtouch (Any amount of damage…)"` | `Parsed(Deathtouch)`, `Ignored(ReminderText, "(Any amount…)")` |
| `"({T}: Add {G}.)"` | `Ignored(ReminderText, "({T}: Add {G}.)")` |
| `"Landfall — Whenever a land you control enters, you gain 1 life."` | `Ignored(AbilityWord, "Landfall — ")`, `Unparsed("Whenever a land you control enters, you gain 1 life.")` |
| `"Cumulative upkeep—Add {R}."` | `Ignored(AbilityWord, "Cumulative upkeep—")`, `Unparsed("Add {R}.")` |
| `"When this creature enters, draw a card."` | `Unparsed("When this creature enters, draw a card.")` |

The last example is the Phase C case. When triggered ability parsing lands, the `Unparsed` variant is replaced by `Parsed(AbilityAST::Triggered(…))` for lines we can now handle.

---

## Section 3: Engine and Database Impact

### `scryfall::parse_card`

Oracle text line simplifies from:

```rust
let abilities = parse_oracle_text(&oracle_text).map_err(|e| e.to_string())?;
```

to:

```rust
let abilities = parse_oracle_text(&oracle_text);
```

Cards are never skipped for oracle text reasons. They can still fail for structural JSON errors (missing `name`, unparseable `type_line`, etc.).

### `CardDatabase` load-time summary

The log-and-skip behaviour for oracle parse errors is replaced with a load-time summary line:

```
[cards] Loaded 12,543 cards (847 with unparsed abilities)
```

`CardDatabase` gains:

```rust
pub fn unparsed_count(&self) -> usize
```

for programmatic access (e.g. a future `--stats` CLI flag).

### Engine files unchanged

`src/engine/combat.rs`, `src/engine/state_based_actions.rs`, `src/engine/turn.rs`, `src/engine/casting.rs`, `src/engine/mana.rs` — no changes. All ability queries go through `has_keyword`, which is updated as described in Section 1.

---

## Section 4: UI

### `CardView` in `serve.rs`

`oracle_text: String` → `oracle_text: Vec<OracleSpanView>`

New types:

```rust
#[derive(Serialize)]
#[serde(rename_all = "snake_case")]
enum SpanKind { Parsed, Ignored, Unparsed }

#[derive(Serialize)]
struct OracleSpanView {
    kind: SpanKind,
    text: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    ignored_kind: Option<IgnoredKind>,  // reuses IgnoredKind from types if Serialize works; fallback: local mirror
}
```

`StaticAbility` gets `fn display_name(&self) -> &'static str` returning `"Flying"`, `"First strike"`, `"Double strike"`, etc.

Mapping in `to_card_view`:

```rust
oracle_text: obj.definition.abilities.iter().map(|span| match span {
    OracleSpan::Parsed(AbilityAST::Static(kw)) =>
        OracleSpanView { kind: SpanKind::Parsed, text: kw.display_name().into(), ignored_kind: None },
    OracleSpan::Ignored(kind, t) =>
        OracleSpanView { kind: SpanKind::Ignored, text: t.clone(), ignored_kind: Some(kind.clone()) },
    OracleSpan::Unparsed(t) =>
        OracleSpanView { kind: SpanKind::Unparsed, text: t.clone(), ignored_kind: None },
    _ =>
        OracleSpanView { kind: SpanKind::Unparsed, text: format!("{span:?}"), ignored_kind: None },
}).collect(),
```

The catch-all `_` arm handles future `Parsed(Triggered(…))` and `Parsed(Activated(…))` variants added in Phases B and C — they'll show up as red underlined until a proper arm is added.

### Frontend (`serve.html`)

Replace all `card.oracle_text` string renders with:

```js
function renderOracleText(spans) {
    return spans.map(span => {
        if (span.kind === 'parsed')   return `<span>${span.text}</span>`;
        if (span.kind === 'ignored')  return `<span style="font-style:italic">${span.text}</span>`;
        if (span.kind === 'unparsed') return `<span style="color:red;text-decoration:underline">${span.text}</span>`;
        return span.text;
    }).join('<br>');
}
```

---

## Test Strategy

### Parser tests (`src/parser/oracle.rs`)

- Empty text → empty vec
- Reminder-text-only → `[Ignored(ReminderText, …)]`
- Single keyword → `[Parsed(…)]`
- Comma-separated keywords → multiple `Parsed` spans
- Keyword + reminder text → `Parsed` + `Ignored(ReminderText, …)`
- Ability word line → `Ignored(AbilityWord, …)` + `Unparsed(…)`
- Triggered ability text → `[Unparsed(…)]`
- Em-dash inside parens (e.g. `"(Choose one — do A; or do B.)"`) → treated as a single `Ignored(ReminderText, …)`, not split as ability word

### `CardDefinition` tests

- `has_unparsed()` returns `false` for keyword-only card
- `has_unparsed()` returns `true` for card with ability word body

### Fixture additions (`tests/fixtures/oracle_cards_test.json`)

Add one card with an ability word (e.g. a Landfall creature) to verify end-to-end loading and `has_unparsed()`.

### Existing tests

All existing parser, engine, and integration tests continue to pass — the keyword-matching logic is unchanged; only the return type and error path change.

---

## Files Changed

| File | Change |
|---|---|
| `src/types/ability.rs` | Add `IgnoredKind`, `OracleSpan`; add `Serialize` to `IgnoredKind` |
| `src/types/card.rs` | `CardDefinition.abilities` type; `has_unparsed()` |
| `src/types/mod.rs` | Re-export `OracleSpan`, `IgnoredKind` |
| `src/types/card_object.rs` | Update `has_keyword` |
| `src/parser/mod.rs` | Remove `ParseError`; update re-exports |
| `src/parser/oracle.rs` | Full algorithm rewrite; new signature |
| `src/cards/scryfall.rs` | Remove `.map_err()?` on oracle parse |
| `src/cards/mod.rs` | `unparsed_count()`; update load summary log |
| `src/engine/turn.rs` | — (no change) |
| `src/serve.rs` | `SpanKind`, `OracleSpanView`, `CardView` field, `StaticAbility::display_name`, mapping |
| `src/serve.html` | `renderOracleText()` function; replace all oracle text renders |
| `tests/fixtures/oracle_cards_test.json` | Add ability-word card fixture |
| `src/parser/oracle.rs` (tests) | New parser tests as above |
