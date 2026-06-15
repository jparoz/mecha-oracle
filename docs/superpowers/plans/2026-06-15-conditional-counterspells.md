# Conditional Counterspells Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Implement conditional counterspells (mana-value/color targeting restrictions + "unless pays" conditions) and migrate Ward from the bespoke `WardTrigger` mechanism to the new generic `Payment` effect step.

**Architecture:** Add `EffectStep::Payment { cost, on_paid, on_declined }` as a generic conditional step; during resolution, `execute_effect_steps` pauses into `GameState::pending_payment`, which `pay_pending_cost`/`decline_pending_cost` then resume. `SpellFilter` gains mana-value and color predicates for targeting restrictions. Ward's `WardTrigger` payload is replaced by `TriggeredAbility` carrying a `Payment` step.

**Tech Stack:** Rust, Cargo, no external deps beyond existing codebase.

---

## File Map

| File | Change |
|------|--------|
| `src/types/ability.rs` | Rename `ActivationCost → Cost`; extend `SpellFilter` with `min_mana_value`, `max_mana_value`, `any_of_colors`; update `matches` signature |
| `src/types/mod.rs` | Update re-exports: `ActivationCost → Cost`, add `PendingPayment` |
| `src/types/effect.rs` | Add `EffectStep::Payment { cost: Cost, on_paid: Effect, on_declined: Effect }` |
| `src/types/game_state.rs` | Add `PendingPayment` struct; add `pending_payment: Option<PendingPayment>` to `GameState` |
| `src/types/stack.rs` | Remove `WardTrigger` variant |
| `src/engine/targeting.rs` | Add `mana_value()` helper; update `is_legal_target` to pass `mana_value`/`colors` to `SpellFilter::matches` |
| `src/engine/stack.rs` | Make `execute_effect_steps` `pub(crate)`; add `Payment` arm; update `resolve_top` to route priority to `paying_player` when `pending_payment` is set; remove `WardTrigger` arm |
| `src/engine/costs.rs` | Add `pay_pending_cost` / `decline_pending_cost`; remove `pay_stack_cost` / `resolve_stack_cost_decline` |
| `src/engine/triggered.rs` | Update `collect_ward_triggers` to emit `TriggeredAbility` with `Payment` effect |
| `src/parser/oracle.rs` | Refactor counter section to support mana-value, color, and "unless pays" patterns |
| `src/serve.rs` | Remove `WardTrigger` rendering; add `pending_payment` to `GameView`; replace `PayCost`/`DeclineCost` with `PayPendingCost`/`DeclinePendingCost` |
| `docs/todo.md` | Remove the `🔁 Conditional counter spells` section |

---

### Task 1: Rename `ActivationCost` → `Cost`

**Files:**
- Modify: `src/types/ability.rs:64`
- Modify: `src/types/mod.rs:14`

- [ ] **Step 1: Write a failing test** (in `types/ability.rs` tests)

```rust
#[test]
fn cost_type_alias_is_vec_cost_component() {
    let c: Cost = vec![CostComponent::Tap];
    assert_eq!(c.len(), 1);
}
```

Run: `cargo test -p mecha_oracle cost_type_alias 2>&1 | grep -E "^test result|FAILED|error\["` → Expected: compile error `Cost` not found.

- [ ] **Step 2: Rename `ActivationCost` to `Cost` in `ability.rs`**

In `src/types/ability.rs`, line 64, change:
```rust
pub type ActivationCost = Vec<CostComponent>;
```
to:
```rust
pub type Cost = Vec<CostComponent>;
```

Also update `ActivatedAbility::cost` field type from `ActivationCost` to `Cost` (line 59):
```rust
pub struct ActivatedAbility {
    pub cost: Cost,
    pub target_requirements: Vec<TargetFilter>,
    pub effect: Effect,
}
```

Also update `StaticAbility::Ward` — it takes `Vec<CostComponent>` directly so no change needed there.

- [ ] **Step 3: Update re-export in `src/types/mod.rs`**

Change line 14 from:
```rust
pub use ability::{
    Ability, ActivatedAbility, ActivationCost, CardFilter, CastFilter, CostComponent, IgnoredKind,
    LandwalkKind, OracleSpan, PermanentFilter, SpellAbility, StaticAbility, TargetFilter,
    TriggerEvent, TriggeredAbility,
};
```
to:
```rust
pub use ability::{
    Ability, ActivatedAbility, Cost, CardFilter, CastFilter, CostComponent, IgnoredKind,
    LandwalkKind, OracleSpan, PermanentFilter, SpellAbility, StaticAbility, TargetFilter,
    TriggerEvent, TriggeredAbility,
};
```

- [ ] **Step 4: Fix call sites** — grep for `ActivationCost` across the whole codebase and replace with `Cost`:

```bash
grep -rn "ActivationCost" src/
```

Update any occurrences found (expected: imports in `engine/activated.rs` or similar).

- [ ] **Step 5: Run tests**

```bash
cargo test 2>&1 | grep -E "^test result|FAILED|error\["
```
Expected: all pass.

- [ ] **Step 6: Commit**

```bash
git add src/types/ability.rs src/types/mod.rs
git commit -m "refactor: rename ActivationCost to Cost in types/ability.rs"
```

---

### Task 2: Extend `SpellFilter` with mana-value and color predicates

**Files:**
- Modify: `src/types/ability.rs` — `SpellFilter` struct + `matches` method + all `matches` call sites within the file

- [ ] **Step 1: Write failing tests** (add to `ability.rs` tests)

```rust
#[test]
fn spell_filter_min_mana_value_accepts_at_or_above() {
    let f = SpellFilter { min_mana_value: Some(4), ..SpellFilter::default() };
    assert!(f.matches(&[], 4, &[]));
    assert!(f.matches(&[], 5, &[]));
    assert!(!f.matches(&[], 3, &[]));
}

#[test]
fn spell_filter_max_mana_value_accepts_at_or_below() {
    let f = SpellFilter { max_mana_value: Some(2), ..SpellFilter::default() };
    assert!(f.matches(&[], 0, &[]));
    assert!(f.matches(&[], 2, &[]));
    assert!(!f.matches(&[], 3, &[]));
}

#[test]
fn spell_filter_any_of_colors_must_match_at_least_one() {
    use crate::types::mana::ManaColor;
    let f = SpellFilter { any_of_colors: vec![ManaColor::Red, ManaColor::Green], ..SpellFilter::default() };
    assert!(f.matches(&[], 0, &[ManaColor::Red]));
    assert!(f.matches(&[], 0, &[ManaColor::Green]));
    assert!(!f.matches(&[], 0, &[ManaColor::Blue]));
    assert!(!f.matches(&[], 0, &[]));
}

#[test]
fn spell_filter_combined_mv_and_color() {
    use crate::types::mana::ManaColor;
    let f = SpellFilter {
        any_of_colors: vec![ManaColor::Blue],
        min_mana_value: Some(3),
        ..SpellFilter::default()
    };
    assert!(f.matches(&[], 3, &[ManaColor::Blue]));
    assert!(!f.matches(&[], 2, &[ManaColor::Blue])); // MV too low
    assert!(!f.matches(&[], 3, &[ManaColor::Red])); // wrong color
}
```

