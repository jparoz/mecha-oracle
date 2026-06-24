# Alternative Casting Cost Framework Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Implement a framework for alternative and additional casting costs, with first mechanics: Kicker, Multikicker, Dash, and Evoke.

**Architecture:** Add `CastMode` enum to `types/ability.rs` and `cast_mode` field to `StackObject`; extend `cast_spell` to validate mode vs. card rules and pay the correct cost; add parser branches and serve.rs action generation; add `DelayedTrigger` to `GameState` and drain in `apply_step_start` for Dash's return-to-hand; synthesise a sacrifice TriggeredAbility on ETB for Evoke.

**Tech Stack:** Rust, Cargo, Axum (serve.rs HTTP layer), serde/serde_json.

## Global Constraints

- No `ReturnToHand` or `Sacrifice` EffectStep exists — use `EffectStep::MoveZone { from, to, to_player }`.
- Haste injected into `PermanentState.definition.rules_text` (NOT `CardObject.definition`) so it disappears when the permanent leaves the battlefield.
- `CastMode` must derive `serde::Serialize` + `serde::Deserialize` for serve.rs action JSON round-trips.
- All existing `cast_spell` call sites receive an extra `CastMode::Standard` argument.
- `cargo clippy --all-targets` must be clean before each commit.
- Run tests with: `cargo test 2>&1 | grep -E "^test result|FAILED|error\["`.
- CR references: grep `docs/CR.txt` before committing any new reference number.

---

### Task 1: CastMode enum + Rule variants + EngineError::InvalidCastMode

**Files:**
- Modify: `src/types/ability.rs`
- Modify: `src/engine/mod.rs`

**Interfaces:**
- Produces:
  - `pub enum CastMode { Standard, Kicked, Multikicked(u32), Dashed, Evoked }` in `src/types/ability.rs`
  - `Rule::Kicker { additional_cost: ManaCost }` (702.33a)
  - `Rule::Multikicker { additional_cost: ManaCost }` (702.33c)
  - `Rule::Dash { alternative_cost: ManaCost }` (702.109a)
  - `Rule::Evoke { alternative_cost: ManaCost }` (702.74a)
  - `EngineError::InvalidCastMode` in `src/engine/mod.rs`

- [ ] **Step 1: Add CastMode enum to `src/types/ability.rs`**

  Add after the `CardFilter` struct (around line 335, before the `KeywordAbility::display_name` impl):

  ```rust
  // Records how a spell was cast — used by cast_spell and StackObject.
  // Standard: paid normal mana cost.
  // Kicked: paid mana cost + Kicker cost (702.33a).
  // Multikicked(n): paid mana cost + n × Multikicker cost (702.33c); n ≥ 1.
  // Dashed: paid Dash alternative cost instead of mana cost (702.109a).
  // Evoked: paid Evoke alternative cost instead of mana cost (702.74a).
  #[derive(Debug, Clone, Copy, PartialEq, Eq, Default, serde::Serialize, serde::Deserialize)]
  #[serde(rename_all = "snake_case")]
  pub enum CastMode {
      #[default]
      Standard,
      Kicked,
      Multikicked(u32),
      Dashed,
      Evoked,
  }
  ```

- [ ] **Step 2: Add four Rule variants to the `Rule` enum in `src/types/ability.rs`**

  The `Rule` enum starts around line 511. Add after `Rule::Equip { .. }`:

  ```rust
  // (702.33a) Optional additional cost; pays mana_cost + additional_cost.
  Kicker { additional_cost: ManaCost },
  // (702.33c) Repeatable additional cost; pays mana_cost + n × additional_cost, n ≥ 1.
  Multikicker { additional_cost: ManaCost },
  // (702.109a) Alternative cost that replaces mana_cost; grants Haste; returns to hand at end step.
  Dash { alternative_cost: ManaCost },
  // (702.74a) Alternative cost that replaces mana_cost; ETB trigger sacrifices the permanent.
  Evoke { alternative_cost: ManaCost },
  ```

- [ ] **Step 3: Add `InvalidCastMode` to `EngineError` in `src/engine/mod.rs`**

  Open `src/engine/mod.rs` and add to the `EngineError` enum:

  ```rust
  InvalidCastMode,
  ```

  Place it alongside the other invalid-operation variants (e.g. after `InvalidPaymentPlan`).

- [ ] **Step 4: Verify compile**

  ```bash
  cargo build 2>&1 | grep -E "^error"
  ```

  Expected: clean (no errors). Warnings about unused variants are fine at this stage.

- [ ] **Step 5: Commit**

  ```bash
  git add src/types/ability.rs src/engine/mod.rs
  git commit -m "feat: add CastMode enum, Rule::Kicker/Multikicker/Dash/Evoke, EngineError::InvalidCastMode"
  ```

---

### Task 2: Add `cast_mode` to `StackObject` and update all construction sites

**Files:**
- Modify: `src/types/stack.rs`
- Modify: many files — find all sites via compile errors

**Interfaces:**
- Consumes: `CastMode` from Task 1 (`src/types/ability.rs`)
- Produces: `StackObject.cast_mode: CastMode` (default `Standard`)

- [ ] **Step 1: Add `cast_mode` field to `StackObject` in `src/types/stack.rs`**

  The struct (line 31) currently ends with `x_value`. Add after it:

  ```rust
  // CR 601.2b: records how the spell was cast, so resolution effects (Dash, Evoke) and
  // conditional rules text ("if this spell was kicked") can query it.
  pub cast_mode: crate::types::ability::CastMode,
  ```

- [ ] **Step 2: Find all `StackObject { ... }` construction sites**

  ```bash
  cargo build 2>&1 | grep "E0063\|missing field"
  ```

  This lists every file and line missing the new field. The main production sites are:

  | File | Lines (approximate) |
  |---|---|
  | `src/engine/casting.rs` | 216 |
  | `src/engine/activated.rs` | 214 |
  | `src/engine/triggered.rs` | 541, 597, 664, 708, 1963 |
  | `src/engine/stack.rs` | 712, 726, 856, 882 |
  | `src/engine/cycling.rs` | 66 |
  | `src/engine/equip.rs` | 90, 243 |
  | `src/engine/costs.rs` | 338 |
  | `src/serve.rs` | 2614, 2663, 2713, 2761, 2806 |
  | `src/engine/targeting.rs` | 645, 721, 805 (tests) |

  Plus all test-only construction sites in those files.

- [ ] **Step 3: Add `cast_mode: CastMode::Standard` to every construction site**

  For each site found in Step 2, add `cast_mode: crate::types::ability::CastMode::Standard` (or `CastMode::Standard` where `CastMode` is already imported).

  Pattern — every `StackObject { id: ..., payload: ..., controller: ..., targets: ..., x_value: ... }` becomes:

  ```rust
  StackObject {
      id: ...,
      payload: ...,
      controller: ...,
      targets: ...,
      x_value: ...,
      cast_mode: CastMode::Standard,
  }
  ```

  The `cast_spell` site in `casting.rs` (line 216) will be updated in Task 3 to use the actual cast mode; set it to `Standard` here as a placeholder.

