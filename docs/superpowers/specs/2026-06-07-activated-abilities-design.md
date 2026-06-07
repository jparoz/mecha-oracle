# Activated Abilities — Design Spec

**Date:** 2026-06-07
**Status:** Approved

---

## Goal

Phase B of the parsing expansion: parse `{cost}: effect` syntax from oracle text and execute the resulting abilities in the engine. Targeted scope: mana abilities, mill N, and draw N cards. This gives a complete end-to-end pipeline for oracle-text-derived activated abilities without requiring a targeting system.

This is Phase B of three:
- **Phase A (done):** Fault-tolerant span parser; ability words, reminder text, CR 702 keyword recognition.
- **Phase B (this spec):** Activated ability parsing and execution.
- **Phase C (future):** Triggered ability parsing (`When/Whenever/At…`).

---

## Background

`ActivatedAbility` is currently a unit struct stub. The parser does not recognise `{cost}: effect` lines — they become `Unparsed`. The existing `tap_land_for_mana` in `engine/mana.rs` handles basic land mana abilities via hardcoded subtype lookup (`land_produces`); this is a workaround for intrinsic abilities (CR 305.6) and stays untouched. Phase B adds a parallel path for oracle-text-defined activated abilities.

**Note on intrinsic abilities (future work):** CR 305.6 grants `{T}: Add [mana]` intrinsically to any object with a basic land type (Plains/Island/Swamp/Mountain/Forest), including dual-type lands like Bayou (Swamp Forest → {B} and {G}) and Breeding Pool (Forest Island → {G} and {U}). These must not be represented as oracle text — they belong to the card type system. The existing `land_produces()` only returns one color and is therefore incorrect for multi-subtype lands. A future "intrinsic abilities" plan will address this holistically; other card types (Sagas, etc.) also have intrinsic abilities per the CR.

---

## Section 1: Data Model

### `src/types/ability.rs`

Replace the unit-struct `ActivatedAbility` with:

```rust
pub struct ActivatedAbility {
    pub cost: ActivationCost,
    pub effect: AbilityEffect,
}

pub type ActivationCost = Vec<CostComponent>;
pub type AbilityEffect  = Vec<EffectStep>;

pub enum CostComponent {
    Tap,
    Mana(ManaCost),
    PayLife(u32),
    Sacrifice(u32, PermanentFilter),
    Discard(u32, CardFilter),
    Unimplemented(String), // recognised as a cost token but not yet enforced
}

pub enum EffectStep {
    AddMana(ManaPool),
    Mill(u32),
    DrawCard(u32),
}

pub struct PermanentFilter;  // stub — no fields needed for Phase B
pub struct CardFilter;       // stub — no fields needed for Phase B
```

`ManaPool` comes from `src/types/mana.rs` (existing type, holds per-color u32 fields).

The parser emits `OracleSpan::Parsed(AbilityAST::Activated(ActivatedAbility { … }))` for all lines with a recognised activated-ability structure, even if some cost components are unimplemented. A line with a depth-0 colon whose **effect** cannot be parsed becomes `ParsedUnimplemented` (not `Unparsed`) — it's clearly an ability, just not one we handle yet. Unrecognised cost components become `CostComponent::Unimplemented(String)` and do not block parsing.

---

## Section 2: Parser

### Detection

Activated ability detection happens at the **paragraph level**, before the existing comma-split. The paragraph-processing order becomes:

1. **Em-dash check** (existing) — `Boast — {1}, {T}: …` is caught here and emitted as `ParsedUnimplemented` for the whole line.
2. **NEW: Colon check** — scan for `:` at depth 0, tracking both `{…}` and `(…)` as depth-increasing. If found, attempt to parse as an activated ability.
3. **Comma-split + keyword matching** (existing).

This ordering ensures `({T}: Add {G}.)` (reminder text) is never mistaken for an activated ability: its colon is inside `()` at depth 1.

A new helper replaces `find_at_depth_zero` for this step:

```rust
fn find_colon_at_depth_zero(text: &str) -> Option<usize>
// tracks '(' / ')' and '{' / '}' as nesting delimiters
```

