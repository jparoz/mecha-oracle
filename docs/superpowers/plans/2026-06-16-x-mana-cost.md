# X Mana Cost Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Wire player-chosen X values through the full stack so casting and activating abilities with `{X}` in their cost deducts the chosen amount from the mana pool.

**Architecture:** Four Rust tasks thread `x_value: Option<u32>` from `ActionRequest` down through `cast_spell`/`activate_ability` → `pay_cost_components` → `greedy_payment_plan` → `pay_mana_cost`. Two frontend tasks add a numeric input to the payment panel that is shown only when `{X}` appears in the cost label, and inject the chosen value into the confirm action. `greedy_payment_plan` and `pay_mana_cost` both use a two-pass approach (non-X pips first, X pips last) so fixed colored costs are satisfied before the X allocation is deducted.

**Tech Stack:** Rust (Cargo workspace), vanilla JS + HTML (no build step)

---

### Task 1: Update `greedy_payment_plan`, `can_pay_mana`, and `pay_mana_cost` in `engine/mana.rs`

**Files:**
- Modify: `src/engine/mana.rs`
- Modify: `src/engine/costs.rs` (call-site fix only — add `, None`)

- [ ] **Step 1: Write four new failing tests at the bottom of `mana.rs`'s `tests` module**

Append inside the `mod tests { … }` block:

```rust
#[test]
fn greedy_plan_x_pip_deducts_chosen_amount() {
    let cost = ManaCost {
        pips: vec![ManaPip::X, ManaPip::Red],
    };
    let pool = ManaPool { red: 1, green: 3, ..Default::default() };
    let plan = super::greedy_payment_plan(&cost, &pool, 20, Some(3)).unwrap();
    assert_eq!(plan.x_value, Some(3));
    assert_eq!(plan.red, 1);
    assert_eq!(plan.green, 3);
}

#[test]
fn greedy_plan_x_pip_none_deducts_zero() {
    let cost = ManaCost {
        pips: vec![ManaPip::X, ManaPip::Red],
    };
    let pool = ManaPool { red: 1, ..Default::default() };
    let plan = super::greedy_payment_plan(&cost, &pool, 20, None).unwrap();
    assert_eq!(plan.x_value, None);
    assert_eq!(plan.red, 1);
}

#[test]
fn greedy_plan_x_pip_returns_none_if_insufficient() {
    let cost = ManaCost {
        pips: vec![ManaPip::X],
    };
    let pool = ManaPool { green: 2, ..Default::default() };
    assert!(super::greedy_payment_plan(&cost, &pool, 20, Some(5)).is_none());
}

#[test]
fn pay_mana_cost_x_none_pays_zero() {
    let mut gs = make_state();
    gs.get_player_mut(PlayerId(0)).unwrap().mana_pool.red = 1;
    let cost = ManaCost {
        pips: vec![ManaPip::X, ManaPip::Red],
    };
    let plan = PaymentPlan { red: 1, x_value: None, ..Default::default() };
    let gs = pay_mana_cost(gs, PlayerId(0), &cost, &plan).unwrap();
    assert_eq!(gs.get_player(PlayerId(0)).unwrap().mana_pool.red, 0);
}
```

- [ ] **Step 2: Run tests to confirm they fail**

```bash
cargo test 2>&1 | grep -E "^test result|FAILED|error\["
```

Expected: compile errors — `greedy_payment_plan` called with wrong number of arguments.

- [ ] **Step 3: Replace `greedy_payment_plan` signature and body**

Replace the entire `greedy_payment_plan` function (lines ~200–323 of `src/engine/mana.rs`). Key changes: add `x_value: Option<u32>` param; set `plan.x_value = x_value` unconditionally before the pip loop; skip X pips in the main loop; add a second pass after the loop for X pips.

