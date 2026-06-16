# X Mana Cost — Design Spec

**Date:** 2026-06-16  
**Status:** Approved

---

## Problem

`ManaPip::X` and `PaymentPlan::x_value` are already modelled in the type system, but X is always treated as 0 at payment time. Players have no way to choose an X value when casting or activating abilities with `{X}` in the cost.

---

## Scope

Wire X value choice through the full stack: UI → action request → engine. Covers both `cast_spell` and `activate_ability`. No changes to game-rule semantics beyond what CR 107.4 and CR 202.3 already require.

---

## Backend

### `greedy_payment_plan` (engine/mana.rs)

Signature gains a fourth parameter: `x_value: Option<u32>`.

Behaviour change:
- Remove the current auto-detect block that sets `plan.x_value = Some(0)` when an X pip is found.
- Set `plan.x_value = x_value` unconditionally — the field now faithfully mirrors what the caller passed.
- In the X pip arm, deduct `x_value.unwrap_or(0)` generic mana from the remaining pool via `spend_generic_rem`. Return `None` if the pool cannot cover that amount.

Invariant: `plan.x_value == None` means "X not specified, treat as 0." `plan.x_value == Some(n)` means the player explicitly chose n.

### `pay_mana_cost` (engine/mana.rs)

The X pip arm changes:
```
// before
let x_val = plan.x_value.ok_or(EngineError::InvalidPaymentPlan)?;
// after
let x_val = plan.x_value.unwrap_or(0);
```
`None` is now a valid plan for X pips (pays 0), consistent with the new `greedy_payment_plan` contract.

### `can_pay_mana` (engine/mana.rs)

Gains `x_value: Option<u32>` parameter; threads it to `greedy_payment_plan`. All existing call sites pass `None`.

### `pay_cost_components` (engine/costs.rs)

Gains `x_value: Option<u32>` parameter; threads it to `greedy_payment_plan` inside the `CostComponent::Mana` arm.

### `cast_spell` (engine/casting.rs)

Gains `x_value: Option<u32>` parameter; passes it to `pay_cost_components`.

### `activate_ability` (engine/activated.rs)

The existing `_x_value: Option<u32>` parameter is renamed to `x_value` and threaded to `pay_cost_components`.

### `ActionRequest` (serve.rs)

`CastSpell` variant gains:
```rust
#[serde(default)]
x_value: Option<u32>,
```

The `dispatch_action` handler passes it to `cast_spell`. `ActivateAbility` already has the field; its handler already passes it to `activate_ability` (which now uses it).

---

## Frontend

### serve.html

Add an X chooser row inside `#payment-panel`, hidden by default:
```html
<div id="payment-x-row" style="display:none">
  <label>X = <input type="number" id="payment-x-input" min="0" value="0"
         oninput="renderPaymentPanel()"></label>
</div>
```

### serve.js

**`enterPaymentContext`** — unchanged signature; X detection happens in `renderPaymentPanel` from the stored `costLabel`.

**`renderPaymentPanel`** — detects `{X}` in `paymentContext.costLabel`. When present:
- Shows `#payment-x-row`.
- Sets `max` on `#payment-x-input` to `poolTotal - nonXPipSum` (client-side usability cap; server validates authoritatively).

When absent:
- Hides `#payment-x-row`.
- Resets input value to 0.

**`canPayCost(costLabel, pool, xValue)`** — gains `xValue` parameter (default 0). The `{X}` pip branch is removed from the skip list; instead it adds `xValue` to the generic requirement, identical to how `{N}` pips are handled.

**`confirmPayment`** — reads `#payment-x-input` value when `{X}` is in the cost. Injects `x_value: <n>` into the action object before sending. For non-X costs the field is omitted (server default is `None`).

---

## Error handling

Server-side `pay_mana_cost` and `greedy_payment_plan` return `Err(InsufficientMana)` / `None` if the player cannot afford X at the chosen value. The client surfaces this as a toast (existing error path). The confirm button's disabled state provides a first line of defence.

---

## Testing

- Update all `greedy_payment_plan` / `can_pay_mana` call sites in existing tests to pass the new `x_value` / `None` argument.
- Add unit tests:
  - `greedy_plan_x_pip_deducts_chosen_amount` — Some(3) deducts 3 generic, plan.x_value = Some(3).
  - `greedy_plan_x_pip_none_deducts_zero` — None deducts 0, plan.x_value = None.
  - `greedy_plan_x_pip_returns_none_if_insufficient` — Some(5) with pool of 3 returns None.
  - `pay_mana_cost_x_none_pays_zero` — X pip with plan.x_value = None succeeds and deducts nothing.

No new integration tests for the serve layer (existing dispatch tests cover the happy path structure).

---

## Out of scope

- Multiple `{X}` pips (e.g. `{X}{X}`) — each X pip deducts the same chosen value, which is correct per CR 107.4. No extra work needed.
- XX costs — same treatment.
- X in non-mana cost components (e.g. "sacrifice X creatures") — unimplemented cost components are already skipped.