- [ ] **Step 4: Verify compile and tests pass**

  ```bash
  cargo test 2>&1 | grep -E "^test result|FAILED|error\["
  ```

  Expected: all tests pass.

- [ ] **Step 5: Commit**

  ```bash
  git add src/types/stack.rs src/engine/casting.rs src/engine/activated.rs src/engine/triggered.rs src/engine/stack.rs src/engine/cycling.rs src/engine/equip.rs src/engine/costs.rs src/serve.rs src/engine/targeting.rs
  git commit -m "feat: add cast_mode field to StackObject; default Standard at all construction sites"
  ```

---

### Task 3: extend `cast_spell` to validate and pay per-mode costs

**Files:**
- Modify: `src/engine/casting.rs`
- Modify: `src/serve.rs` (dispatch only — action generation is Task 7)

**Interfaces:**
- Consumes: `CastMode`, `Rule::Kicker`, `Rule::Multikicker`, `Rule::Dash`, `Rule::Evoke` from Task 1; `StackObject.cast_mode` from Task 2
- Produces: `cast_spell(state, player_id, object_id, targets, x_value, cast_mode: CastMode)`

- [ ] **Step 1: Write the failing tests**

  Add to `src/engine/casting.rs` in the `#[cfg(test)]` module:

  ```rust
  fn make_creature_with_kicker() -> CardDefinition {
      use crate::types::ability::Rule;
      use crate::types::mana::{ManaCost, ManaPip};
      CardDefinition {
          name: "Kor Sanctifiers".into(),
          mana_cost: Some(ManaCost { pips: vec![ManaPip::Generic(2), ManaPip::White] }),
          type_line: TypeLine {
              supertypes: vec![],
              card_types: vec![CardType::Creature],
              subtypes: vec!["Kor".into(), "Cleric".into()],
          },
          oracle_text: "Kicker {W}\nWhen this enters, if it was kicked, destroy target artifact or enchantment.".into(),
          rules_text: vec![
              RulesText::Active(Rule::Kicker {
                  additional_cost: ManaCost { pips: vec![ManaPip::White] },
              }),
          ],
          text_annotations: vec![],
          power: Some(2), toughness: Some(4), colors: vec![],
      }
  }

  fn make_creature_with_multikicker() -> CardDefinition {
      use crate::types::ability::Rule;
      use crate::types::mana::{ManaCost, ManaPip};
      CardDefinition {
          name: "Wolfbriar Elemental".into(),
          mana_cost: Some(ManaCost { pips: vec![ManaPip::Generic(2), ManaPip::Green, ManaPip::Green] }),
          type_line: TypeLine {
              supertypes: vec![],
              card_types: vec![CardType::Creature],
              subtypes: vec!["Elemental".into()],
          },
          oracle_text: "Multikicker {G}".into(),
          rules_text: vec![
              RulesText::Active(Rule::Multikicker {
                  additional_cost: ManaCost { pips: vec![ManaPip::Green] },
              }),
          ],
          text_annotations: vec![],
          power: Some(4), toughness: Some(4), colors: vec![],
      }
  }

  fn make_creature_with_dash() -> CardDefinition {
      use crate::types::ability::Rule;
      use crate::types::mana::{ManaCost, ManaPip};
      CardDefinition {
          name: "Hellspark Elemental".into(),
          mana_cost: Some(ManaCost { pips: vec![ManaPip::Generic(1), ManaPip::Red] }),
          type_line: TypeLine {
              supertypes: vec![],
              card_types: vec![CardType::Creature],
              subtypes: vec!["Elemental".into()],
          },
          oracle_text: "Dash {R}".into(),
          rules_text: vec![
              RulesText::Active(Rule::Dash {
                  alternative_cost: ManaCost { pips: vec![ManaPip::Red] },
              }),
          ],
          text_annotations: vec![],
          power: Some(3), toughness: Some(1), colors: vec![],
      }
  }

  fn make_creature_with_evoke() -> CardDefinition {
      use crate::types::ability::Rule;
      use crate::types::mana::{ManaCost, ManaPip};
      CardDefinition {
          name: "Mulldrifter".into(),
          mana_cost: Some(ManaCost { pips: vec![ManaPip::Generic(4), ManaPip::Blue] }),
          type_line: TypeLine {
              supertypes: vec![],
              card_types: vec![CardType::Creature],
              subtypes: vec!["Elemental".into()],
          },
          oracle_text: "Flying\nWhen this enters, draw two cards.\nEvoke {2}{U}".into(),
          rules_text: vec![
              RulesText::Active(Rule::Static(KeywordAbility::Flying)),
              RulesText::Active(Rule::Evoke {
                  alternative_cost: ManaCost { pips: vec![ManaPip::Generic(2), ManaPip::Blue] },
              }),
          ],
          text_annotations: vec![],
          power: Some(2), toughness: Some(2), colors: vec![],
      }
  }

  #[test]
  fn cast_kicked_deducts_mana_cost_plus_kicker() {
      use crate::types::ability::CastMode;
      let mut gs = make_state();
      // 2W (standard) + W (kicker) = 2WW = 2 generic + 2 white
      gs.get_player_mut(PlayerId(0)).unwrap().mana_pool.white += 4;
      let id = put_in_hand(&mut gs, PlayerId(0), make_creature_with_kicker());
      let gs = cast_spell(gs, PlayerId(0), id, vec![], None, CastMode::Kicked).unwrap();
      let pool = &gs.get_player(PlayerId(0)).unwrap().mana_pool;
      assert_eq!(pool.white, 0, "all 4 white should be spent");
      assert_eq!(gs.stack_objects[gs.stack.last().unwrap()].cast_mode, CastMode::Kicked);
  }

  #[test]
  fn cast_multikicked_twice_deducts_base_plus_two_kicker() {
      use crate::types::ability::CastMode;
      let mut gs = make_state();
      // 2GG (standard) + G + G (2× kicker) = 2GGGG = 2 generic + 4 green
      gs.get_player_mut(PlayerId(0)).unwrap().mana_pool.green += 6;
      let id = put_in_hand(&mut gs, PlayerId(0), make_creature_with_multikicker());
      let gs = cast_spell(gs, PlayerId(0), id, vec![], None, CastMode::Multikicked(2)).unwrap();
      let pool = &gs.get_player(PlayerId(0)).unwrap().mana_pool;
      assert_eq!(pool.green, 0, "all 6 green should be spent");
  }

  #[test]
  fn cast_multikicked_zero_returns_invalid_cast_mode() {
      use crate::types::ability::CastMode;
      let mut gs = make_state();
      gs.get_player_mut(PlayerId(0)).unwrap().mana_pool.green += 6;
      let id = put_in_hand(&mut gs, PlayerId(0), make_creature_with_multikicker());
      let result = cast_spell(gs, PlayerId(0), id, vec![], None, CastMode::Multikicked(0));
      assert!(matches!(result, Err(EngineError::InvalidCastMode)));
  }

  #[test]
  fn cast_dashed_deducts_only_alternative_cost() {
      use crate::types::ability::CastMode;
      let mut gs = make_state();
      // Dash cost = {R}. Standard = {1}{R}. With Dash only 1 red is spent.
      gs.get_player_mut(PlayerId(0)).unwrap().mana_pool.red += 2;
      let id = put_in_hand(&mut gs, PlayerId(0), make_creature_with_dash());
      let gs = cast_spell(gs, PlayerId(0), id, vec![], None, CastMode::Dashed).unwrap();
      let pool = &gs.get_player(PlayerId(0)).unwrap().mana_pool;
      assert_eq!(pool.red, 1, "only 1 red spent for Dash cost {R}");
      assert_eq!(gs.stack_objects[gs.stack.last().unwrap()].cast_mode, CastMode::Dashed);
  }

  #[test]
  fn cast_evoked_deducts_only_alternative_cost() {
      use crate::types::ability::CastMode;
      let mut gs = make_state();
      // Evoke cost = {2}{U}. Give just enough.
      gs.get_player_mut(PlayerId(0)).unwrap().mana_pool.blue += 3;
      let id = put_in_hand(&mut gs, PlayerId(0), make_creature_with_evoke());
      let gs = cast_spell(gs, PlayerId(0), id, vec![], None, CastMode::Evoked).unwrap();
      let pool = &gs.get_player(PlayerId(0)).unwrap().mana_pool;
      assert_eq!(pool.blue, 0);
      assert_eq!(gs.stack_objects[gs.stack.last().unwrap()].cast_mode, CastMode::Evoked);
  }

  #[test]
  fn cast_dashed_without_dash_rule_returns_invalid_cast_mode() {
      use crate::types::ability::CastMode;
      let db = test_db();
      let mut gs = make_state();
      gs.get_player_mut(PlayerId(0)).unwrap().mana_pool.green += 2;
      let id = put_in_hand(&mut gs, PlayerId(0), db.get("Grizzly Bears").unwrap().clone());
      let result = cast_spell(gs, PlayerId(0), id, vec![], None, CastMode::Dashed);
      assert!(matches!(result, Err(EngineError::InvalidCastMode)));
  }

  #[test]
  fn cast_kicked_without_kicker_rule_returns_invalid_cast_mode() {
      use crate::types::ability::CastMode;
      let db = test_db();
      let mut gs = make_state();
      gs.get_player_mut(PlayerId(0)).unwrap().mana_pool.green += 2;
      let id = put_in_hand(&mut gs, PlayerId(0), db.get("Grizzly Bears").unwrap().clone());
      let result = cast_spell(gs, PlayerId(0), id, vec![], None, CastMode::Kicked);
      assert!(matches!(result, Err(EngineError::InvalidCastMode)));
  }
  ```