```rust
/// Build a greedy payment plan for `cost` given current `pool` and player `life`.
/// Returns `None` if no valid plan exists.
/// Non-X pips are satisfied first; X pips are paid from the remaining pool afterward
/// so fixed colored costs are never stolen by the X allocation.
/// CR 107.4: handles all pip types including hybrid, Phyrexian, snow.
pub fn greedy_payment_plan(
    cost: &ManaCost,
    pool: &ManaPool,
    life: i32,
    x_value: Option<u32>,
) -> Option<PaymentPlan> {
    use crate::types::mana::ManaPip::*;
    let mut plan = PaymentPlan::default();
    plan.x_value = x_value;
    let mut rem = pool.clone();
    let mut rem_life = life;

    // First pass: all non-X pips
    for pip in &cost.pips {
        match pip {
            White => deduct_one_color(&ManaColor::White, &mut rem, &mut plan)?,
            Blue => deduct_one_color(&ManaColor::Blue, &mut rem, &mut plan)?,
            Black => deduct_one_color(&ManaColor::Black, &mut rem, &mut plan)?,
            Red => deduct_one_color(&ManaColor::Red, &mut rem, &mut plan)?,
            Green => deduct_one_color(&ManaColor::Green, &mut rem, &mut plan)?,
            Colorless => deduct_one_color(&ManaColor::Colorless, &mut rem, &mut plan)?,
            X => {} // handled in second pass below
            Snow => {
                // Pick first available snow-tagged color (CR 107.4k)
                if rem.snow_white > 0 && rem.white > 0 {
                    rem.white -= 1;
                    rem.snow_white -= 1;
                    plan.white += 1;
                    plan.snow_white += 1;
                } else if rem.snow_blue > 0 && rem.blue > 0 {
                    rem.blue -= 1;
                    rem.snow_blue -= 1;
                    plan.blue += 1;
                    plan.snow_blue += 1;
                } else if rem.snow_black > 0 && rem.black > 0 {
                    rem.black -= 1;
                    rem.snow_black -= 1;
                    plan.black += 1;
                    plan.snow_black += 1;
                } else if rem.snow_red > 0 && rem.red > 0 {
                    rem.red -= 1;
                    rem.snow_red -= 1;
                    plan.red += 1;
                    plan.snow_red += 1;
                } else if rem.snow_green > 0 && rem.green > 0 {
                    rem.green -= 1;
                    rem.snow_green -= 1;
                    plan.green += 1;
                    plan.snow_green += 1;
                } else if rem.snow_colorless > 0 && rem.colorless > 0 {
                    rem.colorless -= 1;
                    rem.snow_colorless -= 1;
                    plan.colorless += 1;
                    plan.snow_colorless += 1;
                } else {
                    return None;
                }
            }
            Phyrexian(c) => {
                // CR 107.4f: may pay 2 life instead of colored mana; prefer blood when enough life
                if rem_life >= 2 {
                    rem_life -= 2;
                    plan.blood += 1;
                } else {
                    deduct_one_color(c, &mut rem, &mut plan)?;
                }
            }
            HybridPhyrexian(c1, c2) => {
                // CR 107.4g: pay either color or 2 life
                if rem_life >= 2 {
                    rem_life -= 2;
                    plan.blood += 1;
                } else {
                    let a1 = amount_for_color(c1, &rem);
                    let a2 = amount_for_color(c2, &rem);
                    let chosen = if a1 >= a2 { c1 } else { c2 };
                    deduct_one_color(chosen, &mut rem, &mut plan)?;
                }
            }
            Hybrid(c1, c2) => {
                // CR 107.4b: pay either color; prefer the side with more available
                let a1 = amount_for_color(c1, &rem);
                let a2 = amount_for_color(c2, &rem);
                if a1 == 0 && a2 == 0 {
                    return None;
                }
                let chosen = if a1 >= a2 { c1 } else { c2 };
                deduct_one_color(chosen, &mut rem, &mut plan)?;
            }
            ColorlessHybrid(c) => {
                // CR 107.4d: pay 1 colorless or 1 of the specified color
                if rem.colorless > 0 {
                    rem.colorless -= 1;
                    plan.colorless += 1;
                } else {
                    deduct_one_color(c, &mut rem, &mut plan)?;
                }
            }
            GenericHybrid(n, c) => {
                // CR 107.4c: pay N generic or 1 of the specified color
                let ca = amount_for_color(c, &rem);
                if ca > 0 {
                    deduct_one_color(c, &mut rem, &mut plan)?;
                } else {
                    let total =
                        rem.white + rem.blue + rem.black + rem.red + rem.green + rem.colorless;
                    if total < *n {
                        return None;
                    }
                    spend_generic_rem(*n, &mut rem, &mut plan);
                }
            }
            Generic(n) => {
                let total = rem.white + rem.blue + rem.black + rem.red + rem.green + rem.colorless;
                if total < *n {
                    return None;
                }
                spend_generic_rem(*n, &mut rem, &mut plan);
            }
        }
    }

    // Second pass: X pips (after fixed costs are satisfied)
    let x_count = cost.pips.iter().filter(|p| matches!(p, X)).count() as u32;
    if x_count > 0 {
        let n = x_value.unwrap_or(0);
        let total_needed = n * x_count;
        let total = rem.white + rem.blue + rem.black + rem.red + rem.green + rem.colorless;
        if total < total_needed {
            return None;
        }
        spend_generic_rem(total_needed, &mut rem, &mut plan);
    }

    Some(plan)
}
```

