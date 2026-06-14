# Unified Cost Payment Framework Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Replace the ad-hoc `PayWard` action and duplicated mana-payment logic with a unified cost payment framework — a single `pay_cost_components` function, two new server actions (`PayCost`/`DeclineCost`), and a client-side payment context UI.

**Architecture:** Backend consolidation: `engine/costs.rs` becomes the single source of truth for cost payment; `ward.rs` is deleted and its logic moves to `triggered.rs` and `costs.rs`. Frontend: cards enter a payment context on click instead of posting actions directly, and a payment panel handles Confirm/Cancel/Decline.

**Tech Stack:** Rust (Axum, Serde), Vanilla JS (no framework)

---

## File Map

| File | Change |
|---|---|
| `src/types/ability.rs` | Remove `WardCost` enum; collapse `WardMana`/`WardLife` → `Ward(Vec<CostComponent>)`; update `display_name` |
| `src/types/stack.rs` | `WardTrigger.cost: WardCost` → `Vec<CostComponent>`; rename `paid` → `settled` |
| `src/types/mod.rs` | Remove `WardCost` from re-export |
| `src/parser/oracle.rs` | Parse Ward into `StaticAbility::Ward(vec![...])` |
| `src/engine/costs.rs` | **NEW** — `pay_cost_components`, `can_pay_cost_components`, `pay_stack_cost`, `resolve_stack_cost_decline` |
| `src/engine/triggered.rs` | Receive `collect_ward_triggers` from ward.rs |
| `src/engine/ward.rs` | **DELETED** |
| `src/engine/stack.rs` | Make `counter_spell_on_stack` `pub(crate)`; update `resolve_top` for renamed `settled` field |
| `src/engine/mod.rs` | Remove `pub mod ward`; add `pub mod costs` |
| `src/engine/casting.rs` | Use `pay_cost_components` for mana payment |
| `src/engine/activated.rs` | Use `can_pay_cost_components` (structural only); use `pay_cost_components`; remove `payment_plan` param |
| `src/engine/cycling.rs` | Use `pay_cost_components`; remove `payment_plan` param |
| `src/serve.rs` | Remove `PayWard`; add `PayCost`/`DeclineCost`; remove `can_pay_cost` from `ActionItemView`; remove `payment_plan` from `ActivateAbility`; add `cost_label` to `StackItemView` |
| `src/serve.js` | Payment context state; card/ability clicks enter context; ward auto-detection; payment panel |

---

## Task 1: Unify Ward Cost Type

This is a pure type migration. It touches several files simultaneously because all references to `WardCost`, `WardMana`, and `WardLife` must change together to compile.

**Files:**
- Modify: `src/types/ability.rs`
- Modify: `src/types/stack.rs`
- Modify: `src/types/mod.rs`
- Modify: `src/parser/oracle.rs` (lines 466-470, 979-993, 2255-2278)
- Modify: `src/engine/ward.rs` (temporary, until Task 3 deletes it)

- [ ] **Step 1: In `src/types/ability.rs`, remove `WardCost` and collapse Ward ability variants**

  Replace:
  ```rust
  // CR 702.21
  #[derive(Debug, Clone, PartialEq, Eq)]
  pub enum WardCost {
      Mana(ManaCost),
      Life(u32),
  }
  ```
  with nothing (delete the whole block).

  In `StaticAbility`, replace:
  ```rust
  WardMana(ManaCost),             // CR 702.21 — Ward {cost}
  WardLife(u32),                  // CR 702.21 — Ward—Pay N life
  ```
  with:
  ```rust
  Ward(Vec<CostComponent>),       // CR 702.21
  ```

  In `StaticAbility::display_name`, replace:
  ```rust
  Self::WardMana(cost) => format!("Ward {cost}"),
  Self::WardLife(n) => format!("Ward\u{2014}Pay {n} life"),
  ```
  with:
  ```rust
  Self::Ward(components) => {
      if let [CostComponent::Mana(c)] = components.as_slice() {
          format!("Ward {c}")
      } else if let [CostComponent::PayLife(n)] = components.as_slice() {
          format!("Ward\u{2014}Pay {n} life")
      } else {
          "Ward".to_string()
      }
  }
  ```

  Update the two display_name tests at the bottom of `ability.rs` (around lines 336-344):
  ```rust
  #[test]
  fn ward_mana_display_name() {
      assert_eq!(
          StaticAbility::Ward(vec![CostComponent::Mana(ManaCost {
              pips: vec![ManaPip::Generic(2)]
          })])
          .display_name(),
          "Ward {2}"
      );
  }

  #[test]
  fn ward_life_display_name() {
      assert_eq!(
          StaticAbility::Ward(vec![CostComponent::PayLife(2)]).display_name(),
          "Ward\u{2014}Pay 2 life"
      );
  }
  ```

- [ ] **Step 2: In `src/types/stack.rs`, update `WardTrigger`**

  Remove the import at line 1:
  ```rust
  use super::ability::WardCost;
  ```

  In `StackPayload`, replace:
  ```rust
  /// Counters the triggering spell/ability if the Ward cost is not paid.
  WardTrigger {
      counters_if_unpaid: StackId,
      cost: WardCost,
      paid: bool,
  ```
  with:
  ```rust
  /// CR 702.21a — Counters the triggering spell/ability if the Ward cost is not settled.
  WardTrigger {
      counters_if_unpaid: StackId,
      cost: Vec<super::ability::CostComponent>,
      settled: bool,
  ```

- [ ] **Step 3: In `src/types/mod.rs`, remove `WardCost` from the re-export**

  Replace:
  ```rust
  pub use ability::{
      ...
      TriggerEvent, TriggeredAbility, WardCost,
  ```
  with:
  ```rust
  pub use ability::{
      ...
      TriggerEvent, TriggeredAbility,
  ```
  (just remove `WardCost` from the list — keep all other names)

- [ ] **Step 4: In `src/parser/oracle.rs`, update the Ward mana parse (line ~466)**

  Replace:
  ```rust
  // Ward {cost} (CR 702.21a) — mana cost form e.g. "Ward {2}"
  if let Some(_rest) = s.strip_prefix("ward ")
      && let Some(cost) = try_parse_mana_cost(kw["ward ".len()..].trim())
  {
      return OracleSpan::Parsed(Ability::Static(StaticAbility::WardMana(cost)));
  }
  ```
  with:
  ```rust
  // Ward {cost} (CR 702.21a) — mana cost form e.g. "Ward {2}"
  if let Some(_rest) = s.strip_prefix("ward ")
      && let Some(cost) = try_parse_mana_cost(kw["ward ".len()..].trim())
  {
      use crate::types::ability::CostComponent;
      return OracleSpan::Parsed(Ability::Static(StaticAbility::Ward(vec![
          CostComponent::Mana(cost),
      ])));
  }
  ```

- [ ] **Step 5: In `src/parser/oracle.rs`, update the Ward em-dash life parse (line ~989)**

  Replace:
  ```rust
  spans.push(OracleSpan::Parsed(Ability::Static(
      StaticAbility::WardLife(n),
  )));
  ```
  with:
  ```rust
  use crate::types::ability::CostComponent;
  spans.push(OracleSpan::Parsed(Ability::Static(
      StaticAbility::Ward(vec![CostComponent::PayLife(n)]),
  )));
  ```

- [ ] **Step 6: Update the oracle.rs Ward parser tests (lines ~2255-2278)**

  Replace both test bodies:
  ```rust
  #[test]
  fn ward_mana_parses_as_ward_mana() {
      use crate::types::ability::CostComponent;
      use crate::types::mana::{ManaCost, ManaPip};
      let (spans, _) = parse_permanent("Ward {2}", "Test");
      assert_eq!(
          spans,
          vec![OracleSpan::Parsed(Ability::Static(StaticAbility::Ward(vec![
              CostComponent::Mana(ManaCost {
                  pips: vec![ManaPip::Generic(2)]
              })
          ])))]
      );
  }

  #[test]
  fn ward_life_parses_from_em_dash_paragraph() {
      use crate::types::ability::CostComponent;
      let (spans, _) = parse_permanent("Ward\u{2014}Pay 2 life.", "Test");
      assert_eq!(
          spans,
          vec![OracleSpan::Parsed(Ability::Static(StaticAbility::Ward(
              vec![CostComponent::PayLife(2)]
          )))]
      );
  }
  ```