- [ ] **Step 2: Run tests to confirm they fail**

  ```bash
  cargo test 2>&1 | grep -E "^test result|FAILED|error\["
  ```

  Expected: compile errors (cast_spell has wrong arity).

- [ ] **Step 3: Update `cast_spell` signature and cost logic in `src/engine/casting.rs`**

  Change the function signature from:
  ```rust
  pub fn cast_spell(
      mut state: GameState,
      player_id: PlayerId,
      object_id: ObjectId,
      declared_targets: Vec<crate::types::effect::EffectTarget>,
      x_value: Option<u32>,
  ) -> Result<GameState, EngineError>
  ```

  To:
  ```rust
  pub fn cast_spell(
      mut state: GameState,
      player_id: PlayerId,
      object_id: ObjectId,
      declared_targets: Vec<crate::types::effect::EffectTarget>,
      x_value: Option<u32>,
      cast_mode: crate::types::ability::CastMode,
  ) -> Result<GameState, EngineError>
  ```

  Replace the cost extraction block (currently lines 165–203):
  ```rust
  // existing:
  let cost = { ... obj.definition.mana_cost.clone().ok_or(CannotCastNow)? };
  use crate::types::ability::CostComponent;
  state = super::costs::pay_cost_components(state, player_id, &[CostComponent::Mana(cost.clone())], x_value)?;
  ```

  With:
  ```rust
  let cost_components = {
      use crate::types::ability::{CastMode, CostComponent, Rule};
      use crate::types::RulesText;
      let hand = state.hands.get(&player_id).ok_or(EngineError::CardNotFound)?;
      if !hand.contains(&object_id) {
          return Err(EngineError::CardNotInHand);
      }
      let obj = state.objects.get(&object_id).ok_or(EngineError::CardNotFound)?;

      if !is_instant_speed(obj) {
          if state.active_player != player_id {
              return Err(EngineError::CannotCastNow);
          }
          if !matches!(state.step(), Step::PreCombatMain | Step::PostCombatMain) {
              return Err(EngineError::CannotCastNow);
          }
          if !state.stack.is_empty() {
              return Err(EngineError::CannotCastNow);
          }
      }

      match cast_mode {
          CastMode::Standard => {
              let mana = obj.definition.mana_cost.clone().ok_or(EngineError::CannotCastNow)?;
              vec![CostComponent::Mana(mana)]
          }
          CastMode::Kicked => {
              let mana = obj.definition.mana_cost.clone().ok_or(EngineError::CannotCastNow)?;
              let kicker = obj.definition.rules_text.iter().find_map(|span| {
                  if let RulesText::Active(Rule::Kicker { additional_cost }) = span {
                      Some(additional_cost.clone())
                  } else {
                      None
                  }
              }).ok_or(EngineError::InvalidCastMode)?;
              vec![CostComponent::Mana(mana), CostComponent::Mana(kicker)]
          }
          CastMode::Multikicked(n) => {
              if n == 0 {
                  return Err(EngineError::InvalidCastMode);
              }
              let mana = obj.definition.mana_cost.clone().ok_or(EngineError::CannotCastNow)?;
              let kicker = obj.definition.rules_text.iter().find_map(|span| {
                  if let RulesText::Active(Rule::Multikicker { additional_cost }) = span {
                      Some(additional_cost.clone())
                  } else {
                      None
                  }
              }).ok_or(EngineError::InvalidCastMode)?;
              let mut components = vec![CostComponent::Mana(mana)];
              for _ in 0..n {
                  components.push(CostComponent::Mana(kicker.clone()));
              }
              components
          }
          CastMode::Dashed => {
              let dash = obj.definition.rules_text.iter().find_map(|span| {
                  if let RulesText::Active(Rule::Dash { alternative_cost }) = span {
                      Some(alternative_cost.clone())
                  } else {
                      None
                  }
              }).ok_or(EngineError::InvalidCastMode)?;
              vec![CostComponent::Mana(dash)]
          }
          CastMode::Evoked => {
              let evoke = obj.definition.rules_text.iter().find_map(|span| {
                  if let RulesText::Active(Rule::Evoke { alternative_cost }) = span {
                      Some(alternative_cost.clone())
                  } else {
                      None
                  }
              }).ok_or(EngineError::InvalidCastMode)?;
              vec![CostComponent::Mana(evoke)]
          }
      }
  };

  state = super::costs::pay_cost_components(state, player_id, &cost_components, x_value)?;
  ```

  Also update the `StackObject` construction (around line 216) to pass the actual `cast_mode`:
  ```rust
  let stack_obj = crate::types::StackObject {
      id: stack_id,
      payload: crate::types::StackPayload::Spell { card_id: object_id },
      controller: player_id,
      targets: declared_targets,
      x_value,
      cast_mode,
  };
  ```

  Remove the now-redundant `hand.contains` and `obj` borrow from the original cost block (they are now inside the `cost_components` block above).