- [ ] **Step 4: Replace the X arm in `pay_mana_cost`'s pip-by-pip validation block**

In `pay_mana_cost` (around line 385), the `for pip in &cost.pips` loop currently handles `ManaPip::X` inline. Change it to skip X pips in the main loop and add a second pass after.

Change the X arm inside the loop from:
```rust
ManaPip::X => {
    let x_val = plan.x_value.ok_or(EngineError::InvalidPaymentPlan)?;
    let total =
        rem.white + rem.blue + rem.black + rem.red + rem.green + rem.colorless;
    if total < x_val {
        return Err(EngineError::InvalidPaymentPlan);
    }
    spend_from_rem(x_val, &mut rem);
}
```

To:
```rust
ManaPip::X => continue, // handled in second pass after the loop
```

Then immediately after the closing `}` of the `for pip` loop (still inside the outer `{ }` validation block), add:

```rust
// Second pass: X pips (after fixed costs consumed from rem)
let x_count = cost.pips.iter().filter(|p| matches!(p, ManaPip::X)).count() as u32;
if x_count > 0 {
    let x_val = plan.x_value.unwrap_or(0);
    let total_needed = x_val * x_count;
    let total = rem.white + rem.blue + rem.black + rem.red + rem.green + rem.colorless;
    if total < total_needed {
        return Err(EngineError::InvalidPaymentPlan);
    }
    spend_from_rem(total_needed, &mut rem);
}
```

- [ ] **Step 5: Update `can_pay_mana` signature**

```rust
pub fn can_pay_mana(cost: &ManaCost, pool: &ManaPool, life: i32, x_value: Option<u32>) -> bool {
    greedy_payment_plan(cost, pool, life, x_value).is_some()
}
```

- [ ] **Step 6: Fix existing `greedy_payment_plan` and `can_pay_mana` call sites in `mana.rs` tests**

Add `, None` to every existing test call of `greedy_payment_plan` and `can_pay_mana`. There are ~10 occurrences. Use a targeted find-and-replace:

```bash
# Verify the call sites
grep -n "greedy_payment_plan\|can_pay_mana" /Users/jlp/dev/projects/mecha-oracle/src/engine/mana.rs | grep -v "^.*pub fn\|^.*//\|^.*\->"
```

Each call looks like one of:
- `super::greedy_payment_plan(&cost, &pool, 20).unwrap()` → `super::greedy_payment_plan(&cost, &pool, 20, None).unwrap()`
- `super::greedy_payment_plan(&cost, &pool, 20)` → `super::greedy_payment_plan(&cost, &pool, 20, None)`
- `super::can_pay_mana(&cost, &pool, 20)` → `super::can_pay_mana(&cost, &pool, 20, None)`
- `super::can_pay_mana(&cost, &pool, 1)` → `super::can_pay_mana(&cost, &pool, 1, None)`

Update all occurrences manually.

- [ ] **Step 7: Fix the `greedy_payment_plan` call site in `costs.rs`**

In `src/engine/costs.rs`, inside `pay_cost_components`, the line:
```rust
greedy_payment_plan(cost, &player.mana_pool, player.life)
```
becomes:
```rust
greedy_payment_plan(cost, &player.mana_pool, player.life, None)
```

(The `None` is temporary — Task 3 will pass the real x_value through `pay_cost_components`.)

- [ ] **Step 8: Run tests**

```bash
cargo test 2>&1 | grep -E "^test result|FAILED|error\["
```

Expected: all tests pass, including the four new ones.

- [ ] **Step 9: Run clippy**

