# Triggered Abilities — Design Spec

**Date:** 2026-06-08
**Status:** Approved

---

## Goal

Phase C of the parsing expansion: parse `When/Whenever this enters…` ETB trigger syntax from oracle text and execute the resulting abilities in the engine. Scope: ETB self-triggers with `DrawCard` and `GainLife` effects. Immediate fire-and-forget execution (no stack); designed for easy stack wiring in the next project.

This is Phase C of three:
- **Phase A (done):** Fault-tolerant span parser; ability words, reminder text, CR 702 keyword recognition.
- **Phase B (done):** Activated ability parsing and execution.
- **Phase C (this spec):** ETB triggered ability parsing and immediate execution.

---

## Background

`TriggeredAbility` is currently a stub (`trigger: TriggerEvent` where both are unit structs). Triggered oracle text like "When this enters, draw a card." currently emits two `Unparsed` spans. Phase C adds a full parse → engine execution path for self-ETB triggers.

The engine uses immediate fire-and-forget execution: when a permanent enters the battlefield, ETB triggers resolve instantly before returning the new game state. This is structurally simpler than the full CR 603 stack-based resolution, but the trigger dispatch function is designed so its signature stays stable when the stack project replaces the body.

**Effect type consolidation (also in this spec):** `EffectStep` and `AbilityEffect` currently live in `ability.rs` alongside a dead `Effect` enum in `effect.rs`. This spec consolidates all effect types into `effect.rs`, renames `AbilityEffect` → `Effect`, and removes the dead `Effect` enum. This makes effects generic between spells and abilities ahead of the stack/targeting projects.

---

## Section 1: Data Model

### `src/types/effect.rs` — canonical home for all effect types

Replace the dead `Effect` enum entirely:

```rust
// Retained for future targeting system
pub enum EffectTarget {
    Player(PlayerId),
    Object(ObjectId),
}

// Replaces the dead Effect enum; absorbs EffectStep from ability.rs; adds GainLife
pub enum EffectStep {
    AddMana(ManaPool),
    Mill(u32),
    DrawCard(u32),
    GainLife(u32),   // new for Phase C
}

// Replaces AbilityEffect from ability.rs
pub type Effect = Vec<EffectStep>;
```

### `src/types/ability.rs` — remove EffectStep/AbilityEffect; add TriggerEvent

Remove `EffectStep`, `AbilityEffect`. Import `Effect` from `effect.rs`. Expand stubs:

```rust
pub struct ActivatedAbility {
    pub cost: ActivationCost,
    pub effect: Effect,  // was AbilityEffect
}

pub enum TriggerEvent {
    // "When/Whenever this enters [the battlefield]"
    // subject_is_self: always true in Phase C; false reserved for future cross-permanent triggers
    EntersTheBattlefield { subject_is_self: bool },
    // future: Attacks, Dies, BeginningOfUpkeep, DealsCombatDamage, …
}

pub struct TriggeredAbility {
    pub trigger: TriggerEvent,
    pub effect: Effect,
}
```

### `src/types/mod.rs` — update re-exports

Remove `AbilityEffect`, `EffectStep` from the ability re-export line. Add `EffectStep`, `Effect` to the effect re-export line (alongside the retained `EffectTarget`).

### Call site updates

All callers currently importing `EffectStep` or `AbilityEffect` from `crate::types::ability` (or `crate::types`) switch to importing from `crate::types::effect` (or `crate::types` if re-exported via `mod.rs`). Affected files: `engine/activated.rs`, `parser/oracle.rs`, `serve.rs`, test modules in those files.

---

## Section 2: Parser (`src/parser/oracle.rs`)

### Paragraph processing order

```
1. Em-dash check (existing)
2. Colon check → activated ability (existing)
3. NEW: ETB trigger check → triggered ability
4. Comma-split + keyword matching (existing)
```

### Detection

A new helper:

```rust
fn try_parse_etb_trigger(paragraph: &str, card_name: &str) -> Option<OracleSpan>
```

Detection steps:
1. Strip leading `"When "` or `"Whenever "` (case-insensitive). If neither present, return `None`.
2. Check the subject matches `"this"` or `card_name` (case-insensitive).
3. Check the next word(s) are `"enters"` optionally followed by `"the battlefield"`.
4. Find the first `,` at depth zero — that's the trigger/effect boundary.
5. Parse the effect clause with the existing `parse_ability_effect`.
6. If effect parses → `Parsed(Triggered(TriggeredAbility { trigger: EntersTheBattlefield { subject_is_self: true }, effect }))`.
7. If effect doesn't parse → `ParsedUnimplemented(paragraph)` — recognised as a trigger, not yet enforced.

The card name must be threaded into `parse_oracle_text` — its signature changes from `(text: &str)` to `(text: &str, card_name: &str)`. All call sites (`scryfall.rs`, integration tests, bench fixtures) must be updated. Alternatively, a separate `parse_oracle_text_for_card` overload avoids breaking the existing signature, but a single unified function is cleaner.

### New effect step pattern

`try_parse_effect_step` gains:

| Pattern | Result |
|---|---|
| `"you gain N life"` / `"gain N life"` | `EffectStep::GainLife(N)` |

N may be a digit string or a number word ("three", etc.) via the existing `parse_number_word`.

### Examples

