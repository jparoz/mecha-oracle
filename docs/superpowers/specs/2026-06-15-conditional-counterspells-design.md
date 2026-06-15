# Conditional Counterspells Design

**Date:** 2026-06-15  
**Scope:** Targeting restrictions on counter spells (mana value thresholds, color filters) and inline "unless its controller pays" cost conditions (fixed mana or life). X-cost variants (Syncopate, Condescend's X) are out of scope. Ward is migrated from `WardTrigger` to the same `IfPaid` mechanism.

---

## Background

Unconditional counterspells are implemented: `SpellFilter`, `EffectTarget::StackObject`, and `EffectStep::CounterSpell` all exist. The remaining work is:

1. **Category 1 — targeting restrictions**: cards like Disdainful Stroke ("counter target spell with mana value 4 or greater") and Flashfreeze ("counter target red or green spell") require `SpellFilter` to carry mana-value and color predicates, evaluated at targeting time.

2. **Category 2 — inline cost conditions**: cards like Mana Leak ("counter target spell unless its controller pays {3}") and Quench ("counter target spell unless its controller pays {2}") require pausing mid-resolution to offer the targeted spell's controller an optional payment, then countering only if they decline.

Ward already implements case 2 via a bespoke `StackPayload::WardTrigger` + `pay_stack_cost`/`resolve_stack_cost_decline`. This design replaces that mechanism with a general `EffectStep::IfPaid` variant and a `PendingPayment` game-state field, so Ward and Mana Leak share the same code path.

---

## Section 1 — Type System Changes (`types/`)

### 1a. `Cost` type alias (`types/ability.rs`)

Rename `ActivationCost` to `Cost`. Activated abilities, Ward's cost components, and the new `IfPaid` step all describe the same concept.

```rust
pub type Cost = Vec<CostComponent>;
```

All existing references to `ActivationCost` (currently only `ActivatedAbility::cost`) are updated to `Cost`.

### 1b. Extended `SpellFilter` (`types/ability.rs`)

Add three new optional fields to `SpellFilter`:

```rust
pub struct SpellFilter {
    pub included_types: Vec<CardType>,
    pub excluded_types: Vec<CardType>,
    pub min_mana_value: Option<u32>, // spell MV must be ≥ this (CR 202.3)
    pub max_mana_value: Option<u32>, // spell MV must be ≤ this
    pub any_of_colors: Vec<ManaColor>, // spell must have ≥1 of these colors; empty = no constraint
}
```

All three new fields default to `None` / empty vec via the existing `#[derive(Default)]`, so `SpellFilter::any()` and all existing constructors are unaffected.

Update `SpellFilter::matches` signature:

```rust
pub fn matches(&self, card_types: &[CardType], mana_value: u32, colors: &[ManaColor]) -> bool
```

Logic additions (both must hold alongside the existing `included`/`excluded` checks):
- `min_mana_value`: if `Some(n)`, `mana_value >= n`
- `max_mana_value`: if `Some(n)`, `mana_value <= n`
- `any_of_colors`: if non-empty, `colors` must contain at least one element from `any_of_colors`

Mana value computation (CR 202.3): sum the converted pip values from `ManaCost::pips`. Generic pips contribute their face value; coloured pips contribute 1 each; X pips contribute 0 (X = 0 for this purpose). This is a pure function on `ManaCost` and lives in `engine/targeting.rs` alongside the call site.

### 1c. `EffectStep::IfPaid` (`types/effect.rs`)

New variant:

```rust
IfPaid {
    cost: Cost,
    then: Effect,   // steps to run if the player pays
    else_: Effect,  // steps to run if the player declines
}
```

`Effect = Vec<EffectStep>` is already defined. `Vec` is heap-allocated so no `Box` is needed to break the recursive type.

Example — Mana Leak:
```rust
EffectStep::IfPaid {
    cost: vec![CostComponent::Mana(ManaCost { pips: vec![ManaPip::Generic(3)] })],
    then: vec![],
    else_: vec![EffectStep::CounterSpell],
}
```

Example — Ward {2}:
```rust
EffectStep::IfPaid {
    cost: vec![CostComponent::Mana(ManaCost { pips: vec![ManaPip::Generic(2)] })],
    then: vec![],
    else_: vec![EffectStep::CounterSpell],
}
```

---

## Section 2 — `PendingPayment` and `GameState` (`types/game_state.rs`)

### `PendingPayment`

```rust
pub struct PendingPayment {
    /// Player who must pay or decline (derived from the targeted spell's controller).
    pub paying_player: PlayerId,
    pub cost: Cost,
    /// Steps to execute if the player pays (often empty).
    pub on_paid: Effect,
    /// Steps to execute if the player declines (e.g. [CounterSpell]).
    pub on_declined: Effect,
    /// Steps that always execute after the paid/declined branch (e.g. Scry 2 on Condescend).
    /// For all in-scope cards this is empty, but the field supports future cards.
    pub continuation: Effect,
    /// Targets passed through from the resolving stack object (used by CounterSpell in else_).
    pub targets: Vec<EffectTarget>,
    /// Controller of the spell/ability that contained the IfPaid step.
    pub controller: PlayerId,
}
```

The `paying_player` is derived in `execute_effect_steps` from the resolving stack object's `targets`: the first `EffectTarget::StackObject` target's stack object carries a `controller` — that controller is the paying player. This correctly handles both Mana Leak (caster targets opponent's spell → opponent pays) and Ward (trigger targets the triggering spell → the targeting player pays).

### `GameState` field

```rust
pub pending_payment: Option<PendingPayment>,
```

Added to `GameState` struct, initialised `None` in `GameState::new`.

---

## Section 3 — Engine Changes

### 3a. `execute_effect_steps` (`engine/stack.rs`)

In the `EffectStep::IfPaid` arm:

1. Determine `paying_player`: look up `targets.iter().find_map(|t| if let EffectTarget::StackObject { id } = t { state.stack_objects.get(id).map(|o| o.controller) } else { None })`. If no stack-object target is found, fall back to the resolving object's controller (handles life-payment Ward variants).
2. Capture `continuation = steps[current_index + 1 ..]` (the remaining steps after `IfPaid`).
3. Set `state.pending_payment = Some(PendingPayment { paying_player, cost: step.cost.clone(), on_paid: step.then.clone(), on_declined: step.else_.clone(), continuation, targets: targets.to_vec(), controller })`.
4. Return `state` immediately (skipping remaining steps; they are stored in `continuation`).

For the cards in scope (Mana Leak, Quench, Ward), `IfPaid` is always the last step, so `continuation` is always empty. The mechanism is in place for cards with post-payment steps.

### 3b. `pay_pending_cost` (`engine/costs.rs`)

New function:

```rust
pub fn pay_pending_cost(state: GameState, player_id: PlayerId) -> Result<GameState, EngineError>
```

Steps:
1. Get `pending` from `state.pending_payment`; error (`NotYourPriority`) if `None` or `paying_player != player_id`.
2. `state = pay_cost_components(state, player_id, &pending.cost)?`
3. Execute `pending.on_paid` steps via `execute_effect_steps(&mut state, &pending.on_paid, &pending.targets)`.
4. Execute `pending.continuation` steps via `execute_effect_steps`.
5. `state.pending_payment = None`.
6. `state.priority_player = state.active_player`.
7. Return `Ok(state)`.

### 3c. `decline_pending_cost` (`engine/costs.rs`)

New function:

```rust
pub fn decline_pending_cost(state: GameState) -> Result<GameState, EngineError>
```

Steps:
1. Get `pending` from `state.pending_payment`; error if `None`.
2. Execute `pending.on_declined` steps (e.g. `CounterSpell`).
3. Execute `pending.continuation` steps.
4. `state.pending_payment = None`.
5. `state.priority_player = state.active_player`.
6. Return `Ok(state)`.

No player parameter — any player can call this (realistically only `paying_player` will, but the engine doesn't enforce it; the UI enforces who sees the action).

### 3d. Ward migration (`engine/triggered.rs`)

`collect_ward_triggers` is updated to emit `StackPayload::TriggeredAbility` instead of `WardTrigger`:

```rust
StackObject {
    id: sid,
    payload: StackPayload::TriggeredAbility {
        source_id: ward_permanent_id,
        effect: vec![EffectStep::IfPaid {
            cost: ward_cost,
            then: vec![],
            else_: vec![EffectStep::CounterSpell],
        }],
        label: format!("Ward — {}", cost_display),
    },
    controller: ward_permanent_controller,
    targets: vec![EffectTarget::StackObject { id: triggering_stack_id }],
}
```

The `EffectTarget::StackObject` target carries the triggering spell's `StackId`. When `CounterSpell` in `else_` executes, it reads `targets.first()` (as in all other counterspells) to find what to counter. No special-casing needed.

### 3e. Removals

- `StackPayload::WardTrigger` — removed from `types/stack.rs`.
- `pay_stack_cost` — removed from `engine/costs.rs` (replaced by `pay_pending_cost`).
- `resolve_stack_cost_decline` — removed from `engine/costs.rs` (replaced by `decline_pending_cost`).
- All `WardTrigger`-specific tests in `costs.rs` — replaced by `IfPaid` integration tests.

---

## Section 4 — Parser Changes (`parser/oracle.rs`)

### 4a. Category 1 — targeting restriction patterns

After parsing the base "counter target spell" clause, check for additional restriction phrases and merge them into the `SpellFilter`. Patterns to recognise (case-insensitive, after stripping the terminal period):

| Additional phrase | `SpellFilter` field set |
|---|---|
| `"with mana value N or greater"` | `min_mana_value: Some(N)` |
| `"with mana value N or less"` | `max_mana_value: Some(N)` |
| `"[color] spell"` (e.g. "red spell") | `any_of_colors: [Red]` |
| `"[color] or [color] spell"` | `any_of_colors: [c1, c2]` |

Color names map to `ManaColor` variants (white, blue, black, red, green). Combinations may be merged: "counter target blue or black spell with mana value 3 or less" would produce `any_of_colors: [Blue, Black], max_mana_value: Some(3)`.

The parser handles these as suffix modifiers on the existing "counter target [type] spell" patterns. The restriction phrase appears between the type word and the period.

### 4b. Category 2 — "unless" suffix

After the full counter-target pattern is identified (including any category 1 restrictions), strip the optional "unless its controller pays {N}" or "unless its controller pays N life" suffix:

- `"unless its controller pays {N}"` → `CostComponent::Mana(ManaCost { pips: vec![ManaPip::Generic(N)] })`
- `"unless its controller pays N life"` → `CostComponent::PayLife(N)`

When this suffix is present, wrap the `CounterSpell` step in `IfPaid`:

```
counter target spell unless its controller pays {3}.
→ SpellAbility {
    target_requirements: [TargetFilter::Spell(SpellFilter::any())],
    steps: [EffectStep::IfPaid {
        cost: vec![CostComponent::Mana(ManaCost { pips: vec![ManaPip::Generic(3)] })],
        then: vec![],
        else_: vec![EffectStep::CounterSpell],
    }],
  }
```

Without the suffix, `steps` is simply `[EffectStep::CounterSpell]` as before.

---

## Section 5 — Serve Layer (`serve.rs`)

### WardTrigger rendering removed

The `WardTrigger` match arm in any stack-label or action-building code is removed. Ward triggers now render as ordinary `TriggeredAbility` entries using their `label` field (e.g. "Ward — {2}").

### Action changes

Remove: `PayStackCost` and `ResolveStackCostDecline` actions.

Add: `PayPendingCost` and `DeclinePendingCost` actions. These are emitted when `state.pending_payment.is_some()` and `state.pending_payment.paying_player == player_id`.

- `PayPendingCost` label: `"Pay {cost} (Ward / Mana Leak)"` — or derived from `pending_payment.cost` using the existing cost-display helper.
- `DeclinePendingCost` label: `"Decline (spell will be countered)"`.

The UI presents these actions to the paying player only. All other players see no actions while a `pending_payment` is set.

### `format_cost_label` helper

Rename `format_ward_cost_label` → `format_cost_label`. Used to produce human-readable cost strings from `Vec<CostComponent>`.

### `is_legal_target` call site in `serve.rs`

Updated to pass `mana_value` and `colors` to `SpellFilter::matches`. The `mana_value` is computed from the stack object's `ManaCost`; `colors` come from the card's `colors` field (added in the color-tracking feature).

---

## Testing

### `types/ability.rs`

- `spell_filter_min_mana_value_passes` — MV ≥ 4 filter accepts MV=4, MV=5; rejects MV=3
- `spell_filter_max_mana_value_passes` — MV ≤ 2 filter accepts MV=0, MV=2; rejects MV=3
- `spell_filter_any_of_colors_blue` — blue filter accepts blue spell, rejects red spell, rejects colorless
- `spell_filter_color_and_mana_value` — combined: blue OR black AND MV ≤ 3

### `engine/targeting.rs`

- `disdainful_stroke_rejects_low_mv_spell` — `min_mana_value: Some(4)` rejects a spell with MV=2
- `disdainful_stroke_accepts_high_mv_spell` — same filter accepts MV=4
- `flashfreeze_rejects_white_spell` — `any_of_colors: [Red, Green]` rejects a white spell
- `flashfreeze_accepts_red_spell` — same filter accepts a red spell

### `engine/costs.rs`

- `pay_pending_cost_deducts_mana_and_clears` — mana payment, counter not fired, `pending_payment = None`
- `pay_pending_cost_deducts_life_and_clears` — life payment variant
- `pay_pending_cost_insufficient_mana_returns_error` — payment fails, state unchanged
- `decline_pending_cost_fires_on_declined_and_clears` — CounterSpell in `else_` executes, spell in graveyard, `pending_payment = None`
- `pay_pending_cost_wrong_player_returns_error` — wrong `player_id` → `NotYourPriority`

### `engine/stack.rs` (integration)

- `mana_leak_sets_pending_payment` — Mana Leak resolves against a spell; `pending_payment` is set; targeted spell still on stack
- `mana_leak_counter_on_decline` — after `decline_pending_cost`, targeted spell in graveyard
- `mana_leak_survives_on_payment` — after `pay_pending_cost` (with 3 mana), targeted spell still on stack
- `ward_now_uses_if_paid` — Ward trigger payload is `TriggeredAbility` with `IfPaid` effect (no `WardTrigger` variant exists)

### `parser/oracle.rs`

- `disdainful_stroke_parses_min_mana_value` — "counter target spell with mana value 4 or greater." → `min_mana_value: Some(4)`
- `quench_parses_max_mana_value` — "counter target spell with mana value 3 or less." → `max_mana_value: Some(3)`
- `flashfreeze_parses_color_filter` — "counter target red or green spell." → `any_of_colors: [Red, Green]`
- `mana_leak_parses_unless_mana` — "counter target spell unless its controller pays {3}." → `IfPaid` with generic(3)
- `quench_parses_unless_mana` — "counter target spell unless its controller pays {2}." → `IfPaid` with generic(2)
- `life_payment_parses_unless_life` — "counter target spell unless its controller pays 3 life." → `IfPaid` with `PayLife(3)`