```bash
cargo clippy --all-targets 2>&1 | grep -E "^error|^warning"
```

Fix any warnings before continuing.

- [ ] **Step 10: Commit**

```bash
git add src/engine/mana.rs src/engine/costs.rs
git commit -m "feat: greedy_payment_plan takes explicit x_value; X pips paid in second pass"
```

---

### Task 2: Thread `x_value` through `pay_cost_components` in `engine/costs.rs`

**Files:**
- Modify: `src/engine/costs.rs`

- [ ] **Step 1: Write a failing test**

Add inside `mod tests` in `costs.rs`:

```rust
#[test]
fn pay_mana_component_with_x_deducts_x_mana() {
    let mut gs = two_player_state();
    gs.get_player_mut(PlayerId(0)).unwrap().mana_pool.green = 5;
    let components = vec![CostComponent::Mana(ManaCost {
        pips: vec![ManaPip::X, ManaPip::Green],
    })];
    // x_value = Some(3): pay 3 generic + 1 green = 4 green total
    let gs = pay_cost_components(gs, PlayerId(0), &components, Some(3)).unwrap();
    assert_eq!(gs.get_player(PlayerId(0)).unwrap().mana_pool.green, 1);
}
```

- [ ] **Step 2: Run test to confirm it fails**

```bash
cargo test 2>&1 | grep -E "^test result|FAILED|error\["
```

Expected: compile error — `pay_cost_components` called with wrong number of arguments.

- [ ] **Step 3: Update `pay_cost_components` signature**

Change:
```rust
pub fn pay_cost_components(
    mut state: GameState,
    player_id: PlayerId,
    components: &[CostComponent],
) -> Result<GameState, EngineError> {
```

To:
```rust
pub fn pay_cost_components(
    mut state: GameState,
    player_id: PlayerId,
    components: &[CostComponent],
    x_value: Option<u32>,
) -> Result<GameState, EngineError> {
```

And inside the function, update the `greedy_payment_plan` call in the `CostComponent::Mana` arm:
```rust
greedy_payment_plan(cost, &player.mana_pool, player.life, x_value)
```

- [ ] **Step 4: Fix all `pay_cost_components` call sites in `costs.rs` tests**

Every test call of `pay_cost_components` gets `, None` appended. There are ~5 calls. Find them:

```bash
grep -n "pay_cost_components(gs" /Users/jlp/dev/projects/mecha-oracle/src/engine/costs.rs
```

Change each `pay_cost_components(gs, PlayerId(0), &components)` to `pay_cost_components(gs, PlayerId(0), &components, None)`.

- [ ] **Step 5: Fix `pay_cost_components` call site in `casting.rs`**

In `src/engine/casting.rs` line ~166:
```rust
super::costs::pay_cost_components(state, player_id, &[CostComponent::Mana(cost.clone())])?;
```
→
```rust
super::costs::pay_cost_components(state, player_id, &[CostComponent::Mana(cost.clone())], None)?;
```

(Temporary `None` — updated to `x_value` in Task 3.)

- [ ] **Step 6: Fix `pay_cost_components` call site in `activated.rs` and rename `_x_value`**

In `src/engine/activated.rs`, rename the parameter in the function signature:
```rust
// before
_x_value: Option<u32>,
// after
x_value: Option<u32>,
```

Then update the `pay_cost_components` call:
```rust
state = pay_cost_components(state, activating_player, &non_tap, x_value)?;
```

(The leading underscore suppresses the unused-variable warning. Once the variable is used in the call, clippy requires removing the underscore.)

- [ ] **Step 7: Run tests**

```bash
cargo test 2>&1 | grep -E "^test result|FAILED|error\["
```

Expected: all tests pass.

- [ ] **Step 8: Run clippy and commit**

```bash
cargo clippy --all-targets 2>&1 | grep -E "^error|^warning"
git add src/engine/costs.rs src/engine/casting.rs src/engine/activated.rs
git commit -m "feat: pay_cost_components accepts x_value; threads to greedy_payment_plan"
```

---

### Task 3: Thread `x_value` through `cast_spell` in `engine/casting.rs` and `serve.rs`

**Files:**
- Modify: `src/engine/casting.rs`
- Modify: `src/serve.rs`

- [ ] **Step 1: Write a failing test in `casting.rs`**

