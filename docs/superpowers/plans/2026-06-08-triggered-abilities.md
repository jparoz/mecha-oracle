# Triggered Abilities Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Parse `When/Whenever this enters…` ETB triggered abilities from oracle text and fire their effects (draw N cards, gain N life) immediately when permanents enter the battlefield.

**Architecture:** `EffectStep`/`Effect` consolidate into `effect.rs` (canonical effect types shared between abilities and future spells). `parse_oracle_text` gains a `card_name` parameter and a new ETB trigger check. A new `engine/triggered.rs` module fires triggers immediately on ETB; `cast_creature` and `play_land` call it after placing the permanent.

**Tech Stack:** Rust, existing `engine/`, `parser/`, `types/` modules. Tests: `cargo test 2>&1 | grep -E "^test result|FAILED|error\["`.

---

## File Map

| File | Action | Responsibility |
|---|---|---|
| `src/types/effect.rs` | Modify | Canonical `EffectStep` enum + `Effect` type alias; replaces dead `Effect` enum |
| `src/types/ability.rs` | Modify | Remove `EffectStep`/`AbilityEffect`; expand `TriggerEvent`/`TriggeredAbility` |
| `src/types/mod.rs` | Modify | Update re-exports |
| `src/parser/oracle.rs` | Modify | `try_parse_etb_trigger`; `GainLife` pattern; thread `card_name` |
| `src/engine/triggered.rs` | Create | `fire_etb_triggers` function |
| `src/engine/mod.rs` | Modify | `pub mod triggered` |
| `src/engine/activated.rs` | Modify | Update import; add `GainLife` no-op arm |
| `src/engine/casting.rs` | Modify | Call `fire_etb_triggers` after ETB placement |
| `src/serve.rs` | Modify | `format_triggered_ability`; `Triggered` span arm; `GainLife` label arm |
| `tests/fixtures/oracle_cards_test.json` | Modify | Add ETB creature entry |

---

## Task 1: Consolidate effect types + expand TriggeredAbility

Structural refactor — no behaviour change. Goal: move `EffectStep`/`Effect` to `effect.rs`, expand `TriggerEvent`/`TriggeredAbility`, make everything compile, all existing tests pass.

**Files:** `src/types/effect.rs`, `src/types/ability.rs`, `src/types/mod.rs`, `src/engine/activated.rs`, `src/parser/oracle.rs`, `src/serve.rs`

- [ ] **Step 1: Rewrite `src/types/effect.rs`**

Replace the entire file:

```rust
use super::ids::{ObjectId, PlayerId};
use super::mana::ManaPool;

#[derive(Debug, Clone)]
pub enum EffectTarget {
    Player(PlayerId),
    Object(ObjectId),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EffectStep {
    AddMana(ManaPool),
    Mill(u32),
    DrawCard(u32),
    GainLife(u32),
}

pub type Effect = Vec<EffectStep>;
```

- [ ] **Step 2: Update `src/types/ability.rs`**

At the top, replace `use super::mana::{ManaCost, ManaPool};` with:
```rust
use super::effect::Effect;
use super::mana::ManaCost;
```

Remove these items entirely (search and delete):
- `pub enum EffectStep { ... }` (the whole enum block, lines ~53–57)
- `pub type AbilityEffect = Vec<EffectStep>;` (the type alias)

Update `ActivatedAbility` — change `effect: AbilityEffect` to `effect: Effect`:
```rust
pub struct ActivatedAbility {
    pub cost: ActivationCost,
    pub effect: Effect,
}
```

Replace the stub `TriggerEvent` and `TriggeredAbility` with:
```rust
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TriggerEvent {
    EntersTheBattlefield { subject_is_self: bool },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TriggeredAbility {
    pub trigger: TriggerEvent,
    pub effect: Effect,
}
```

In the test module inside `ability.rs`, fix the `activated_ability_construction` test's import. Replace `use super::super::mana::ManaPool;` with:
```rust
use crate::types::effect::EffectStep;
use super::super::mana::ManaPool;
```
And update the test to construct `EffectStep::AddMana(...)` (the type is still the same struct, just imported differently — no logic change).

- [ ] **Step 3: Update `src/types/mod.rs`**

Change the re-export block to:
```rust
pub use ability::{
    AbilityAST, ActivatedAbility, ActivationCost, CardFilter, CostComponent,
    IgnoredKind, OracleSpan, PermanentFilter, StaticAbility, TriggerEvent,
    TriggeredAbility,
};
pub use effect::{Effect, EffectStep, EffectTarget};
```

(`AbilityEffect` and the old `EffectStep` from ability are removed; `EffectStep` and `Effect` now come from effect.)