- [ ] **Step 4: Fix all existing `cast_spell` call sites — add `CastMode::Standard`**

  ```bash
  cargo build 2>&1 | grep "E0061\|expected.*argument"
  ```

  Call sites to update:
  - `src/serve.rs` line ~1198: `cast_spell(state, player, ObjectId(object_id), targets, x_value, CastMode::Standard)`
  - Every test in `src/engine/casting.rs` that calls `cast_spell(gs, ..., None)` — append `, CastMode::Standard`

  Import `CastMode` in serve.rs:
  ```rust
  use mecha_oracle::types::ability::{..., CastMode};
  ```

  Also add `cast_mode` field to `ActionRequest::CastSpell` in serve.rs (deserialization only — action generation updated in Task 7):
  ```rust
  CastSpell {
      object_id: u64,
      #[serde(default)]
      targets: Vec<mecha_oracle::types::effect::EffectTarget>,
      #[serde(default)]
      x_value: Option<u32>,
      #[serde(default)]
      cast_mode: Option<CastMode>,  // None → Standard; backwards-compatible
  },
  ```

  Update the dispatch arm:
  ```rust
  ActionRequest::CastSpell { object_id, targets, x_value, cast_mode } => {
      let player = state.priority_player;
      let mode = cast_mode.unwrap_or(CastMode::Standard);
      cast_spell(state, player, ObjectId(object_id), targets, x_value, mode)
          .map_err(|e| format!("{e:?}"))
  }
  ```

- [ ] **Step 5: Run tests and clippy**

  ```bash
  cargo test 2>&1 | grep -E "^test result|FAILED|error\["
  cargo clippy --all-targets 2>&1 | grep -E "^error|^warning"
  ```

  Expected: all tests pass, clippy clean.

- [ ] **Step 6: Commit**

  ```bash
  git add src/engine/casting.rs src/serve.rs
  git commit -m "feat: extend cast_spell with CastMode; validate and pay per-mode costs (702.33/702.109/702.74)"
  ```

---

### Task 4: Parser — four new parse branches

**Files:**
- Modify: `src/parser/oracle.rs`

**Interfaces:**
- Consumes: `Rule::Kicker`, `Rule::Multikicker`, `Rule::Dash`, `Rule::Evoke` from Task 1
- Produces: parse branches converting oracle text keywords to Rule variants

- [ ] **Step 1: Write failing tests**

  Add to `src/parser/oracle.rs` tests module:

  ```rust
  #[test]
  fn parse_kicker_mana_cost() {
      use crate::types::mana::{ManaCost, ManaPip};
      let result = parse_oracle_text("Kicker {1}{U}");
      assert!(
          result.iter().any(|span| matches!(
              span,
              crate::types::RulesText::Active(crate::types::Rule::Kicker {
                  additional_cost: ManaCost { pips }
              }) if pips == &[ManaPip::Generic(1), ManaPip::Blue]
          )),
          "expected Rule::Kicker with {{1}}{{U}}, got {result:?}"
      );
  }

  #[test]
  fn parse_multikicker_mana_cost() {
      use crate::types::mana::{ManaCost, ManaPip};
      let result = parse_oracle_text("Multikicker {G}");
      assert!(
          result.iter().any(|span| matches!(
              span,
              crate::types::RulesText::Active(crate::types::Rule::Multikicker {
                  additional_cost: ManaCost { pips }
              }) if pips == &[ManaPip::Green]
          )),
          "expected Rule::Multikicker with {{G}}, got {result:?}"
      );
  }

  #[test]
  fn parse_dash_mana_cost() {
      use crate::types::mana::{ManaCost, ManaPip};
      let result = parse_oracle_text("Dash {R}");
      assert!(
          result.iter().any(|span| matches!(
              span,
              crate::types::RulesText::Active(crate::types::Rule::Dash {
                  alternative_cost: ManaCost { pips }
              }) if pips == &[ManaPip::Red]
          )),
          "expected Rule::Dash with {{R}}, got {result:?}"
      );
  }

  #[test]
  fn parse_evoke_mana_cost() {
      use crate::types::mana::{ManaCost, ManaPip};
      let result = parse_oracle_text("Evoke {2}{U}");
      assert!(
          result.iter().any(|span| matches!(
              span,
              crate::types::RulesText::Active(crate::types::Rule::Evoke {
                  alternative_cost: ManaCost { pips }
              }) if pips == &[ManaPip::Generic(2), ManaPip::Blue]
          )),
          "expected Rule::Evoke with {{2}}{{U}}, got {result:?}"
      );
  }

  #[test]
  fn parse_kicker_malformed_cost_falls_back_to_unimplemented() {
      let result = parse_oracle_text("Kicker badcost");
      assert!(
          result.iter().any(|span| matches!(span, crate::types::RulesText::ParsedUnimplemented(_))),
          "malformed kicker cost should be ParsedUnimplemented"
      );
  }
  ```

- [ ] **Step 2: Run tests to confirm they fail**

  ```bash
  cargo test parser 2>&1 | grep -E "^test result|FAILED|error\["
  ```

  Expected: the new tests fail (currently parsed as `ParsedUnimplemented`).

- [ ] **Step 3: Add the four parse branches in `src/parser/oracle.rs`**

  In `parse_keyword_line` (the function that returns `RulesText`), add after the Cycling branch (around line 521):

  ```rust
  // Kicker [cost] (702.33a): optional additional mana cost.
  if s.starts_with("kicker ")
      && let Some(cost) = try_parse_mana_cost(kw["kicker ".len()..].trim())
  {
      return RulesText::Active(Rule::Kicker { additional_cost: cost });
  }

  // Multikicker [cost] (702.33c): repeatable additional mana cost.
  if s.starts_with("multikicker ")
      && let Some(cost) = try_parse_mana_cost(kw["multikicker ".len()..].trim())
  {
      return RulesText::Active(Rule::Multikicker { additional_cost: cost });
  }

  // Dash [cost] (702.109a): alternative cost; grants Haste; returns to hand at end step.
  if s.starts_with("dash ")
      && let Some(cost) = try_parse_mana_cost(kw["dash ".len()..].trim())
  {
      return RulesText::Active(Rule::Dash { alternative_cost: cost });
  }

  // Evoke [cost] (702.74a): alternative cost; ETB trigger sacrifices the permanent.
  if s.starts_with("evoke ")
      && let Some(cost) = try_parse_mana_cost(kw["evoke ".len()..].trim())
  {
      return RulesText::Active(Rule::Evoke { alternative_cost: cost });
  }
  ```

  Note: `s` is the lowercase version of `kw`; use `kw[...]` (not `s[...]`) for the cost slice so mana symbols stay uppercase (`{U}` not `{u}`).