- [ ] **Step 7: Update `src/engine/ward.rs` to use the new types**

  Replace the `use` line that imports `WardCost`:
  ```rust
  use crate::types::{GameState, PlayerId, StackObject, WardCost};
  ```
  with:
  ```rust
  use crate::types::{GameState, PlayerId, StackObject};
  ```

  In `collect_ward_triggers`, replace the inner import and collection:
  ```rust
  use crate::types::ability::{Ability, OracleSpan, StaticAbility, WardCost};
  ...
  let ward_costs: Vec<WardCost> = target_obj
      .definition
      .abilities
      .iter()
      .filter_map(|span| match span {
          OracleSpan::Parsed(Ability::Static(StaticAbility::WardMana(cost))) => {
              Some(WardCost::Mana(cost.clone()))
          }
          OracleSpan::Parsed(Ability::Static(StaticAbility::WardLife(n))) => {
              Some(WardCost::Life(*n))
          }
          _ => None,
      })
      .collect();
  for cost in ward_costs {
  ```
  with:
  ```rust
  use crate::types::ability::{Ability, CostComponent, OracleSpan, StaticAbility};
  ...
  let ward_cost_sets: Vec<Vec<CostComponent>> = target_obj
      .definition
      .abilities
      .iter()
      .filter_map(|span| match span {
          OracleSpan::Parsed(Ability::Static(StaticAbility::Ward(components))) => {
              Some(components.clone())
          }
          _ => None,
      })
      .collect();
  for cost in ward_cost_sets {
  ```

  In the `StackPayload::WardTrigger` construction inside `collect_ward_triggers`, rename `paid: false` to `settled: false`.

  In `pay_ward`, update the `already_paid` extraction:
  ```rust
  let (cost, already_paid) = {
      let obj = state.stack_objects.get(&trigger_id).ok_or(EngineError::CardNotFound)?;
      match &obj.payload {
          StackPayload::WardTrigger { cost, settled, .. } => (cost.clone(), *settled),
          _ => return Err(EngineError::NotYourPriority),
      }
  };
  if already_paid {
  ```

  Replace the match over `WardCost` variants:
  ```rust
  match &cost {
      WardCost::Mana(mana_cost) => { ... }
      WardCost::Life(n) => { ... }
  }
  ```
  with a loop over `CostComponent`:
  ```rust
  for component in &cost {
      match component {
          crate::types::ability::CostComponent::Mana(mana_cost) => {
              let plan = {
                  let player = state
                      .get_player(player_id)
                      .ok_or(EngineError::CardNotFound)?;
                  super::mana::greedy_payment_plan(mana_cost, &player.mana_pool, player.life)
                      .ok_or(EngineError::InsufficientMana)?
              };
              state = super::mana::pay_mana_cost(state, player_id, mana_cost, &plan)?;
          }
          crate::types::ability::CostComponent::PayLife(n) => {
              let n = *n;
              let player = state
                  .get_player_mut(player_id)
                  .ok_or(EngineError::CardNotFound)?;
              if player.life < n as i32 {
                  return Err(EngineError::InsufficientLife);
              }
              player.life -= n as i32;
          }
          _ => {}
      }
  }
  ```

  In the `paid` mark-as-paid step:
  ```rust
  if let StackPayload::WardTrigger { paid, .. } = &mut obj.payload {
      *paid = true;
  }
  ```
  change to:
  ```rust
  if let StackPayload::WardTrigger { settled, .. } = &mut obj.payload {
      *settled = true;
  }
  ```

  Update the ward.rs tests: replace all uses of `WardCost::Mana(...)` with
  `vec![CostComponent::Mana(...)]`, `WardCost::Life(n)` with `vec![CostComponent::PayLife(n)]`,
  and `paid:` fields with `settled:` in `WardTrigger` construction. Replace all:
  ```rust
  StackPayload::WardTrigger { paid, .. } => assert!(*paid, ...)
  ```
  with:
  ```rust
  StackPayload::WardTrigger { settled, .. } => assert!(*settled, ...)
  ```

- [ ] **Step 8: Update `src/engine/stack.rs` — rename `paid` → `settled` in `resolve_top`**

  In the `resolve_top` function, find the `WardTrigger` match arm:
  ```rust
  StackPayload::WardTrigger { counters_if_unpaid, paid, .. } => {
      if !paid {
  ```
  and change to:
  ```rust
  StackPayload::WardTrigger { counters_if_unpaid, settled, .. } => {
      if !settled {
  ```

  Also find any tests in `stack.rs` that construct `WardTrigger` with `paid:` and rename to `settled:`, and any that use `WardCost` type (replace with `Vec<CostComponent>`).

- [ ] **Step 9: Verify compilation and run tests**

  ```bash
  cargo test 2>&1 | grep -E "^test result|FAILED|error\["
  ```

  Expected: all tests pass (no new failures). Clippy:
  ```bash
  cargo clippy --all-targets 2>&1 | grep -E "^error|^warning"
  ```

- [ ] **Step 10: Commit**

  ```bash
  git add src/types/ability.rs src/types/stack.rs src/types/mod.rs \
          src/parser/oracle.rs src/engine/ward.rs src/engine/stack.rs
  git commit -m "refactor: unify Ward cost type — WardCost → Vec<CostComponent>, Ward(Vec<CostComponent>), paid → settled"
  ```

---

## Task 2: Create `engine/costs.rs`

New file: unified cost payment engine. TDD: write tests first, implement after.

**Files:**
- Create: `src/engine/costs.rs`
- Modify: `src/engine/mod.rs`
- Modify: `src/engine/stack.rs` (make `counter_spell_on_stack` accessible)

- [ ] **Step 1: In `src/engine/stack.rs`, make `counter_spell_on_stack` pub(crate)**

  Find:
  ```rust
  fn counter_spell_on_stack(state: &mut GameState, stack_id: crate::types::stack::StackId) {
  ```
  Change to:
  ```rust
  pub(crate) fn counter_spell_on_stack(state: &mut GameState, stack_id: crate::types::stack::StackId) {
  ```

- [ ] **Step 2: Add `pub mod costs` to `src/engine/mod.rs`**

  Append at the end:
  ```rust
  pub mod costs;
  ```