| Oracle text | Span emitted |
|---|---|
| `When this enters, draw a card.` | `Parsed(Triggered { trigger: ETB{self}, effect: [DrawCard(1)] })` |
| `Whenever this enters the battlefield, you gain 3 life.` | `Parsed(Triggered { trigger: ETB{self}, effect: [GainLife(3)] })` |
| `When this enters, draw a card. You gain 2 life.` | `Parsed(Triggered { effect: [DrawCard(1), GainLife(2)] })` |
| `When this enters, create a 1/1 token.` | `ParsedUnimplemented(…)` |
| `When <CardName> enters, draw a card.` | `Parsed(Triggered { … })` — card name matched |

---

## Section 3: Engine (`src/engine/triggered.rs`)

New module with one public function:

```rust
// CR 603.2: triggered abilities trigger when their trigger event occurs.
// Phase C fires ETB triggers immediately (no stack). When the stack project
// lands, this function body becomes "collect onto stack" while the signature stays.
pub fn fire_etb_triggers(state: GameState, entering_id: ObjectId) -> GameState
```

**Steps:**

1. Collect the entering object's triggered abilities: iterate its `abilities` spans, extract `Parsed(Triggered(t))` where `t.trigger == EntersTheBattlefield { subject_is_self: true }`.
2. Determine the entering object's controller.
3. For each ability, apply each `EffectStep` against the controller:
   - `DrawCard(n)` — call existing `draw_card` n times.
   - `GainLife(n)` — add n to `controller.life`.
   - Other variants — `debug_assert!(false, "unexpected EffectStep in ETB trigger")`, then no-op.
4. Return updated state.

**Call sites:** `casting.rs` (creature/artifact/enchantment resolution) and the land-play path in `turn.rs`. Both call `fire_etb_triggers(state, id)` immediately after placing the object on the battlefield.

No new `EngineError` variants needed — draw and gain life cannot fail in the current model.

---

## Section 4: UI (`src/serve.rs`)

No new sidebar section or endpoint. ETB effects resolve before the action response is sent, so the updated life total and hand count appear automatically in the re-rendered state.

**`serve.rs` — add `Parsed(Triggered(_))` arm** to the span-rendering match (currently falls through to the `_ =>` wildcard which emits debug text):

```rust
OracleSpan::Parsed(AbilityAST::Triggered(t)) => OracleSpanView {
    kind: SpanKind::Parsed,
    text: format_triggered_ability(t),
    ignored_kind: None,
},
```

`format_triggered_ability` reconstructs an oracle-like label from the struct (e.g. `"When this enters, draw a card."`). For Phase C it only needs to handle `EntersTheBattlefield` + `DrawCard`/`GainLife` effects, mirroring the pattern of `format_activated_label`.

---

## Section 5: Test Strategy

### Parser tests (`src/parser/oracle.rs`)

- `"When this enters, draw a card."` → `Parsed(Triggered { trigger: ETB{self}, effect: [DrawCard(1)] })`
- `"Whenever this enters the battlefield, you gain 3 life."` → `Parsed(Triggered { effect: [GainLife(3)] })`
- `"When this enters, draw a card. You gain 2 life."` → `effect: [DrawCard(1), GainLife(2)]`
- `"When this enters, create a 1/1 token."` → `ParsedUnimplemented`
- `"When <CardName> enters, draw a card."` → matches on card name as subject
- `"you gain N life"` and `"gain N life"` both parse as `GainLife(N)`
- Existing `triggered_ability_becomes_unparsed` test updated to reflect new behaviour

### Engine tests (`src/engine/triggered.rs`)

- ETB trigger `DrawCard(1)` → controller's hand gains one card
- ETB trigger `GainLife(3)` → controller's life increases by 3
- ETB trigger `[DrawCard(1), GainLife(2)]` → both effects apply
- Object with no triggered abilities → state unchanged
- Trigger fires for entering object's controller, not opponent

### Integration tests

- Cast a creature with `"When this enters, draw a card."` → draw happened (verify hand count and library count after cast)
- Cast a creature with `"When this enters, you gain 3 life."` → life total +3 after cast

### Fixtures

Add an ETB creature to `tests/fixtures/oracle_cards_test.json` (e.g. Elvish Visionary with `"When this enters, draw a card."`).

---

## Files Changed

| File | Change |
|---|---|
| `src/types/effect.rs` | Replace dead `Effect` enum with `EffectStep` (+ `GainLife` variant) and `Effect` type alias; retain `EffectTarget` |
| `src/types/ability.rs` | Remove `EffectStep`, `AbilityEffect`; expand `TriggerEvent` enum; add `effect: Effect` field to `TriggeredAbility` |
| `src/types/mod.rs` | Update re-exports: `EffectStep`/`Effect` from effect, remove `AbilityEffect`/`EffectStep` from ability |
| `src/parser/oracle.rs` | Add `try_parse_etb_trigger`; insert ETB check in paragraph loop; add `GainLife` to `try_parse_effect_step`; thread `card_name` into `parse_oracle_text` signature |
| `src/engine/triggered.rs` | New — `fire_etb_triggers` function |
| `src/engine/mod.rs` | Add `pub mod triggered` |
| `src/engine/activated.rs` | Add `GainLife` match arm (no-op or assert); update `EffectStep` import path |
| `src/serve.rs` | Add `Parsed(Triggered(_))` span arm; add `format_triggered_ability`; update `EffectStep` import path |
| `tests/fixtures/oracle_cards_test.json` | Add ETB creature fixture |