- [ ] **Step 4: Run tests**

  ```bash
  cargo test parser 2>&1 | grep -E "^test result|FAILED|error\["
  ```

  Expected: all parser tests pass.

- [ ] **Step 5: Run full test suite and clippy**

  ```bash
  cargo test 2>&1 | grep -E "^test result|FAILED|error\["
  cargo clippy --all-targets 2>&1 | grep -E "^error|^warning"
  ```

  Expected: all tests pass, clippy clean.

- [ ] **Step 6: Commit**

  ```bash
  git add src/parser/oracle.rs
  git commit -m "feat: parse Kicker/Multikicker/Dash/Evoke keywords to Rule variants (702.33/702.109/702.74)"
  ```

---

### Task 5: Delayed trigger infrastructure + Dash resolution (Haste injection + return-to-hand)

**Files:**
- Modify: `src/types/game_state.rs`
- Modify: `src/engine/turn.rs`
- Modify: `src/engine/stack.rs`

**Interfaces:**
- Consumes: `CastMode::Dashed`, `Rule::Dash`, `StackObject.cast_mode` from previous tasks
- Produces:
  - `DelayedTrigger` struct in `src/types/game_state.rs`
  - `GameState.delayed_triggers: Vec<DelayedTrigger>`
  - `apply_step_start` drains matching triggers at step start
  - `resolve_top` injects Haste and registers delayed return-to-hand for Dashed permanents

- [ ] **Step 1: Write failing test**

  Add to `src/engine/stack.rs` tests:

  ```rust
  fn make_dash_creature_def() -> crate::types::card::CardDefinition {
      use crate::types::ability::{CastMode, KeywordAbility, Rule};
      use crate::types::card::{CardDefinition, CardType, TypeLine};
      use crate::types::mana::{ManaCost, ManaPip};
      use crate::types::RulesText;
      CardDefinition {
          name: "Hellspark Elemental".into(),
          mana_cost: Some(ManaCost { pips: vec![ManaPip::Generic(1), ManaPip::Red] }),
          type_line: TypeLine {
              supertypes: vec![],
              card_types: vec![CardType::Creature],
              subtypes: vec![],
          },
          oracle_text: "Haste\nDash {R}".into(),
          rules_text: vec![
              RulesText::Active(Rule::Static(KeywordAbility::Haste)),
              RulesText::Active(Rule::Dash {
                  alternative_cost: ManaCost { pips: vec![ManaPip::Red] },
              }),
          ],
          text_annotations: vec![],
          power: Some(3), toughness: Some(1), colors: vec![],
      }
  }

  #[test]
  fn dash_resolution_injects_haste_into_permanent_state() {
      use crate::engine::casting::cast_spell;
      use crate::types::ability::{CastMode, KeywordAbility};
      use crate::types::{CardObject, Player, Zone};
      let mut gs = GameState::new(vec![
          Player::new(PlayerId(0), "Alice"),
          Player::new(PlayerId(1), "Bob"),
      ]);
      gs.step = crate::types::Step::PreCombatMain;
      gs.get_player_mut(PlayerId(0)).unwrap().mana_pool.red += 2;
      let id = gs.alloc_id();
      let obj = CardObject::new(id, make_dash_creature_def(), PlayerId(0), Zone::Hand);
      gs.hands.get_mut(&PlayerId(0)).unwrap().push(id);
      gs.add_object(obj);

      let gs = cast_spell(gs, PlayerId(0), id, vec![], None, CastMode::Dashed).unwrap();
      let gs = pass_priority(gs, PlayerId(0)).unwrap();
      let gs = pass_priority(gs, PlayerId(1)).unwrap();

      // Permanent should be on battlefield with Haste in PermanentState.
      assert!(gs.battlefield.contains_key(&id), "creature should be on battlefield");
      let perm = &gs.battlefield[&id];
      assert!(
          perm.has_keyword(KeywordAbility::Haste),
          "dashed creature must have Haste in PermanentState"
      );
      // CardObject.definition must NOT have injected Haste beyond what was parsed.
      // (The creature already has Haste in oracle text; the injected copy is a duplicate,
      // but it must be in perm.definition, not obj.definition — covered by the rule above.)

      // A delayed trigger for end step must be registered.
      assert!(
          !gs.delayed_triggers.is_empty(),
          "a delayed return-to-hand trigger should be registered"
      );
  }

  #[test]
  fn dash_creature_returns_to_hand_at_end_step() {
      use crate::engine::casting::cast_spell;
      use crate::engine::turn::apply_step_start;
      use crate::types::ability::CastMode;
      use crate::types::{CardObject, Player, Step, Zone};
      let mut gs = GameState::new(vec![
          Player::new(PlayerId(0), "Alice"),
          Player::new(PlayerId(1), "Bob"),
      ]);
      gs.step = crate::types::Step::PreCombatMain;
      gs.get_player_mut(PlayerId(0)).unwrap().mana_pool.red += 2;
      let id = gs.alloc_id();
      let obj = CardObject::new(id, make_dash_creature_def(), PlayerId(0), Zone::Hand);
      gs.hands.get_mut(&PlayerId(0)).unwrap().push(id);
      gs.add_object(obj);

      // Cast with Dash, resolve.
      let gs = cast_spell(gs, PlayerId(0), id, vec![], None, CastMode::Dashed).unwrap();
      let gs = pass_priority(gs, PlayerId(0)).unwrap();
      let gs = pass_priority(gs, PlayerId(1)).unwrap();
      assert!(gs.battlefield.contains_key(&id));

      // Advance to end step.
      let mut gs = gs;
      gs.step = Step::EndStep;
      let gs = apply_step_start(gs);

      // End-step delayed trigger is now on the stack. Both players pass → it resolves.
      assert!(!gs.stack.is_empty(), "return-to-hand trigger should be on stack");
      let gs = pass_priority(gs, PlayerId(0)).unwrap();
      let gs = pass_priority(gs, PlayerId(1)).unwrap();

      assert!(
          !gs.battlefield.contains_key(&id),
          "creature should have left the battlefield"
      );
      assert!(
          gs.hands[&PlayerId(0)].contains(&id),
          "creature should be in owner's hand"
      );
  }
  ```

- [ ] **Step 2: Run tests to confirm they fail**

  ```bash
  cargo test 2>&1 | grep -E "^test result|FAILED|error\["
  ```

  Expected: compile errors or test failures (no `DelayedTrigger` yet).