Add inside `mod tests` in `casting.rs`:

```rust
#[test]
fn cast_spell_with_x_deducts_x_mana() {
    use crate::types::mana::ManaPip;
    let mut gs = GameState::new(vec![
        Player::new(PlayerId(0), "Alice"),
        Player::new(PlayerId(1), "Bob"),
    ]);
    gs.get_player_mut(PlayerId(0)).unwrap().mana_pool.red = 4;
    let id = gs.alloc_id();
    let def = make_instant_def("Fireball", vec![ManaPip::X, ManaPip::Red]);
    let obj = CardObject::new(id, def, PlayerId(0), crate::types::Zone::Hand);
    gs.add_object(obj);
    gs.hands.entry(PlayerId(0)).or_default().push(id);
    gs.priority_player = PlayerId(0);

    let gs = cast_spell(gs, PlayerId(0), id, vec![], Some(3)).unwrap();
    // X=3 + R=1 = 4 red spent
    assert_eq!(gs.get_player(PlayerId(0)).unwrap().mana_pool.red, 0);
}
```

- [ ] **Step 2: Run test to confirm it fails**

```bash
cargo test 2>&1 | grep -E "^test result|FAILED|error\["
```

Expected: compile error.

- [ ] **Step 3: Update `cast_spell` signature**

Change:
```rust
pub fn cast_spell(
    mut state: GameState,
    player_id: PlayerId,
    object_id: ObjectId,
    declared_targets: Vec<crate::types::effect::EffectTarget>,
) -> Result<GameState, EngineError> {
```

To:
```rust
pub fn cast_spell(
    mut state: GameState,
    player_id: PlayerId,
    object_id: ObjectId,
    declared_targets: Vec<crate::types::effect::EffectTarget>,
    x_value: Option<u32>,
) -> Result<GameState, EngineError> {
```

And update the `pay_cost_components` call inside the function from:
```rust
super::costs::pay_cost_components(state, player_id, &[CostComponent::Mana(cost.clone())], None)?;
```
to:
```rust
super::costs::pay_cost_components(state, player_id, &[CostComponent::Mana(cost.clone())], x_value)?;
```

- [ ] **Step 4: Fix all `cast_spell` call sites in `casting.rs` tests**

Every test call of `cast_spell` inside `casting.rs` gets `, None` appended as the last argument. There are ~20 occurrences. Find them:

```bash
grep -n "cast_spell(gs\b" /Users/jlp/dev/projects/mecha-oracle/src/engine/casting.rs
```

Patterns to update:
- `cast_spell(gs, PlayerId(0), id, vec![])` → `cast_spell(gs, PlayerId(0), id, vec![], None)`
- `cast_spell(gs, PlayerId(0), bear_id, vec![])` → `cast_spell(gs, PlayerId(0), bear_id, vec![], None)`
- Any multi-line calls: add `, None` before the closing `)`.

- [ ] **Step 5: Fix the `cast_spell` call site in `serve.rs` dispatch**

In `src/serve.rs`, the `ActionRequest::CastSpell` variant (line ~815):
```rust
CastSpell {
    object_id: u64,
    #[serde(default)]
    targets: Vec<mecha_oracle::types::effect::EffectTarget>,
},
```
→
```rust
CastSpell {
    object_id: u64,
    #[serde(default)]
    targets: Vec<mecha_oracle::types::effect::EffectTarget>,
    #[serde(default)]
    x_value: Option<u32>,
},
```

And the dispatch handler (line ~921):
```rust
ActionRequest::CastSpell { object_id, targets } => {
    let player = state.priority_player;
    cast_spell(state, player, ObjectId(object_id), targets).map_err(|e| format!("{e:?}"))
}
```
→
```rust
ActionRequest::CastSpell { object_id, targets, x_value } => {
    let player = state.priority_player;
    cast_spell(state, player, ObjectId(object_id), targets, x_value).map_err(|e| format!("{e:?}"))
}
```

- [ ] **Step 6: Fix the `cast_spell` call site in `serve.rs` tests**

```bash
grep -n "cast_spell(" /Users/jlp/dev/projects/mecha-oracle/src/serve.rs
```

There is 1 production call (already fixed above) and likely ~0 direct test calls of `cast_spell` in `serve.rs` (serve tests use the HTTP dispatch path). Confirm and fix any found.