- [ ] **Step 3: Write the tests first in `src/engine/costs.rs`**

  Create the file with tests and stub implementations:

  ```rust
  use super::EngineError;
  use crate::engine::mana::{greedy_payment_plan, pay_mana_cost};
  use crate::types::ability::CostComponent;
  use crate::types::stack::{StackId, StackPayload};
  use crate::types::{GameState, ObjectId, PlayerId};

  // CR 116.1, 601.2h — unified cost payment for all cost-bearing game actions.
  pub fn pay_cost_components(
      state: GameState,
      player_id: PlayerId,
      components: &[CostComponent],
  ) -> Result<GameState, EngineError> {
      todo!()
  }

  // Structural affordability check (Tap only). Mana/life deferred to payment context.
  pub fn can_pay_cost_components(
      state: &GameState,
      player_id: PlayerId,
      object_id: Option<ObjectId>,
      components: &[CostComponent],
  ) -> bool {
      todo!()
  }

  // CR 702.21a: pay the ward cost; immediately resolve trigger (spell survives).
  pub fn pay_stack_cost(
      state: GameState,
      player_id: PlayerId,
      stack_id: StackId,
  ) -> Result<GameState, EngineError> {
      todo!()
  }

  // CR 702.21a: decline optional stack cost; immediately counter the targeted spell.
  pub fn resolve_stack_cost_decline(
      state: GameState,
      stack_id: StackId,
  ) -> Result<GameState, EngineError> {
      todo!()
  }

  #[cfg(test)]
  mod tests {
      use super::*;
      use crate::types::ability::CostComponent;
      use crate::types::mana::{ManaCost, ManaPip};
      use crate::types::stack::{StackObject, StackPayload};
      use crate::types::{GameState, Player, PlayerId, StackId};

      fn two_player_state() -> GameState {
          GameState::new(vec![
              Player::new(PlayerId(0), "Alice"),
              Player::new(PlayerId(1), "Bob"),
          ])
      }

      fn push_ward_trigger(
          state: &mut GameState,
          cost: Vec<CostComponent>,
          counters: StackId,
      ) -> StackId {
          let sid = state.alloc_stack_id();
          state.stack_objects.insert(
              sid,
              StackObject {
                  id: sid,
                  payload: StackPayload::WardTrigger {
                      counters_if_unpaid: counters,
                      cost,
                      settled: false,
                  },
                  controller: PlayerId(1),
                  targets: vec![],
              },
          );
          state.stack.push(sid);
          sid
      }

      fn push_spell(state: &mut GameState) -> StackId {
          use crate::types::card::{CardDefinition, CardType, TypeLine};
          use crate::types::mana::ManaCost;
          use crate::types::{CardObject, OracleSpan, Zone};
          let spell_id = state.alloc_id();
          let def = CardDefinition {
              name: "Lightning Bolt".into(),
              mana_cost: Some(ManaCost { pips: vec![] }),
              type_line: TypeLine {
                  supertypes: vec![],
                  card_types: vec![CardType::Instant],
                  subtypes: vec![],
              },
              oracle_text: String::new(),
              abilities: vec![],
              text_annotations: vec![],
              power: None,
              toughness: None,
              colors: vec![],
          };
          let obj = CardObject::new(spell_id, def, PlayerId(0), Zone::Stack);
          state.add_object(obj);
          let sid = state.alloc_stack_id();
          state.stack_objects.insert(
              sid,
              StackObject {
                  id: sid,
                  payload: StackPayload::Spell { card_id: spell_id },
                  controller: PlayerId(0),
                  targets: vec![],
              },
          );
          state.stack.push(sid);
          sid
      }

      // ── pay_cost_components ──────────────────────────────────────────────

      #[test]
      fn pay_mana_component_deducts_from_pool() {
          let mut gs = two_player_state();
          gs.get_player_mut(PlayerId(0)).unwrap().mana_pool.colorless = 2;
          let components = vec![CostComponent::Mana(ManaCost {
              pips: vec![ManaPip::Generic(2)],
          })];
          let gs = pay_cost_components(gs, PlayerId(0), &components).unwrap();
          assert_eq!(gs.get_player(PlayerId(0)).unwrap().mana_pool.colorless, 0);
      }

      #[test]
      fn pay_life_component_deducts_life() {
          let mut gs = two_player_state();
          let components = vec![CostComponent::PayLife(3)];
          let gs = pay_cost_components(gs, PlayerId(0), &components).unwrap();
          assert_eq!(gs.get_player(PlayerId(0)).unwrap().life, 17);
      }

      #[test]
      fn pay_mana_insufficient_returns_error() {
          let gs = two_player_state();
          let components = vec![CostComponent::Mana(ManaCost {
              pips: vec![ManaPip::Generic(2)],
          })];
          let result = pay_cost_components(gs, PlayerId(0), &components);
          assert!(matches!(result, Err(EngineError::InsufficientMana)));
      }

      #[test]
      fn pay_life_insufficient_returns_error() {
          let mut gs = two_player_state();
          gs.get_player_mut(PlayerId(0)).unwrap().life = 1;
          let components = vec![CostComponent::PayLife(3)];
          let result = pay_cost_components(gs, PlayerId(0), &components);
          assert!(matches!(result, Err(EngineError::InsufficientLife)));
      }

      #[test]
      fn tap_component_is_skipped_by_pay_cost_components() {
          // Tap is caller's responsibility; pay_cost_components ignores it
          let gs = two_player_state();
          let components = vec![CostComponent::Tap];
          let result = pay_cost_components(gs, PlayerId(0), &components);
          assert!(result.is_ok());
      }

      // ── can_pay_cost_components ──────────────────────────────────────────

      #[test]
      fn can_pay_tap_returns_false_when_already_tapped() {
          use crate::types::card::{CardDefinition, CardType, TypeLine};
          use crate::types::{CardObject, PermanentState, Zone};
          let mut gs = two_player_state();
          let id = gs.alloc_id();
          let def = CardDefinition {
              name: "Forest".into(),
              mana_cost: None,
              type_line: TypeLine {
                  supertypes: vec![],
                  card_types: vec![CardType::Land],
                  subtypes: vec!["Forest".into()],
              },
              oracle_text: String::new(),
              abilities: vec![],
              text_annotations: vec![],
              power: None,
              toughness: None,
              colors: vec![],
          };
          let obj = CardObject::new(id, def, PlayerId(0), Zone::Battlefield);
          gs.battlefield.insert(id, PermanentState::new(&obj.definition));
          gs.battlefield.get_mut(&id).unwrap().tapped = true;
          gs.add_object(obj);
          let components = vec![CostComponent::Tap];
          assert!(!can_pay_cost_components(&gs, PlayerId(0), Some(id), &components));
      }

      #[test]
      fn can_pay_mana_always_returns_true_structurally() {
          // Mana affordability is deferred to payment context; structural check is always true
          let gs = two_player_state(); // no mana in pool
          let components = vec![CostComponent::Mana(ManaCost {
              pips: vec![ManaPip::Generic(5)],
          })];
          assert!(can_pay_cost_components(&gs, PlayerId(0), None, &components));
      }

      // ── pay_stack_cost ───────────────────────────────────────────────────

      #[test]
      fn pay_stack_cost_mana_removes_trigger_and_deducts_mana() {
          let mut gs = two_player_state();
          gs.get_player_mut(PlayerId(0)).unwrap().mana_pool.colorless = 2;
          let spell_sid = push_spell(&mut gs);
          let trigger_sid = push_ward_trigger(
              &mut gs,
              vec![CostComponent::Mana(ManaCost {
                  pips: vec![ManaPip::Generic(2)],
              })],
              spell_sid,
          );

          let gs = pay_stack_cost(gs, PlayerId(0), trigger_sid).unwrap();

          // Trigger removed from stack
          assert!(!gs.stack.contains(&trigger_sid));
          assert!(!gs.stack_objects.contains_key(&trigger_sid));
          // Spell still on stack (cost was paid, not countered)
          assert!(gs.stack.contains(&spell_sid));
          // Mana was paid
          assert_eq!(gs.get_player(PlayerId(0)).unwrap().mana_pool.colorless, 0);
      }

      #[test]
      fn pay_stack_cost_life_removes_trigger_and_deducts_life() {
          let mut gs = two_player_state();
          let spell_sid = push_spell(&mut gs);
          let trigger_sid = push_ward_trigger(
              &mut gs,
              vec![CostComponent::PayLife(2)],
              spell_sid,
          );

          let gs = pay_stack_cost(gs, PlayerId(0), trigger_sid).unwrap();

          assert!(!gs.stack.contains(&trigger_sid));
          assert!(gs.stack.contains(&spell_sid));
          assert_eq!(gs.get_player(PlayerId(0)).unwrap().life, 18);
      }

      #[test]
      fn pay_stack_cost_not_on_top_returns_error() {
          let mut gs = two_player_state();
          let spell_sid = push_spell(&mut gs);
          let trigger_sid = push_ward_trigger(
              &mut gs,
              vec![CostComponent::PayLife(1)],
              spell_sid,
          );
          // Push something else on top
          let extra = gs.alloc_stack_id();
          gs.stack.push(extra);

          let result = pay_stack_cost(gs, PlayerId(0), trigger_sid);
          assert!(matches!(result, Err(EngineError::NotYourPriority)));
      }

      // ── resolve_stack_cost_decline ───────────────────────────────────────

      #[test]
      fn decline_removes_trigger_and_counters_spell() {
          use crate::types::Zone;
          let mut gs = two_player_state();
          let spell_sid = push_spell(&mut gs);
          let trigger_sid = push_ward_trigger(
              &mut gs,
              vec![CostComponent::PayLife(2)],
              spell_sid,
          );

          let gs = resolve_stack_cost_decline(gs, trigger_sid).unwrap();

          // Both trigger and spell removed from stack
          assert!(!gs.stack.contains(&trigger_sid));
          assert!(!gs.stack.contains(&spell_sid));
          // Spell card moved to graveyard
          let gy = gs.graveyards.get(&PlayerId(0)).unwrap();
          assert!(!gy.is_empty(), "countered spell should be in graveyard");
      }

      #[test]
      fn decline_not_on_top_returns_error() {
          let mut gs = two_player_state();
          let spell_sid = push_spell(&mut gs);
          let trigger_sid = push_ward_trigger(
              &mut gs,
              vec![CostComponent::PayLife(1)],
              spell_sid,
          );
          let extra = gs.alloc_stack_id();
          gs.stack.push(extra);

          let result = resolve_stack_cost_decline(gs, trigger_sid);
          assert!(matches!(result, Err(EngineError::NotYourPriority)));
      }
  }
  ```

