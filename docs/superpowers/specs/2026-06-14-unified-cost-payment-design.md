# Unified Cost Payment Framework

**Date:** 2026-06-14  
**Status:** Approved

## Summary

Replace the ad-hoc `PayWard` action and duplicated cost-payment logic with a unified cost payment framework. This involves backend code consolidation, type unification, two new server actions, and a client-side payment context that enables players to select a spell or ability before tapping lands.

---

## 1. Cost Type Unification

### Remove `WardCost`

`WardCost { Mana(ManaCost), Life(u32) }` is a strict subset of `CostComponent`. It is eliminated entirely.

Mapping:
- `WardCost::Mana(c)` → `CostComponent::Mana(c)`
- `WardCost::Life(n)` → `CostComponent::PayLife(n)`

### Collapse Ward ability variants

`StaticAbility::WardMana(ManaCost)` and `StaticAbility::WardLife(u32)` merge into:

```rust
StaticAbility::Ward(Vec<CostComponent>)
```

This is more general (future: ward—sacrifice a permanent) and eliminates the special-cased `WardCost` type.

### `StackPayload::WardTrigger`

The `cost` field changes from `WardCost` to `Vec<CostComponent>`. The `paid` flag is renamed to `settled: bool` for clarity, since "paid" is ambiguous when cost can also be declined.

---

## 2. Unified Payment Engine (`engine/costs.rs`)

A new module `src/engine/costs.rs` is the single source of truth for cost payment.

### `pay_cost_components`

```rust
pub fn pay_cost_components(
    state: GameState,
    player_id: PlayerId,
    components: &[CostComponent],
) -> Result<GameState, EngineError>
```

Handles all currently-payable cost types:
- `CostComponent::Mana(cost)` — runs `greedy_payment_plan`, deducts from pool
- `CostComponent::PayLife(n)` — validates sufficient life, deducts
- `CostComponent::Tap` — ignored here; tap is handled by the caller before invoking payment
- `CostComponent::Sacrifice`, `Discard`, `Unimplemented` — silently skipped (not yet implemented)

Used by: `cast_spell`, `activate_ability`, `cycle_card`, and `pay_stack_cost`.

### `can_pay_cost_components`

```rust
pub fn can_pay_cost_components(
    state: &GameState,
    player_id: PlayerId,
    object_id: Option<ObjectId>,
    components: &[CostComponent],
) -> bool
```

Read-only affordability check. `object_id` is required for `Tap` checks (summoning sickness, already-tapped state). Replaces the duplicated logic currently spread across `activated.rs` and `serve.rs`.

### `pay_stack_cost`

```rust
pub fn pay_stack_cost(
    state: GameState,
    player_id: PlayerId,
    stack_id: StackId,
) -> Result<GameState, EngineError>
```

Looks up the `WardTrigger` at `stack_id`, extracts its `Vec<CostComponent>`, calls `pay_cost_components`, then immediately resolves the trigger: since the cost was paid the targeted spell/ability is left on the stack untouched. Removes the WardTrigger from the stack. Priority returns to the appropriate player (APNAP). Called by the `PayCost` server action.

### `resolve_stack_cost_decline`

```rust
pub fn resolve_stack_cost_decline(
    state: GameState,
    stack_id: StackId,
) -> Result<GameState, EngineError>
```

Handles explicit refusal of an optional cost. Immediately resolves the WardTrigger: counters the targeted spell/ability (moves it to graveyard) and removes both the trigger and the countered object from the stack. Priority returns to the appropriate player (APNAP). Called by the `DeclineCost` server action.

---

## 3. Module Restructuring

### `ward.rs` is deleted

Its two responsibilities redistribute:
- `collect_ward_triggers` → moves to `triggered.rs` (alongside `collect_cast_triggers`, `collect_etb_triggers`)
- `pay_ward` → replaced by `pay_stack_cost` / `resolve_stack_cost_decline` in `costs.rs`

Ward-related tests redistribute to `triggered.rs` and `costs.rs` accordingly.

### Updated engine module list

`activated`, `casting`, `combat`, `costs`, `cycling`, `mana`, `stack`, `state_based_actions`, `targeting`, `triggered`, `turn`