- [ ] **Step 7: Run tests**

```bash
cargo test 2>&1 | grep -E "^test result|FAILED|error\["
```

Expected: all tests pass including the new Fireball test.

- [ ] **Step 8: Run clippy and commit**

```bash
cargo clippy --all-targets 2>&1 | grep -E "^error|^warning"
git add src/engine/casting.rs src/serve.rs
git commit -m "feat: cast_spell accepts x_value; ActionRequest::CastSpell carries x_value field"
```

---

### Task 4: Add X chooser row to the payment panel in `serve.html`

**Files:**
- Modify: `src/serve.html`

- [ ] **Step 1: Add the X row inside `#payment-panel`**

In `src/serve.html`, inside the `<div id="payment-panel" …>` block, add the X row between `#payment-cost` and `#payment-remaining`:

```html
<!-- Payment context panel (hidden when not in a payment flow) -->
<div id="payment-panel" class="payment-panel" style="display:none">
  <div class="payment-title" id="payment-title">Pay cost</div>
  <div class="payment-cost" id="payment-cost"></div>
  <div id="payment-x-row" style="display:none">
    <label>X = <input type="number" id="payment-x-input" min="0" value="0"
           oninput="renderPaymentPanel()"></label>
  </div>
  <div class="payment-pool" id="payment-pool"></div>
  <div class="payment-remaining" id="payment-remaining"></div>
  <div class="payment-buttons">
    <button id="payment-confirm" onclick="confirmPayment()">Pay</button>
    <button id="payment-cancel" onclick="cancelPayment()" style="display:none">Cancel</button>
    <button id="payment-decline" onclick="declinePayment()" style="display:none">Decline</button>
  </div>
</div>
```

- [ ] **Step 2: Commit**

```bash
git add src/serve.html
git commit -m "feat: add X chooser row to payment panel (hidden until {X} in cost)"
```

---

### Task 5: Wire X logic in `serve.js`

**Files:**
- Modify: `src/serve.js`

- [ ] **Step 1: Update `canPayCost` to accept and apply `xValue`**

Replace the existing `canPayCost` function with:

```js
function canPayCost(costLabel, pool, xValue = 0) {
  if (!costLabel) return true;
  const pips = costLabel.match(/\{([^}]+)\}/g) || [];
  let generic = 0;
  const colored = { W: 0, U: 0, B: 0, R: 0, G: 0, C: 0 };
  for (const pip of pips) {
    const inner = pip.slice(1, -1);
    if (inner === 'T' || inner === 'Q') continue; // tap/untap: structural only
    const n = parseInt(inner, 10);
    if (!isNaN(n)) { generic += n; continue; }
    if (inner === 'X') { generic += xValue; continue; } // each X pip costs xValue
    if (inner.includes('/')) continue; // hybrid/phyrexian: skip
    const col = inner.toUpperCase();
    if (col in colored) colored[col]++;
    else generic++;
  }
  const lifeMatch = costLabel.match(/Pay (\d+) life/);
  if (lifeMatch) {
    const myPid = currentState.priority_player;
    const myPlayer = myPid === 0 ? currentState.p1 : currentState.p2;
    if ((myPlayer?.life || 0) < parseInt(lifeMatch[1], 10)) return false;
  }
  if ((pool.w || 0) < colored.W) return false;
  if ((pool.u || 0) < colored.U) return false;
  if ((pool.b || 0) < colored.B) return false;
  if ((pool.r || 0) < colored.R) return false;
  if ((pool.g || 0) < colored.G) return false;
  if ((pool.c || 0) < colored.C) return false;
  const poolTotal = (pool.w||0)+(pool.u||0)+(pool.b||0)+(pool.r||0)+(pool.g||0)+(pool.c||0);
  const coloredUsed = colored.W+colored.U+colored.B+colored.R+colored.G+colored.C;
  return poolTotal - coloredUsed >= generic;
}
```

- [ ] **Step 2: Update `enterPaymentContext` to reset the X input**

Replace the existing `enterPaymentContext` function:

```js
function enterPaymentContext(kind, actionLabel, costLabel, confirmAction, declineable, declineAction) {
  paymentContext = { kind, actionLabel, costLabel, confirmAction, declineable, declineAction };
  document.getElementById('payment-x-input').value = 0;
  renderPaymentPanel();
}
```