- [ ] **Step 3: Add `DelayedTrigger` struct and field to `GameState` in `src/types/game_state.rs`**

  Add after the `PendingPayment` struct (around line 66):

  ```rust
  // A one-shot trigger registered by an engine effect (e.g. Dash — CR 702.109a).
  // Fires at the start of the matching step; drained from the list after firing.
  #[derive(Debug, Clone)]
  pub struct DelayedTrigger {
      /// The step at which this trigger fires.
      pub fires_on_step: Step,
      /// The effect to execute when the trigger resolves.
      pub effect: crate::types::effect::Effect,
      /// Pre-declared targets for the effect.
      pub targets: Vec<crate::types::effect::EffectTarget>,
      /// The player who controls this trigger.
      pub controller: PlayerId,
  }
  ```

  Add `delayed_triggers` field to `GameState` struct:
  ```rust
  pub delayed_triggers: Vec<DelayedTrigger>,
  ```

  In `GameState::new`:
  ```rust
  delayed_triggers: vec![],
  ```

- [ ] **Step 4: Drain delayed triggers at step start in `src/engine/turn.rs`**

  In `apply_step_start`, after the PhaseStep triggers are pushed (after the `for t in step_triggers` loop, before the `match state.step` block):

  ```rust
  // Drain and fire one-shot delayed triggers matching the current step.
  // (e.g. Dash's return-to-hand — CR 702.109a.)
  let (to_fire, to_keep): (Vec<_>, Vec<_>) = state.delayed_triggers.drain(..)
      .partition(|t| t.fires_on_step == current_step);
  state.delayed_triggers = to_keep;
  for trigger in to_fire {
      let stack_id = state.alloc_stack_id();
      let stack_obj = crate::types::stack::StackObject {
          id: stack_id,
          payload: crate::types::stack::StackPayload::TriggeredAbility {
              source_id: crate::types::ids::ObjectId(0),
              effect: trigger.effect,
              label: "Delayed trigger".into(),
          },
          controller: trigger.controller,
          targets: trigger.targets,
          x_value: None,
          cast_mode: crate::types::ability::CastMode::Standard,
      };
      state.stack.push(stack_id);
      state.stack_objects.insert(stack_id, stack_obj);
  }
  ```

  Add import at top of `turn.rs` if not already present:
  ```rust
  use crate::types::game_state::DelayedTrigger;
  ```

- [ ] **Step 5: Handle Dash in `resolve_top` in `src/engine/stack.rs`**

  In `resolve_top`, after `let x_value = stack_obj.x_value;` (around line 467), add:
  ```rust
  let cast_mode = stack_obj.cast_mode;
  ```

  In the permanent spell resolution block, after `state.battlefield.insert(card_id, perm);` (around line 515), add:

  ```rust
  // (702.109a) Dash: inject Haste into the PermanentState copy and register a delayed
  // return-to-hand trigger. Only the battlefield copy is modified — CardObject.definition
  // stays untouched so the card retains its original rules on future casts.
  if cast_mode == crate::types::ability::CastMode::Dashed {
      if let Some(perm) = state.battlefield.get_mut(&card_id) {
          perm.definition.rules_text.push(
              crate::types::RulesText::Active(
                  crate::types::Rule::Static(
                      crate::types::ability::KeywordAbility::Haste
                  )
              )
          );
      }
      state.delayed_triggers.push(crate::types::game_state::DelayedTrigger {
          fires_on_step: crate::types::Step::EndStep,
          effect: vec![crate::types::effect::EffectStep::MoveZone {
              from: crate::types::Zone::Battlefield,
              to: crate::types::Zone::Hand,
              to_player: crate::types::ZoneOwner::CardController,
          }],
          targets: vec![crate::types::effect::EffectTarget::Object { id: card_id }],
          controller,
      });
  }
  ```

  Verify `Step` and `ZoneOwner` are in scope — they are already imported in `stack.rs` via `crate::types`.

- [ ] **Step 6: Run tests**

  ```bash
  cargo test 2>&1 | grep -E "^test result|FAILED|error\["
  ```

  Expected: Dash tests pass.

- [ ] **Step 7: Run clippy**

  ```bash
  cargo clippy --all-targets 2>&1 | grep -E "^error|^warning"
  ```

- [ ] **Step 8: Commit**

  ```bash
  git add src/types/game_state.rs src/engine/turn.rs src/engine/stack.rs
  git commit -m "feat: DelayedTrigger infrastructure + Dash Haste injection and return-to-hand (702.109a)"
  ```

---

### Task 6: Evoke resolution — ETB sacrifice trigger

**Files:**
- Modify: `src/engine/stack.rs`

**Interfaces:**
- Consumes: `CastMode::Evoked`, `cast_mode` from `StackObject` (Task 2)
- Produces: when an Evoked permanent enters the battlefield, a TriggeredAbility (`MoveZone` to Graveyard) is pushed onto the stack

- [ ] **Step 1: Write failing test**

  Add to `src/engine/stack.rs` tests:

  ```rust
  fn make_evoke_creature_def() -> crate::types::card::CardDefinition {
      use crate::types::ability::Rule;
      use crate::types::card::{CardDefinition, CardType, TypeLine};
      use crate::types::mana::{ManaCost, ManaPip};
      use crate::types::RulesText;
      CardDefinition {
          name: "Shriekmaw".into(),
          mana_cost: Some(ManaCost { pips: vec![ManaPip::Generic(4), ManaPip::Black] }),
          type_line: TypeLine {
              supertypes: vec![],
              card_types: vec![CardType::Creature],
              subtypes: vec!["Elemental".into()],
          },
          oracle_text: "Fear\nWhen this enters, destroy target nonblack, nonartifact creature.\nEvoke {B}".into(),
          rules_text: vec![
              RulesText::Active(Rule::Evoke {
                  alternative_cost: ManaCost { pips: vec![ManaPip::Black] },
              }),
          ],
          text_annotations: vec![],
          power: Some(3), toughness: Some(2), colors: vec![],
      }
  }

  #[test]
  fn evoke_resolution_pushes_sacrifice_trigger() {
      use crate::engine::casting::cast_spell;
      use crate::types::ability::CastMode;
      use crate::types::{CardObject, Player, Zone};
      let mut gs = GameState::new(vec![
          Player::new(PlayerId(0), "Alice"),
          Player::new(PlayerId(1), "Bob"),
      ]);
      gs.step = crate::types::Step::PreCombatMain;
      gs.get_player_mut(PlayerId(0)).unwrap().mana_pool.black += 1;
      let id = gs.alloc_id();
      let obj = CardObject::new(id, make_evoke_creature_def(), PlayerId(0), Zone::Hand);
      gs.hands.get_mut(&PlayerId(0)).unwrap().push(id);
      gs.add_object(obj);

      let gs = cast_spell(gs, PlayerId(0), id, vec![], None, CastMode::Evoked).unwrap();
      let gs = pass_priority(gs, PlayerId(0)).unwrap();
      let gs = pass_priority(gs, PlayerId(1)).unwrap(); // spell resolves → creature ETBs

      // After ETB: creature is on battlefield + sacrifice trigger is on stack.
      assert!(gs.battlefield.contains_key(&id), "creature should be on battlefield after ETB");
      assert!(!gs.stack.is_empty(), "Evoke sacrifice trigger should be on stack");

      // Resolve the sacrifice trigger.
      let gs = pass_priority(gs, PlayerId(0)).unwrap();
      let gs = pass_priority(gs, PlayerId(1)).unwrap();

      assert!(
          !gs.battlefield.contains_key(&id),
          "creature should have left the battlefield after Evoke sacrifice"
      );
      assert!(
          gs.graveyards[&PlayerId(0)].contains(&id),
          "creature should be in owner's graveyard"
      );
  }
  ```