- [ ] **Step 4: Update `src/engine/activated.rs` — import + GainLife arm**

Change the import on line 5 from:
```rust
use crate::types::ability::{AbilityAST, ActivatedAbility, CostComponent, EffectStep, OracleSpan};
```
to:
```rust
use crate::types::ability::{AbilityAST, ActivatedAbility, CostComponent, OracleSpan};
use crate::types::effect::EffectStep;
```

In the effect application loop (around line 130), the `match step { ... }` block will fail to compile because `GainLife` is now a variant. Add the arm:
```rust
EffectStep::GainLife(_) => {
    debug_assert!(false, "GainLife not expected in activated ability effect");
}
```

In the test module inside `activated.rs`, fix the `EffectStep` import — replace:
```rust
use crate::types::ability::{AbilityAST, ActivatedAbility, CostComponent, EffectStep};
```
with:
```rust
use crate::types::ability::{AbilityAST, ActivatedAbility, CostComponent};
use crate::types::effect::EffectStep;
```

- [ ] **Step 5: Update `src/parser/oracle.rs` — import**

Line 2 currently imports `AbilityEffect` and `EffectStep` from ability:
```rust
use crate::types::ability::{AbilityEffect, ActivationCost, CostComponent, EffectStep};
```
Replace with:
```rust
use crate::types::ability::{ActivationCost, CostComponent};
use crate::types::effect::{Effect, EffectStep};
```

Change the return type of `parse_ability_effect` from `Option<AbilityEffect>` to `Option<Effect>`:
```rust
fn parse_ability_effect(s: &str) -> Option<Effect> {
```

In the test module, update the `EffectStep` import wherever it appears (search for `use crate::types::ability::EffectStep` and change to `use crate::types::effect::EffectStep`).

- [ ] **Step 6: Update `src/serve.rs` — import + GainLife arm**

Line 14 currently imports `EffectStep` from ability:
```rust
AbilityAST, ActivatedAbility, CostComponent, EffectStep, OracleSpan,
```
Remove `EffectStep` from that line. Add a separate import:
```rust
use mecha_oracle::types::effect::EffectStep;
```

In `format_activated_label` (the function that builds the label string for activated abilities, around line 292–307), the `match e { ... }` block will fail to compile because `GainLife` is now a variant. Add:
```rust
EffectStep::GainLife(n) => format!("Gain {n} life"),
```

- [ ] **Step 7: Run tests — all existing tests must pass**

```bash
cargo test 2>&1 | grep -E "^test result|FAILED|error\["
```

Expected: `test result: ok. N passed; 0 failed` with no `error[` lines.

If there are compile errors, fix them before moving on — they will be import mismatches or missing match arms.

- [ ] **Step 8: Commit**

```bash
git add src/types/effect.rs src/types/ability.rs src/types/mod.rs \
        src/engine/activated.rs src/parser/oracle.rs src/serve.rs
git commit -m "refactor: consolidate EffectStep/Effect into effect.rs; expand TriggeredAbility"
```

---

## Task 2: Add GainLife effect parsing

**Files:** `src/parser/oracle.rs`

- [ ] **Step 1: Write the failing test**

Add to the `#[cfg(test)]` block inside `src/parser/oracle.rs`:

```rust
#[test]
fn parse_ability_effect_gain_life() {
    use crate::types::effect::EffectStep;
    assert_eq!(
        super::parse_ability_effect("You gain 3 life."),
        Some(vec![EffectStep::GainLife(3)])
    );
    assert_eq!(
        super::parse_ability_effect("gain 1 life."),
        Some(vec![EffectStep::GainLife(1)])
    );
    assert_eq!(
        super::parse_ability_effect("you gain two life."),
        Some(vec![EffectStep::GainLife(2)])
    );
}
```

- [ ] **Step 2: Run to confirm it fails**

```bash
cargo test parse_ability_effect_gain_life 2>&1 | grep -E "^test result|FAILED|error\["
```

Expected: `FAILED` (not a compile error — the test should compile but return `None` instead of `Some`).

- [ ] **Step 3: Add GainLife pattern to `try_parse_effect_step`**

In `try_parse_effect_step`, after the existing Mill pattern, add:

```rust
let stripped = lower
    .strip_prefix("you gain ")
    .or_else(|| lower.strip_prefix("gain "));
if let Some(rest) = stripped {
    let s = rest.trim_end_matches(" life").trim();
    if let Some(n) = parse_number_word(s) {
        return Some(EffectStep::GainLife(n));
    }
}
```

- [ ] **Step 4: Run to confirm it passes**