Run: `cargo test spell_filter_min 2>&1 | grep -E "^test result|FAILED|error\["` → Expected: compile error (new fields don't exist yet).

- [ ] **Step 2: Extend `SpellFilter` struct** (in `src/types/ability.rs`, around line 173)

```rust
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct SpellFilter {
    pub included_types: Vec<CardType>,
    pub excluded_types: Vec<CardType>,
    pub min_mana_value: Option<u32>, // CR 202.3: spell MV must be ≥ this; None = no constraint
    pub max_mana_value: Option<u32>, // spell MV must be ≤ this; None = no constraint
    pub any_of_colors: Vec<ManaColor>, // spell must share ≥1 color; empty = no constraint
}
```

Add `ManaColor` to the imports at the top of `ability.rs`:
```rust
use super::mana::{ManaColor, ManaCost};
```
(ManaColor is already used by `ProtectionFromColor` so it's likely already imported — check and add if missing.)

- [ ] **Step 3: Update `SpellFilter::matches`** (around line 207)

```rust
pub fn matches(&self, card_types: &[CardType], mana_value: u32, colors: &[ManaColor]) -> bool {
    let included_ok = self.included_types.is_empty()
        || self.included_types.iter().any(|t| card_types.contains(t));
    let excluded_ok = self.excluded_types.iter().all(|t| !card_types.contains(t));
    let min_ok = self.min_mana_value.map_or(true, |n| mana_value >= n);
    let max_ok = self.max_mana_value.map_or(true, |n| mana_value <= n);
    let color_ok = self.any_of_colors.is_empty()
        || self.any_of_colors.iter().any(|c| colors.contains(c));
    included_ok && excluded_ok && min_ok && max_ok && color_ok
}
```

- [ ] **Step 4: Fix existing `matches` call sites inside `ability.rs` tests**

Every test that calls `f.matches(&[...])` needs updating to `f.matches(&[...], 0, &[])`. Existing tests to update:
- `spell_filter_any_matches_all_types`: `f.matches(&[CardType::Creature])` → `f.matches(&[CardType::Creature], 0, &[])`
- `spell_filter_noncreature_excludes_creature_spells`: same pattern
- `spell_filter_creature_includes_creature_only`: same
- `spell_filter_instant_or_sorcery_matches_either`: same

- [ ] **Step 5: Fix `matches` call site in `engine/targeting.rs`** (line 80)

Change:
```rust
spell_filter.matches(card_types)
```
to (compute mana_value from card definition — a helper defined in next step):
```rust
let mv = card_obj.definition.mana_cost.as_ref().map(mana_value_of).unwrap_or(0);
let colors = &card_obj.definition.colors;
spell_filter.matches(card_types, mv, colors)
```

And update the surrounding block to also fetch `card_obj`:
```rust
EffectTarget::StackObject { id } => {
    if let TargetFilter::Spell(spell_filter) = filter {
        let Some(sobj) = state.stack_objects.get(id) else {
            return false;
        };
        let StackPayload::Spell { card_id } = &sobj.payload else {
            return false;
        };
        let Some(card_obj) = state.objects.get(card_id) else {
            return false;
        };
        let card_types = &card_obj.definition.type_line.card_types;
        let mv = card_obj.definition.mana_cost.as_ref().map(mana_value_of).unwrap_or(0);
        let colors = &card_obj.definition.colors;
        spell_filter.matches(card_types, mv, colors)
    } else {
        false
    }
}
```

Add the helper function at the top of `engine/targeting.rs` (before `is_legal_target`):

```rust
// CR 202.3: mana value is the sum of pip values. X = 0, colored pips = 1 each,
// generic pips = face value, hybrid pips = 1.
fn mana_value_of(cost: &crate::types::mana::ManaCost) -> u32 {
    use crate::types::mana::ManaPip;
    cost.pips
        .iter()
        .map(|pip| match pip {
            ManaPip::Generic(n) => *n,
            ManaPip::X => 0,
            ManaPip::White
            | ManaPip::Blue
            | ManaPip::Black
            | ManaPip::Red
            | ManaPip::Green
            | ManaPip::Colorless => 1,
            ManaPip::Hybrid(_, _) => 1,
        })
        .sum()
}
```

- [ ] **Step 6: Run tests**

```bash
cargo test 2>&1 | grep -E "^test result|FAILED|error\["
```
Expected: all pass (including the new SpellFilter tests).

- [ ] **Step 7: Commit**

```bash
git add src/types/ability.rs src/engine/targeting.rs
git commit -m "feat: extend SpellFilter with mana value and color predicates (CR 202.3)"
```

---

### Task 3: Add `EffectStep::Payment`, `PendingPayment`, and `GameState::pending_payment`

**Files:**
- Modify: `src/types/effect.rs`
- Modify: `src/types/game_state.rs`
- Modify: `src/types/mod.rs`

- [ ] **Step 1: Write failing tests**

In `src/types/effect.rs` tests:
```rust
#[test]
fn payment_step_construction() {
    use crate::types::ability::{Cost, CostComponent};
    use crate::types::mana::{ManaCost, ManaPip};
    let step = EffectStep::Payment {
        cost: vec![CostComponent::Mana(ManaCost { pips: vec![ManaPip::Generic(3)] })],
        on_paid: vec![],
        on_declined: vec![EffectStep::CounterSpell],
    };
    assert!(matches!(step, EffectStep::Payment { .. }));
}
```

In `src/types/game_state.rs` tests:
```rust
#[test]
fn pending_payment_starts_none() {
    let gs = two_player_state();
    assert!(gs.pending_payment.is_none());
}
```

Run: `cargo test payment_step_construction 2>&1 | grep -E "^test result|FAILED|error\["` → Expected: compile error.

- [ ] **Step 2: Add `EffectStep::Payment`** (in `src/types/effect.rs`, after the `CounterSpell` variant)

First, add the `Cost` import at the top of `effect.rs`:
```rust
use super::ability::Cost;
```

Then add the variant to `EffectStep`:
```rust
pub enum EffectStep {
    AddMana(ManaPool),
    Mill(u32),
    DrawCard(u32),
    GainLife(u32),
    BoostPermanentPT(PTDelta),
    DealDamage(u32),
    CounterSpell,
    /// CR 118.12: inline cost-payment obligation raised during resolution.
    /// Pauses effect resolution; `pay_pending_cost`/`decline_pending_cost` resume it.
    Payment {
        cost: Cost,
        on_paid: Effect,
        on_declined: Effect,
    },
    Unimplemented(String),
}
```

- [ ] **Step 3: Add `PendingPayment` and update `GameState`** (in `src/types/game_state.rs`)

Add imports at the top:
```rust
use super::ability::Cost;
use super::effect::{Effect, EffectTarget};
```

Add the struct before `GameState`:
```rust
/// CR 118.12: an inline cost-payment obligation raised during the resolution
/// of a spell or ability. Set by `EffectStep::Payment`; cleared by
/// `pay_pending_cost` or `decline_pending_cost`.
#[derive(Debug, Clone)]
pub struct PendingPayment {
    /// The player who must pay or decline.
    pub paying_player: PlayerId,
    pub cost: Cost,
    /// Steps to execute if the player pays (often empty).
    pub on_paid: Effect,
    /// Steps to execute if the player declines (e.g. [CounterSpell]).
    pub on_declined: Effect,
    /// Steps after the payment decision that always run (for future use).
    pub continuation: Effect,
    /// Targets from the resolving stack object; passed to on_paid/on_declined.
    pub targets: Vec<EffectTarget>,
    /// Controller of the spell/ability containing the Payment step.
    pub controller: PlayerId,
}
```

Add `pub pending_payment: Option<PendingPayment>,` to `GameState` struct (after `game_over`):
```rust
pub game_over: bool,
pub pending_payment: Option<PendingPayment>,
```

Add `pending_payment: None,` to `GameState::new` initializer:
```rust
Self {
    // ... existing fields ...
    game_over: false,
    pending_payment: None,
}
```

- [ ] **Step 4: Re-export `PendingPayment` from `src/types/mod.rs`**

Change the `game_state` re-export line from:
```rust
pub use game_state::{CombatState, GameState, ManaCheckpoint, Phase, Step};
```
to:
```rust
pub use game_state::{CombatState, GameState, ManaCheckpoint, PendingPayment, Phase, Step};
```

- [ ] **Step 5: Run tests**

```bash
cargo test 2>&1 | grep -E "^test result|FAILED|error\["
```
Expected: all pass.

- [ ] **Step 6: Commit**

```bash
git add src/types/effect.rs src/types/game_state.rs src/types/mod.rs
git commit -m "feat: add EffectStep::Payment, PendingPayment, GameState::pending_payment"
```

---

### Task 4: Handle `Payment` in `execute_effect_steps`; add `pay_pending_cost`/`decline_pending_cost`

**Files:**
- Modify: `src/engine/stack.rs`
- Modify: `src/engine/costs.rs`

- [ ] **Step 1: Write failing tests**

In `src/engine/stack.rs` tests (below existing tests, around the end of the test module):
```rust
#[test]
fn payment_step_sets_pending_payment() {
    use crate::types::ability::{CostComponent, Cost};
    use crate::types::effect::{EffectStep, EffectTarget};
    use crate::types::mana::{ManaCost, ManaPip};
    use crate::types::stack::{StackId, StackObject, StackPayload};
    use crate::types::{CardObject, Zone};
    use crate::types::card::{CardDefinition, CardType, TypeLine};

    let mut gs = make_state();

    // Put a target spell on the stack (the spell being "paid against")
    let target_card_id = gs.alloc_id();
    let def = CardDefinition {
        name: "Target Spell".into(),
        mana_cost: Some(ManaCost { pips: vec![] }),
        type_line: TypeLine { supertypes: vec![], card_types: vec![CardType::Instant], subtypes: vec![] },
        oracle_text: String::new(),
        abilities: vec![],
        text_annotations: vec![],
        power: None, toughness: None, colors: vec![],
    };
    let target_obj = CardObject::new(target_card_id, def, PlayerId(1), Zone::Stack);
    gs.add_object(target_obj);
    let target_sid = gs.alloc_stack_id();
    gs.stack_objects.insert(target_sid, StackObject {
        id: target_sid,
        payload: StackPayload::Spell { card_id: target_card_id },
        controller: PlayerId(1),
        targets: vec![],
    });
    gs.stack.push(target_sid);

    let cost: Cost = vec![CostComponent::Mana(ManaCost { pips: vec![ManaPip::Generic(3)] })];
    let steps = vec![EffectStep::Payment {
        cost: cost.clone(),
        on_paid: vec![],
        on_declined: vec![EffectStep::CounterSpell],
    }];
    let targets = vec![EffectTarget::StackObject { id: target_sid }];

    let gs = execute_effect_steps(gs, PlayerId(0), &steps, &targets);

    assert!(gs.pending_payment.is_some());
    let pp = gs.pending_payment.as_ref().unwrap();
    assert_eq!(pp.paying_player, PlayerId(1)); // target spell's controller
    assert_eq!(pp.cost, cost);
    assert_eq!(pp.on_declined, vec![EffectStep::CounterSpell]);
    // target spell still on stack (not countered yet)
    assert!(gs.stack.contains(&target_sid));
}
```

In `src/engine/costs.rs` tests:
```rust
#[test]
fn pay_pending_cost_clears_payment_and_runs_on_paid() {
    use crate::types::ability::CostComponent;
    use crate::types::effect::{Effect, EffectStep, EffectTarget};
    use crate::types::game_state::PendingPayment;
    use crate::types::mana::{ManaCost, ManaPip};

    let mut gs = two_player_state();
    gs.get_player_mut(PlayerId(0)).unwrap().mana_pool.colorless = 3;
    let cost = vec![CostComponent::Mana(ManaCost { pips: vec![ManaPip::Generic(3)] })];
    gs.pending_payment = Some(PendingPayment {
        paying_player: PlayerId(0),
        cost: cost.clone(),
        on_paid: vec![],
        on_declined: vec![EffectStep::CounterSpell],
        continuation: vec![],
        targets: vec![],
        controller: PlayerId(1),
    });

    let gs = pay_pending_cost(gs, PlayerId(0)).unwrap();

    assert!(gs.pending_payment.is_none());
    assert_eq!(gs.get_player(PlayerId(0)).unwrap().mana_pool.colorless, 0);
}

#[test]
fn pay_pending_cost_wrong_player_returns_error() {
    use crate::types::ability::CostComponent;
    use crate::types::effect::EffectStep;
    use crate::types::game_state::PendingPayment;

    let mut gs = two_player_state();
    gs.pending_payment = Some(PendingPayment {
        paying_player: PlayerId(0),
        cost: vec![CostComponent::PayLife(1)],
        on_paid: vec![],
        on_declined: vec![EffectStep::CounterSpell],
        continuation: vec![],
        targets: vec![],
        controller: PlayerId(1),
    });

    let result = pay_pending_cost(gs, PlayerId(1)); // wrong player
    assert!(matches!(result, Err(EngineError::NotYourPriority)));
}

#[test]
fn decline_pending_cost_executes_on_declined_and_clears() {
    use crate::types::ability::CostComponent;
    use crate::types::effect::EffectStep;
    use crate::types::game_state::PendingPayment;
    use crate::types::stack::{StackId, StackObject, StackPayload};
    use crate::types::{CardObject, Zone};
    use crate::types::card::{CardDefinition, CardType, TypeLine};
    use crate::types::mana::ManaCost;
    use crate::types::effect::EffectTarget;

    let mut gs = two_player_state();

    // Put a spell on the stack so CounterSpell has something to counter
    let card_id = gs.alloc_id();
    let def = CardDefinition {
        name: "Victim".into(),
        mana_cost: Some(ManaCost { pips: vec![] }),
        type_line: TypeLine { supertypes: vec![], card_types: vec![CardType::Instant], subtypes: vec![] },
        oracle_text: String::new(),
        abilities: vec![],
        text_annotations: vec![],
        power: None, toughness: None, colors: vec![],
    };
    let obj = CardObject::new(card_id, def, PlayerId(1), Zone::Stack);
    gs.add_object(obj);
    let sid = gs.alloc_stack_id();
    gs.stack_objects.insert(sid, StackObject {
        id: sid,
        payload: StackPayload::Spell { card_id },
        controller: PlayerId(1),
        targets: vec![],
    });
    gs.stack.push(sid);

    gs.pending_payment = Some(PendingPayment {
        paying_player: PlayerId(1),
        cost: vec![CostComponent::PayLife(3)],
        on_paid: vec![],
        on_declined: vec![EffectStep::CounterSpell],
        continuation: vec![],
        targets: vec![EffectTarget::StackObject { id: sid }],
        controller: PlayerId(0),
    });

    let gs = decline_pending_cost(gs).unwrap();

    assert!(gs.pending_payment.is_none());
    assert!(!gs.stack.contains(&sid));
    assert!(!gs.stack_objects.contains_key(&sid));
    let gy = gs.graveyards.get(&PlayerId(1)).unwrap();
    assert!(gy.contains(&card_id));
}
```

Run: `cargo test payment_step_sets_pending 2>&1 | grep -E "^test result|FAILED|error\["` → Expected: compile error (functions not public or not defined yet).

- [ ] **Step 2: Make `execute_effect_steps` `pub(crate)` and add `Payment` arm** (in `src/engine/stack.rs`)

Change the function signature from `fn execute_effect_steps(` to `pub(crate) fn execute_effect_steps(`.

Change the loop from `for step in steps` to indexed iteration, and add the `Payment` arm. The full updated function body:

```rust
pub(crate) fn execute_effect_steps(
    mut state: GameState,
    controller: PlayerId,
    steps: &[EffectStep],
    targets: &[crate::types::effect::EffectTarget],
) -> GameState {
    use crate::types::effect::EffectTarget;
    use crate::types::game_state::PendingPayment;
    for (i, step) in steps.iter().enumerate() {
        match step {
            EffectStep::DrawCard(n) => {
                for _ in 0..*n {
                    state = draw_card(state, controller);
                }
            }
            EffectStep::GainLife(n) => {
                if let Some(player) = state.get_player_mut(controller) {
                    player.life += *n as i32;
                }
            }
            EffectStep::Mill(n) => {
                let to_mill = (*n as usize)
                    .min(state.libraries.get(&controller).map_or(0, |l| l.len()));
                for _ in 0..to_mill {
                    if let Some(card_id) = state
                        .libraries
                        .get_mut(&controller)
                        .filter(|l| !l.is_empty())
                        .map(|l| l.remove(0))
                    {
                        if let Some(gy) = state.graveyards.get_mut(&controller) {
                            gy.push(card_id);
                        }
                        if let Some(obj) = state.objects.get_mut(&card_id) {
                            obj.zone = Zone::Graveyard;
                        }
                    }
                }
            }
            EffectStep::AddMana(_) => {
                unreachable!("AddMana in stack object");
            }
            EffectStep::BoostPermanentPT(delta) => {
                if let Some(EffectTarget::Object { id }) = targets.first()
                    && let Some(perm) = state.battlefield.get_mut(id)
                {
                    perm.pt_boost_until_eot.power += delta.power;
                    perm.pt_boost_until_eot.toughness += delta.toughness;
                }
            }
            EffectStep::DealDamage(n) => match targets.first() {
                Some(EffectTarget::Object { id }) => {
                    if let Some(perm) = state.battlefield.get_mut(id) {
                        perm.damage_marked += n;
                    }
                }
                Some(EffectTarget::Player { id }) => {
                    if let Some(player) = state.get_player_mut(*id) {
                        player.life -= *n as i32;
                    }
                }
                _ => {}
            },
            EffectStep::CounterSpell => {
                if let Some(EffectTarget::StackObject { id }) = targets.first() {
                    counter_spell_on_stack(&mut state, *id);
                }
            }
            // CR 118.12: pause resolution and raise a cost-payment obligation.
            // The paying player is derived from the first StackObject target's controller
            // (the caster of the targeted spell). Falls back to the resolving controller.
            EffectStep::Payment { cost, on_paid, on_declined } => {
                let paying_player = targets
                    .iter()
                    .find_map(|t| {
                        if let EffectTarget::StackObject { id } = t {
                            state.stack_objects.get(id).map(|o| o.controller)
                        } else {
                            None
                        }
                    })
                    .unwrap_or(controller);
                let continuation = steps[i + 1..].to_vec();
                state.pending_payment = Some(PendingPayment {
                    paying_player,
                    cost: cost.clone(),
                    on_paid: on_paid.clone(),
                    on_declined: on_declined.clone(),
                    continuation,
                    targets: targets.to_vec(),
                    controller,
                });
                return state;
            }
            EffectStep::Unimplemented(_) => {}
        }
    }
    state
}
```

- [ ] **Step 3: Update `resolve_top` to route priority when `pending_payment` is set**

In the `StackPayload::Spell` branch of `resolve_top` (around line 215 in original), after the `execute_effect_steps` call and after moving the card to graveyard, add the pending_payment check before the existing `state.priority_player = state.active_player` line:

```rust
state = execute_effect_steps(state, controller, &steps, &targets);

if let Some(obj) = state.objects.get_mut(&card_id) {
    obj.zone = Zone::Graveyard;
}
if let Some(gy) = state.graveyards.get_mut(&controller) {
    gy.push(card_id);
}

// If a Payment step paused resolution, give priority to the paying player.
if let Some(pp) = &state.pending_payment {
    let paying_player = pp.paying_player;
    state.consecutive_passes = 0;
    state.priority_player = paying_player;
    return check_and_apply_sbas(state);
}

state.consecutive_passes = 0;
state.priority_player = state.active_player;
check_and_apply_sbas(state)
```

Do the same in the `StackPayload::TriggeredAbility | StackPayload::ActivatedAbility` branch (after `execute_effect_steps`, before `state.priority_player = state.active_player`):

```rust
state = execute_effect_steps(state, controller, &effect, &targets);

if let Some(pp) = &state.pending_payment {
    let paying_player = pp.paying_player;
    state.consecutive_passes = 0;
    state.priority_player = paying_player;
    return check_and_apply_sbas(state);
}

state.consecutive_passes = 0;
state.priority_player = state.active_player;
check_and_apply_sbas(state)
```

- [ ] **Step 4: Add `pay_pending_cost` and `decline_pending_cost`** (in `src/engine/costs.rs`)

Add imports at the top:
```rust
use crate::engine::stack::execute_effect_steps;
use crate::types::game_state::PendingPayment;
```

Add both functions after `can_pay_cost_components`:

```rust
// CR 118.12: pay an inline cost obligation and execute on_paid + continuation steps.
pub fn pay_pending_cost(
    mut state: GameState,
    player_id: PlayerId,
) -> Result<GameState, EngineError> {
    let pending = match state.pending_payment.take() {
        Some(p) => p,
        None => return Err(EngineError::NotYourPriority),
    };
    if pending.paying_player != player_id {
        state.pending_payment = Some(pending);
        return Err(EngineError::NotYourPriority);
    }
    state = pay_cost_components(state, player_id, &pending.cost)?;
    state = execute_effect_steps(state, pending.controller, &pending.on_paid, &pending.targets);
    state = execute_effect_steps(state, pending.controller, &pending.continuation, &pending.targets);
    state.consecutive_passes = 0;
    state.priority_player = state.active_player;
    Ok(state)
}

// CR 118.12: decline an inline cost obligation; execute on_declined + continuation steps.
pub fn decline_pending_cost(mut state: GameState) -> Result<GameState, EngineError> {
    let pending = state.pending_payment.take().ok_or(EngineError::NotYourPriority)?;
    state = execute_effect_steps(state, pending.controller, &pending.on_declined, &pending.targets);
    state = execute_effect_steps(state, pending.controller, &pending.continuation, &pending.targets);
    state.consecutive_passes = 0;
    state.priority_player = state.active_player;
    Ok(state)
}
```

- [ ] **Step 5: Run tests**

```bash
cargo test 2>&1 | grep -E "^test result|FAILED|error\["
```
Expected: all pass.

- [ ] **Step 6: Commit**

```bash
git add src/engine/stack.rs src/engine/costs.rs
git commit -m "feat: Payment step in execute_effect_steps; pay_pending_cost / decline_pending_cost"
```

---

### Task 5: Ward migration — remove `WardTrigger`, update triggered.rs, serve.rs

This task is atomic: removing `WardTrigger` breaks compile until all call sites are updated.

**Files:**
- Modify: `src/types/stack.rs` — remove `WardTrigger`
- Modify: `src/engine/triggered.rs` — update `collect_ward_triggers`
- Modify: `src/engine/stack.rs` — remove `WardTrigger` arm in `resolve_top`
- Modify: `src/engine/costs.rs` — remove `pay_stack_cost` / `resolve_stack_cost_decline`
- Modify: `src/serve.rs` — remove `WardTrigger` rendering; update actions

- [ ] **Step 1: Write failing tests**

In `src/engine/triggered.rs` tests, add:
```rust
#[test]
fn collect_ward_triggers_emits_triggered_ability_with_payment() {
    use crate::engine::triggered::collect_ward_triggers;
    use crate::types::OracleSpan;
    use crate::types::ability::{Ability, Cost, CostComponent, StaticAbility};
    use crate::types::card::{CardDefinition, CardType, TypeLine};
    use crate::types::effect::{EffectStep, EffectTarget};
    use crate::types::mana::{ManaCost, ManaPip};
    use crate::types::stack::{StackObject, StackPayload};

    let mut gs = two_player_state();

    // A creature with Ward {2} controlled by P1
    let ward_cost = vec![CostComponent::Mana(ManaCost { pips: vec![ManaPip::Generic(2)] })];
    let ward_def = CardDefinition {
        name: "Ward Creature".into(),
        mana_cost: None,
        type_line: TypeLine {
            supertypes: vec![],
            card_types: vec![CardType::Creature],
            subtypes: vec![],
        },
        oracle_text: String::new(),
        abilities: vec![OracleSpan::Parsed(Ability::Static(StaticAbility::Ward(
            ward_cost.clone(),
        )))],
        text_annotations: vec![],
        power: Some(2), toughness: Some(2), colors: vec![],
    };
    let ward_id = place_on_battlefield(&mut gs, ward_def, PlayerId(1));

    // A spell on the stack controlled by P0 targeting the ward creature
    let triggering_sid = gs.alloc_stack_id();
    gs.stack_objects.insert(triggering_sid, StackObject {
        id: triggering_sid,
        payload: StackPayload::Spell { card_id: gs.alloc_id() },
        controller: PlayerId(0),
        targets: vec![],
    });
    gs.stack.push(triggering_sid);

    let targets = vec![EffectTarget::Object { id: ward_id }];
    let triggers = collect_ward_triggers(&mut gs, triggering_sid, PlayerId(0), &targets);

    assert_eq!(triggers.len(), 1);
    let trigger = &triggers[0];
    assert_eq!(trigger.controller, PlayerId(1));
    // Must be TriggeredAbility (not WardTrigger)
    let StackPayload::TriggeredAbility { effect, .. } = &trigger.payload else {
        panic!("expected TriggeredAbility, got something else");
    };
    assert_eq!(effect.len(), 1);
    assert!(matches!(&effect[0], EffectStep::Payment { .. }));
    // The target of the ward trigger should be the triggering spell
    assert_eq!(trigger.targets, vec![EffectTarget::StackObject { id: triggering_sid }]);
}
```

Run: `cargo test ward_triggers_emits_triggered 2>&1 | grep -E "^test result|FAILED|error\["` — Expected: fail (wrong variant).

- [ ] **Step 2: Update `collect_ward_triggers` in `src/engine/triggered.rs`**

Replace the loop body that builds `StackPayload::WardTrigger { ... }` with `StackPayload::TriggeredAbility`:

```rust
for cost in ward_cost_sets {
    let sid = state.alloc_stack_id();
    let label = if cost.len() == 1 {
        match &cost[0] {
            CostComponent::Mana(m) => format!("Ward \u{2014} {m}"),
            CostComponent::PayLife(n) => format!("Ward \u{2014} Pay {n} life"),
            _ => "Ward".to_string(),
        }
    } else {
        "Ward".to_string()
    };
    triggers.push(StackObject {
        id: sid,
        payload: StackPayload::TriggeredAbility {
            source_id: target_obj_id,
            effect: vec![crate::types::effect::EffectStep::Payment {
                cost,
                on_paid: vec![],
                on_declined: vec![crate::types::effect::EffectStep::CounterSpell],
            }],
            label,
        },
        controller: ward_permanent_controller,
        targets: vec![crate::types::effect::EffectTarget::StackObject {
            id: triggering_stack_id,
        }],
    });
}
```

Remove the `CostComponent` import from the `use` block inside the function if it is now unused there (it's still used as part of the label-building match).

- [ ] **Step 3: Remove `WardTrigger` variant from `src/types/stack.rs`**

Delete lines 27-31 (the `WardTrigger` variant and its doc comment):
```rust
// DELETE THIS:
/// CR 702.21a — Counters the triggering spell/ability if the Ward cost is not settled.
WardTrigger {
    counters_if_unpaid: StackId,
    cost: Vec<super::ability::CostComponent>,
    settled: bool,
},
```

- [ ] **Step 4: Remove `WardTrigger` arm from `resolve_top` in `src/engine/stack.rs`**

Delete the entire `StackPayload::WardTrigger { ... }` arm (lines 250-262 in original):
```rust
// DELETE THIS:
// CR 702.21a: WardTrigger resolution — if not settled, counter the spell.
StackPayload::WardTrigger {
    counters_if_unpaid,
    settled,
    ..
} => {
    if !settled {
        counter_spell_on_stack(&mut state, counters_if_unpaid);
    }
    state.consecutive_passes = 0;
    state.priority_player = state.active_player;
    check_and_apply_sbas(state)
}
```

- [ ] **Step 5: Remove `pay_stack_cost` and `resolve_stack_cost_decline` from `src/engine/costs.rs`**

Delete the functions `pay_stack_cost` (lines 74-109) and `resolve_stack_cost_decline` (lines 111-138), along with their doc comments.

Delete the now-unused import `use crate::types::stack::{StackId, StackPayload};` if it is no longer needed (check; `StackPayload` may still be used in tests, but `StackId` is only used by the removed functions).

Delete or update the Ward-specific unit tests that used `push_ward_trigger` / `pay_stack_cost` / `resolve_stack_cost_decline`:
- `pay_stack_cost_mana_removes_trigger_and_deducts_mana`
- `pay_stack_cost_life_removes_trigger_and_deducts_life`
- `pay_stack_cost_not_on_top_returns_error`
- `pay_stack_cost_insufficient_mana_returns_error`
- `pay_stack_cost_insufficient_life_returns_error`
- `decline_removes_trigger_and_counters_spell`
- `decline_not_on_top_returns_error`

These are now replaced by the `pay_pending_cost` / `decline_pending_cost` tests added in Task 4.

Also delete the `push_ward_trigger` and `push_spell` helper functions from the test module in `costs.rs` if they are now unused.

- [ ] **Step 6: Update `src/serve.rs`**

**6a.** Update the import at the top (line 11-13). Remove `pay_stack_cost, resolve_stack_cost_decline`, add `pay_pending_cost, decline_pending_cost`:
```rust
use mecha_oracle::engine::costs::{
    can_pay_cost_components, pay_pending_cost, decline_pending_cost,
};
```

**6b.** Remove `format_ward_cost_label` (lines 703-713). It is replaced by the general `format_cost_label` (which already exists as `format_ability_cost_label`). If you want a simpler cost-to-string helper, rename `format_ability_cost_label` (or keep both — but `format_ward_cost_label` must be removed since it was Ward-specific).

Alternatively, just remove `format_ward_cost_label`; `format_ability_cost_label` already handles `Mana` and `PayLife` components.

**6c.** In `build_game_view`, delete the `StackPayload::WardTrigger { ... }` match arm (lines 782-797).

**6d.** Add `pending_payment` field to `GameView`. In the struct definition:
```rust
struct GameView {
    // ... existing fields ...
    pending_payment: Option<PendingPaymentView>,
}
```

Add a new inner struct:
```rust
#[derive(Serialize)]
struct PendingPaymentView {
    paying_player: PlayerId,
    cost_label: String,
}
```

**6e.** In `build_game_view`, populate the new field (add after the `stack` computation):
```rust
pending_payment: state.pending_payment.as_ref().map(|pp| PendingPaymentView {
    paying_player: pp.paying_player,
    cost_label: format_ability_cost_label(&pp.cost),
}),
```

**6f.** Update `ActionRequest` enum: replace `PayCost { stack_id: u64 }` and `DeclineCost { stack_id: u64 }` with:
```rust
/// CR 118.12: pay the current inline cost obligation.
PayPendingCost,
/// CR 118.12: decline the current inline cost obligation (spell will be countered).
DeclinePendingCost,
```

**6g.** Update `dispatch_action`: replace the `PayCost`/`DeclineCost` arms with:
```rust
ActionRequest::PayPendingCost => {
    let player = state.priority_player;
    pay_pending_cost(state, player).map_err(|e| format!("{e:?}"))
}
ActionRequest::DeclinePendingCost => {
    decline_pending_cost(state).map_err(|e| format!("{e:?}"))
}
```

- [ ] **Step 7: Run tests**

```bash
cargo test 2>&1 | grep -E "^test result|FAILED|error\["
```
Expected: all pass.

- [ ] **Step 8: Clippy**

```bash
cargo clippy --all-targets 2>&1 | grep -E "^error|^warning"
```
Fix any warnings.

- [ ] **Step 9: Commit**

```bash
git add src/types/stack.rs src/engine/triggered.rs src/engine/stack.rs src/engine/costs.rs src/serve.rs
git commit -m "feat: migrate Ward from WardTrigger to TriggeredAbility+Payment; remove WardTrigger"
```

---

### Task 6: Parser — Category 1 (mana-value and color targeting restrictions)

**Files:**
- Modify: `src/parser/oracle.rs` — refactor counter-pattern section

- [ ] **Step 1: Write failing tests** (add to `oracle.rs` tests)

```rust
#[test]
fn disdainful_stroke_parses_min_mana_value() {
    use crate::types::ability::{Ability, OracleSpan, SpellAbility, SpellFilter, TargetFilter};
    use crate::types::effect::EffectStep;
    let text = "Counter target spell with mana value 4 or greater.";
    let (spans, _) = parse_instant_or_sorcery(text, "Disdainful Stroke");
    let OracleSpan::Parsed(Ability::SpellEffect(sa)) = &spans[0] else { panic!() };
    assert_eq!(sa.steps, vec![EffectStep::CounterSpell]);
    let TargetFilter::Spell(f) = &sa.target_requirements[0] else { panic!() };
    assert_eq!(f.min_mana_value, Some(4));
    assert_eq!(f.max_mana_value, None);
    assert!(f.any_of_colors.is_empty());
}

#[test]
fn max_mana_value_spell_parses_correctly() {
    use crate::types::ability::{Ability, OracleSpan, TargetFilter};
    use crate::types::effect::EffectStep;
    let text = "Counter target spell with mana value 3 or less.";
    let (spans, _) = parse_instant_or_sorcery(text, "Test");
    let OracleSpan::Parsed(Ability::SpellEffect(sa)) = &spans[0] else { panic!() };
    assert_eq!(sa.steps, vec![EffectStep::CounterSpell]);
    let TargetFilter::Spell(f) = &sa.target_requirements[0] else { panic!() };
    assert_eq!(f.max_mana_value, Some(3));
    assert_eq!(f.min_mana_value, None);
}

#[test]
fn flashfreeze_parses_color_filter() {
    use crate::types::ability::{Ability, OracleSpan, TargetFilter};
    use crate::types::effect::EffectStep;
    use crate::types::mana::ManaColor;
    let text = "Counter target red or green spell.";
    let (spans, _) = parse_instant_or_sorcery(text, "Flashfreeze");
    let OracleSpan::Parsed(Ability::SpellEffect(sa)) = &spans[0] else { panic!() };
    assert_eq!(sa.steps, vec![EffectStep::CounterSpell]);
    let TargetFilter::Spell(f) = &sa.target_requirements[0] else { panic!() };
    assert!(f.any_of_colors.contains(&ManaColor::Red));
    assert!(f.any_of_colors.contains(&ManaColor::Green));
}

#[test]
fn single_color_filter_parses_correctly() {
    use crate::types::ability::{Ability, OracleSpan, TargetFilter};
    use crate::types::mana::ManaColor;
    let text = "Counter target blue spell.";
    let (spans, _) = parse_instant_or_sorcery(text, "Test");
    let OracleSpan::Parsed(Ability::SpellEffect(sa)) = &spans[0] else { panic!() };
    let TargetFilter::Spell(f) = &sa.target_requirements[0] else { panic!() };
    assert_eq!(f.any_of_colors, vec![ManaColor::Blue]);
}
```

Run: `cargo test disdainful_stroke_parses 2>&1 | grep -E "^test result|FAILED|error\["` → Expected: fail (Unparsed or wrong filter).

- [ ] **Step 2: Refactor the counter section in `parse_spell_paragraph`**

Replace the existing counter section (lines 1168-1191) with a call to a new helper:

```rust
// Counter patterns — CR 701.5
if let Some(spell_ability) = try_parse_counter(lc.as_str()) {
    return spell_ability;
}
```

Add the helper function (outside `parse_spell_paragraph`, before it in the file):

```rust
/// Try to parse a "counter target [type] spell [restrictions]" paragraph.
/// Returns None if the paragraph isn't a counter pattern.
fn try_parse_counter(lc: &str) -> Option<crate::types::ability::SpellAbility> {
    use crate::types::ability::{SpellAbility, SpellFilter, TargetFilter};
    use crate::types::effect::EffectStep;
    use crate::types::mana::ManaColor;

    // Must start with "counter target "
    let rest = lc.strip_prefix("counter target ")?;

    // 1. Try color prefix: "[color] spell" or "[color] or [color] spell"
    //    Color names appear before the type word.
    let color_names: &[(&str, ManaColor)] = &[
        ("white", ManaColor::White),
        ("blue", ManaColor::Blue),
        ("black", ManaColor::Black),
        ("red", ManaColor::Red),
        ("green", ManaColor::Green),
    ];

    let mut colors: Vec<ManaColor> = Vec::new();
    let rest = {
        // Try "color or color spell" first, then "color spell"
        let mut matched_rest = rest;
        'outer: for (name1, c1) in color_names {
            let color_or_prefix = format!("{name1} or ");
            if let Some(after_c1) = rest.strip_prefix(color_or_prefix.as_str()) {
                for (name2, c2) in color_names {
                    let type_prefix = format!("{name2} spell");
                    if after_c1.starts_with(type_prefix.as_str()) {
                        colors = vec![*c1, *c2];
                        matched_rest = &after_c1[name2.len() + 1..]; // skip "[name2] "
                        break 'outer;
                    }
                }
            }
            let single_prefix = format!("{name1} ");
            if let Some(after_c1) = rest.strip_prefix(single_prefix.as_str()) {
                if after_c1.starts_with("spell") {
                    colors = vec![*c1];
                    matched_rest = after_c1;
                    break 'outer;
                }
            }
        }
        matched_rest
    };

    // 2. Parse type word: "instant or sorcery spell", "noncreature spell",
    //    "creature spell", "spell"
    let (base_filter, rest) = if let Some(r) = rest.strip_prefix("instant or sorcery spell") {
        (SpellFilter::instant_or_sorcery(), r)
    } else if let Some(r) = rest.strip_prefix("noncreature spell") {
        (SpellFilter::noncreature(), r)
    } else if let Some(r) = rest.strip_prefix("creature spell") {
        (SpellFilter::creature(), r)
    } else if let Some(r) = rest.strip_prefix("spell") {
        (SpellFilter::any(), r)
    } else {
        return None; // unrecognised type
    };

    let rest = rest.trim();

    // 3. Parse "with mana value N or greater/less" suffix
    let (rest, min_mv, max_mv) = parse_mana_value_suffix(rest);

    // 4. Nothing else should remain (period already stripped by caller)
    if !rest.is_empty() {
        return None;
    }

    let filter = SpellFilter {
        any_of_colors: colors,
        min_mana_value: min_mv,
        max_mana_value: max_mv,
        ..base_filter
    };

    Some(SpellAbility {
        target_requirements: vec![TargetFilter::Spell(filter)],
        steps: vec![EffectStep::CounterSpell],
    })
}

/// Strip "with mana value N or greater" / "with mana value N or less".
/// Returns (remaining, min_mana_value, max_mana_value).
fn parse_mana_value_suffix(s: &str) -> (&str, Option<u32>, Option<u32>) {
    if let Some(rest) = s.strip_prefix("with mana value ") {
        if let Some(rest) = rest.strip_suffix(" or greater") {
            if let Ok(n) = rest.parse::<u32>() {
                return ("", Some(n), None);
            }
        }
        if let Some(rest) = rest.strip_suffix(" or less") {
            if let Ok(n) = rest.parse::<u32>() {
                return ("", None, Some(n));
            }
        }
    }
    (s, None, None)
}
```

- [ ] **Step 3: Run tests**

```bash
cargo test 2>&1 | grep -E "^test result|FAILED|error\["
```
Expected: all pass including the new category 1 tests.

- [ ] **Step 4: Commit**

```bash
git add src/parser/oracle.rs
git commit -m "feat: parser recognises mana-value and color counter-spell restrictions (CR 202.3)"
```

---

### Task 7: Parser — Category 2 ("unless its controller pays" suffix)

**Files:**
- Modify: `src/parser/oracle.rs` — extend `try_parse_counter` to handle the "unless pays" suffix

- [ ] **Step 1: Write failing tests**

```rust
#[test]
fn mana_leak_parses_unless_mana() {
    use crate::types::ability::{Ability, Cost, CostComponent, OracleSpan, TargetFilter};
    use crate::types::effect::EffectStep;
    use crate::types::mana::{ManaCost, ManaPip};
    let text = "Counter target spell unless its controller pays {3}.";
    let (spans, _) = parse_instant_or_sorcery(text, "Mana Leak");
    let OracleSpan::Parsed(Ability::SpellEffect(sa)) = &spans[0] else { panic!() };
    let TargetFilter::Spell(f) = &sa.target_requirements[0] else { panic!() };
    assert_eq!(f.min_mana_value, None);
    assert_eq!(f.any_of_colors, vec![]);
    assert_eq!(sa.steps.len(), 1);
    let EffectStep::Payment { cost, on_paid, on_declined } = &sa.steps[0] else { panic!() };
    assert_eq!(cost, &vec![CostComponent::Mana(ManaCost { pips: vec![ManaPip::Generic(3)] })]);
    assert!(on_paid.is_empty());
    assert_eq!(on_declined, &vec![EffectStep::CounterSpell]);
}

#[test]
fn quench_parses_unless_two_mana() {
    use crate::types::ability::{Ability, OracleSpan, TargetFilter};
    use crate::types::effect::EffectStep;
    use crate::types::mana::{ManaCost, ManaPip};
    let text = "Counter target spell unless its controller pays {2}.";
    let (spans, _) = parse_instant_or_sorcery(text, "Quench");
    let OracleSpan::Parsed(Ability::SpellEffect(sa)) = &spans[0] else { panic!() };
    let EffectStep::Payment { cost, .. } = &sa.steps[0] else { panic!() };
    assert_eq!(cost, &vec![crate::types::ability::CostComponent::Mana(ManaCost { pips: vec![ManaPip::Generic(2)] })]);
}

#[test]
fn life_payment_counter_parses_unless_life() {
    use crate::types::ability::{Ability, CostComponent, OracleSpan};
    use crate::types::effect::EffectStep;
    let text = "Counter target spell unless its controller pays 3 life.";
    let (spans, _) = parse_instant_or_sorcery(text, "Test");
    let OracleSpan::Parsed(Ability::SpellEffect(sa)) = &spans[0] else { panic!() };
    let EffectStep::Payment { cost, .. } = &sa.steps[0] else { panic!() };
    assert_eq!(cost, &vec![CostComponent::PayLife(3)]);
}
```

Run: `cargo test mana_leak_parses 2>&1 | grep -E "^test result|FAILED|error\["` → Expected: fail (CounterSpell instead of Payment).

- [ ] **Step 2: Add "unless" suffix parsing to `try_parse_counter`**

In `try_parse_counter`, **before** step 3 ("Parse mana value suffix"), add a step to strip the "unless" suffix from `rest`. Insert between step 2 and step 3:

```rust
// 2b. Strip "unless its controller pays {N}" or "unless its controller pays N life"
let (rest, payment_cost) = parse_unless_suffix(rest.trim());
let rest = rest.trim();
```

Then in the step that builds `SpellAbility`, change the `steps` field based on whether `payment_cost` is present:

```rust
let steps = if let Some(cost) = payment_cost {
    vec![EffectStep::Payment {
        cost,
        on_paid: vec![],
        on_declined: vec![EffectStep::CounterSpell],
    }]
} else {
    vec![EffectStep::CounterSpell]
};

Some(SpellAbility {
    target_requirements: vec![TargetFilter::Spell(filter)],
    steps,
})
```

Add the helper function:

```rust
/// Strip "unless its controller pays {N}" or "unless its controller pays N life".
/// Returns (remaining, Some(cost_components)) or (original, None).
fn parse_unless_suffix(s: &str) -> (&str, Option<crate::types::ability::Cost>) {
    use crate::types::ability::CostComponent;
    use crate::types::mana::{ManaCost, ManaPip};

    const PREFIX: &str = "unless its controller pays ";
    let Some(tail) = s.strip_prefix(PREFIX) else {
        return (s, None);
    };
    // Try "{N}" mana cost
    if let Some(inner) = tail.strip_prefix('{') {
        if let Some(n_str) = inner.strip_suffix('}') {
            if let Ok(n) = n_str.parse::<u32>() {
                return (
                    "",
                    Some(vec![CostComponent::Mana(ManaCost {
                        pips: vec![ManaPip::Generic(n)],
                    })]),
                );
            }
        }
    }
    // Try "N life"
    if let Some(n_str) = tail.strip_suffix(" life") {
        if let Ok(n) = n_str.parse::<u32>() {
            return ("", Some(vec![CostComponent::PayLife(n)]));
        }
    }
    (s, None) // unrecognised unless suffix — leave as-is
}
```

- [ ] **Step 3: Run tests**

```bash
cargo test 2>&1 | grep -E "^test result|FAILED|error\["
```
Expected: all pass.

- [ ] **Step 4: Clippy**

```bash
cargo clippy --all-targets 2>&1 | grep -E "^error|^warning"
```
Fix any warnings.

- [ ] **Step 5: Commit**

```bash
git add src/parser/oracle.rs
git commit -m "feat: parser recognises 'unless its controller pays' counter-spell conditions (CR 118.12)"
```

---

### Task 8: Clean up `docs/todo.md`

**Files:**
- Modify: `docs/todo.md` — remove the `🔁 Conditional counter spells` section

- [ ] **Step 1: Remove the section**

Delete the entire `## 🔁 Conditional counter spells` section from `docs/todo.md` (everything from the `## 🔁 Conditional counter spells` heading through the final bullet, approximately lines 120-129 in the current file).

- [ ] **Step 2: Verify no regressions**

```bash
cargo test 2>&1 | grep -E "^test result|FAILED|error\["
```
Expected: all pass.

- [ ] **Step 3: Commit**

```bash
git add docs/todo.md
git commit -m "docs: remove conditional counterspells from todo (implemented)"
```

---

## Self-Review

**Spec coverage:**

| Spec section | Task |
|---|---|
| `Cost` type alias rename | Task 1 |
| `SpellFilter` mana-value + color fields | Task 2 |
| `SpellFilter::matches` new signature | Task 2 |
| `EffectStep::Payment` | Task 3 |
| `PendingPayment` struct + `GameState` field | Task 3 |
| `execute_effect_steps` Payment arm + priority routing | Task 4 |
| `pay_pending_cost` / `decline_pending_cost` | Task 4 |
| Ward migration to `TriggeredAbility` + `Payment` | Task 5 |
| `WardTrigger` removal | Task 5 |
| `serve.rs` `pending_payment` view + actions | Task 5 |
| Parser category 1 (mana value + color) | Task 6 |
| Parser category 2 (unless suffix) | Task 7 |
| `todo.md` cleanup | Task 8 |

**No placeholders found.**

**Type consistency check:**
- `Cost = Vec<CostComponent>` — used in `EffectStep::Payment`, `PendingPayment`, `pay_pending_cost`, and `collect_ward_triggers` ✓
- `PendingPayment::on_paid`/`on_declined` match `EffectStep::Payment::on_paid`/`on_declined` ✓
- `execute_effect_steps` signature (now `pub(crate)`) matches calls in `costs.rs` ✓
- `SpellFilter::matches(card_types, mana_value, colors)` — all 3 args supplied at both call sites (targeting.rs and existing tests) ✓