- [ ] **Step 4: Run tests to verify they fail**

  ```bash
  cargo test engine::costs 2>&1 | grep -E "^test result|FAILED|error\[|panicked"
  ```

  Expected: compile errors from `todo!()` (the test module compiles; functions panic at runtime). Or just `error[E0308]` / panics — both acceptable at this point.

- [ ] **Step 5: Implement `pay_cost_components`**

  Replace the `todo!()` body:
  ```rust
  pub fn pay_cost_components(
      mut state: GameState,
      player_id: PlayerId,
      components: &[CostComponent],
  ) -> Result<GameState, EngineError> {
      for component in components {
          match component {
              CostComponent::Mana(cost) => {
                  let plan = {
                      let player = state
                          .get_player(player_id)
                          .ok_or(EngineError::CardNotFound)?;
                      greedy_payment_plan(cost, &player.mana_pool, player.life)
                          .ok_or(EngineError::InsufficientMana)?
                  };
                  state = pay_mana_cost(state, player_id, cost, &plan)?;
              }
              CostComponent::PayLife(n) => {
                  let n = *n;
                  let player = state
                      .get_player_mut(player_id)
                      .ok_or(EngineError::CardNotFound)?;
                  if player.life < n as i32 {
                      return Err(EngineError::InsufficientLife);
                  }
                  player.life -= n as i32;
              }
              // Tap is handled by the caller before invoking this function.
              CostComponent::Tap
              | CostComponent::Sacrifice(_, _)
              | CostComponent::Discard(_, _)
              | CostComponent::Unimplemented(_) => {}
          }
      }
      Ok(state)
  }
  ```

- [ ] **Step 6: Implement `can_pay_cost_components`**

  ```rust
  pub fn can_pay_cost_components(
      state: &GameState,
      player_id: PlayerId,
      object_id: Option<ObjectId>,
      components: &[CostComponent],
  ) -> bool {
      use crate::types::ability::StaticAbility;
      for component in components {
          if let CostComponent::Tap = component {
              let Some(id) = object_id else { return false };
              let Some(perm) = state.battlefield.get(&id) else { return false };
              if perm.tapped {
                  return false;
              }
              let cmt = state.controllers_most_recent_turn(player_id);
              if perm.summoning_sick(cmt) && !perm.has_keyword(StaticAbility::Haste) {
                  return false;
              }
          }
      }
      true
  }
  ```

- [ ] **Step 7: Implement `pay_stack_cost`**

  ```rust
  pub fn pay_stack_cost(
      mut state: GameState,
      player_id: PlayerId,
      stack_id: StackId,
  ) -> Result<GameState, EngineError> {
      if state.stack.last() != Some(&stack_id) {
          return Err(EngineError::NotYourPriority);
      }
      let cost: Vec<CostComponent> = {
          let obj = state
              .stack_objects
              .get(&stack_id)
              .ok_or(EngineError::CardNotFound)?;
          match &obj.payload {
              StackPayload::WardTrigger { cost, .. } => cost.clone(),
              _ => return Err(EngineError::NotYourPriority),
          }
      };
      state = pay_cost_components(state, player_id, &cost)?;
      state.stack_objects.remove(&stack_id);
      state.stack.retain(|&id| id != stack_id);
      state.consecutive_passes = 0;
      state.priority_player = state.active_player;
      Ok(state)
  }
  ```

- [ ] **Step 8: Implement `resolve_stack_cost_decline`**

  ```rust
  pub fn resolve_stack_cost_decline(
      mut state: GameState,
      stack_id: StackId,
  ) -> Result<GameState, EngineError> {
      if state.stack.last() != Some(&stack_id) {
          return Err(EngineError::NotYourPriority);
      }
      let counters_if_unpaid = {
          let obj = state
              .stack_objects
              .get(&stack_id)
              .ok_or(EngineError::CardNotFound)?;
          match &obj.payload {
              StackPayload::WardTrigger { counters_if_unpaid, .. } => *counters_if_unpaid,
              _ => return Err(EngineError::NotYourPriority),
          }
      };
      state.stack_objects.remove(&stack_id);
      state.stack.retain(|&id| id != stack_id);
      super::stack::counter_spell_on_stack(&mut state, counters_if_unpaid);
      state.consecutive_passes = 0;
      state.priority_player = state.active_player;
      Ok(state)
  }
  ```

- [ ] **Step 9: Run costs tests and all tests**

  ```bash
  cargo test 2>&1 | grep -E "^test result|FAILED|error\["
  ```

  Expected: all pass.

- [ ] **Step 10: Clippy**

  ```bash
  cargo clippy --all-targets 2>&1 | grep -E "^error|^warning"
  ```

- [ ] **Step 11: Commit**

  ```bash
  git add src/engine/costs.rs src/engine/mod.rs src/engine/stack.rs
  git commit -m "feat: add engine/costs.rs — pay_cost_components, can_pay_cost_components, pay_stack_cost, resolve_stack_cost_decline"
  ```

---

## Task 3: Restructure Ward Module

Move `collect_ward_triggers` to `triggered.rs`, delete `ward.rs`.

**Files:**
- Modify: `src/engine/triggered.rs`
- Delete: `src/engine/ward.rs`
- Modify: `src/engine/mod.rs`

- [ ] **Step 1: Add `collect_ward_triggers` to `src/engine/triggered.rs`**

  Add at the end of the file (before the `#[cfg(test)]` block):

  ```rust
  /// CR 702.21a: Collect WardTrigger stack objects for any declared targets that are
  /// opponent-controlled permanents with Ward. Each Ward ability on such a target generates
  /// one WardTrigger pushed above the triggering spell/ability on the stack.
  /// The trigger is controlled by the Ward permanent's controller (CR 603.3a).
  pub fn collect_ward_triggers(
      state: &mut GameState,
      triggering_stack_id: crate::types::stack::StackId,
      acting_player: PlayerId,
      targets: &[crate::types::effect::EffectTarget],
  ) -> Vec<crate::types::stack::StackObject> {
      use crate::types::ability::{Ability, CostComponent, OracleSpan, StaticAbility};
      use crate::types::effect::EffectTarget;
      use crate::types::stack::{StackObject, StackPayload};

      let mut triggers = Vec::new();
      for target in targets {
          let target_obj_id = match target {
              EffectTarget::Object { id } => *id,
              EffectTarget::Player { .. } => continue,
          };
          if !state.battlefield.contains_key(&target_obj_id) {
              continue;
          }
          let target_obj = match state.objects.get(&target_obj_id) {
              Some(o) => o,
              None => continue,
          };
          if target_obj.controller == acting_player {
              continue;
          }
          let ward_permanent_controller = target_obj.controller;
          let ward_cost_sets: Vec<Vec<CostComponent>> = target_obj
              .definition
              .abilities
              .iter()
              .filter_map(|span| match span {
                  OracleSpan::Parsed(Ability::Static(StaticAbility::Ward(components))) => {
                      Some(components.clone())
                  }
                  _ => None,
              })
              .collect();
          for cost in ward_cost_sets {
              let sid = state.alloc_stack_id();
              triggers.push(StackObject {
                  id: sid,
                  payload: StackPayload::WardTrigger {
                      counters_if_unpaid: triggering_stack_id,
                      cost,
                      settled: false,
                  },
                  controller: ward_permanent_controller,
                  targets: vec![],
              });
          }
      }
      triggers
  }
  ```

- [ ] **Step 2: Update all callers of `collect_ward_triggers`**

  Search for `ward::collect_ward_triggers` (or similar):
  ```bash
  grep -rn "collect_ward_triggers" src/
  ```

  In `src/engine/casting.rs` and `src/engine/activated.rs`, replace:
  ```rust
  use crate::engine::ward::collect_ward_triggers;
  // or
  super::ward::collect_ward_triggers(...)
  ```
  with:
  ```rust
  use crate::engine::triggered::collect_ward_triggers;
  // or
  super::triggered::collect_ward_triggers(...)
  ```

  (The exact import form will depend on how they currently import it. Grep to confirm.)

- [ ] **Step 3: Update `src/engine/mod.rs`**

  Replace:
  ```rust
  pub mod ward;
  ```
  with:
  ```rust
  pub mod costs;
  ```
  (Note: `costs` was already added in Task 2. If so, just remove `pub mod ward;`.)

- [ ] **Step 4: Delete `src/engine/ward.rs`**

  ```bash
  rm src/engine/ward.rs
  ```