```bash
cargo test parse_ability_effect_gain_life 2>&1 | grep -E "^test result|FAILED|error\["
```

Expected: `test result: ok. 1 passed; 0 failed`

- [ ] **Step 5: Commit**

```bash
git add src/parser/oracle.rs
git commit -m "feat: parse 'you gain N life' as EffectStep::GainLife(N)"
```

---

## Task 3: ETB trigger parser

**Files:** `src/parser/oracle.rs`, `src/cards/scryfall.rs`

- [ ] **Step 1: Write the failing tests**

Add to the test module in `src/parser/oracle.rs`. Note: after this task, `parse_oracle_text` takes a second `card_name: &str` argument; write the tests using the new signature so they compile once the implementation is in place.

```rust
#[test]
fn etb_self_draw_parses_as_triggered() {
    use crate::types::ability::{AbilityAST, TriggerEvent, TriggeredAbility};
    use crate::types::effect::EffectStep;
    let result = parse_oracle_text("When this enters, draw a card.", "");
    assert_eq!(result.len(), 1);
    assert!(matches!(
        &result[0],
        OracleSpan::Parsed(AbilityAST::Triggered(TriggeredAbility {
            trigger: TriggerEvent::EntersTheBattlefield { subject_is_self: true },
            effect,
        })) if effect == &vec![EffectStep::DrawCard(1)]
    ));
}

#[test]
fn etb_creature_form_parses_as_triggered() {
    use crate::types::ability::{AbilityAST, TriggerEvent, TriggeredAbility};
    use crate::types::effect::EffectStep;
    // Older template: "When this creature enters"
    let result = parse_oracle_text("When this creature enters, draw a card.", "");
    assert_eq!(result.len(), 1);
    assert!(matches!(
        &result[0],
        OracleSpan::Parsed(AbilityAST::Triggered(TriggeredAbility {
            trigger: TriggerEvent::EntersTheBattlefield { subject_is_self: true },
            effect,
        })) if effect == &vec![EffectStep::DrawCard(1)]
    ));
}

#[test]
fn etb_battlefield_form_parses_as_triggered() {
    use crate::types::ability::{AbilityAST, TriggerEvent, TriggeredAbility};
    use crate::types::effect::EffectStep;
    let result = parse_oracle_text(
        "Whenever this enters the battlefield, you gain 3 life.",
        "",
    );
    assert_eq!(result.len(), 1);
    assert!(matches!(
        &result[0],
        OracleSpan::Parsed(AbilityAST::Triggered(TriggeredAbility {
            trigger: TriggerEvent::EntersTheBattlefield { subject_is_self: true },
            effect,
        })) if effect == &vec![EffectStep::GainLife(3)]
    ));
}

#[test]
fn etb_card_name_subject_parses_as_triggered() {
    use crate::types::ability::{AbilityAST, TriggerEvent, TriggeredAbility};
    use crate::types::effect::EffectStep;
    let result = parse_oracle_text(
        "When Elvish Visionary enters the battlefield, draw a card.",
        "Elvish Visionary",
    );
    assert_eq!(result.len(), 1);
    assert!(matches!(
        &result[0],
        OracleSpan::Parsed(AbilityAST::Triggered(TriggeredAbility {
            trigger: TriggerEvent::EntersTheBattlefield { subject_is_self: true },
            effect,
        })) if effect == &vec![EffectStep::DrawCard(1)]
    ));
}

#[test]
fn etb_multistep_effect_parses_as_triggered() {
    use crate::types::ability::{AbilityAST, TriggeredAbility};
    use crate::types::effect::EffectStep;
    let result = parse_oracle_text("When this enters, draw a card. You gain 2 life.", "");
    assert_eq!(result.len(), 1);
    assert!(matches!(
        &result[0],
        OracleSpan::Parsed(AbilityAST::Triggered(TriggeredAbility { effect, .. }))
        if effect == &vec![EffectStep::DrawCard(1), EffectStep::GainLife(2)]
    ));
}

#[test]
fn etb_unknown_effect_becomes_parsed_unimplemented() {
    let result = parse_oracle_text("When this enters, create a 1/1 token.", "");
    assert_eq!(result.len(), 1);
    assert!(matches!(&result[0], OracleSpan::ParsedUnimplemented(_)));
}
```

- [ ] **Step 2: Run to confirm they fail (compile error expected)**

```bash
cargo test etb_ 2>&1 | grep -E "^test result|FAILED|error\["
```

Expected: compile error — `parse_oracle_text` doesn't yet accept a second argument.

- [ ] **Step 3: Add `try_parse_etb_trigger` to `src/parser/oracle.rs`**

Add the following private function. Place it just before `parse_oracle_text`:

```rust
fn try_parse_etb_trigger(paragraph: &str, card_name: &str) -> Option<OracleSpan> {
    use crate::types::ability::{AbilityAST, TriggerEvent, TriggeredAbility};

    // Strip "When " or "Whenever " prefix (case-insensitive).
    let lower = paragraph.to_lowercase();
    let rest: &str = if lower.starts_with("when ") {
        &paragraph[5..]
    } else if lower.starts_with("whenever ") {
        &paragraph[9..]
    } else {
        return None;
    };
    let rest = rest.trim_start();
    let rest_lower = rest.to_lowercase();

    // Match subject: "this", "this <type>" (e.g. "this creature"), or card_name.
    let after_subject: &str =
        if rest_lower.starts_with("this") && (rest.len() == 4 || rest.as_bytes().get(4) == Some(&b' ')) {
            // Skip "this" (4 bytes) and optional one-word type
            let after_this = rest[4..].trim_start();
            if after_this.to_lowercase().starts_with("enters") {
                after_this
            } else {
                // Skip one type word (e.g. "creature", "permanent")
                let word_end = after_this.find(' ').unwrap_or(after_this.len());
                after_this[word_end..].trim_start()
            }
        } else if !card_name.is_empty() && rest_lower.starts_with(&card_name.to_lowercase()) {
            rest[card_name.len()..].trim_start()
        } else {
            return None;
        };

    // Expect "enters" optionally followed by "the battlefield".
    let after_enters: &str = {
        let al = after_subject.to_lowercase();
        if al.starts_with("enters the battlefield") {
            &after_subject["enters the battlefield".len()..]
        } else if al.starts_with("enters") {
            &after_subject["enters".len()..]
        } else {
            return None;
        }
    };

    // Find the comma separating trigger clause from effect clause.
    let comma_pos = find_at_depth_zero(after_enters, ',')?;
    let effect_str = after_enters[comma_pos + 1..].trim();

    match parse_ability_effect(effect_str) {
        Some(effect) => Some(OracleSpan::Parsed(AbilityAST::Triggered(TriggeredAbility {
            trigger: TriggerEvent::EntersTheBattlefield { subject_is_self: true },
            effect,
        }))),
        None => Some(OracleSpan::ParsedUnimplemented(paragraph.to_string())),
    }
}
```

- [ ] **Step 4: Update `parse_oracle_text` signature and insert ETB check**

Change the function signature from:
```rust
pub fn parse_oracle_text(text: &str) -> Vec<OracleSpan> {
```
to:
```rust
pub fn parse_oracle_text(text: &str, card_name: &str) -> Vec<OracleSpan> {
```

Inside the per-paragraph loop, after the colon check's `continue` and before the comma-split, add:

```rust
// ETB trigger check: "When/Whenever this enters…" or "When <CardName> enters…"
if let Some(span) = try_parse_etb_trigger(paragraph, card_name) {
    spans.push(span);
    continue;
}
```

- [ ] **Step 5: Update the production call site in `src/cards/scryfall.rs`**

Line 49:
```rust
let abilities = parse_oracle_text(&oracle_text);
```
→
```rust
let abilities = parse_oracle_text(&oracle_text, &name);
```

(`name` is already bound above at line 33.)

- [ ] **Step 6: Update ALL test call sites in `src/parser/oracle.rs`**