- [ ] **Step 3: Update `renderPaymentPanel` to show/hide the X row and pass `xValue` to `canPayCost`**

Replace the existing `renderPaymentPanel` function:

```js
function renderPaymentPanel() {
  const panel = document.getElementById('payment-panel');
  if (!paymentContext || !currentState) {
    panel.style.display = 'none';
    return;
  }
  panel.style.display = '';
  document.getElementById('payment-title').textContent = paymentContext.actionLabel || 'Pay cost';
  document.getElementById('payment-cost').textContent = paymentContext.costLabel || '(no cost)';
  document.getElementById('payment-pool').textContent = '';

  const myPid = currentState.priority_player;
  const myPlayer = myPid === 0 ? currentState.p1 : currentState.p2;
  const pool = myPlayer ? myPlayer.mana_pool : {};

  const hasX = !!(paymentContext.costLabel && paymentContext.costLabel.includes('{X}'));
  const xRow = document.getElementById('payment-x-row');
  const xInput = document.getElementById('payment-x-input');

  if (hasX) {
    // Compute max X: pool total minus fixed (non-X) pip requirements
    const pips = (paymentContext.costLabel.match(/\{([^}]+)\}/g) || []);
    let fixedGeneric = 0;
    const fixedColored = { W: 0, U: 0, B: 0, R: 0, G: 0, C: 0 };
    let xCount = 0;
    for (const pip of pips) {
      const inner = pip.slice(1, -1);
      if (inner === 'T' || inner === 'Q') continue;
      if (inner === 'X') { xCount++; continue; }
      if (inner.includes('/')) continue;
      const n = parseInt(inner, 10);
      if (!isNaN(n)) { fixedGeneric += n; continue; }
      const col = inner.toUpperCase();
      if (col in fixedColored) fixedColored[col]++;
      else fixedGeneric++;
    }
    const fixedColoredTotal = Object.values(fixedColored).reduce((a, b) => a + b, 0);
    const poolTotal = (pool.w||0)+(pool.u||0)+(pool.b||0)+(pool.r||0)+(pool.g||0)+(pool.c||0);
    const budgetForX = Math.max(0, poolTotal - fixedColoredTotal - fixedGeneric);
    xInput.max = Math.floor(budgetForX / (xCount || 1));
    xRow.style.display = '';
  } else {
    xInput.value = 0;
    xRow.style.display = 'none';
  }

  const xValue = hasX ? parseInt(xInput.value || '0', 10) : 0;
  document.getElementById('payment-confirm').disabled = !canPayCost(paymentContext.costLabel, pool, xValue);
  document.getElementById('payment-cancel').style.display  = paymentContext.declineable ? 'none' : '';
  document.getElementById('payment-decline').style.display = paymentContext.declineable ? '' : 'none';
}
```

- [ ] **Step 4: Update `confirmPayment` to inject `x_value` into the action**

Replace the existing `confirmPayment` function:

```js
function confirmPayment() {
  if (!paymentContext) return;
  const action = { ...paymentContext.confirmAction };
  if (paymentContext.costLabel && paymentContext.costLabel.includes('{X}')) {
    action.x_value = parseInt(document.getElementById('payment-x-input').value || '0', 10);
  }
  paymentContext = null;
  renderPaymentPanel();
  sendAction(action);
}
```

- [ ] **Step 5: Run clippy on Rust (no JS tests), then start the server and manually verify**

```bash
cargo clippy --all-targets 2>&1 | grep -E "^error|^warning"
cargo run -- serve config/example.json
```

Open the UI at `http://localhost:3000`. Load a deck containing an X spell (e.g., Fireball). Tap enough mana. Click the spell — confirm that:
1. The payment panel appears with `X = [0 spinbox]`.
2. Adjusting X updates the Pay button's enabled state.
3. Setting X beyond available mana disables Pay.
4. Clicking Pay with X=3 and sufficient mana successfully casts the spell and deducts (3 + fixed costs) mana.
5. No X row appears for non-X spells.

- [ ] **Step 6: Commit**

```bash
git add src/serve.js src/serve.html
git commit -m "feat: payment panel shows X chooser for {X} costs; injects x_value on confirm"
```