- [ ] **Step 5: Run all tests**

  ```bash
  cargo test 2>&1 | grep -E "^test result|FAILED|error\["
  ```

  Expected: all pass (the ward tests that were in ward.rs are now gone; equivalent coverage is in costs.rs from Task 2).

- [ ] **Step 6: Clippy**

  ```bash
  cargo clippy --all-targets 2>&1 | grep -E "^error|^warning"
  ```

- [ ] **Step 7: Update serve.rs ward import**

  In `src/serve.rs`, find the import for `pay_ward`:
  ```rust
  use mecha_oracle::engine::ward::pay_ward;
  ```
  and remove it (or comment it out temporarily — it will be fully replaced in Task 7).

  If removing it breaks serve.rs dispatch (the `PayWard` arm still calls `pay_ward`), temporarily replace the dispatch arm with a stub:
  ```rust
  ActionRequest::PayWard { trigger_id } => {
      Err(format!("PayWard is being removed; use PayCost instead: trigger_id={trigger_id}"))
  }
  ```

- [ ] **Step 8: Run tests and clippy again**

  ```bash
  cargo test 2>&1 | grep -E "^test result|FAILED|error\["
  cargo clippy --all-targets 2>&1 | grep -E "^error|^warning"
  ```

- [ ] **Step 9: Commit**

  ```bash
  git add src/engine/triggered.rs src/engine/mod.rs src/serve.rs
  git rm src/engine/ward.rs
  git commit -m "refactor: delete ward.rs — collect_ward_triggers moves to triggered.rs, payment logic to costs.rs"
  ```

---

## Task 4: Update `casting.rs`

Replace the manual `greedy_payment_plan` + `pay_mana_cost` call with `pay_cost_components`.

**Files:**
- Modify: `src/engine/casting.rs`

- [ ] **Step 1: Add `costs` import to `casting.rs`**

  In the use block at the top:
  ```rust
  use super::costs::pay_cost_components;
  ```

- [ ] **Step 2: Replace the mana payment block**

  Find (around line 168):
  ```rust
  let plan = {
      let player = state
          .get_player(player_id)
          .ok_or(EngineError::CardNotFound)?;
      greedy_payment_plan(&cost, &player.mana_pool, player.life)
          .ok_or(EngineError::InsufficientMana)?
  };
  state = pay_mana_cost(state, player_id, &cost, &plan)?;
  state.mana_checkpoint = None;
  ```
  Replace with:
  ```rust
  use crate::types::ability::CostComponent;
  state = pay_cost_components(state, player_id, &[CostComponent::Mana(cost.clone())])?;
  state.mana_checkpoint = None;
  ```

- [ ] **Step 3: Remove now-unused imports from `casting.rs`**

  Remove `greedy_payment_plan` and `pay_mana_cost` from the top-level use if no longer needed:
  ```rust
  use super::mana::{greedy_payment_plan, pay_mana_cost};
  ```
  becomes:
  ```rust
  // (remove or keep only what's still used)
  ```
  Run `cargo clippy` to identify exactly what to remove.

- [ ] **Step 4: Run tests**

  ```bash
  cargo test engine::casting 2>&1 | grep -E "^test result|FAILED|error\["
  ```

  Expected: all casting tests pass.

- [ ] **Step 5: Clippy**

  ```bash
  cargo clippy --all-targets 2>&1 | grep -E "^error|^warning"
  ```

- [ ] **Step 6: Commit**

  ```bash
  git add src/engine/casting.rs
  git commit -m "refactor: casting.rs uses pay_cost_components from costs.rs"
  ```

---

## Task 5: Update `activated.rs`

Replace duplicated mana check/pay with `can_pay_cost_components` + `pay_cost_components`. Remove `payment_plan` parameter. Update serve.rs dispatch to match.

**Files:**
- Modify: `src/engine/activated.rs`
- Modify: `src/serve.rs` (only the `ActivateAbility` dispatch + import change)