Every call to `parse_oracle_text` inside the `#[cfg(test)]` module must gain `""` as the second argument (tests don't test card-name matching except the dedicated `etb_card_name_subject_parses_as_triggered` test). There are ~30 such calls. Update them all:

```rust
// before:
parse_oracle_text("Flying")
// after:
parse_oracle_text("Flying", "")
```

Also replace the outdated test `triggered_ability_becomes_unparsed` entirely. The old assertion was that `"When this creature enters, draw a card."` emits two `Unparsed` spans; this is now wrong. Delete that test — it is superseded by `etb_creature_form_parses_as_triggered` added in Step 1.

- [ ] **Step 7: Run to confirm all tests pass**

```bash
cargo test 2>&1 | grep -E "^test result|FAILED|error\["
```

Expected: all pass. If any etb_ tests still fail, re-check the `try_parse_etb_trigger` logic.

- [ ] **Step 8: Commit**

```bash
git add src/parser/oracle.rs src/cards/scryfall.rs
git commit -m "feat: parse ETB triggered abilities (When/Whenever this enters)"
```

---

## Task 4: fire_etb_triggers engine module

**Files:** `src/engine/triggered.rs` (create), `src/engine/mod.rs`

- [ ] **Step 1: Write the failing tests**

Create `src/engine/triggered.rs` with only the test module first:

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::ability::{AbilityAST, TriggerEvent, TriggeredAbility};
    use crate::types::card::{CardDefinition, CardType, TypeLine};
    use crate::types::effect::EffectStep;
    use crate::types::mana::ManaCost;
    use crate::types::{CardObject, GameState, ObjectId, OracleSpan, Player, PlayerId, Zone};

    fn two_player_state() -> GameState {
        GameState::new(vec![
            Player::new(PlayerId(0), "Alice"),
            Player::new(PlayerId(1), "Bob"),
        ])
    }

    fn place_on_battlefield(state: &mut GameState, def: CardDefinition, owner: PlayerId) -> ObjectId {
        let id = state.alloc_id();
        let obj = CardObject::new(id, def, owner, Zone::Battlefield);
        state.battlefield.push(id);
        state.add_object(obj);
        id
    }

    fn put_in_library(state: &mut GameState, owner: PlayerId) -> ObjectId {
        let def = CardDefinition {
            name: "Dummy".into(),
            mana_cost: None,
            type_line: TypeLine {
                supertypes: vec![],
                card_types: vec![CardType::Creature],
                subtypes: vec![],
            },
            oracle_text: String::new(),
            abilities: vec![],
            power: Some(1),
            toughness: Some(1),
        };
        let id = state.alloc_id();
        let obj = CardObject::new(id, def, owner, Zone::Library);
        state.libraries.get_mut(&owner).unwrap().push(id);
        state.add_object(obj);
        id
    }

    fn etb_draw_def() -> CardDefinition {
        CardDefinition {
            name: "Elvish Visionary".into(),
            mana_cost: Some(ManaCost { pips: vec![] }),
            type_line: TypeLine {
                supertypes: vec![],
                card_types: vec![CardType::Creature],
                subtypes: vec!["Elf".into(), "Scout".into()],
            },
            oracle_text: "When this enters, draw a card.".into(),
            abilities: vec![OracleSpan::Parsed(AbilityAST::Triggered(TriggeredAbility {
                trigger: TriggerEvent::EntersTheBattlefield { subject_is_self: true },
                effect: vec![EffectStep::DrawCard(1)],
            }))],
            power: Some(1),
            toughness: Some(1),
        }
    }

    fn etb_gain_life_def() -> CardDefinition {
        CardDefinition {
            name: "Pelakka Wurm".into(),
            mana_cost: Some(ManaCost { pips: vec![] }),
            type_line: TypeLine {
                supertypes: vec![],
                card_types: vec![CardType::Creature],
                subtypes: vec!["Wurm".into()],
            },
            oracle_text: "When this enters, you gain 7 life.".into(),
            abilities: vec![OracleSpan::Parsed(AbilityAST::Triggered(TriggeredAbility {
                trigger: TriggerEvent::EntersTheBattlefield { subject_is_self: true },
                effect: vec![EffectStep::GainLife(7)],
            }))],
            power: Some(7),
            toughness: Some(7),
        }
    }

    #[test]
    fn etb_draw_trigger_draws_card_for_controller() {
        let mut gs = two_player_state();
        let library_card = put_in_library(&mut gs, PlayerId(0));
        let creature_id = place_on_battlefield(&mut gs, etb_draw_def(), PlayerId(0));

        let gs = fire_etb_triggers(gs, creature_id);

        assert!(gs.hands[&PlayerId(0)].contains(&library_card));
        assert!(gs.libraries[&PlayerId(0)].is_empty());
        // Opponent's hand unchanged
        assert!(gs.hands[&PlayerId(1)].is_empty());
    }

    #[test]
    fn etb_gain_life_trigger_increases_controller_life() {
        let mut gs = two_player_state();
        let creature_id = place_on_battlefield(&mut gs, etb_gain_life_def(), PlayerId(0));
        let before = gs.get_player(PlayerId(0)).unwrap().life;

        let gs = fire_etb_triggers(gs, creature_id);

        assert_eq!(gs.get_player(PlayerId(0)).unwrap().life, before + 7);
        // Opponent's life unchanged
        assert_eq!(gs.get_player(PlayerId(1)).unwrap().life, before);
    }

    #[test]
    fn etb_multistep_effect_applies_all_steps() {
        let def = CardDefinition {
            name: "Multi".into(),
            mana_cost: None,
            type_line: TypeLine {
                supertypes: vec![],
                card_types: vec![CardType::Creature],
                subtypes: vec![],
            },
            oracle_text: "When this enters, draw a card. You gain 2 life.".into(),
            abilities: vec![OracleSpan::Parsed(AbilityAST::Triggered(TriggeredAbility {
                trigger: TriggerEvent::EntersTheBattlefield { subject_is_self: true },
                effect: vec![EffectStep::DrawCard(1), EffectStep::GainLife(2)],
            }))],
            power: Some(1),
            toughness: Some(1),
        };
        let mut gs = two_player_state();
        put_in_library(&mut gs, PlayerId(0));
        let creature_id = place_on_battlefield(&mut gs, def, PlayerId(0));
        let before_life = gs.get_player(PlayerId(0)).unwrap().life;

        let gs = fire_etb_triggers(gs, creature_id);

        assert_eq!(gs.hands[&PlayerId(0)].len(), 1);
        assert_eq!(gs.get_player(PlayerId(0)).unwrap().life, before_life + 2);
    }

    #[test]
    fn no_triggered_abilities_returns_state_unchanged() {
        let def = CardDefinition {
            name: "Vanilla".into(),
            mana_cost: None,
            type_line: TypeLine {
                supertypes: vec![],
                card_types: vec![CardType::Creature],
                subtypes: vec![],
            },
            oracle_text: String::new(),
            abilities: vec![],
            power: Some(2),
            toughness: Some(2),
        };
        let mut gs = two_player_state();
        let creature_id = place_on_battlefield(&mut gs, def, PlayerId(0));
        let before_life = gs.get_player(PlayerId(0)).unwrap().life;

        let gs = fire_etb_triggers(gs, creature_id);

        assert_eq!(gs.get_player(PlayerId(0)).unwrap().life, before_life);
        assert!(gs.hands[&PlayerId(0)].is_empty());
    }
}
```

- [ ] **Step 2: Add `pub mod triggered` to `src/engine/mod.rs`**

```rust
pub mod triggered;
```

(Add alongside the existing `pub mod activated;` etc.)

- [ ] **Step 3: Run to confirm tests fail**

```bash
cargo test engine::triggered 2>&1 | grep -E "^test result|FAILED|error\["
```

Expected: compile error — `fire_etb_triggers` is not defined yet.

- [ ] **Step 4: Implement `fire_etb_triggers`**

Add the function body to `src/engine/triggered.rs`, above the test module:

```rust
use crate::engine::turn::draw_card;
use crate::types::ability::{AbilityAST, TriggerEvent};
use crate::types::effect::EffectStep;
use crate::types::{GameState, ObjectId, OracleSpan};