---

## 4. Server Actions

### Removed

```rust
PayWard { trigger_id: u64 }
```

### Added

```rust
PayCost { stack_id: u64 }
DeclineCost { stack_id: u64 }
```

**`PayCost`**: Calls `pay_stack_cost(state, priority_player, StackId(stack_id))`. Pays the cost, immediately resolves the WardTrigger (spell survives), removes the trigger from the stack, and returns priority. No subsequent priority pass required.

**`DeclineCost`**: Calls `resolve_stack_cost_decline(state, StackId(stack_id))`. Immediately resolves the WardTrigger — counters the targeted spell, removes both stack objects, and returns priority. No subsequent priority pass required.

Both actions are symmetric: they fully settle the trigger in a single round-trip, without leaving a "pending" trigger on the stack that players must then pass around.

Both actions require the named `stack_id` to be on top of the stack and to be a cost-bearing stack object (`WardTrigger`). Any other stack object returns `EngineError::NotYourPriority`.

### `can_pay_cost` field removed from `ActionItemView`

The field was used to grey out action buttons when the mana pool was insufficient. With the payment context handling affordability display, this is no longer the right level to express cost information. An action is either structurally impossible (not emitted) or structurally legal (emitted and always interactable). The field is removed from `ActionItemView` and the serialised API response.

---

## 5. Client-Side Payment Context

The client maintains a single `paymentContext` object in JS state (`null` when inactive).

### Shape

```js
{
  kind: "cast" | "activate" | "ward",
  cost: CostComponent[],     // what needs to be paid; drives the payment panel display
  confirmAction: Object,     // JSON payload to POST to /action on Confirm/Pay
  declineable: bool,         // true for ward (and future optional costs); false for cast/activate
  declineAction: Object,     // present iff declineable; for ward: { type: "decline_cost", stack_id }
}
```

### Entry points

| Trigger | kind | declineable |
|---|---|---|
| Player clicks a "Cast X" action button | `"cast"` | false |
| Player clicks an "Activate Y" action button | `"activate"` | false |
| Game view contains a `WardTrigger` on top of stack with local player having priority | `"ward"` | true |

Ward context is entered automatically (no button click) when the game view snapshot detects the condition.

### Buttons

| Button | Condition | Behaviour |
|---|---|---|
| **Confirm / Pay** | cost is fully covered by current mana pool / life | POST `confirmAction`; clear context on response |
| **Cancel** | `kind` is `"cast"` or `"activate"` | Clear context; POST `reset_mana` if `mana_checkpoint` is present in game view |
| **Decline** | `declineable` is true | POST `declineAction`; clear context on response |

No Cancel button for ward — the trigger is on the stack and cannot be un-targeted.

### Payment panel

Shows while context is active:
- Cost breakdown (e.g. `{2}`, `Pay 2 life`)
- Current mana pool
- Remaining to pay (cost minus pool contents)
- Confirm/Pay button (enabled when remaining = zero)
- Cancel or Decline as appropriate

### Land taps during payment context

Land tap buttons remain live. Each tap POSTs `tap_land` as before — the payment panel refreshes automatically on each state update, showing the updated pool and remaining cost.

### Cast / activate button behaviour change

Cast and activate buttons no longer POST directly to `/action`. Instead they enter the payment context. They are shown as interactable whenever they are structurally legal (correct phase, player has priority, etc.), regardless of mana pool contents.

---

## 6. Parser update

`oracle.rs` must be updated to parse Ward into the new `StaticAbility::Ward(Vec<CostComponent>)` shape rather than `WardMana`/`WardLife`. Both "Ward {N}" (mana cost) and "Ward—Pay N life" (life cost) must produce the correct `CostComponent` vector.

---

## Out of Scope

- Tap, Sacrifice, Discard cost components are not yet payable via the payment panel (displayed but not interactive)
- Kicker, buyback, overload, and other optional/alternative costs are not yet modelled
- Explicit payment plans (the client supplying a `PaymentPlan` to override greedy) are not added in this change
- Multi-cost ward (e.g. ward—pay 2 life and {2}) is supported by the type but not yet parsed