- [ ] **Step 1: Update imports in `activated.rs`**

  Replace:
  ```rust
  use crate::engine::mana::{can_pay_mana, greedy_payment_plan, pay_mana_cost};
  use crate::types::{GameState, ManaCheckpoint, ObjectId, PaymentPlan, PlayerId, Zone};
  ```
  with:
  ```rust
  use crate::engine::costs::{can_pay_cost_components, pay_cost_components};
  use crate::engine::mana::pay_mana_cost;
  use crate::types::{GameState, ManaCheckpoint, ObjectId, PlayerId, Zone};
  ```

  (Keep `pay_mana_cost` in case it's still used for the Tap+mana combo; clippy will flag if not needed.)

- [ ] **Step 2: Remove `payment_plan` from the `activate_ability` signature**

  Change:
  ```rust
  pub fn activate_ability(
      mut state: GameState,
      object_id: ObjectId,
      ability_index: usize,
      activating_player: PlayerId,
      x_value: Option<u32>,
      payment_plan: Option<PaymentPlan>,
      declared_targets: Vec<crate::types::effect::EffectTarget>,
  ) -> Result<GameState, EngineError> {
  ```
  to:
  ```rust
  pub fn activate_ability(
      mut state: GameState,
      object_id: ObjectId,
      ability_index: usize,
      activating_player: PlayerId,
      x_value: Option<u32>,
      declared_targets: Vec<crate::types::effect::EffectTarget>,
  ) -> Result<GameState, EngineError> {
  ```

- [ ] **Step 3: Replace the cost read-only check and pay loops**

  Remove the entire "Check costs (read-only)" loop (the first `for component in &ability.cost` block that returns early errors for Tap/Mana). Replace it with just the structural check:
  ```rust
  // CR 602.2: verify structural feasibility (tap, summoning sickness) before mutating state.
  if !can_pay_cost_components(&state, activating_player, Some(object_id), &ability.cost) {
      // Determine which structural error to return.
      use crate::types::ability::CostComponent;
      for component in &ability.cost {
          if let CostComponent::Tap = component {
              let perm = state.battlefield.get(&object_id).unwrap();
              if perm.tapped {
                  return Err(EngineError::AlreadyTapped);
              }
              return Err(EngineError::SummoningSick);
          }
      }
      return Err(EngineError::NotYourPriority);
  }
  ```

  In the "Pay costs" loop (the second `for component in ability.cost.iter()` block), keep the `CostComponent::Tap` arm exactly as-is (it handles checkpoint + tapping), and replace the `CostComponent::Mana` arm with a call to `pay_cost_components` for non-Tap components:

  After the Tap payment loop, add:
  ```rust
  // Pay non-Tap costs (mana, life) via unified payment function.
  let non_tap: Vec<_> = ability
      .cost
      .iter()
      .filter(|c| !matches!(c, CostComponent::Tap))
      .cloned()
      .collect();
  if !non_tap.is_empty() {
      state = pay_cost_components(state, activating_player, &non_tap)?;
  }
  ```

  Remove the old `CostComponent::Mana` arm from the pay loop. Keep `CostComponent::Tap` arm unchanged.

  The full replacement of the pay loop (originally around lines 111-149):
  ```rust
  // Pay costs.
  for component in &ability.cost {
      match component {
          CostComponent::Tap => {
              if produces_mana {
                  state
                      .mana_checkpoint
                      .as_mut()
                      .unwrap()
                      .tapped_lands
                      .push(object_id);
              }
              state.battlefield.get_mut(&object_id).unwrap().tapped = true;
          }
          _ => {}
      }
  }
  let non_tap: Vec<_> = ability
      .cost
      .iter()
      .filter(|c| !matches!(c, CostComponent::Tap))
      .cloned()
      .collect();
  if !non_tap.is_empty() {
      state = pay_cost_components(state, activating_player, &non_tap)?;
  }
  ```

- [ ] **Step 4: Remove `can_pay_cost` from `activated.rs`**

  Delete the entire `pub fn can_pay_cost(...)` function (it will be replaced by `costs::can_pay_cost_components` in serve.rs).

- [ ] **Step 5: Update `src/serve.rs` for `ActivateAbility`**

  In `serve.rs`, update the import:
  ```rust
  use mecha_oracle::engine::activated::{activate_ability, can_pay_cost};
  ```
  to:
  ```rust
  use mecha_oracle::engine::activated::activate_ability;
  use mecha_oracle::engine::costs::can_pay_cost_components;
  ```

  Update the `ActionRequest::ActivateAbility` variant — remove `payment_plan`:
  ```rust
  ActivateAbility {
      object_id: u64,
      ability_index: usize,
      x_value: Option<u32>,
      targets: Vec<mecha_oracle::types::effect::EffectTarget>,
  },
  ```

  Update the dispatch arm:
  ```rust
  ActionRequest::ActivateAbility {
      object_id,
      ability_index,
      x_value,
      targets,
  } => {
      let player = state.priority_player;
      activate_ability(
          state,
          ObjectId(object_id),
          ability_index,
          player,
          x_value,
          targets,
      )
      .map_err(|e| format!("{e:?}"))
  }
  ```

  In `compute_battlefield_actions`, replace the `can_pay_cost` call:
  ```rust
  let cost_ok = can_pay_cost(state, obj.id, ability, pid);
  actions.push(ActionItemView {
      label: ...,
      can_pay_cost: cost_ok,
      ...
  })
  ```
  with:
  ```rust
  if !can_pay_cost_components(state, pid, Some(obj.id), &ability.cost) {
      continue; // already tapped or summoning sick — skip this action
  }
  actions.push(ActionItemView {
      label: ...,
      // no can_pay_cost field yet — will be removed in Task 7
      can_pay_cost: true, // temporary: structurally OK
      ...
  })
  ```

  Note: we leave `can_pay_cost: true` as a temporary placeholder; Task 7 removes the field entirely.

- [ ] **Step 6: Run all tests**

  ```bash
  cargo test 2>&1 | grep -E "^test result|FAILED|error\["
  ```

  Expected: all pass. Any test that was calling `activate_ability` with `payment_plan` will need updating (the parameter is removed).

- [ ] **Step 7: Clippy**

  ```bash
  cargo clippy --all-targets 2>&1 | grep -E "^error|^warning"
  ```

- [ ] **Step 8: Commit**

  ```bash
  git add src/engine/activated.rs src/serve.rs
  git commit -m "refactor: activated.rs uses pay_cost_components; remove can_pay_cost; remove payment_plan param"
  ```

---

## Task 6: Update `cycling.rs`

Replace manual mana check + pay with `pay_cost_components`. Remove `payment_plan` parameter.

**Files:**
- Modify: `src/engine/cycling.rs`
- Modify: `src/serve.rs` (remove `None` arg from `cycle_card` call — already passes `None`, but the param is gone)

- [ ] **Step 1: Update imports in `cycling.rs`**

  Replace:
  ```rust
  use crate::engine::mana::{can_pay_mana, greedy_payment_plan, pay_mana_cost};
  ```
  with:
  ```rust
  use crate::engine::costs::pay_cost_components;
  ```

  Remove `PaymentPlan` from the types import if present.

- [ ] **Step 2: Remove `payment_plan` from `cycle_card` signature**

  Change:
  ```rust
  pub fn cycle_card(
      mut state: GameState,
      object_id: ObjectId,
      player_id: PlayerId,
      payment_plan: Option<PaymentPlan>,
  ) -> Result<GameState, EngineError> {
  ```
  to:
  ```rust
  pub fn cycle_card(
      mut state: GameState,
      object_id: ObjectId,
      player_id: PlayerId,
  ) -> Result<GameState, EngineError> {
  ```

- [ ] **Step 3: Replace the mana check and pay block**

  Find (around lines 46-65):
  ```rust
  // Check and pay mana cost.
  {
      let player = ...;
      if !can_pay_mana(&cycling_cost, &player.mana_pool, player.life) {
          return Err(EngineError::InsufficientMana);
      }
  }
  let plan = match payment_plan {
      Some(p) => p,
      None => {
          let player = ...;
          greedy_payment_plan(&cycling_cost, &player.mana_pool, player.life)
              .ok_or(EngineError::InsufficientMana)?
      }
  };
  state = pay_mana_cost(state, player_id, &cycling_cost, &plan)?;
  ```
  Replace with:
  ```rust
  use crate::types::ability::CostComponent;
  state = pay_cost_components(
      state,
      player_id,
      &[CostComponent::Mana(cycling_cost.clone())],
  )?;
  ```

- [ ] **Step 4: Update `src/serve.rs` — remove the `None` arg from `cycle_card`**

  Find:
  ```rust
  cycle_card(state, ObjectId(object_id), player, None).map_err(|e| format!("{e:?}"))
  ```
  Change to:
  ```rust
  cycle_card(state, ObjectId(object_id), player).map_err(|e| format!("{e:?}"))
  ```

- [ ] **Step 5: Run tests**

  ```bash
  cargo test engine::cycling 2>&1 | grep -E "^test result|FAILED|error\["
  cargo test 2>&1 | grep -E "^test result|FAILED|error\["
  ```

  Expected: all pass.

- [ ] **Step 6: Clippy**

  ```bash
  cargo clippy --all-targets 2>&1 | grep -E "^error|^warning"
  ```

- [ ] **Step 7: Commit**

  ```bash
  git add src/engine/cycling.rs src/serve.rs
  git commit -m "refactor: cycling.rs uses pay_cost_components; remove payment_plan param"
  ```

---

## Task 7: Update `serve.rs`

Remove `PayWard`, add `PayCost`/`DeclineCost`, remove `can_pay_cost` from `ActionItemView`, expose ward cost to client via `StackItemView`.

**Files:**
- Modify: `src/serve.rs`

- [ ] **Step 1: Update imports**

  Remove:
  ```rust
  use mecha_oracle::engine::mana::{
      can_pay_mana, greedy_payment_plan, reset_mana, tap_land_for_mana,
  };
  ```
  Replace with:
  ```rust
  use mecha_oracle::engine::mana::{reset_mana, tap_land_for_mana};
  ```

  The `costs` module import from Task 5 should already be present (`can_pay_cost_components`). Add:
  ```rust
  use mecha_oracle::engine::costs::{
      can_pay_cost_components, pay_stack_cost, resolve_stack_cost_decline,
  };
  ```

- [ ] **Step 2: Remove `can_pay_cost` from `ActionItemView`**

  Change:
  ```rust
  #[derive(Serialize)]
  struct ActionItemView {
      label: String,
      can_pay_cost: bool,
      #[serde(flatten)]
      kind: ActionItemKind,
  }
  ```
  to:
  ```rust
  #[derive(Serialize)]
  struct ActionItemView {
      label: String,
      #[serde(flatten)]
      kind: ActionItemKind,
  }
  ```

- [ ] **Step 3: Remove all `can_pay_cost:` fields from `ActionItemView` construction sites**

  Search:
  ```bash
  grep -n "can_pay_cost" src/serve.rs
  ```

  For each `actions.push(ActionItemView { ... can_pay_cost: X ... })`, remove the `can_pay_cost: X` line.

  Also remove the mana_ok variables that are no longer needed:
  - In `compute_hand_actions`: remove `let mana_ok = greedy_payment_plan(...)` and `let mana_ok = can_pay_mana(...)` assignments.

- [ ] **Step 4: Add `cost_label` to `StackItemView`**

  Change:
  ```rust
  struct StackItemView {
      id: u64,
      kind: String,
      label: String,
      controller: PlayerId,
      card: Option<CardView>,
  }
  ```
  to:
  ```rust
  struct StackItemView {
      id: u64,
      kind: String,
      label: String,
      controller: PlayerId,
      card: Option<CardView>,
      #[serde(skip_serializing_if = "Option::is_none")]
      cost_label: Option<String>,
  }
  ```

- [ ] **Step 5: Update all `StackItemView` construction sites to include `cost_label: None`**

  For `StackPayload::Spell`, `StackPayload::TriggeredAbility`, `StackPayload::ActivatedAbility` arms, add `cost_label: None`.

- [ ] **Step 6: Add a helper function and update `WardTrigger` serialization**

  Add before `build_game_view`:
  ```rust
  fn format_ward_cost_label(components: &[crate::types::ability::CostComponent]) -> String {
      use crate::types::ability::CostComponent;
      components
          .iter()
          .filter_map(|c| match c {
              CostComponent::Mana(m) => Some(format!("{m}")),
              CostComponent::PayLife(n) => Some(format!("Pay {n} life")),
              _ => None,
          })
          .collect::<Vec<_>>()
          .join(", ")
  }
  ```

  Update the `WardTrigger` arm in `build_game_view`:
  ```rust
  StackPayload::WardTrigger { cost, .. } => {
      let cl = format_ward_cost_label(cost);
      StackItemView {
          id: sid.0,
          kind: "ward_trigger".into(),
          label: format!("Ward trigger — {cl}"),
          controller: obj.controller,
          card: None,
          cost_label: Some(cl),
      }
  }
  ```

- [ ] **Step 7: Replace `PayWard` with `PayCost` and `DeclineCost` in `ActionRequest`**

  Find:
  ```rust
  PayWard {
      /// StackId.0 of the WardTrigger on top of the stack.
      trigger_id: u64,
  },
  ```
  Replace with:
  ```rust
  /// CR 116.1: Pay the cost of a cost-bearing stack object (e.g. ward trigger).
  PayCost {
      stack_id: u64,
  },
  /// CR 702.21a: Decline an optional stack cost; the targeted spell is countered.
  DeclineCost {
      stack_id: u64,
  },
  ```

- [ ] **Step 8: Update the dispatch function**

  Remove:
  ```rust
  // CR 702.21a: pay the Ward cost to prevent the WardTrigger from countering the spell.
  ActionRequest::PayWard { trigger_id } => {
      let player = state.priority_player;
      pay_ward(state, player, StackId(trigger_id)).map_err(|e| format!("{e:?}"))
  }
  ```
  Add:
  ```rust
  ActionRequest::PayCost { stack_id } => {
      let player = state.priority_player;
      pay_stack_cost(state, player, StackId(stack_id)).map_err(|e| format!("{e:?}"))
  }
  ActionRequest::DeclineCost { stack_id } => {
      resolve_stack_cost_decline(state, StackId(stack_id)).map_err(|e| format!("{e:?}"))
  }
  ```

- [ ] **Step 9: Run all tests**

  ```bash
  cargo test 2>&1 | grep -E "^test result|FAILED|error\["
  ```

- [ ] **Step 10: Clippy**

  ```bash
  cargo clippy --all-targets 2>&1 | grep -E "^error|^warning"
  ```

- [ ] **Step 11: Commit**

  ```bash
  git add src/serve.rs
  git commit -m "feat: serve.rs — PayCost/DeclineCost actions, remove can_pay_cost from API, expose ward cost_label"
  ```

---

## Task 8: Update `serve.js`

Add client-side payment context: cards enter a payment panel on click instead of dispatching directly; ward triggers auto-enter the panel.

**Files:**
- Modify: `src/serve.js`
- Modify: `src/serve.html` (add payment panel div)
- Modify: `src/serve.css` (style payment panel)

### Background on current serve.js flow

- `handleCardClick(cardId, pid, event, autoDispatchIfSingle)` — looks up card actions, auto-dispatches if single and `can_pay_cost` is true, else shows popup.
- `buildPopupItems(actions)` — maps actions to items, disabling if `!can_pay_cost`.
- `dispatchAction(item)` — sends action to server via `sendAction`.
- `renderStack(stack)` — renders stack objects as cards (ward triggers show as "TRIG").

### What changes

1. A global `paymentContext` object (null when inactive) tracks what's being paid.
2. Cast/activate action buttons enter payment context instead of calling `sendAction` directly.
3. Ward triggers on top of the stack auto-enter payment context when the local player has priority.
4. A payment panel div (always in DOM, hidden when `paymentContext === null`) shows cost, mana pool, and Confirm/Cancel/Decline buttons.
5. `can_pay_cost` checks are removed from `buildPopupItems` and `handleCardClick`.

- [ ] **Step 1: Add payment panel to `src/serve.html`**

  Find the main game layout div and add a payment panel element. Placement: after the stack area, before the hand area (or in a fixed overlay — your choice). The exact location in the HTML depends on your layout. Add:

  ```html
  <!-- Payment context panel (hidden when not in a payment flow) -->
  <div id="payment-panel" class="payment-panel" style="display:none">
    <div class="payment-title" id="payment-title">Pay cost</div>
    <div class="payment-cost" id="payment-cost"></div>
    <div class="payment-pool" id="payment-pool"></div>
    <div class="payment-remaining" id="payment-remaining"></div>
    <div class="payment-buttons">
      <button id="payment-confirm" onclick="confirmPayment()">Pay</button>
      <button id="payment-cancel" onclick="cancelPayment()" style="display:none">Cancel</button>
      <button id="payment-decline" onclick="declinePayment()" style="display:none">Decline</button>
    </div>
  </div>
  ```

- [ ] **Step 2: Add payment panel styles to `src/serve.css`**

  Add:
  ```css
  .payment-panel {
    position: fixed;
    bottom: 120px;
    left: 50%;
    transform: translateX(-50%);
    background: #1e2a3a;
    border: 2px solid #4a90d9;
    border-radius: 8px;
    padding: 16px 24px;
    min-width: 260px;
    text-align: center;
    z-index: 200;
    box-shadow: 0 4px 24px rgba(0,0,0,0.5);
  }
  .payment-title {
    font-weight: bold;
    font-size: 1.1em;
    margin-bottom: 8px;
    color: #a0c4ff;
  }
  .payment-cost {
    font-size: 1.3em;
    margin-bottom: 6px;
  }
  .payment-pool, .payment-remaining {
    font-size: 0.9em;
    color: #ccc;
    margin-bottom: 4px;
  }
  .payment-buttons {
    display: flex;
    gap: 8px;
    justify-content: center;
    margin-top: 12px;
  }
  .payment-buttons button {
    padding: 6px 18px;
    border-radius: 4px;
    border: none;
    cursor: pointer;
    font-weight: bold;
  }
  #payment-confirm { background: #2d7d46; color: #fff; }
  #payment-confirm:disabled { background: #444; color: #888; cursor: not-allowed; }
  #payment-cancel { background: #7a3030; color: #fff; }
  #payment-decline { background: #6a4a00; color: #fff; }
  ```

- [ ] **Step 3: Add `paymentContext` state and helper to `serve.js`**

  At the top of `serve.js`, after the other global `let` declarations, add:
  ```js
  let paymentContext = null; // null when no payment is in progress
  ```

  Add the payment context functions (near `dispatchAction`):
  ```js
  // kind: "cast" | "activate" | "ward"
  // costLabel: human-readable string e.g. "{2}" or "Pay 2 life"
  // confirmAction: JSON payload to POST on Pay
  // declineable: bool (false for cast/activate, true for ward)
  // declineAction: JSON payload to POST on Decline (only when declineable)
  function enterPaymentContext(kind, costLabel, confirmAction, declineable, declineAction) {
    paymentContext = { kind, costLabel, confirmAction, declineable, declineAction };
    renderPaymentPanel();
  }

  function renderPaymentPanel() {
    const panel = document.getElementById('payment-panel');
    if (!paymentContext || !currentState) {
      panel.style.display = 'none';
      return;
    }
    panel.style.display = '';
    document.getElementById('payment-title').textContent =
      paymentContext.kind === 'ward' ? 'Ward — pay to protect your spell'
      : paymentContext.kind === 'cast' ? 'Cast — pay cost'
      : 'Activate — pay cost';
    document.getElementById('payment-cost').textContent = paymentContext.costLabel || '(no cost)';

    // Pool summary from current state
    const pool = currentState.mana_pool || {};
    const poolParts = [];
    if (pool.white)     poolParts.push(`W×${pool.white}`);
    if (pool.blue)      poolParts.push(`U×${pool.blue}`);
    if (pool.black)     poolParts.push(`B×${pool.black}`);
    if (pool.red)       poolParts.push(`R×${pool.red}`);
    if (pool.green)     poolParts.push(`G×${pool.green}`);
    if (pool.colorless) poolParts.push(`C×${pool.colorless}`);
    document.getElementById('payment-pool').textContent =
      'Pool: ' + (poolParts.length ? poolParts.join(' ') : 'empty');

    // TODO: compute remaining-to-pay for display (non-trivial for colored mana)
    document.getElementById('payment-remaining').textContent = '';

    const confirmBtn = document.getElementById('payment-confirm');
    confirmBtn.disabled = false; // server validates; always allow attempt
    const cancelBtn  = document.getElementById('payment-cancel');
    const declineBtn = document.getElementById('payment-decline');
    cancelBtn.style.display  = paymentContext.declineable ? 'none' : '';
    declineBtn.style.display = paymentContext.declineable ? '' : 'none';
  }

  function confirmPayment() {
    if (!paymentContext) return;
    sendAction(paymentContext.confirmAction);
    paymentContext = null;
    renderPaymentPanel();
  }

  function cancelPayment() {
    if (!paymentContext) return;
    paymentContext = null;
    renderPaymentPanel();
    // Reset mana if a checkpoint was active (undoes land taps)
    if (currentState && currentState.mana_checkpoint) {
      sendAction({ type: 'reset_mana' });
    }
  }

  function declinePayment() {
    if (!paymentContext || !paymentContext.declineable) return;
    sendAction(paymentContext.declineAction);
    paymentContext = null;
    renderPaymentPanel();
  }
  ```

- [ ] **Step 4: Auto-detect ward context in the render loop**

  In the `render(state)` function (or wherever state is applied to the DOM after each update), add a call to check for ward payment context:
  ```js
  function maybeEnterWardContext(state) {
    if (paymentContext !== null) return; // already in a payment flow
    if (!state.stack || state.stack.length === 0) return;
    const top = state.stack[state.stack.length - 1];
    if (top.kind !== 'ward_trigger') return;
    // Show payment UI to whoever has priority (both players share this UI)
    enterPaymentContext(
      'ward',
      top.cost_label || 'unknown cost',
      { type: 'pay_cost', stack_id: top.id },
      true,
      { type: 'decline_cost', stack_id: top.id }
    );
  }
  ```

  Call `maybeEnterWardContext(s)` in `render(s)` after updating `currentState`:
  ```js
  // Find the existing render function and add after currentState = s:
  currentState = s;
  maybeEnterWardContext(s);
  renderPaymentPanel(); // refresh panel in case mana pool changed
  ```

  Also call `renderPaymentPanel()` whenever state updates (land taps, etc.) so the pool display stays fresh.

- [ ] **Step 5: Update `buildPopupItems` to remove `can_pay_cost` logic**

  Replace:
  ```js
  function buildPopupItems(actions) {
    return actions.map(a => ({
      label: a.label,
      disabled: !a.can_pay_cost,
      onClick: a.can_pay_cost ? () => dispatchAction(a) : () => {},
    }));
  }
  ```
  with:
  ```js
  function buildPopupItems(actions) {
    return actions.map(a => ({
      label: a.label,
      disabled: false,
      onClick: () => dispatchAction(a),
    }));
  }
  ```

- [ ] **Step 6: Update `handleCardClick` to remove `can_pay_cost` check**

  Replace:
  ```js
  if (autoDispatchIfSingle) {
    if (actions.length === 1 && actions[0].can_pay_cost) {
      dispatchAction(actions[0]); return;
    }
    if (actions.length === 0) return;
  }
  ```
  with:
  ```js
  if (autoDispatchIfSingle) {
    if (actions.length === 1) { dispatchAction(actions[0]); return; }
    if (actions.length === 0) return;
  }
  ```

- [ ] **Step 7: Update `cardHTML` to remove `can_pay_cost` check**

  Replace:
  ```js
  if (card.actions && card.actions.some(a => a.can_pay_cost)) {
    classes += ' actionable';
  }
  ```
  with:
  ```js
  if (card.actions && card.actions.length > 0) {
    classes += ' actionable';
  }
  ```

- [ ] **Step 8: Update `dispatchAction` for cast/activate actions to enter payment context**

  Currently `dispatchAction` posts server actions immediately. The simplest approach for cast/activate: intercept the action before posting and enter payment context instead.

  The `ActionItemView` from the server has `kind: "server"` with an `action` payload. To distinguish cast/activate from other server actions, check the `action.type` field:
  ```js
  function dispatchAction(item) {
    if (item.kind === 'server') {
      const t = item.action.type;
      if (t === 'cast_spell' || t === 'activate_ability' || t === 'cycle_card') {
        // Enter payment context: cost comes from card/ability metadata.
        // For now, derive cost from the server's label or use a placeholder.
        // The server handles actual payment validation; the panel is UX.
        const kind = t === 'cast_spell' ? 'cast' : t === 'cycle_card' ? 'cast' : 'activate';
        const costLabel = item.cost_label || item.label; // server can add cost_label to ActionItemView in future
        enterPaymentContext(kind, costLabel, item.action, false, null);
        return;
      }
      sendAction(item.action);
    } else if (item.kind === 'toggle_attacker') {
      const idx = attackersSelected.indexOf(item.object_id);
      if (idx >= 0) attackersSelected.splice(idx, 1);
      else attackersSelected.push(item.object_id);
      render(currentState);
    } else if (item.kind === 'assign_blocker') {
      if (blockersAssignment[item.blocker_id] === item.attacker_id)
        delete blockersAssignment[item.blocker_id];
      else
        blockersAssignment[item.blocker_id] = item.attacker_id;
      render(currentState);
    }
  }
  ```

  Note: the `cost_label` on `ActionItemView` is not yet populated by the server (the server doesn't currently include cost info in hand actions). For the first pass, the Confirm button will always be enabled (server validates). A follow-up task can add `cost_label` to `ActionItemView` so the panel can show remaining cost accurately.

- [ ] **Step 9: Manual test**

  Start the server: `cargo run -- game.json` (or equivalent)

  Test the golden path:
  1. Load a game where P0 has a spell they can cast with enough mana tapped.
  2. Click a land to tap it. Verify mana pool updates.
  3. Click a castable card in hand. Verify the payment panel appears (instead of action posting immediately).
  4. Click "Pay" in the panel. Verify the spell goes to the stack.
  5. If an opponent permanent has Ward, cast a targeting spell at it. Verify the ward payment panel auto-appears.
  6. Click "Decline" in the ward panel. Verify the spell is countered.

  Edge cases:
  - Repeat with no mana — panel should still appear; "Pay" should fail gracefully (server error → do not clear context).
  - Activate an ability — panel should appear.

- [ ] **Step 10: Commit**

  ```bash
  git add src/serve.js src/serve.html src/serve.css
  git commit -m "feat: serve.js — client-side payment context panel with ward auto-detect, cancel/decline"
  ```

---

## Self-Review

### Spec coverage check

| Spec Section | Covered by Task |
|---|---|
| Remove WardCost | Task 1 |
| Ward(Vec<CostComponent>) | Task 1 |
| WardTrigger cost/settled rename | Task 1 |
| pay_cost_components | Task 2 |
| can_pay_cost_components | Task 2 |
| pay_stack_cost | Task 2 |
| resolve_stack_cost_decline | Task 2 |
| ward.rs deleted | Task 3 |
| collect_ward_triggers → triggered.rs | Task 3 |
| casting.rs uses pay_cost_components | Task 4 |
| activated.rs uses costs module | Task 5 |
| cycling.rs uses pay_cost_components | Task 6 |
| Remove PayWard; add PayCost/DeclineCost | Task 7 |
| can_pay_cost removed from ActionItemView | Task 7 |
| cost_label in WardTrigger serialization | Task 7 |
| Payment context JS | Task 8 |
| Ward auto-detect | Task 8 |
| Cast/activate enter payment context | Task 8 |
| Remove can_pay_cost from JS | Task 8 |

### Type consistency

- `CostComponent` — defined in `types/ability.rs`; used across all tasks.
- `StackPayload::WardTrigger { cost: Vec<CostComponent>, settled: bool, counters_if_unpaid: StackId }` — defined in Task 1; used in Tasks 2, 7.
- `pay_cost_components(GameState, PlayerId, &[CostComponent]) -> Result<GameState, EngineError>` — Task 2; used in Tasks 4, 5, 6.
- `can_pay_cost_components(&GameState, PlayerId, Option<ObjectId>, &[CostComponent]) -> bool` — Task 2; used in Task 5 (activated.rs) and Task 7 (serve.rs).
- `pay_stack_cost(GameState, PlayerId, StackId) -> Result<GameState, EngineError>` — Task 2; used in Task 7.
- `resolve_stack_cost_decline(GameState, StackId) -> Result<GameState, EngineError>` — Task 2; used in Task 7.
- `StackItemView.cost_label: Option<String>` — Task 7; read in Task 8 JS as `item.cost_label`.
- `ActionRequest::PayCost { stack_id: u64 }` / `DeclineCost { stack_id: u64 }` — Task 7; posted in Task 8 JS as `{ type: "pay_cost", stack_id: ... }` / `{ type: "decline_cost", stack_id: ... }`.

All consistent.

### Out of scope (per spec)

- Tap, Sacrifice, Discard in payment panel UI (displayed but not interactive)
- Kicker, buyback, overload
- Explicit client-supplied PaymentPlan
- mana_pool field on GameView (needed for "remaining to pay" display in panel) — the panel notes a TODO; follow-up task