// CR 603.2: triggered abilities trigger when their trigger event occurs.
// Phase C fires ETB triggers immediately (fire-and-forget, no stack).
// Stack project: replace this body with "collect onto stack"; signature stays.
pub fn fire_etb_triggers(mut state: GameState, entering_id: ObjectId) -> GameState {
    let (controller, effects): (_, Vec<_>) = {
        let obj = match state.objects.get(&entering_id) {
            Some(o) => o,
            None => return state,
        };
        let triggered = obj
            .definition
            .abilities
            .iter()
            .filter_map(|span| match span {
                OracleSpan::Parsed(AbilityAST::Triggered(t))
                    if matches!(
                        t.trigger,
                        TriggerEvent::EntersTheBattlefield { subject_is_self: true }
                    ) =>
                {
                    Some(t.effect.clone())
                }
                _ => None,
            })
            .collect();
        (obj.controller, triggered)
    };

    for effect in effects {
        for step in &effect {
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
                _ => {
                    debug_assert!(false, "unexpected EffectStep in ETB trigger: {step:?}");
                }
            }
        }
    }

    state
}
```

- [ ] **Step 5: Run to confirm tests pass**

```bash
cargo test engine::triggered 2>&1 | grep -E "^test result|FAILED|error\["
```

Expected: `test result: ok. 4 passed; 0 failed`

- [ ] **Step 6: Commit**

```bash
git add src/engine/triggered.rs src/engine/mod.rs
git commit -m "feat: add fire_etb_triggers — immediate ETB trigger dispatch"
```

---

## Task 5: Wire ETB call sites

**Files:** `src/engine/casting.rs`

- [ ] **Step 1: Write the failing integration test**

Add to the `#[cfg(test)]` block in `src/engine/casting.rs`:

```rust
#[test]
fn cast_creature_fires_etb_draw_trigger() {
    use crate::types::ability::{AbilityAST, TriggerEvent, TriggeredAbility};
    use crate::types::card::{CardType, TypeLine};
    use crate::types::effect::EffectStep;
    use crate::types::mana::{ManaCost, ManaPip};

    let mut gs = make_state();
    gs.get_player_mut(PlayerId(0)).unwrap().mana_pool.green = 1;

    // Put a card in library so the draw trigger has something to draw.
    let library_card = {
        let def = crate::types::CardDefinition {
            name: "Dummy".into(),
            mana_cost: None,
            type_line: TypeLine {
                supertypes: vec![],
                card_types: vec![CardType::Creature],
                subtypes: vec![],
            },
            oracle_text: String::new(),
            abilities: vec![],
            power: Some(1),
            toughness: Some(1),
        };
        let id = gs.alloc_id();
        let obj = CardObject::new(id, def, PlayerId(0), Zone::Library);
        gs.libraries.get_mut(&PlayerId(0)).unwrap().push(id);
        gs.add_object(obj);
        id
    };

    let def = crate::types::CardDefinition {
        name: "Elvish Visionary".into(),
        mana_cost: Some(ManaCost { pips: vec![ManaPip::Green] }),
        type_line: TypeLine {
            supertypes: vec![],
            card_types: vec![CardType::Creature],
            subtypes: vec!["Elf".into(), "Scout".into()],
        },
        oracle_text: "When this enters, draw a card.".into(),
        abilities: vec![OracleSpan::Parsed(AbilityAST::Triggered(TriggeredAbility {
            trigger: TriggerEvent::EntersTheBattlefield { subject_is_self: true },
            effect: vec![EffectStep::DrawCard(1)],
        }))],
        power: Some(1),
        toughness: Some(1),
    };
    let creature_id = put_in_hand(&mut gs, PlayerId(0), def);

    let gs = cast_creature(gs, PlayerId(0), creature_id).unwrap();

    // Creature on battlefield.
    assert!(gs.battlefield.contains(&creature_id));
    // Draw trigger fired: library card now in hand.
    assert!(gs.hands[&PlayerId(0)].contains(&library_card));
    assert!(gs.libraries[&PlayerId(0)].is_empty());
}

#[test]
fn cast_creature_fires_etb_gain_life_trigger() {
    use crate::types::ability::{AbilityAST, TriggerEvent, TriggeredAbility};
    use crate::types::card::{CardType, TypeLine};
    use crate::types::effect::EffectStep;
    use crate::types::mana::{ManaCost, ManaPip};

    let mut gs = make_state();
    gs.get_player_mut(PlayerId(0)).unwrap().mana_pool.green = 2;
    let before_life = gs.get_player(PlayerId(0)).unwrap().life;

    let def = crate::types::CardDefinition {
        name: "Thragtusk".into(),
        mana_cost: Some(ManaCost { pips: vec![ManaPip::Generic(4), ManaPip::Green] }),
        type_line: TypeLine {
            supertypes: vec![],
            card_types: vec![CardType::Creature],
            subtypes: vec!["Beast".into()],
        },
        oracle_text: "When this enters, you gain 5 life.".into(),
        abilities: vec![OracleSpan::Parsed(AbilityAST::Triggered(TriggeredAbility {
            trigger: TriggerEvent::EntersTheBattlefield { subject_is_self: true },
            effect: vec![EffectStep::GainLife(5)],
        }))],
        power: Some(5),
        toughness: Some(3),
    };
    let creature_id = put_in_hand(&mut gs, PlayerId(0), def);

    // Give enough mana for {4}{G} — 5 total, but we only check that it passes.
    gs.get_player_mut(PlayerId(0)).unwrap().mana_pool.colorless = 4;

    let gs = cast_creature(gs, PlayerId(0), creature_id).unwrap();

    assert_eq!(
        gs.get_player(PlayerId(0)).unwrap().life,
        before_life + 5
    );
}
```

- [ ] **Step 2: Run to confirm tests fail**

```bash
cargo test cast_creature_fires_etb 2>&1 | grep -E "^test result|FAILED|error\["
```