- [ ] **Step 2: Run tests to confirm they fail**

  ```bash
  cargo test evoke 2>&1 | grep -E "^test result|FAILED|error\["
  ```

  Expected: test fails (no Evoke logic yet).

- [ ] **Step 3: Add Evoke logic in `resolve_top` in `src/engine/stack.rs`**

  After the ETB triggers are pushed (after the `for trigger in etb_triggers` loop, around line 529), add:

  ```rust
  // (702.74a) Evoke: when cast with the Evoke alternative cost, synthesise an ETB
  // sacrifice trigger. This replicates "when this enters, if its evoke cost was paid,
  // sacrifice it" without parsing the conditional oracle clause.
  if cast_mode == crate::types::ability::CastMode::Evoked {
      let evoke_stack_id = state.alloc_stack_id();
      let evoke_trigger = crate::types::stack::StackObject {
          id: evoke_stack_id,
          payload: crate::types::stack::StackPayload::TriggeredAbility {
              source_id: card_id,
              effect: vec![crate::types::effect::EffectStep::MoveZone {
                  from: crate::types::Zone::Battlefield,
                  to: crate::types::Zone::Graveyard,
                  to_player: crate::types::ZoneOwner::CardOwner,
              }],
              label: "Evoke".into(),
          },
          controller,
          targets: vec![crate::types::effect::EffectTarget::Object { id: card_id }],
          x_value: None,
          cast_mode: crate::types::ability::CastMode::Standard,
      };
      state.stack.push(evoke_stack_id);
      state.stack_objects.insert(evoke_stack_id, evoke_trigger);
  }
  ```

- [ ] **Step 4: Run tests**

  ```bash
  cargo test 2>&1 | grep -E "^test result|FAILED|error\["
  ```

  Expected: Evoke test passes, all others continue to pass.

- [ ] **Step 5: Run clippy**

  ```bash
  cargo clippy --all-targets 2>&1 | grep -E "^error|^warning"
  ```

- [ ] **Step 6: Commit**

  ```bash
  git add src/engine/stack.rs
  git commit -m "feat: Evoke resolution — synthesise ETB sacrifice trigger (702.74a)"
  ```

---

### Task 7: serve.rs — per-mode action generation

**Files:**
- Modify: `src/serve.rs`

**Interfaces:**
- Consumes: `CastMode`, `Rule::Kicker`, `Rule::Multikicker`, `Rule::Dash`, `Rule::Evoke`
- Produces: per-mode `cast_spell` action entries in the hand card action list

- [ ] **Step 1: Write failing tests**

  Add to `src/serve.rs` tests:

  ```rust
  #[test]
  fn hand_card_with_dash_generates_both_standard_and_dashed_actions() {
      use mecha_oracle::types::ability::Rule;
      use mecha_oracle::types::card::{CardDefinition, CardType, TypeLine};
      use mecha_oracle::types::mana::{ManaCost, ManaPip};
      use mecha_oracle::types::{CardObject, Player, RulesText, Zone};

      let mut gs = GameState::new(vec![
          Player::new(PlayerId(0), "Alice"),
          Player::new(PlayerId(1), "Bob"),
      ]);
      gs.step = crate::types::Step::PreCombatMain; // wait, Step is from mecha_oracle
      // use mecha_oracle::types::Step;
      gs.step = mecha_oracle::types::Step::PreCombatMain;

      let dash_def = CardDefinition {
          name: "Hellspark Elemental".into(),
          mana_cost: Some(ManaCost { pips: vec![ManaPip::Generic(1), ManaPip::Red] }),
          type_line: TypeLine {
              supertypes: vec![],
              card_types: vec![CardType::Creature],
              subtypes: vec![],
          },
          oracle_text: "Dash {R}".into(),
          rules_text: vec![RulesText::Active(Rule::Dash {
              alternative_cost: ManaCost { pips: vec![ManaPip::Red] },
          })],
          text_annotations: vec![],
          power: Some(3), toughness: Some(1), colors: vec![],
      };
      let id = gs.alloc_id();
      let obj = CardObject::new(id, dash_def, PlayerId(0), Zone::Hand);
      gs.hands.get_mut(&PlayerId(0)).unwrap().push(id);
      gs.add_object(obj);
      gs.get_player_mut(PlayerId(0)).unwrap().mana_pool.red += 2;

      let gs_ref = &gs;
      let obj_ref = gs_ref.objects.get(&id).unwrap();
      let actions = card_actions(gs_ref, PlayerId(0), obj_ref);

      let labels: Vec<&str> = actions.iter().map(|a| a.label.as_str()).collect();
      assert!(
          labels.iter().any(|l| l.contains("Dash")),
          "expected a Dashed action, got: {labels:?}"
      );
      assert!(
          labels.iter().any(|l| !l.contains("Dash") && l.contains("Cast")),
          "expected a Standard action, got: {labels:?}"
      );
  }

  #[test]
  fn hand_card_with_kicker_generates_both_standard_and_kicked_actions() {
      use mecha_oracle::types::ability::Rule;
      use mecha_oracle::types::card::{CardDefinition, CardType, TypeLine};
      use mecha_oracle::types::mana::{ManaCost, ManaPip};
      use mecha_oracle::types::{CardObject, Player, RulesText, Zone};

      let mut gs = GameState::new(vec![
          Player::new(PlayerId(0), "Alice"),
          Player::new(PlayerId(1), "Bob"),
      ]);
      gs.step = mecha_oracle::types::Step::PreCombatMain;

      let kick_def = CardDefinition {
          name: "Kor Sanctifiers".into(),
          mana_cost: Some(ManaCost { pips: vec![ManaPip::Generic(2), ManaPip::White] }),
          type_line: TypeLine {
              supertypes: vec![],
              card_types: vec![CardType::Creature],
              subtypes: vec![],
          },
          oracle_text: "Kicker {W}".into(),
          rules_text: vec![RulesText::Active(Rule::Kicker {
              additional_cost: ManaCost { pips: vec![ManaPip::White] },
          })],
          text_annotations: vec![],
          power: Some(2), toughness: Some(4), colors: vec![],
      };
      let id = gs.alloc_id();
      let obj = CardObject::new(id, kick_def, PlayerId(0), Zone::Hand);
      gs.hands.get_mut(&PlayerId(0)).unwrap().push(id);
      gs.add_object(obj);
      gs.get_player_mut(PlayerId(0)).unwrap().mana_pool.white += 4;

      let gs_ref = &gs;
      let obj_ref = gs_ref.objects.get(&id).unwrap();
      let actions = card_actions(gs_ref, PlayerId(0), obj_ref);
      let labels: Vec<&str> = actions.iter().map(|a| a.label.as_str()).collect();
      assert!(labels.iter().any(|l| l.contains("Kick")), "expected a Kicked action, got: {labels:?}");
      assert!(labels.iter().any(|l| !l.contains("Kick") && l.contains("Cast")), "expected a Standard action");
  }
  ```

  Note: `card_actions` is currently `fn card_actions(state: &GameState, pid: PlayerId, obj: &CardObject) -> Vec<ActionItemView>`. Check if it is `pub(crate)` or needs to be made accessible to the test. If it's private, make it `pub(crate)`.