### Cost parsing

Split the left-of-colon string on `,`, trim each token, match:

| Token | Result |
|---|---|
| `{T}` | `CostComponent::Tap` |
| Mana symbols (`{2}`, `{G}`, `{W}{U}`, …) | `CostComponent::Mana(…)` |
| Anything else | `CostComponent::Unimplemented(token)` — ability still parses |

Unimplemented cost components are silently skipped during execution — the ability fires but that part of the cost is not enforced.

### Effect parsing

Strip trailing `.` from the right-of-colon string, lowercase, split on `. ` to get individual effect sentences, then match each:

| Pattern | Result |
|---|---|
| `add {mana}` | `EffectStep::AddMana(…)` |
| `mill N` | `EffectStep::Mill(N)` |
| `draw a card` | `EffectStep::DrawCard(1)` |
| `draw N cards` | `EffectStep::DrawCard(N)` |
| Unrecognised | whole ability → `ParsedUnimplemented` |

Number words ("two", "three") are converted to u32 for Mill and DrawCard.

### Examples

| Oracle text | Spans emitted |
|---|---|
| `{T}: Add {G}.` | `Parsed(Activated { cost: [Tap], effect: [AddMana({G})] })` |
| `{2}, {T}: Add {G}{G}.` | `Parsed(Activated { cost: [Mana({2}), Tap], effect: [AddMana({G}{G})] })` |
| `{1}: Draw a card.` | `Parsed(Activated { cost: [Mana({1})], effect: [DrawCard(1)] })` |
| `{T}: Mill 2.` | `Parsed(Activated { cost: [Tap], effect: [Mill(2)] })` |
| `{T}: Mill 2. Draw a card.` | `Parsed(Activated { cost: [Tap], effect: [Mill(2), DrawCard(1)] })` |
| `({T}: Add {G}.)` | `Ignored(ReminderText, …)` — unchanged |
| `Sacrifice a creature: Add {C}{C}.` | `Parsed(Activated { cost: [Unimplemented("Sacrifice a creature")], effect: [AddMana({C}{C})] })` |
| `{T}: Create a 1/1 token.` | `ParsedUnimplemented(…)` — effect unrecognised |

---

## Section 3: Engine

### New module: `src/engine/activated.rs`

```rust
pub fn activate_ability(
    state: GameState,
    object_id: ObjectId,
    ability_index: usize,
    activating_player: PlayerId,
) -> Result<GameState, EngineError>
```

**Steps:**

1. Look up object → `CardNotFound` if missing; `CardNotOnBattlefield` if not on battlefield.
2. Check `obj.controller == activating_player` → `NotYourCard` if not.
3. Collect the object's activated abilities (iterate spans, extract `Parsed(Activated(_))`), index into them → `AbilityIndexOutOfRange` if out of bounds.
4. Check cost can be paid:
   - `Tap`: object must not be tapped → `AlreadyTapped`; if it's a creature, must not have summoning sickness (unless it has Haste) → `SummoningSick`.
   - `Mana`: controller's pool must cover it → `InsufficientMana`.
   - `Unimplemented`: skip — not checked.
5. Pay cost: tap the object if `Tap` in cost; deduct mana if `Mana` in cost; skip `Unimplemented` components.
6. Apply each `EffectStep` in order:
   - `AddMana(pool)` — add to controller's mana pool; update `mana_checkpoint` (same pattern as `tap_land_for_mana`, so mana reset works uniformly).
   - `Mill(n)` — move top `n` cards of controller's library to their graveyard; if library has fewer than `n`, move all (CR 701.13b, no loss of life).
   - `DrawCard(n)` — call existing `draw_card` `n` times.

### `EngineError` additions (`src/engine/mod.rs`)

```rust
AbilityIndexOutOfRange,
```

### Coexistence with `tap_land_for_mana`

`tap_land_for_mana` (intrinsic-ability path, subtype-driven) and `activate_ability` (oracle-text path) are independent. Basic land mana comes through `tap_land_for_mana`; Llanowar Elves mana comes through `activate_ability`. No conflict.

