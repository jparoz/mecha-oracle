# Instants & Sorceries Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add instant and sorcery card types to the engine — they cast onto the stack, resolve with their parsed effects, and move to the graveyard; draw spells draw cards; Flash creatures cast at instant speed.

**Architecture:** Six sequential tasks, each compilable and test-passing on its own. New types land first (Task 1), then the oracle parser is split and extended (Tasks 2–3), then casting and resolution are unified (Tasks 4–5), then the API and UI are updated (Task 6).

**Tech Stack:** Rust, Axum, serde_json; cargo test for all tests; plain JS in `src/serve.html`.

---

## File map

| File | Change |
|------|--------|
| `src/types/effect.rs` | Add `EffectStep::Unimplemented(String)` |
| `src/types/ability.rs` | Add `StaticAbility::Flash`, `Ability::SpellEffect(Effect)` |
| `src/engine/stack.rs` | Add Unimplemented arm; extract `execute_effect_steps`; branch on permanent vs. spell |
| `src/engine/casting.rs` | Replace `cast_creature` with `cast_spell` (unified timing) |
| `src/parser/oracle.rs` | Rename `parse_oracle_text` → `parse_permanent`; add `parse_instant_or_sorcery`; move Flash to implemented |
| `src/parser/mod.rs` | Update pub exports |
| `src/cards/scryfall.rs` | Dispatch to correct parser based on `TypeLine` |
| `src/serve.rs` | `CastCreature` → `CastSpell`; add `can_cast`; add SpellEffect display arm |
| `src/serve.html` | Update JS action type; add Cast button |

---

## Task 1: Add type variants and stub exhaustive match arms

**Files:**
- Modify: `src/types/effect.rs`
- Modify: `src/types/ability.rs`
- Modify: `src/engine/stack.rs` (match arm only)
- Modify: `src/serve.rs` (match arms only)

- [ ] **Add `EffectStep::Unimplemented` to `src/types/effect.rs`**

```rust
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EffectStep {
    AddMana(ManaPool),
    Mill(u32),
    DrawCard(u32),
    GainLife(u32),
    Unimplemented(String), // parsed but not yet executable; skipped at resolution
}
```

- [ ] **Add `StaticAbility::Flash` to `src/types/ability.rs`**

In the `StaticAbility` enum, add:
```rust
Flash,
```

In `display_name`, add:
```rust
Self::Flash => "Flash",
```

- [ ] **Add `Ability::SpellEffect` to `src/types/ability.rs`**

In the `Ability` enum, add:
```rust
SpellEffect(Effect),
```

- [ ] **Add `Unimplemented` arm to the effect executor in `src/engine/stack.rs`**

Find the `for step in &effect` loop inside the `StackPayload::TriggeredAbility | StackPayload::ActivatedAbility` arm. Add:
```rust
EffectStep::Unimplemented(_) => {}
```

- [ ] **Add `Unimplemented` arm to `format_activated_ability` in `src/serve.rs`**

In `format_activated_ability`, the `effect_parts` map, add:
```rust
EffectStep::Unimplemented(s) => s.clone(),
```

- [ ] **Add `Unimplemented` arm to `format_triggered_ability` in `src/serve.rs`**

In `format_triggered_ability`, the `effect_parts` map, add:
```rust
EffectStep::Unimplemented(s) => s.to_string(),
```

- [ ] **Add stub `SpellEffect` arm to the oracle span match in `src/serve.rs`**

In `build_player_view`, inside the `to_card_view` closure, in the `oracle_text` iterator match, add:
```rust
OracleSpan::Parsed(Ability::SpellEffect(_)) => OracleSpanView {
    kind: SpanKind::Parsed,
    text: String::new(),
    ignored_kind: None,
},
```
(This will be replaced with a real formatter in Task 6.)

- [ ] **Run tests — expect all passing**

```bash
cargo test 2>&1 | grep -E "^test result|FAILED|error\["
```
Expected: `test result: ok`

- [ ] **Commit**

```bash
git add src/types/effect.rs src/types/ability.rs src/engine/stack.rs src/serve.rs
git commit -m "feat: add EffectStep::Unimplemented, StaticAbility::Flash, Ability::SpellEffect stubs"
```

---

## Task 2: Implement `parse_instant_or_sorcery`

**Files:**
- Modify: `src/parser/oracle.rs`

The new public function `parse_instant_or_sorcery(text: &str) -> Vec<OracleSpan>` treats the entire oracle text body as an effect. Each newline-separated paragraph becomes one `OracleSpan::Parsed(Ability::SpellEffect(...))`. Within each paragraph, effects are split first on `". "` (sentences), then each sentence on `", then "` (intra-sentence linking). Unknown sub-steps become `EffectStep::Unimplemented`.

- [ ] **Write failing tests at the bottom of `src/parser/oracle.rs`**

Add inside the existing `#[cfg(test)]` block:

```rust
// ── parse_instant_or_sorcery ─────────────────────────────────────────────────

fn spell_effect(steps: Vec<EffectStep>) -> OracleSpan {
    OracleSpan::Parsed(Ability::SpellEffect(steps))
}
fn unimpl(s: &str) -> EffectStep {
    EffectStep::Unimplemented(s.to_string())
}

#[test]
fn instant_draw_one_card() {
    let result = parse_instant_or_sorcery("Draw a card.");
    assert_eq!(result, vec![spell_effect(vec![EffectStep::DrawCard(1)])]);
}

#[test]
fn brainstorm_then_split() {
    // ", then " splits intra-sentence; DrawCard(3) is parseable, the rest is not
    let result = parse_instant_or_sorcery(
        "Draw three cards, then put two cards from your hand on top of your library in any order.",
    );
    assert_eq!(result, vec![spell_effect(vec![
        EffectStep::DrawCard(3),
        unimpl("put two cards from your hand on top of your library in any order"),
    ])]);
}

#[test]
fn opt_period_then_draw() {
    // ". " splits sentences; first sentence unimplemented, second parseable
    let result = parse_instant_or_sorcery("Scry 1. Draw a card.");
    assert_eq!(result, vec![spell_effect(vec![
        unimpl("Scry 1"),
        EffectStep::DrawCard(1),
    ])]);
}

#[test]
fn serum_visions_draw_then_scry() {
    // "Draw a card, then scry 2." — draw is parsed, scry is unimplemented
    let result = parse_instant_or_sorcery("Draw a card, then scry 2.");
    assert_eq!(result, vec![spell_effect(vec![
        EffectStep::DrawCard(1),
        unimpl("scry 2"),
    ])]);
}

#[test]
fn counterspell_fully_unimplemented() {
    let result = parse_instant_or_sorcery("Counter target spell.");
    assert_eq!(result, vec![spell_effect(vec![
        unimpl("Counter target spell"),
    ])]);
}

#[test]
fn ponder_multi_sentence_mixed() {
    // One paragraph; three sentences; first has ", then " inside it
    let result = parse_instant_or_sorcery(
        "Look at the top three cards of your library, then put them back in any order. You may shuffle. Draw a card.",
    );
    assert_eq!(result, vec![spell_effect(vec![
        unimpl("Look at the top three cards of your library"),
        unimpl("put them back in any order"),
        unimpl("You may shuffle"),
        EffectStep::DrawCard(1),
    ])]);
}

#[test]
fn empty_oracle_text_returns_empty() {
    let result = parse_instant_or_sorcery("");
    assert_eq!(result, vec![]);
}
```

- [ ] **Run tests — expect failures on the new tests**