- [ ] **Step 2: Run tests to confirm they fail**

  ```bash
  cargo test serve 2>&1 | grep -E "^test result|FAILED|error\["
  ```

  Expected: fail (no Dash/Kicked actions generated yet).

- [ ] **Step 3: Add per-mode action generation in `src/serve.rs`**

  After the standard "Cast spell" block (untargeted + targeted, ending around line 607), add a new block for Dash/Kicker/Multikicker/Evoke modes. This block runs whenever `can_cast_structural` is true:

  ```rust
  // Per-mode actions for Dash, Kicker, Multikicker, Evoke.
  if !is_land && !is_aura && can_cast_structural(state, pid, obj) {
      let std_cost_label = obj
          .definition
          .mana_cost
          .as_ref()
          .map(format_mana_cost_braced)
          .unwrap_or_default();

      // Dash: generate a Dashed alternative action.
      if let Some(dash_cost) = obj.definition.rules_text.iter().find_map(|span| {
          if let RulesText::Active(Rule::Dash { alternative_cost }) = span {
              Some(alternative_cost)
          } else {
              None
          }
      }) {
          let dash_label = format_mana_cost_braced(dash_cost);
          let cast_mode_val = serde_json::to_value(CastMode::Dashed).unwrap();
          actions.push(ActionItemView {
              label: format!("Cast {} (Dash {})", obj.definition.name, dash_label),
              kind: ActionItemKind::Server {
                  action: serde_json::json!({
                      "type": "cast_spell",
                      "object_id": obj.id.0,
                      "cast_mode": cast_mode_val,
                      "cost_label": dash_label,
                  }),
              },
          });
      }

      // Kicker: generate a Kicked action with combined cost label.
      if let Some(kicker_cost) = obj.definition.rules_text.iter().find_map(|span| {
          if let RulesText::Active(Rule::Kicker { additional_cost }) = span {
              Some(additional_cost)
          } else {
              None
          }
      }) {
          let kicker_label = format_mana_cost_braced(kicker_cost);
          let combined_label = format!("{} + {}", std_cost_label, kicker_label);
          let cast_mode_val = serde_json::to_value(CastMode::Kicked).unwrap();
          actions.push(ActionItemView {
              label: format!("Cast {} (Kicked {})", obj.definition.name, combined_label),
              kind: ActionItemKind::Server {
                  action: serde_json::json!({
                      "type": "cast_spell",
                      "object_id": obj.id.0,
                      "cast_mode": cast_mode_val,
                      "cost_label": combined_label,
                  }),
              },
          });
      }

      // Multikicker: generate a Multikicked(1) action (once — simple first pass).
      if let Some(kicker_cost) = obj.definition.rules_text.iter().find_map(|span| {
          if let RulesText::Active(Rule::Multikicker { additional_cost }) = span {
              Some(additional_cost)
          } else {
              None
          }
      }) {
          let kicker_label = format_mana_cost_braced(kicker_cost);
          let combined_label = format!("{} + {}", std_cost_label, kicker_label);
          let cast_mode_val = serde_json::to_value(CastMode::Multikicked(1)).unwrap();
          actions.push(ActionItemView {
              label: format!("Cast {} (Multikick ×1 {})", obj.definition.name, combined_label),
              kind: ActionItemKind::Server {
                  action: serde_json::json!({
                      "type": "cast_spell",
                      "object_id": obj.id.0,
                      "cast_mode": cast_mode_val,
                      "cost_label": combined_label,
                  }),
              },
          });
      }

      // Evoke: generate an Evoked alternative action.
      if let Some(evoke_cost) = obj.definition.rules_text.iter().find_map(|span| {
          if let RulesText::Active(Rule::Evoke { alternative_cost }) = span {
              Some(alternative_cost)
          } else {
              None
          }
      }) {
          let evoke_label = format_mana_cost_braced(evoke_cost);
          let cast_mode_val = serde_json::to_value(CastMode::Evoked).unwrap();
          actions.push(ActionItemView {
              label: format!("Cast {} (Evoke {})", obj.definition.name, evoke_label),
              kind: ActionItemKind::Server {
                  action: serde_json::json!({
                      "type": "cast_spell",
                      "object_id": obj.id.0,
                      "cast_mode": cast_mode_val,
                      "cost_label": evoke_label,
                  }),
              },
          });
      }
  }
  ```

  Add `CastMode` to the `use mecha_oracle::types::ability::{ ... }` import at the top of `serve.rs`:
  ```rust
  use mecha_oracle::types::ability::{
      ActivatedAbility, AnnotationKind, CastMode, CostComponent, KeywordAbility, Rule, RulesText,
      TextAnnotation,
  };
  ```

- [ ] **Step 4: Run tests**

  ```bash
  cargo test 2>&1 | grep -E "^test result|FAILED|error\["
  ```

  Expected: all tests pass.

- [ ] **Step 5: Run clippy**

  ```bash
  cargo clippy --all-targets 2>&1 | grep -E "^error|^warning"
  ```

  Fix any unused variable warnings (e.g. `cast_mode_val` shadowing). Use `let _ = ...` only if truly unused.

- [ ] **Step 6: Commit**

  ```bash
  git add src/serve.rs
  git commit -m "feat: serve.rs generates per-mode cast actions for Dash, Kicker, Multikicker, Evoke"
  ```

---

### Task 8: Update docs/todo.md — mark Kicker, Dash, Evoke complete

**Files:**
- Modify: `docs/todo.md`

- [ ] **Step 1: Remove the completed items from `docs/todo.md`**

  In the `## 🃏 Alternative casting block` section, remove:
  - `- **Kicker [cost]** (702.33): ...`
  - `- **Dash [cost]** (702.109): ...`
  - `- **Evoke [cost]** (702.74): ...`

  Leave Morph, Bestow, Emerge, Mutate (still out of scope).

  Also add "Multikicker" as explicitly done if it's listed — it is part of Kicker's entry.

- [ ] **Step 2: Final test suite and clippy pass**

  ```bash
  cargo test 2>&1 | grep -E "^test result|FAILED|error\["
  cargo clippy --all-targets 2>&1 | grep -E "^error|^warning"
  ```

  Expected: clean.

- [ ] **Step 3: Commit**

  ```bash
  git add docs/todo.md
  git commit -m "docs: mark Kicker, Multikicker, Dash, Evoke as implemented in todo.md"
  ```