---

## Section 4: UI

### `serve.rs` — `CardView`

Add field:

```rust
activated_abilities: Vec<ActivatedAbilityView>,
```

```rust
#[derive(Serialize)]
struct ActivatedAbilityView {
    index: usize,
    label: String,      // e.g. "{T}: Add {G}" or "{2}, {T}: Mill 2"
    can_activate: bool, // read-only cost check; does not mutate state
}
```

A helper `can_pay_cost(state, object_id, cost) -> bool` performs the same checks as steps 1–4 of `activate_ability` without mutation.

`label` is reconstructed from the `ActivatedAbility` struct (mana symbols rendered as `{G}` etc.).

### New endpoint

```
POST /activate-ability
Body: { "object_id": u64, "ability_index": usize }
Response: { "ok": true, "state": GameStateView }
        | { "ok": false, "error": String }
```

### Frontend (`serve.html`)

Activated abilities appear as action items in the existing sidebar, alongside land-tap and cast entries. Each item shows:

> **Llanowar Elves** — {T}: Add {G} &nbsp; [Activate]

Items are greyed out when `can_activate` is false. Clicking sends `POST /activate-ability` and refreshes state identically to existing action flows. No buttons on cards or tooltips.

---

## Section 5: Test Strategy

### Parser tests (`src/parser/oracle.rs`)

- `{T}: Add {G}.` → `Parsed(Activated { cost: [Tap], effect: [AddMana({G})] })`
- `{2}, {T}: Add {G}{G}.` → `Parsed(Activated { cost: [Mana({2}), Tap], effect: [AddMana({G}{G})] })`
- `{1}: Draw a card.` → `Parsed(Activated { cost: [Mana({1})], effect: [DrawCard(1)] })`
- `{T}: Mill 2.` → `Parsed(Activated { cost: [Tap], effect: [Mill(2)] })`
- `{T}: Mill 2. Draw a card.` → `Parsed(Activated { cost: [Tap], effect: [Mill(2), DrawCard(1)] })`
- `({T}: Add {G}.)` → `Ignored(ReminderText, …)` — existing behaviour unchanged
- Unrecognised cost token → `CostComponent::Unimplemented`; ability still `Parsed(Activated(…))`
- Unrecognised effect → `ParsedUnimplemented` for the whole ability

### Engine tests (`src/engine/activated.rs`)

- Activate `{T}: Add {G}.` on Llanowar Elves → creature tapped, {G} in pool, checkpoint updated
- Activate on already-tapped creature → `AlreadyTapped`
- Activate tap ability on summoning-sick creature → `SummoningSick`
- Activate `{1}: Draw a card.` with insufficient mana → `InsufficientMana`
- `{T}: Mill 2.` → top 2 library cards in graveyard
- `{T}: Mill 2.` with 1-card library → mills 1, no error
- Ability with `Unimplemented` cost component → component skipped, effect still fires
- `{1}: Draw a card.` → mana deducted, card in hand
- `ability_index` out of range → `AbilityIndexOutOfRange`

### Fixture additions

Add Llanowar Elves to `tests/fixtures/oracle_cards_test.json` as a test card with oracle text `{T}: Add {G}.`

---

## Files Changed

| File | Change |
|---|---|
| `src/types/ability.rs` | Expand `ActivatedAbility`; add `ActivationCost`, `AbilityEffect`, `CostComponent`, `EffectStep`, `PermanentFilter`, `CardFilter` |
| `src/parser/oracle.rs` | Add `find_colon_at_depth_zero`; paragraph-level colon check; cost + effect parsing |
| `src/engine/mod.rs` | Add `AbilityIndexOutOfRange` to `EngineError` |
| `src/engine/activated.rs` | New — `activate_ability` function |
| `src/serve.rs` | `ActivatedAbilityView`; `can_pay_cost` helper; `CardView` field; new endpoint |
| `src/serve.html` | Sidebar items for activated abilities |
| `tests/fixtures/oracle_cards_test.json` | Add Llanowar Elves fixture |