Expected: compile OK, tests FAIL (trigger not yet wired — state doesn't show the draw/life gain).

- [ ] **Step 3: Wire `fire_etb_triggers` into `cast_creature`**

In `src/engine/casting.rs`, add the import at the top:
```rust
use super::triggered::fire_etb_triggers;
```

In `cast_creature`, replace the final `Ok(check_and_apply_sbas(state))` with:
```rust
let state = fire_etb_triggers(state, object_id);
Ok(check_and_apply_sbas(state))
```

- [ ] **Step 4: Wire `fire_etb_triggers` into `play_land`**

In `play_land`, replace `Ok(check_and_apply_sbas(state))` with:
```rust
let state = fire_etb_triggers(state, object_id);
Ok(check_and_apply_sbas(state))
```

(Lands rarely have ETB triggers but the hook must be consistent per CR 603.2.)

- [ ] **Step 5: Run full test suite**

```bash
cargo test 2>&1 | grep -E "^test result|FAILED|error\["
```

Expected: all pass.

- [ ] **Step 6: Commit**

```bash
git add src/engine/casting.rs
git commit -m "feat: wire fire_etb_triggers into cast_creature and play_land"
```

---

## Task 6: Triggered span rendering in serve.rs

**Files:** `src/serve.rs`

- [ ] **Step 1: Add `format_triggered_ability` helper**

Add this function near the existing `format_activated_label` in `src/serve.rs`:

```rust
fn format_triggered_ability(t: &mecha_oracle::types::ability::TriggeredAbility) -> String {
    use mecha_oracle::types::ability::TriggerEvent;
    let trigger_str = match &t.trigger {
        TriggerEvent::EntersTheBattlefield { .. } => "When this enters",
    };
    let effect_parts: Vec<String> = t
        .effect
        .iter()
        .map(|e| match e {
            EffectStep::DrawCard(1) => "draw a card".to_string(),
            EffectStep::DrawCard(n) => format!("draw {n} cards"),
            EffectStep::GainLife(n) => format!("you gain {n} life"),
            EffectStep::AddMana(pool) => format!("add {}", format_mana_pool(pool)),
            EffectStep::Mill(n) => format!("mill {n}"),
        })
        .collect();
    format!("{}, {}.", trigger_str, effect_parts.join(". "))
}
```

- [ ] **Step 2: Add `Parsed(Triggered(_))` arm to the span match**

In `build_player_view` (or wherever the `OracleSpan → OracleSpanView` match lives, around line 330), the `_ =>` fallback currently emits debug text for any unhandled `Parsed` variant. Add a proper arm before the fallback:

```rust
OracleSpan::Parsed(AbilityAST::Triggered(t)) => OracleSpanView {
    kind: SpanKind::Parsed,
    text: format_triggered_ability(t),
    ignored_kind: None,
},
```

- [ ] **Step 3: Run tests**

```bash
cargo test 2>&1 | grep -E "^test result|FAILED|error\["
```

Expected: all pass. (`serve.rs` has no dedicated unit tests, but the build must succeed.)

- [ ] **Step 4: Commit**

```bash
git add src/serve.rs
git commit -m "feat: render triggered abilities in card oracle text view"
```

---

## Task 7: Add ETB fixture + final check

**Files:** `tests/fixtures/oracle_cards_test.json`

- [ ] **Step 1: Add Elvish Visionary to the fixture**

Open `tests/fixtures/oracle_cards_test.json`. Append a new entry to the JSON array (before the closing `]`), using the minimal fields `parse_entry` actually reads:

```json
{
  "object": "card",
  "name": "Elvish Visionary",
  "lang": "en",
  "layout": "normal",
  "mana_cost": "{1}{G}",
  "cmc": 2.0,
  "type_line": "Creature — Elf Scout",
  "oracle_text": "When this enters, draw a card.",
  "power": "1",
  "toughness": "1",
  "colors": ["G"],
  "color_identity": ["G"],
  "keywords": []
}
```

- [ ] **Step 2: Run the full test suite**

```bash
cargo test 2>&1 | grep -E "^test result|FAILED|error\["
```

Expected: all pass. If any test references the fixture by card name and Elvish Visionary now appears in the database, check for conflicts.

- [ ] **Step 3: Commit**

```bash
git add tests/fixtures/oracle_cards_test.json
git commit -m "test: add Elvish Visionary ETB fixture card"
```

---

## Self-Review Checklist

- **Spec coverage:**
  - Section 1 (Data Model): Tasks 1 covers `EffectStep`/`Effect` consolidation and `TriggerEvent`/`TriggeredAbility` expansion. ✓
  - Section 2 (Parser): Tasks 2–3 cover `GainLife` parsing and ETB trigger detection. ✓
  - Section 3 (Engine): Task 4 covers `fire_etb_triggers`; Task 5 wires call sites. ✓
  - Section 4 (UI): Task 6 covers `format_triggered_ability` and span rendering. ✓
  - Section 5 (Tests): All spec-listed tests appear in Tasks 2–7. ✓
  - ETB fixture: Task 7. ✓

- **Types consistent across tasks:**
  - `TriggerEvent::EntersTheBattlefield { subject_is_self: true }` used uniformly in Tasks 1, 3, 4, 5. ✓
  - `Effect = Vec<EffectStep>` used in `TriggeredAbility.effect` and `ActivatedAbility.effect` everywhere. ✓
  - `fire_etb_triggers(state: GameState, entering_id: ObjectId) -> GameState` — signature matches in Tasks 4 and 5. ✓

- **No placeholders:** All steps include actual code. ✓