```bash
cargo test parse_instant_or_sorcery 2>&1 | grep -E "^test result|FAILED|error\["
```
Expected: compile error or test failures (function doesn't exist yet).

- [ ] **Add the internal `parse_spell_effect` helper to `src/parser/oracle.rs`**

Add this private function (before the `pub fn` declarations at the bottom of the file):

```rust
/// Leniently parses a single oracle-text paragraph as a list of effect steps.
/// Splits on ". " (sentence boundary) and ", then " (intra-sentence linking).
/// Steps that cannot be parsed become EffectStep::Unimplemented.
fn parse_spell_effect(paragraph: &str) -> Effect {
    let paragraph = paragraph.trim_end_matches('.');
    paragraph
        .split(". ")
        .flat_map(|sentence| {
            let sentence = sentence.trim_start_matches("Then ").trim();
            sentence.split(", then ").map(|step| {
                let step = step.trim();
                try_parse_effect_step(step)
                    .unwrap_or_else(|| EffectStep::Unimplemented(step.to_string()))
            })
        })
        .collect()
}
```

- [ ] **Add the public `parse_instant_or_sorcery` function to `src/parser/oracle.rs`**

Add alongside `parse_oracle_text` (the existing public function):

```rust
/// Parse the oracle text of an instant or sorcery.
/// Each paragraph becomes one SpellEffect span containing parsed and
/// unimplemented effect steps in written order (CR 609).
pub fn parse_instant_or_sorcery(text: &str) -> Vec<OracleSpan> {
    use crate::types::ability::Ability;
    let mut spans = Vec::new();
    for paragraph in text.split('\n') {
        let paragraph = paragraph.trim();
        if paragraph.is_empty() {
            continue;
        }
        let steps = parse_spell_effect(paragraph);
        spans.push(OracleSpan::Parsed(Ability::SpellEffect(steps)));
    }
    spans
}
```

- [ ] **Run tests — expect new tests passing, all old tests still passing**

```bash
cargo test 2>&1 | grep -E "^test result|FAILED|error\["
```
Expected: `test result: ok`

- [ ] **Commit**

```bash
git add src/parser/oracle.rs
git commit -m "feat: add parse_instant_or_sorcery with lenient spell-body parsing"
```

---

## Task 3: Move Flash to implemented, rename parser, update dispatch

**Files:**
- Modify: `src/parser/oracle.rs`
- Modify: `src/parser/mod.rs`
- Modify: `src/cards/scryfall.rs`

- [ ] **Write a failing test for Flash parsing in `src/parser/oracle.rs`**

In the `#[cfg(test)]` block, inside the existing tests for `parse_oracle_text` (soon to be `parse_permanent`):

```rust
#[test]
fn flash_parses_as_static_ability() {
    let result = parse_permanent("Flash", "");
    assert_eq!(result, vec![OracleSpan::Parsed(Ability::Static(StaticAbility::Flash))]);
}
```

- [ ] **Run test — expect failure**

```bash
cargo test flash_parses_as_static_ability 2>&1 | grep -E "^test result|FAILED|error\["
```
Expected: FAILED (Flash currently emits ParsedUnimplemented).

- [ ] **Move Flash from `is_cr702_keyword` to `match_keyword` in `src/parser/oracle.rs`**

In `match_keyword`, add to the implemented-keywords match:
```rust
"flash" => return parsed!(Flash),
```

In `is_cr702_keyword`, remove the `"flash" |` line (it's in the big `matches!` block at the top).

- [ ] **Rename `parse_oracle_text` to `parse_permanent` in `src/parser/oracle.rs`**

Change the function signature and name:
```rust
pub fn parse_permanent(text: &str, card_name: &str) -> Vec<OracleSpan> {
```

Update the existing tests inside the `#[cfg(test)]` block — every call to `parse_oracle_text` becomes `parse_permanent`.

- [ ] **Update `src/parser/mod.rs` to export both functions**

```rust
mod oracle;
pub use oracle::parse_instant_or_sorcery;
pub use oracle::parse_permanent;
```

- [ ] **Update `src/cards/scryfall.rs` to dispatch based on `TypeLine`**

At the top of the file, update the import:
```rust
use crate::parser::{parse_instant_or_sorcery, parse_permanent};
```

In `parse_entry`, replace:
```rust
let abilities = parse_oracle_text(&oracle_text, &name);
```
With:
```rust
let abilities = if type_line.card_types.iter().any(|t| {
    matches!(t, CardType::Instant | CardType::Sorcery)
}) {
    parse_instant_or_sorcery(&oracle_text)
} else {
    parse_permanent(&oracle_text, &name)
};
```

- [ ] **Run tests — expect all passing**

```bash
cargo test 2>&1 | grep -E "^test result|FAILED|error\["
```
Expected: `test result: ok`

- [ ] **Commit**

```bash
git add src/parser/oracle.rs src/parser/mod.rs src/cards/scryfall.rs
git commit -m "feat: implement Flash keyword; split parser into parse_permanent and parse_instant_or_sorcery"
```

---

## Task 4: Replace `cast_creature` with unified `cast_spell`

**Files:**
- Modify: `src/engine/casting.rs`
- Modify: `src/serve.rs` (call site only)

`cast_spell` handles all spell types. Timing: instants and Flash cards need only priority; everything else needs sorcery-speed conditions (active player, main phase, empty stack). CR 307.1, CR 302.1, CR 702.8a.

- [ ] **Write failing tests in `src/engine/casting.rs`**

Add inside the existing `#[cfg(test)]` block:

```rust
use crate::types::card::{CardDefinition, CardType, TypeLine};
use crate::types::mana::{ManaCost, ManaPip};
use crate::types::{Ability, OracleSpan};
use crate::types::ability::StaticAbility;
use crate::types::effect::EffectStep;

fn make_instant_def(name: &str, cost_pips: Vec<ManaPip>) -> CardDefinition {
    CardDefinition {
        name: name.into(),
        mana_cost: Some(ManaCost { pips: cost_pips }),
        type_line: TypeLine {
            supertypes: vec![],
            card_types: vec![CardType::Instant],
            subtypes: vec![],
        },
        oracle_text: "Draw a card.".into(),
        abilities: vec![OracleSpan::Parsed(Ability::SpellEffect(
            vec![EffectStep::DrawCard(1)],
        ))],
        power: None,
        toughness: None,
    }
}

fn make_flash_creature_def() -> CardDefinition {
    CardDefinition {
        name: "Flash Bear".into(),
        mana_cost: Some(ManaCost { pips: vec![ManaPip::Generic(1), ManaPip::Blue] }),
        type_line: TypeLine {
            supertypes: vec![],
            card_types: vec![CardType::Creature],
            subtypes: vec![],
        },
        oracle_text: "Flash".into(),
        abilities: vec![OracleSpan::Parsed(Ability::Static(StaticAbility::Flash))],
        power: Some(2),
        toughness: Some(2),
    }
}

#[test]
fn cast_instant_succeeds_with_nonempty_stack() {
    use crate::types::stack::{StackObject, StackPayload};
    let mut gs = make_state();
    // put a dummy object on the stack so it's non-empty
    let sid = gs.alloc_stack_id();
    gs.stack.push(sid);
    gs.stack_objects.insert(sid, StackObject {
        id: sid,
        payload: StackPayload::TriggeredAbility {
            source_id: ObjectId(99),
            effect: vec![],
            label: "dummy".into(),
        },
        controller: PlayerId(0),
    });
    gs.get_player_mut(PlayerId(0)).unwrap().mana_pool.blue += 1;
    let id = put_in_hand(&mut gs, PlayerId(0), make_instant_def("Opt", vec![ManaPip::Blue]));

    let gs = cast_spell(gs, PlayerId(0), id).unwrap();

    assert_eq!(gs.objects[&id].zone, Zone::Stack);
}

#[test]
fn cast_instant_when_not_active_player_succeeds() {
    let mut gs = make_state();
    gs.active_player = PlayerId(1); // opponent's turn
    gs.priority_player = PlayerId(0);
    gs.get_player_mut(PlayerId(0)).unwrap().mana_pool.blue += 1;
    let id = put_in_hand(&mut gs, PlayerId(0), make_instant_def("Opt", vec![ManaPip::Blue]));

    let gs = cast_spell(gs, PlayerId(0), id).unwrap();

    assert_eq!(gs.objects[&id].zone, Zone::Stack);
}

#[test]
fn cast_flash_creature_with_nonempty_stack_succeeds() {
    use crate::types::stack::{StackObject, StackPayload};
    let mut gs = make_state();
    let sid = gs.alloc_stack_id();
    gs.stack.push(sid);
    gs.stack_objects.insert(sid, StackObject {
        id: sid,
        payload: StackPayload::TriggeredAbility {
            source_id: ObjectId(99),
            effect: vec![],
            label: "dummy".into(),
        },
        controller: PlayerId(0),
    });
    gs.get_player_mut(PlayerId(0)).unwrap().mana_pool.blue += 1;
    gs.get_player_mut(PlayerId(0)).unwrap().mana_pool.colorless += 1;
    let id = put_in_hand(&mut gs, PlayerId(0), make_flash_creature_def());

    let gs = cast_spell(gs, PlayerId(0), id).unwrap();

    assert_eq!(gs.objects[&id].zone, Zone::Stack);
}

#[test]
fn cast_creature_without_flash_fails_with_nonempty_stack() {
    use crate::types::stack::{StackObject, StackPayload};
    let db = test_db();
    let mut gs = make_state();
    let sid = gs.alloc_stack_id();
    gs.stack.push(sid);
    gs.stack_objects.insert(sid, StackObject {
        id: sid,
        payload: StackPayload::TriggeredAbility {
            source_id: ObjectId(99),
            effect: vec![],
            label: "dummy".into(),
        },
        controller: PlayerId(0),
    });
    gs.get_player_mut(PlayerId(0)).unwrap().mana_pool.green += 2;
    let id = put_in_hand(&mut gs, PlayerId(0), db.get("Grizzly Bears").unwrap().clone());

    assert!(matches!(cast_spell(gs, PlayerId(0), id), Err(EngineError::CannotCastNow)));
}

#[test]
fn cast_spell_moves_card_to_stack() {
    let db = test_db();
    let mut gs = make_state();
    gs.get_player_mut(PlayerId(0)).unwrap().mana_pool.green += 2;
    let id = put_in_hand(&mut gs, PlayerId(0), db.get("Grizzly Bears").unwrap().clone());

    let gs = cast_spell(gs, PlayerId(0), id).unwrap();

    assert!(!gs.hands[&PlayerId(0)].contains(&id));
    assert_eq!(gs.objects[&id].zone, Zone::Stack);
    assert_eq!(gs.stack.len(), 1);
}

#[test]
fn cast_spell_caster_retains_priority() {
    let db = test_db();
    let mut gs = make_state();
    gs.get_player_mut(PlayerId(0)).unwrap().mana_pool.green += 2;
    let id = put_in_hand(&mut gs, PlayerId(0), db.get("Grizzly Bears").unwrap().clone());

    let gs = cast_spell(gs, PlayerId(0), id).unwrap();

    assert_eq!(gs.priority_player, PlayerId(0));
    assert_eq!(gs.consecutive_passes, 0);
}
```

- [ ] **Run tests — expect failures**

```bash
cargo test cast_spell 2>&1 | grep -E "^test result|FAILED|error\["
```
Expected: compile error (function doesn't exist yet).

- [ ] **Add `cast_spell` to `src/engine/casting.rs`**

First, extend the top-level imports in the file:

```rust
// Change:
use crate::types::{GameState, ObjectId, PlayerId, Zone};
// To:
use crate::types::{GameState, ObjectId, PlayerId, Step, Zone};
use crate::types::card::CardType;
use crate::types::ability::StaticAbility;
```

Then add the `is_instant_speed` helper and the main function:

/// Returns true if this card may be cast at instant speed (CR 702.8a, CR 304.1).
fn is_instant_speed(obj: &crate::types::CardObject) -> bool {
    obj.definition.type_line.card_types.contains(&CardType::Instant)
        || obj.has_keyword(StaticAbility::Flash)
}

/// Cast any spell from hand — creatures, instants, sorceries, artifacts, enchantments.
/// Timing: instants and Flash cards need only priority (CR 702.8a).
/// All others require sorcery speed: active player, main phase, empty stack (CR 307.1).
pub fn cast_spell(
    mut state: GameState,
    player_id: PlayerId,
    object_id: ObjectId,
) -> Result<GameState, EngineError> {
    if state.priority_player != player_id {
        return Err(EngineError::NotYourPriority);
    }

    let cost = {
        let hand = state.hands.get(&player_id).ok_or(EngineError::CardNotFound)?;
        if !hand.contains(&object_id) {
            return Err(EngineError::CardNotInHand);
        }
        let obj = state.objects.get(&object_id).ok_or(EngineError::CardNotFound)?;

        if !is_instant_speed(obj) {
            // Sorcery-speed restrictions (CR 307.1)
            if state.active_player != player_id {
                return Err(EngineError::CannotCastNow);
            }
            if !matches!(state.step(), crate::types::Step::PreCombatMain | crate::types::Step::PostCombatMain) {
                return Err(EngineError::CannotCastNow);
            }
            if !state.stack.is_empty() {
                return Err(EngineError::CannotCastNow);
            }
        }

        obj.definition.mana_cost.clone().ok_or(EngineError::CannotCastNow)?
    };

    let plan = {
        let player = state.get_player(player_id).ok_or(EngineError::CardNotFound)?;
        greedy_payment_plan(&cost, &player.mana_pool, player.life)
            .ok_or(EngineError::InsufficientMana)?
    };
    state = pay_mana_cost(state, player_id, &cost, &plan)?;
    state.mana_checkpoint = None;
    state.hands.get_mut(&player_id).unwrap().retain(|&id| id != object_id);
    {
        let obj = state.objects.get_mut(&object_id).unwrap();
        obj.zone = Zone::Stack;
    }

    let stack_id = state.alloc_stack_id();
    let stack_obj = crate::types::StackObject {
        id: stack_id,
        payload: crate::types::StackPayload::Spell { card_id: object_id },
        controller: player_id,
    };
    state.stack.push(stack_id);
    state.stack_objects.insert(stack_id, stack_obj);

    // CR 117.3c: caster retains priority after casting
    state.consecutive_passes = 0;
    state.priority_player = player_id;

    Ok(state)
}
```

- [ ] **Delete `cast_creature` from `src/engine/casting.rs`**

Remove the entire `cast_creature` function (it's replaced by `cast_spell`). Keep `play_land` unchanged. `EngineError::NotACreature` can remain in the enum in `engine/mod.rs` — unused enum variants don't trigger warnings in Rust.

- [ ] **Update `src/serve.rs` to use `cast_spell`**

Change the import:
```rust
use mecha_oracle::engine::casting::{cast_spell, play_land};
```

Change the `CastCreature` action variant:
```rust
CastSpell {
    object_id: u64,
},
```

Change the dispatch arm:
```rust
ActionRequest::CastSpell { object_id } => {
    let player = state.priority_player;
    cast_spell(state, player, ObjectId(object_id)).map_err(|e| format!("{e:?}"))
}
```

- [ ] **Run tests — expect all passing**

```bash
cargo test 2>&1 | grep -E "^test result|FAILED|error\["
```
Expected: `test result: ok` — old `cast_creature` tests in `casting.rs` are replaced by the new `cast_spell` tests.

- [ ] **Commit**

```bash
git add src/engine/casting.rs src/serve.rs
git commit -m "feat: replace cast_creature with unified cast_spell; Flash creatures cast at instant speed"
```

---

## Task 5: Resolve instants and sorceries to graveyard

**Files:**
- Modify: `src/engine/stack.rs`

When a `StackPayload::Spell` resolves, branch on permanent vs. non-permanent. Permanents go to battlefield (existing). Instants and sorceries execute their `SpellEffect` steps then go to the graveyard (CR 608.2b). Extract `execute_effect_steps` so the existing triggered/activated arm can share it.

- [ ] **Write failing tests in `src/engine/stack.rs`**

Add inside the existing `#[cfg(test)]` block. The test helper `put_in_library` and `push_spell` already exist.

```rust
use crate::types::card::{CardDefinition, CardType, TypeLine};
use crate::types::mana::{ManaCost, ManaPip};

fn make_instant_obj(
    state: &mut GameState,
    owner: PlayerId,
    steps: Vec<crate::types::effect::EffectStep>,
) -> ObjectId {
    use crate::types::{Ability, OracleSpan};
    let def = CardDefinition {
        name: "Test Instant".into(),
        mana_cost: Some(ManaCost { pips: vec![ManaPip::Blue] }),
        type_line: TypeLine {
            supertypes: vec![],
            card_types: vec![CardType::Instant],
            subtypes: vec![],
        },
        oracle_text: String::new(),
        abilities: vec![OracleSpan::Parsed(Ability::SpellEffect(steps))],
        power: None,
        toughness: None,
    };
    let id = state.alloc_id();
    let obj = crate::types::CardObject::new(id, def, owner, Zone::Stack);
    state.add_object(obj);
    id
}

fn make_sorcery_obj(
    state: &mut GameState,
    owner: PlayerId,
    steps: Vec<crate::types::effect::EffectStep>,
) -> ObjectId {
    use crate::types::{Ability, OracleSpan};
    let def = CardDefinition {
        name: "Test Sorcery".into(),
        mana_cost: Some(ManaCost { pips: vec![ManaPip::Blue] }),
        type_line: TypeLine {
            supertypes: vec![],
            card_types: vec![CardType::Sorcery],
            subtypes: vec![],
        },
        oracle_text: String::new(),
        abilities: vec![OracleSpan::Parsed(Ability::SpellEffect(steps))],
        power: None,
        toughness: None,
    };
    let id = state.alloc_id();
    let obj = crate::types::CardObject::new(id, def, owner, Zone::Stack);
    state.add_object(obj);
    id
}

#[test]
fn instant_spell_resolves_to_graveyard() {
    let mut gs = make_state();
    let id = make_instant_obj(&mut gs, PlayerId(0), vec![]);
    push_spell(&mut gs, id);

    let gs = resolve_top(gs);

    assert_eq!(gs.objects[&id].zone, Zone::Graveyard);
    assert!(gs.graveyards[&PlayerId(0)].contains(&id));
    assert!(!gs.battlefield.contains(&id));
    assert!(gs.stack.is_empty());
}

#[test]
fn instant_spell_draw_effect_executes_before_graveyard() {
    let mut gs = make_state();
    put_in_library(&mut gs, PlayerId(0));
    let id = make_instant_obj(&mut gs, PlayerId(0), vec![EffectStep::DrawCard(1)]);
    push_spell(&mut gs, id);

    let gs = resolve_top(gs);

    assert_eq!(gs.objects[&id].zone, Zone::Graveyard);
    assert_eq!(gs.hands[&PlayerId(0)].len(), 1);
}

#[test]
fn instant_draw_three_executes_fully() {
    let mut gs = make_state();
    put_in_library(&mut gs, PlayerId(0));
    put_in_library(&mut gs, PlayerId(0));
    put_in_library(&mut gs, PlayerId(0));
    let id = make_instant_obj(&mut gs, PlayerId(0), vec![EffectStep::DrawCard(3)]);
    push_spell(&mut gs, id);

    let gs = resolve_top(gs);

    assert_eq!(gs.hands[&PlayerId(0)].len(), 3);
    assert_eq!(gs.objects[&id].zone, Zone::Graveyard);
}

#[test]
fn unimplemented_steps_are_skipped_silently() {
    let mut gs = make_state();
    put_in_library(&mut gs, PlayerId(0));
    let id = make_instant_obj(&mut gs, PlayerId(0), vec![
        EffectStep::DrawCard(1),
        EffectStep::Unimplemented("scry 2".into()),
    ]);
    push_spell(&mut gs, id);
    let before_life = gs.get_player(PlayerId(0)).unwrap().life;

    let gs = resolve_top(gs);

    assert_eq!(gs.hands[&PlayerId(0)].len(), 1); // drew 1
    assert_eq!(gs.get_player(PlayerId(0)).unwrap().life, before_life); // no life gain
    assert_eq!(gs.objects[&id].zone, Zone::Graveyard);
}

#[test]
fn sorcery_with_no_parseable_effects_just_goes_to_graveyard() {
    let mut gs = make_state();
    let before_hand = gs.hands[&PlayerId(0)].len();
    let id = make_sorcery_obj(&mut gs, PlayerId(0), vec![
        EffectStep::Unimplemented("Counter target spell".into()),
    ]);
    push_spell(&mut gs, id);

    let gs = resolve_top(gs);

    assert_eq!(gs.objects[&id].zone, Zone::Graveyard);
    assert_eq!(gs.hands[&PlayerId(0)].len(), before_hand); // nothing happened
}

#[test]
fn creature_spell_still_resolves_to_battlefield() {
    // Regression: permanents must still go to battlefield, not graveyard
    let db = test_db();
    let mut gs = make_state();
    let id = gs.alloc_id();
    let obj = CardObject::new(id, db.get("Grizzly Bears").unwrap().clone(), PlayerId(0), Zone::Stack);
    gs.add_object(obj);
    push_spell(&mut gs, id);

    let gs = resolve_top(gs);

    assert!(gs.battlefield.contains(&id));
    assert_eq!(gs.objects[&id].zone, Zone::Battlefield);
    assert!(!gs.graveyards[&PlayerId(0)].contains(&id));
}
```

- [ ] **Run tests — expect failures on the new tests**

```bash
cargo test -p mecha-oracle instant_spell 2>&1 | grep -E "^test result|FAILED|error\["
```
Expected: multiple FAILEDs (instants currently go to battlefield).

- [ ] **Extract `execute_effect_steps` and update `resolve_top` in `src/engine/stack.rs`**

Add this private helper function at the top of the file (after the `use` declarations):

```rust
fn execute_effect_steps(
    mut state: GameState,
    controller: PlayerId,
    steps: &[EffectStep],
) -> GameState {
    for step in steps {
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
            EffectStep::Unimplemented(_) => {}
        }
    }
    state
}
```

Update `resolve_top` — replace the `StackPayload::Spell { card_id }` arm:

```rust
StackPayload::Spell { card_id } => {
    let controller = stack_obj.controller;
    let is_permanent = state
        .objects
        .get(&card_id)
        .map(|o| o.definition.type_line.is_permanent())
        .unwrap_or(false);

    if is_permanent {
        // CR 608.3: permanent spells → battlefield
        if let Some(obj) = state.objects.get_mut(&card_id) {
            obj.zone = Zone::Battlefield;
            obj.summoning_sick = true;
        }
        state.battlefield.push(card_id);

        let triggers = collect_etb_triggers(&mut state, card_id);
        for trigger in triggers {
            let id = trigger.id;
            state.stack.push(id);
            state.stack_objects.insert(id, trigger);
        }
    } else {
        // CR 608.2b: instant/sorcery → execute effects, then graveyard
        let steps: Vec<EffectStep> = state
            .objects
            .get(&card_id)
            .map(|obj| {
                obj.definition
                    .abilities
                    .iter()
                    .filter_map(|span| match span {
                        crate::types::OracleSpan::Parsed(
                            crate::types::Ability::SpellEffect(steps),
                        ) => Some(steps.clone()),
                        _ => None,
                    })
                    .flatten()
                    .collect()
            })
            .unwrap_or_default();

        state = execute_effect_steps(state, controller, &steps);

        if let Some(obj) = state.objects.get_mut(&card_id) {
            obj.zone = Zone::Graveyard;
        }
        if let Some(gy) = state.graveyards.get_mut(&controller) {
            gy.push(card_id);
        }
    }

    state.consecutive_passes = 0;
    state.priority_player = state.active_player;
    check_and_apply_sbas(state)
}
```

Update the `StackPayload::TriggeredAbility | StackPayload::ActivatedAbility` arm to delegate to `execute_effect_steps`:

```rust
StackPayload::TriggeredAbility { effect, .. }
| StackPayload::ActivatedAbility { effect, .. } => {
    let controller = stack_obj.controller;
    state = execute_effect_steps(state, controller, &effect);
    state.consecutive_passes = 0;
    state.priority_player = state.active_player;
    check_and_apply_sbas(state)
}
```

- [ ] **Add `EffectStep` to imports in `src/engine/stack.rs`**

The existing import `use crate::types::effect::EffectStep;` should already be present (it's used in tests). Verify it's in the top-level imports (not just the test module).

- [ ] **Run tests — expect all passing**

```bash
cargo test 2>&1 | grep -E "^test result|FAILED|error\["
```
Expected: `test result: ok`

- [ ] **Commit**

```bash
git add src/engine/stack.rs
git commit -m "feat: instants and sorceries resolve to graveyard with spell effects"
```

---

## Task 6: View model, API, and UI

**Files:**
- Modify: `src/serve.rs`
- Modify: `src/serve.html`

- [ ] **Write a test for `can_cast` in the `src/serve.rs` test block**

Add inside the existing `#[cfg(test)]` module:

```rust
#[test]
fn can_cast_true_for_instant_in_hand_with_mana_and_priority() {
    use mecha_oracle::types::{CardObject, Zone};
    use mecha_oracle::types::card::{CardDefinition, CardType, TypeLine};
    use mecha_oracle::types::mana::{ManaCost, ManaPip};
    use mecha_oracle::types::{Ability, OracleSpan};
    use mecha_oracle::types::effect::EffectStep;

    let config = vec![
        (0..10).map(|_| "Forest".to_string()).collect(),
        (0..10).map(|_| "Forest".to_string()).collect(),
    ];
    let db = test_db();
    let mut gs = build_game_state(config, &db, false).unwrap();
    // Priority: Player 0. Active: Player 0. Stack: empty. Step: PreCombatMain.
    let def = CardDefinition {
        name: "Cheap Instant".into(),
        mana_cost: Some(ManaCost { pips: vec![ManaPip::Generic(1)] }),
        type_line: TypeLine {
            supertypes: vec![],
            card_types: vec![CardType::Instant],
            subtypes: vec![],
        },
        oracle_text: "Draw a card.".into(),
        abilities: vec![OracleSpan::Parsed(Ability::SpellEffect(vec![EffectStep::DrawCard(1)]))],
        power: None,
        toughness: None,
    };
    let id = gs.alloc_id();
    let obj = CardObject::new(id, def, mecha_oracle::types::PlayerId(0), Zone::Hand);
    gs.hands.get_mut(&mecha_oracle::types::PlayerId(0)).unwrap().push(id);
    gs.add_object(obj);
    gs.get_player_mut(mecha_oracle::types::PlayerId(0)).unwrap().mana_pool.colorless = 1;

    let view = build_game_view(&gs);
    let card = view.p1.hand.iter().find(|c| c.name == "Cheap Instant").unwrap();
    assert!(card.can_cast, "instant with mana in hand with priority should be castable");
}

#[test]
fn can_cast_false_for_creature_when_not_active_player() {
    use mecha_oracle::types::{CardObject, Zone};
    let db = test_db();
    let config = vec![
        (0..10).map(|_| "Forest".to_string()).collect(),
        (0..10).map(|_| "Forest".to_string()).collect(),
    ];
    let mut gs = build_game_state(config, &db, false).unwrap();
    // Give Player 1 priority by shifting active to Player 1
    gs.active_player = mecha_oracle::types::PlayerId(1);
    gs.priority_player = mecha_oracle::types::PlayerId(0); // P0 has priority but is not active

    let id = gs.alloc_id();
    let mut obj = CardObject::new(
        id,
        db.get("Grizzly Bears").unwrap().clone(),
        mecha_oracle::types::PlayerId(0),
        Zone::Hand,
    );
    gs.hands.get_mut(&mecha_oracle::types::PlayerId(0)).unwrap().push(id);
    gs.add_object(obj);
    gs.get_player_mut(mecha_oracle::types::PlayerId(0)).unwrap().mana_pool.green = 2;

    let view = build_game_view(&gs);
    let card = view.p1.hand.iter().find(|c| c.name == "Grizzly Bears").unwrap();
    assert!(!card.can_cast, "creature cannot be cast when player is not active");
}
```

- [ ] **Run tests — expect failures**

```bash
cargo test can_cast 2>&1 | grep -E "^test result|FAILED|error\["
```
Expected: compile error (`can_cast` field not in `CardView`).

- [ ] **Add `can_cast` to `CardView` in `src/serve.rs`**

In the `CardView` struct, add:
```rust
can_cast: bool,
```

- [ ] **Update imports in `src/serve.rs`**

```rust
// Change the ability import to add StaticAbility:
use mecha_oracle::types::ability::{
    Ability, ActivatedAbility, CostComponent, OracleSpan, StaticAbility, TriggeredAbility,
};
// Change the mana import to add greedy_payment_plan:
use mecha_oracle::engine::mana::{greedy_payment_plan, reset_mana, tap_land_for_mana};
// Change the casting import (cast_creature → cast_spell, already done in Task 4):
use mecha_oracle::engine::casting::{cast_spell, play_land};
```

- [ ] **Add `format_spell_effect` helper to `src/serve.rs`**

Add alongside the other format helpers:

```rust
fn format_spell_effect(effect: &[EffectStep]) -> String {
    effect
        .iter()
        .map(|step| match step {
            EffectStep::DrawCard(1) => "Draw a card".to_string(),
            EffectStep::DrawCard(n) => format!("Draw {n} cards"),
            EffectStep::GainLife(n) => format!("Gain {n} life"),
            EffectStep::Mill(n) => format!("Mill {n}"),
            EffectStep::AddMana(pool) => format!("Add {}", format_mana_pool(pool)),
            EffectStep::Unimplemented(s) => s.clone(),
        })
        .collect::<Vec<_>>()
        .join(", then ")
}
```

- [ ] **Replace the SpellEffect stub arm in `to_card_view` in `src/serve.rs`**

Replace the placeholder from Task 1:
```rust
OracleSpan::Parsed(Ability::SpellEffect(steps)) => OracleSpanView {
    kind: SpanKind::Parsed,
    text: format_spell_effect(steps),
    ignored_kind: None,
},
```

- [ ] **Compute `can_cast` in `to_card_view` and populate it**

`to_card_view` is a closure inside `build_player_view`. It already has access to `state` and `pid`. Add a helper bool before the closure or compute it inline:

```rust
let can_cast = |obj: &mecha_oracle::types::CardObject| -> bool {
    use mecha_oracle::types::ability::StaticAbility;
    use mecha_oracle::types::card::CardType;
    if state.priority_player != pid {
        return false;
    }
    let Some(cost) = obj.definition.mana_cost.as_ref() else {
        return false;
    };
    let player = match state.get_player(pid) {
        Some(p) => p,
        None => return false,
    };
    if mecha_oracle::engine::mana::greedy_payment_plan(cost, &player.mana_pool, player.life)
        .is_none()
    {
        return false;
    }
    let is_instant_speed = obj.definition.type_line.card_types.contains(&CardType::Instant)
        || obj.has_keyword(StaticAbility::Flash);
    if is_instant_speed {
        return true;
    }
    // Sorcery-speed: must be active player, main phase, empty stack
    state.active_player == pid
        && matches!(state.step(), Step::PreCombatMain | Step::PostCombatMain)
        && state.stack.is_empty()
};
```

Then in `to_card_view`, use it:
```rust
can_cast: can_cast(obj),
```

Note: `can_cast` should only apply to cards in hand. In `build_player_view`, `to_card_view` is used for hand, battlefield creatures, lands, and graveyard. The `can_cast` field will be `true` only when those conditions are met (priority, mana), so battlefield and graveyard cards will naturally return `false` since they're not in hand — but to be safe, the closure can check `obj.zone == Zone::Hand` first. Add that as the very first check:

```rust
if obj.zone != Zone::Hand {
    return false;
}
```

- [ ] **Run tests — expect all passing**

```bash
cargo test 2>&1 | grep -E "^test result|FAILED|error\["
```
Expected: `test result: ok`

- [ ] **Update `src/serve.html` to use `cast_spell` and show a Cast button**

Open `src/serve.html`. Search for `cast_creature` and replace all occurrences with `cast_spell`.

Then find where the attack button (or any card action button) is rendered for cards with `can_attack === true`. Add an analogous Cast button for `can_cast`:

```js
if (card.can_cast) {
    const btn = document.createElement('button');
    btn.textContent = 'Cast';
    btn.onclick = () => action({ type: 'cast_spell', object_id: card.id });
    el.appendChild(btn);
}
```

Place this in the hand card rendering section, using the same `action()` helper that existing buttons use.

- [ ] **Delete the Flash todo entry from `docs/todo.md`**

Find and delete this bullet:
```
- **Flash** (702.8): cast at instant speed — requires instant-speed casting infrastructure
```

- [ ] **Run all tests one final time**

```bash
cargo test 2>&1 | grep -E "^test result|FAILED|error\["
```
Expected: `test result: ok`

- [ ] **Commit**

```bash
git add src/serve.rs src/serve.html docs/todo.md
git commit -m "feat: add can_cast to card view; CastSpell API; spell effect display; Flash todo removed"
```
