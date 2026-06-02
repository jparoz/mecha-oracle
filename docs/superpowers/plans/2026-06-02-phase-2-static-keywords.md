# Phase 2: Static Keyword Abilities — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Implement all eleven evergreen static keyword abilities (Flying, Reach, Trample, First Strike, Double Strike, Vigilance, Haste, Lifelink, Deathtouch, Menace, Indestructible) with behaviour derived from parsed Oracle text via a fully typed AST.

**Architecture:** The parser produces `Result<Vec<AbilityAST>, ParseError>` from Oracle text; the engine queries `CardObject::has_keyword(StaticAbility)` rather than oracle text directly. Dynamic step insertion (`GameState::extra_steps: VecDeque<Step>`) handles the two-round first-strike combat damage step per CR 510.4. All tests follow TDD: failing test first, then minimal implementation.

**Tech Stack:** Rust 2024 edition (rustc ≥ 1.85), no new dependencies (`VecDeque` is from std).

---

## File Map

| File | Change |
|---|---|
| `src/types/ability.rs` | `StaticAbility` empty struct → real enum |
| `src/parser/mod.rs` | Export `ParseError`; re-export `parse_oracle_text` |
| `src/parser/oracle.rs` | Full implementation with reminder-text stripping |
| `src/cards/scryfall.rs` | Propagate `Result` from `parse_oracle_text` |
| `src/types/card_object.rs` | `has_keyword`, `can_attack` update, `damaged_by_deathtouch` field |
| `src/types/game_state.rs` | `extra_steps: VecDeque<Step>`, `CombatState::first_strike_done` |
| `src/engine/mod.rs` | Two new `EngineError` variants |
| `src/engine/turn.rs` | `advance_step` queue check; clear `damaged_by_deathtouch` in cleanup |
| `src/engine/combat.rs` | All declare/damage changes for all 11 keywords |
| `src/engine/state_based_actions.rs` | Indestructible exemption; deathtouch SBA (704.5h) |
| `tests/fixtures/oracle_cards_test.json` | Add four keyword creatures |
| `tests/scripted_game.rs` | Integration tests for keyword interactions |

---

## Task 1: `StaticAbility` enum

**Files:**
- Modify: `src/types/ability.rs`

- [ ] **Step 1: Replace `StaticAbility` with the real enum**

Replace the entire contents of `src/types/ability.rs` with:

```rust
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StaticAbility {
    Flying,
    Reach,
    Trample,
    FirstStrike,
    DoubleStrike,
    Vigilance,
    Haste,
    Lifelink,
    Deathtouch,
    Menace,
    Indestructible,
}

/// The event that fires a triggered ability. Phase 2+ adds condition variants.
#[derive(Debug, Clone)]
pub struct TriggerEvent;

/// An ability that triggers on a game event. Phase 2+ adds trigger + effect fields.
#[derive(Debug, Clone)]
pub struct TriggeredAbility {
    pub trigger: TriggerEvent,
}

/// An ability paid for with a cost. Phase 2+ adds cost + effect fields.
#[derive(Debug, Clone)]
pub struct ActivatedAbility;

#[derive(Debug, Clone)]
pub enum AbilityAST {
    Static(StaticAbility),
    Triggered(TriggeredAbility),
    Activated(ActivatedAbility),
}
```

- [ ] **Step 2: Verify the project still compiles**

```bash
cargo check
```

Expected: no errors. `StaticAbility` was an empty struct so no existing code referenced its fields; `AbilityAST::Static(StaticAbility)` still type-checks because `StaticAbility` is still a valid type.

- [ ] **Step 3: Commit**

```bash
git add src/types/ability.rs
git commit -m "feat: StaticAbility becomes a real keyword enum"
```

---

## Task 2: `ParseError` type and `parse_oracle_text` implementation

**Files:**
- Modify: `src/parser/mod.rs`
- Modify: `src/parser/oracle.rs`
- Modify: `src/cards/scryfall.rs`

- [ ] **Step 1: Write failing parser tests in `src/parser/oracle.rs`**

Replace the entire file:

```rust
use crate::types::{AbilityAST, ability::StaticAbility};
use super::ParseError;

/// Strip all parenthetical reminder text from oracle text before tokenising.
/// CR 305.6: parenthetical text on basic lands is reminder text, not rules text.
fn strip_reminder_text(text: &str) -> String {
    let mut result = String::new();
    let mut depth: usize = 0;
    for c in text.chars() {
        match c {
            '(' => depth += 1,
            ')' => depth = depth.saturating_sub(1),
            _ if depth == 0 => result.push(c),
            _ => {}
        }
    }
    result
}

/// Parse Oracle text into ability AST nodes.
/// Returns Err if any token is not a recognised keyword.
/// Blank tokens (blank lines, trailing commas) are silently skipped.
pub fn parse_oracle_text(text: &str) -> Result<Vec<AbilityAST>, ParseError> {
    let stripped = strip_reminder_text(text);
    let mut abilities = vec![];
    for token in stripped.split(['\n', ',']).map(str::trim).filter(|s| !s.is_empty()) {
        let kw = match token.to_lowercase().as_str() {
            "flying"        => StaticAbility::Flying,
            "reach"         => StaticAbility::Reach,
            "trample"       => StaticAbility::Trample,
            "first strike"  => StaticAbility::FirstStrike,
            "double strike" => StaticAbility::DoubleStrike,
            "vigilance"     => StaticAbility::Vigilance,
            "haste"         => StaticAbility::Haste,
            "lifelink"      => StaticAbility::Lifelink,
            "deathtouch"    => StaticAbility::Deathtouch,
            "menace"        => StaticAbility::Menace,
            "indestructible" => StaticAbility::Indestructible,
            other => return Err(ParseError::UnknownKeyword {
                keyword: other.to_string(),
                card_text: text.to_string(),
            }),
        };
        abilities.push(AbilityAST::Static(kw));
    }
    Ok(abilities)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::ability::StaticAbility;

    #[test]
    fn empty_text_returns_empty_vec() {
        assert_eq!(parse_oracle_text("").unwrap(), vec![]);
    }

    #[test]
    fn reminder_text_only_returns_empty_vec() {
        // Forest's oracle text: reminder text, not a keyword
        assert_eq!(parse_oracle_text("({T}: Add {G}.)").unwrap(), vec![]);
    }

    #[test]
    fn single_keyword_newline() {
        let result = parse_oracle_text("Flying").unwrap();
        assert_eq!(result, vec![AbilityAST::Static(StaticAbility::Flying)]);
    }

    #[test]
    fn comma_separated_keywords() {
        let result = parse_oracle_text("Flying, vigilance").unwrap();
        assert_eq!(result, vec![
            AbilityAST::Static(StaticAbility::Flying),
            AbilityAST::Static(StaticAbility::Vigilance),
        ]);
    }

    #[test]
    fn two_word_keyword_first_strike() {
        let result = parse_oracle_text("First strike").unwrap();
        assert_eq!(result, vec![AbilityAST::Static(StaticAbility::FirstStrike)]);
    }

    #[test]
    fn keyword_with_reminder_text_stripped() {
        // Deathtouch reminder text is stripped before tokenising
        let result = parse_oracle_text(
            "Deathtouch (Any amount of damage this deals to a creature is enough to destroy it.)"
        ).unwrap();
        assert_eq!(result, vec![AbilityAST::Static(StaticAbility::Deathtouch)]);
    }

    #[test]
    fn multiline_keywords() {
        let result = parse_oracle_text("Trample\nLifelink").unwrap();
        assert_eq!(result, vec![
            AbilityAST::Static(StaticAbility::Trample),
            AbilityAST::Static(StaticAbility::Lifelink),
        ]);
    }

    #[test]
    fn all_eleven_keywords_parse() {
        let text = "Flying\nReach\nTrample\nFirst strike\nDouble strike\nVigilance\nHaste\nLifelink\nDeathtouch\nMenace\nIndestructible";
        let result = parse_oracle_text(text).unwrap();
        assert_eq!(result.len(), 11);
    }

    #[test]
    fn unknown_keyword_returns_error() {
        let err = parse_oracle_text("Intimidate").unwrap_err();
        match err {
            ParseError::UnknownKeyword { keyword, .. } => assert_eq!(keyword, "intimidate"),
        }
    }

    #[test]
    fn triggered_ability_text_returns_error() {
        // Triggered ability text is not a Phase 2 keyword — should fail
        assert!(parse_oracle_text("When this creature enters, draw a card.").is_err());
    }
}
```

- [ ] **Step 2: Run tests to confirm they fail**

```bash
cargo test parser::oracle
```

Expected: compile error — `ParseError` not defined in `super`.

- [ ] **Step 3: Add `ParseError` to `src/parser/mod.rs`**

Replace `src/parser/mod.rs`:

```rust
use std::fmt;

#[derive(Debug)]
pub enum ParseError {
    UnknownKeyword { keyword: String, card_text: String },
}

impl fmt::Display for ParseError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ParseError::UnknownKeyword { keyword, card_text } => write!(
                f,
                "unknown keyword {:?} in oracle text {:?}",
                keyword, card_text
            ),
        }
    }
}

mod oracle;
pub use oracle::parse_oracle_text;
```

- [ ] **Step 4: Run the parser tests**

```bash
cargo test parser::
```

Expected: 10 tests pass.

- [ ] **Step 5: Update `src/cards/scryfall.rs` to propagate `Result`**

Change the `abilities` line in `parse_card`:

```rust
// Before:
let abilities = parse_oracle_text(&oracle_text);

// After:
let abilities = parse_oracle_text(&oracle_text)
    .map_err(|e| e.to_string())?;
```

Also update the import at the top of the file — `parse_oracle_text` now returns `Result`:

```rust
use crate::parser::parse_oracle_text;
```

(This import is already there; no change needed.)

- [ ] **Step 6: Run all tests**

```bash
cargo test
```

Expected: all existing tests pass. Cards with empty oracle text (Forest, Grizzly Bears, Hill Giant) parse fine — `Ok(vec![])`. The existing scryfall unit tests still pass because those cards have empty or reminder-text-only oracle text.

- [ ] **Step 7: Commit**

```bash
git add src/parser/mod.rs src/parser/oracle.rs src/cards/scryfall.rs
git commit -m "feat: implement parse_oracle_text with keyword mapping and ParseError"
```

---

## Task 3: `has_keyword` API and `CardObject` updates

**Files:**
- Modify: `src/types/card_object.rs`
- Modify: `src/types/mod.rs`

- [ ] **Step 1: Write failing tests**

Add to the `#[cfg(test)] mod tests` block in `src/types/card_object.rs`:

```rust
    // Add after existing tests:

    #[test]
    fn has_keyword_returns_true_for_matching_ability() {
        use crate::types::{AbilityAST, ability::StaticAbility};
        let mut def = grizzly_bears();
        def.abilities = vec![AbilityAST::Static(StaticAbility::Flying)];
        let obj = CardObject::new(ObjectId(1), def, PlayerId(0), Zone::Battlefield);
        assert!(obj.has_keyword(StaticAbility::Flying));
        assert!(!obj.has_keyword(StaticAbility::Trample));
    }

    #[test]
    fn summoning_sick_creature_with_haste_can_attack() {
        use crate::types::{AbilityAST, ability::StaticAbility};
        let mut def = grizzly_bears();
        def.abilities = vec![AbilityAST::Static(StaticAbility::Haste)];
        let mut obj = CardObject::new(ObjectId(1), def, PlayerId(0), Zone::Battlefield);
        obj.summoning_sick = true;
        assert!(obj.can_attack()); // haste bypasses summoning sickness
    }

    #[test]
    fn damaged_by_deathtouch_initialises_false() {
        let obj = CardObject::new(ObjectId(1), grizzly_bears(), PlayerId(0), Zone::Battlefield);
        assert!(!obj.damaged_by_deathtouch);
    }
```

- [ ] **Step 2: Run tests to confirm they fail**

```bash
cargo test types::card_object
```

Expected: compile errors — `has_keyword` method doesn't exist, `damaged_by_deathtouch` field doesn't exist.

- [ ] **Step 3: Update `src/types/card_object.rs`**

Replace the file with this updated version (changes: remove `has_ability`, add `has_keyword`, add `damaged_by_deathtouch`, update `can_attack`):

```rust
use super::ids::{ObjectId, PlayerId};
use super::zone::Zone;
use super::card::CardDefinition;
use super::ability::{AbilityAST, StaticAbility};

/// A card object in the game — a unique instance distinct from its definition.
/// Multiple copies of "Grizzly Bears" each have their own ObjectId.
#[derive(Debug, Clone)]
pub struct CardObject {
    pub id: ObjectId,
    pub definition: CardDefinition,
    /// Current P/T, which may diverge from printed P/T once effects exist.
    pub current_power: Option<i32>,
    pub current_toughness: Option<i32>,
    pub damage_marked: u32,
    /// True if this creature has been dealt damage by a source with deathtouch
    /// since the last time state-based actions were checked (CR 704.5h).
    pub damaged_by_deathtouch: bool,
    pub controller: PlayerId,
    pub owner: PlayerId,
    pub zone: Zone,
    pub tapped: bool,
    /// True until the controller's next untap step (CR 302.6).
    pub summoning_sick: bool,
}

impl CardObject {
    pub fn new(id: ObjectId, definition: CardDefinition, owner: PlayerId, zone: Zone) -> Self {
        let power = definition.power;
        let toughness = definition.toughness;
        Self {
            id,
            definition,
            current_power: power,
            current_toughness: toughness,
            damage_marked: 0,
            damaged_by_deathtouch: false,
            controller: owner,
            owner,
            zone,
            tapped: false,
            summoning_sick: true,
        }
    }

    pub fn is_creature(&self) -> bool { self.definition.type_line.is_creature() }
    pub fn is_land(&self)    -> bool { self.definition.type_line.is_land() }

    pub fn effective_power(&self)     -> Option<i32> { self.current_power }
    pub fn effective_toughness(&self) -> Option<i32> { self.current_toughness }

    /// Returns true if this object has the given static keyword ability in its parsed AST.
    pub fn has_keyword(&self, kw: StaticAbility) -> bool {
        self.definition.abilities.iter().any(|a| {
            matches!(a, AbilityAST::Static(k) if *k == kw)
        })
    }

    pub fn can_attack(&self) -> bool {
        self.is_creature()
            && self.zone == Zone::Battlefield
            && !self.tapped
            && (!self.summoning_sick || self.has_keyword(StaticAbility::Haste))
    }

    pub fn can_block(&self) -> bool {
        self.is_creature()
            && self.zone == Zone::Battlefield
            && !self.tapped
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cards::test_helpers::test_db;

    fn grizzly_bears() -> super::super::card::CardDefinition {
        test_db().get("Grizzly Bears").unwrap().clone()
    }

    #[test]
    fn new_creature_enters_summoning_sick() {
        let obj = CardObject::new(ObjectId(1), grizzly_bears(), PlayerId(0), Zone::Battlefield);
        assert!(obj.summoning_sick);
        assert!(!obj.can_attack());
    }

    #[test]
    fn creature_can_attack_after_sickness_cleared() {
        let mut obj =
            CardObject::new(ObjectId(1), grizzly_bears(), PlayerId(0), Zone::Battlefield);
        obj.summoning_sick = false;
        assert!(obj.can_attack());
    }

    #[test]
    fn tapped_creature_cannot_attack_or_block() {
        let mut obj =
            CardObject::new(ObjectId(1), grizzly_bears(), PlayerId(0), Zone::Battlefield);
        obj.summoning_sick = false;
        obj.tapped = true;
        assert!(!obj.can_attack());
        assert!(!obj.can_block());
    }

    #[test]
    fn has_keyword_returns_true_for_matching_ability() {
        use crate::types::{AbilityAST, ability::StaticAbility};
        let mut def = grizzly_bears();
        def.abilities = vec![AbilityAST::Static(StaticAbility::Flying)];
        let obj = CardObject::new(ObjectId(1), def, PlayerId(0), Zone::Battlefield);
        assert!(obj.has_keyword(StaticAbility::Flying));
        assert!(!obj.has_keyword(StaticAbility::Trample));
    }

    #[test]
    fn summoning_sick_creature_with_haste_can_attack() {
        use crate::types::{AbilityAST, ability::StaticAbility};
        let mut def = grizzly_bears();
        def.abilities = vec![AbilityAST::Static(StaticAbility::Haste)];
        let mut obj = CardObject::new(ObjectId(1), def, PlayerId(0), Zone::Battlefield);
        obj.summoning_sick = true;
        assert!(obj.can_attack());
    }

    #[test]
    fn damaged_by_deathtouch_initialises_false() {
        let obj = CardObject::new(ObjectId(1), grizzly_bears(), PlayerId(0), Zone::Battlefield);
        assert!(!obj.damaged_by_deathtouch);
    }
}
```

- [ ] **Step 4: Export `StaticAbility` from `src/types/mod.rs`**

Add to the re-exports in `src/types/mod.rs`:

```rust
pub use ability::{AbilityAST, StaticAbility, TriggerEvent, TriggeredAbility, ActivatedAbility};
```

(Replace the existing `pub use ability::{...}` line — `StaticAbility` was already exported but now it has variants, so no import change is needed. Just verify it's there.)

- [ ] **Step 5: Run tests**

```bash
cargo test types::card_object
```

Expected: 6 tests pass.

- [ ] **Step 6: Run the full suite to catch any `has_ability` call sites**

```bash
cargo test 2>&1 | grep -E "error|warning.*has_ability"
```

Expected: no errors. (`has_ability` was only called in the now-replaced test.)

- [ ] **Step 7: Commit**

```bash
git add src/types/card_object.rs src/types/mod.rs
git commit -m "feat: add has_keyword, damaged_by_deathtouch, update can_attack for Haste"
```

---

## Task 4: `extra_steps` queue and `first_strike_done` flag

**Files:**
- Modify: `src/types/game_state.rs`

- [ ] **Step 1: Write failing tests**

Add to the `#[cfg(test)] mod tests` block in `src/types/game_state.rs`:

```rust
    #[test]
    fn extra_steps_starts_empty() {
        let gs = two_player_state();
        assert!(gs.extra_steps.is_empty());
    }

    #[test]
    fn first_strike_done_starts_false() {
        let gs = two_player_state();
        assert!(!gs.combat.first_strike_done);
    }
```

- [ ] **Step 2: Run to confirm failure**

```bash
cargo test types::game_state
```

Expected: compile errors — fields don't exist yet.

- [ ] **Step 3: Add the fields to `src/types/game_state.rs`**

Add `use std::collections::VecDeque;` at the top (alongside the existing `use std::collections::HashMap;`).

In `CombatState`:

```rust
#[derive(Debug, Clone)]
pub struct CombatState {
    pub attackers: Vec<ObjectId>,
    pub blocking_map: HashMap<ObjectId, Vec<ObjectId>>,
    /// True after the first-strike combat damage round has resolved (CR 510.4).
    pub first_strike_done: bool,
}

impl CombatState {
    pub fn empty() -> Self {
        Self {
            attackers: vec![],
            blocking_map: HashMap::new(),
            first_strike_done: false,
        }
    }
}
```

In `GameState`, add the field after `combat`:

```rust
    pub combat: CombatState,
    /// Extra steps queued for dynamic insertion (e.g. second combat damage step per CR 510.4,
    /// or extra combat phases from card effects). `advance_step` pops from this before
    /// following the static turn sequence.
    pub(crate) extra_steps: VecDeque<Step>,
```

In `GameState::new`, initialise it:

```rust
            combat: CombatState::empty(),
            extra_steps: VecDeque::new(),
```

- [ ] **Step 4: Run tests**

```bash
cargo test types::game_state
```

Expected: all tests pass including the two new ones.

- [ ] **Step 5: Run the full suite**

```bash
cargo test
```

Expected: all tests pass.

- [ ] **Step 6: Commit**

```bash
git add src/types/game_state.rs
git commit -m "feat: add extra_steps queue and CombatState::first_strike_done"
```

---

## Task 5: `advance_step` queue check

**Files:**
- Modify: `src/engine/turn.rs`

- [ ] **Step 1: Write a failing test**

Add to the test block in `src/engine/turn.rs`:

```rust
    #[test]
    fn advance_step_consumes_extra_steps_before_static_sequence() {
        let mut gs = make_state();
        gs.step = Step::CombatDamage;
        gs.extra_steps.push_back(Step::CombatDamage); // simulate second combat damage round

        let gs = advance_step(gs);

        // Should have consumed the queued step, not gone to EndOfCombat
        assert_eq!(gs.step(), Step::CombatDamage);
        assert!(gs.extra_steps.is_empty());
    }
```

- [ ] **Step 2: Run to confirm failure**

```bash
cargo test engine::turn::tests::advance_step_consumes_extra_steps
```

Expected: FAIL — advance_step goes to EndOfCombat, not CombatDamage.

- [ ] **Step 3: Update `advance_step` in `src/engine/turn.rs`**

```rust
/// Advance to the next step/phase. Checks `extra_steps` queue first (for dynamically
/// inserted steps such as the second combat damage round per CR 510.4).
pub fn advance_step(state: GameState) -> GameState {
    let mut state = state;
    if let Some(next) = state.extra_steps.pop_front() {
        state.step = next;
        return state;
    }
    match state.step {
        Step::Untap              => set(state, Step::Upkeep),
        Step::Upkeep             => set(state, Step::Draw),
        Step::Draw               => set(state, Step::PreCombatMain),
        Step::PreCombatMain      => set(state, Step::BeginningOfCombat),
        Step::BeginningOfCombat  => set(state, Step::DeclareAttackers),
        Step::DeclareAttackers   => set(state, Step::DeclareBlockers),
        Step::DeclareBlockers    => set(state, Step::CombatDamage),
        Step::CombatDamage       => set(state, Step::EndOfCombat),
        Step::EndOfCombat        => set(state, Step::PostCombatMain),
        Step::PostCombatMain     => set(state, Step::End),
        Step::End                => set(state, Step::Cleanup),
        Step::Cleanup            => start_next_turn(state),
    }
}
```

- [ ] **Step 4: Run tests**

```bash
cargo test engine::turn
```

Expected: all turn tests pass including the new one.

- [ ] **Step 5: Commit**

```bash
git add src/engine/turn.rs
git commit -m "feat: advance_step pops extra_steps queue before static sequence"
```

---

## Task 6: New `EngineError` variants, Vigilance, and Haste

**Files:**
- Modify: `src/engine/mod.rs`
- Modify: `src/engine/combat.rs`

- [ ] **Step 1: Add new `EngineError` variants to `src/engine/mod.rs`**

```rust
#[derive(Debug, Clone, PartialEq)]
pub enum EngineError {
    CardNotFound,
    CardNotInHand,
    CardNotOnBattlefield,
    AlreadyTapped,
    InsufficientMana,
    CannotCastNow,
    LandLimitReached,
    NotALand,
    NotACreature,
    NotYourCard,
    SummoningSick,
    CreatureTapped,
    InvalidBlocker,              // blocker can't legally block this attacker
    MenaceRequiresTwoBlockers,   // menace attacker has exactly one blocker
}
```

- [ ] **Step 2: Write failing tests for Vigilance and Haste in `src/engine/combat.rs`**

Add these tests to the existing `#[cfg(test)] mod tests` block in `combat.rs`. First add a new helper at the top of the test module (after existing helpers):

```rust
    fn keyword_creature(
        state: &mut GameState,
        owner: PlayerId,
        power: i32,
        toughness: i32,
        keywords: Vec<crate::types::ability::StaticAbility>,
    ) -> ObjectId {
        use crate::types::{AbilityAST, CardDefinition, card::{CardType, TypeLine}};
        let id = state.alloc_id();
        let def = CardDefinition {
            name: "Test Creature".into(),
            mana_cost: None,
            type_line: TypeLine {
                supertypes: vec![],
                card_types: vec![CardType::Creature],
                subtypes: vec![],
            },
            oracle_text: String::new(),
            abilities: keywords.into_iter()
                .map(|k| AbilityAST::Static(k))
                .collect(),
            power: Some(power),
            toughness: Some(toughness),
        };
        let mut obj = crate::types::CardObject::new(id, def, owner, Zone::Battlefield);
        obj.summoning_sick = false;
        state.battlefield.push(id);
        state.add_object(obj);
        id
    }
```

Then add the new tests:

```rust
    #[test]
    fn vigilant_attacker_does_not_tap() {
        use crate::types::ability::StaticAbility;
        let mut gs = make_combat_state();
        let id = keyword_creature(&mut gs, PlayerId(0), 2, 2, vec![StaticAbility::Vigilance]);
        let gs = declare_attackers(gs, PlayerId(0), &[id]).unwrap();
        assert!(!gs.objects[&id].tapped); // vigilance: does not tap when attacking
    }

    #[test]
    fn haste_creature_can_attack_while_summoning_sick() {
        use crate::types::ability::StaticAbility;
        let mut gs = make_combat_state();
        let id = keyword_creature(&mut gs, PlayerId(0), 2, 2, vec![StaticAbility::Haste]);
        gs.objects.get_mut(&id).unwrap().summoning_sick = true; // still sick
        // Should be able to declare it as attacker
        let gs = declare_attackers(gs, PlayerId(0), &[id]).unwrap();
        assert!(gs.combat.attackers.contains(&id));
    }
```

- [ ] **Step 3: Run to confirm failure**

```bash
cargo test engine::combat::tests::vigilant_attacker_does_not_tap engine::combat::tests::haste_creature_can_attack
```

Expected: FAIL — vigilant creatures still tap; summoning-sick haste creature is rejected.

- [ ] **Step 4: Update `declare_attackers` in `src/engine/combat.rs`**

Locate the two relevant sections:

**Summoning sickness check** — change:
```rust
        // Before:
        if obj.summoning_sick {
            return Err(EngineError::SummoningSick);
        }

        // After:
        if obj.summoning_sick && !obj.has_keyword(StaticAbility::Haste) {
            return Err(EngineError::SummoningSick);
        }
```

Add the import at the top of the file if not present:
```rust
use crate::types::ability::StaticAbility;
```

**Tap loop** — change:
```rust
    // Before:
    for &id in attacker_ids {
        state.objects.get_mut(&id).unwrap().tapped = true;
    }

    // After:
    for &id in attacker_ids {
        if !state.objects.get(&id).unwrap().has_keyword(StaticAbility::Vigilance) {
            state.objects.get_mut(&id).unwrap().tapped = true;
        }
    }
```

- [ ] **Step 5: Run tests**

```bash
cargo test engine::combat
```

Expected: all combat tests pass including the two new ones.

- [ ] **Step 6: Commit**

```bash
git add src/engine/mod.rs src/engine/combat.rs
git commit -m "feat: Vigilance (no tap on attack) and Haste (ignore summoning sickness)"
```

---

## Task 7: Flying, Reach, and Menace in `declare_blockers`

**Files:**
- Modify: `src/engine/combat.rs`

- [ ] **Step 1: Write failing tests**

Add to the test module in `combat.rs` (the `keyword_creature` helper from Task 6 is already available):

```rust
    #[test]
    fn non_flier_cannot_block_flier() {
        use crate::types::ability::StaticAbility;
        let mut gs = make_combat_state();
        let attacker = keyword_creature(&mut gs, PlayerId(0), 2, 2, vec![StaticAbility::Flying]);
        let blocker  = keyword_creature(&mut gs, PlayerId(1), 2, 2, vec![]); // no flying/reach
        gs = declare_attackers(gs, PlayerId(0), &[attacker]).unwrap();
        gs.step = Step::DeclareBlockers;
        assert!(matches!(
            declare_blockers(gs, PlayerId(1), &[(blocker, attacker)]),
            Err(EngineError::InvalidBlocker)
        ));
    }

    #[test]
    fn flier_can_block_flier() {
        use crate::types::ability::StaticAbility;
        let mut gs = make_combat_state();
        let attacker = keyword_creature(&mut gs, PlayerId(0), 2, 2, vec![StaticAbility::Flying]);
        let blocker  = keyword_creature(&mut gs, PlayerId(1), 2, 2, vec![StaticAbility::Flying]);
        gs = declare_attackers(gs, PlayerId(0), &[attacker]).unwrap();
        gs.step = Step::DeclareBlockers;
        assert!(declare_blockers(gs, PlayerId(1), &[(blocker, attacker)]).is_ok());
    }

    #[test]
    fn reach_creature_can_block_flier() {
        use crate::types::ability::StaticAbility;
        let mut gs = make_combat_state();
        let attacker = keyword_creature(&mut gs, PlayerId(0), 2, 2, vec![StaticAbility::Flying]);
        let blocker  = keyword_creature(&mut gs, PlayerId(1), 2, 2, vec![StaticAbility::Reach]);
        gs = declare_attackers(gs, PlayerId(0), &[attacker]).unwrap();
        gs.step = Step::DeclareBlockers;
        assert!(declare_blockers(gs, PlayerId(1), &[(blocker, attacker)]).is_ok());
    }

    #[test]
    fn menace_requires_two_or_more_blockers() {
        use crate::types::ability::StaticAbility;
        let mut gs = make_combat_state();
        let attacker = keyword_creature(&mut gs, PlayerId(0), 2, 2, vec![StaticAbility::Menace]);
        let blocker  = keyword_creature(&mut gs, PlayerId(1), 2, 2, vec![]);
        gs = declare_attackers(gs, PlayerId(0), &[attacker]).unwrap();
        gs.step = Step::DeclareBlockers;
        // Exactly one blocker → illegal
        assert!(matches!(
            declare_blockers(gs, PlayerId(1), &[(blocker, attacker)]),
            Err(EngineError::MenaceRequiresTwoBlockers)
        ));
    }

    #[test]
    fn menace_allows_two_blockers() {
        use crate::types::ability::StaticAbility;
        let mut gs = make_combat_state();
        let attacker = keyword_creature(&mut gs, PlayerId(0), 4, 4, vec![StaticAbility::Menace]);
        let blocker1 = keyword_creature(&mut gs, PlayerId(1), 2, 2, vec![]);
        let blocker2 = keyword_creature(&mut gs, PlayerId(1), 2, 2, vec![]);
        gs = declare_attackers(gs, PlayerId(0), &[attacker]).unwrap();
        gs.step = Step::DeclareBlockers;
        assert!(declare_blockers(gs, PlayerId(1), &[(blocker1, attacker), (blocker2, attacker)]).is_ok());
    }

    #[test]
    fn menace_allows_zero_blockers() {
        use crate::types::ability::StaticAbility;
        let mut gs = make_combat_state();
        let attacker = keyword_creature(&mut gs, PlayerId(0), 2, 2, vec![StaticAbility::Menace]);
        gs = declare_attackers(gs, PlayerId(0), &[attacker]).unwrap();
        gs.step = Step::DeclareBlockers;
        // No blockers declared — legal (creature is unblocked)
        assert!(declare_blockers(gs, PlayerId(1), &[]).is_ok());
    }
```

- [ ] **Step 2: Run to confirm failure**

```bash
cargo test engine::combat::tests::non_flier engine::combat::tests::flier engine::combat::tests::reach engine::combat::tests::menace
```

Expected: all 6 new tests fail.

- [ ] **Step 3: Update `declare_blockers` in `src/engine/combat.rs`**

Inside the per-pair validation loop (after the existing checks for `tapped`, `is_creature`, and `attackers.contains`), add the flying/reach check:

```rust
        // CR 702.9b: a creature with flying can only be blocked by creatures with flying or reach.
        if state.objects.get(&attacker_id)
            .map(|a| a.has_keyword(StaticAbility::Flying))
            .unwrap_or(false)
        {
            if !obj.has_keyword(StaticAbility::Flying) && !obj.has_keyword(StaticAbility::Reach) {
                return Err(EngineError::InvalidBlocker);
            }
        }
```

After the blocking_map is built (at the end of `declare_blockers`, before `Ok(state)`), add the menace check:

```rust
    // CR 702.111b: a creature with menace can't be blocked by exactly one creature.
    for &attacker_id in &state.combat.attackers {
        if state.objects.get(&attacker_id)
            .map(|a| a.has_keyword(StaticAbility::Menace))
            .unwrap_or(false)
        {
            let num_blockers = state.combat.blocking_map
                .get(&attacker_id)
                .map(|v| v.len())
                .unwrap_or(0);
            if num_blockers == 1 {
                return Err(EngineError::MenaceRequiresTwoBlockers);
            }
        }
    }
```

- [ ] **Step 4: Run tests**

```bash
cargo test engine::combat
```

Expected: all combat tests pass.

- [ ] **Step 5: Commit**

```bash
git add src/engine/combat.rs
git commit -m "feat: Flying/Reach blocking restriction and Menace two-blocker requirement"
```

---

## Task 8: First Strike and Double Strike

**Files:**
- Modify: `src/engine/combat.rs`

- [ ] **Step 1: Write failing tests**

Add to the test module:

```rust
    #[test]
    fn first_striker_kills_blocker_before_it_can_deal_damage() {
        // 3/2 First Strike vs 2/2 vanilla:
        // Round 1: first striker deals 3 (lethal to 2/2). 2/2 can't deal back.
        // Round 2: 2/2 is dead, no damage back to first striker.
        use crate::types::ability::StaticAbility;
        let mut gs = make_combat_state();
        let attacker = keyword_creature(&mut gs, PlayerId(0), 3, 2, vec![StaticAbility::FirstStrike]);
        let blocker  = keyword_creature(&mut gs, PlayerId(1), 2, 2, vec![]);
        gs = declare_attackers(gs, PlayerId(0), &[attacker]).unwrap();
        gs.step = Step::DeclareBlockers;
        gs = declare_blockers(gs, PlayerId(1), &[(blocker, attacker)]).unwrap();
        gs.step = Step::CombatDamage;

        // Round 1
        let gs = deal_combat_damage(gs);

        // Blocker should be dead; attacker should be undamaged
        assert!(!gs.battlefield.contains(&blocker));
        assert_eq!(gs.objects[&attacker].damage_marked, 0);
        // A second CombatDamage step should be queued
        assert!(!gs.extra_steps.is_empty());

        // Advance to second round
        let gs = advance_step(gs); // pops extra_steps → CombatDamage
        let gs = deal_combat_damage(gs);

        // No blockers left — attacker still undamaged; player untouched (attacker was blocked)
        assert_eq!(gs.objects[&attacker].damage_marked, 0);
        assert_eq!(gs.get_player(PlayerId(1)).unwrap().life, 20);
    }

    #[test]
    fn double_striker_deals_damage_in_both_rounds() {
        // 2/2 Double Strike vs 3/3:
        // Round 1: double striker deals 2. Round 2: double striker deals another 2.
        // 3/3 deals 3 in round 2. 3/3 has 4 damage total (lethal), double striker has 3 (lethal).
        use crate::types::ability::StaticAbility;
        let mut gs = make_combat_state();
        let attacker = keyword_creature(&mut gs, PlayerId(0), 2, 2, vec![StaticAbility::DoubleStrike]);
        let blocker  = keyword_creature(&mut gs, PlayerId(1), 3, 3, vec![]);
        gs = declare_attackers(gs, PlayerId(0), &[attacker]).unwrap();
        gs.step = Step::DeclareBlockers;
        gs = declare_blockers(gs, PlayerId(1), &[(blocker, attacker)]).unwrap();
        gs.step = Step::CombatDamage;

        // Round 1: only double striker deals damage
        let gs = deal_combat_damage(gs);
        assert_eq!(gs.objects[&blocker].damage_marked, 2);
        assert_eq!(gs.objects[&attacker].damage_marked, 0); // blocker hasn't dealt yet

        // Round 2: double striker AND non-first-strikers (none; blocker is vanilla) deal damage
        let gs = advance_step(gs);
        assert_eq!(gs.step(), Step::CombatDamage);
        let gs = deal_combat_damage(gs);

        // Both die
        assert!(!gs.battlefield.contains(&blocker));
        assert!(!gs.battlefield.contains(&attacker));
    }

    #[test]
    fn no_first_strikers_means_single_round_and_no_extra_step() {
        // Vanilla combat: no extra step should be queued
        let db = test_db();
        let mut gs = make_combat_state();
        let attacker = add_creature(&mut gs, PlayerId(0), db.get("Grizzly Bears").unwrap().clone());
        let blocker  = add_creature(&mut gs, PlayerId(1), db.get("Grizzly Bears").unwrap().clone());
        gs = declare_attackers(gs, PlayerId(0), &[attacker]).unwrap();
        gs.step = Step::DeclareBlockers;
        gs = declare_blockers(gs, PlayerId(1), &[(blocker, attacker)]).unwrap();
        gs.step = Step::CombatDamage;

        let gs = deal_combat_damage(gs);

        // Both die; no extra step queued
        assert!(!gs.battlefield.contains(&attacker));
        assert!(!gs.battlefield.contains(&blocker));
        assert!(gs.extra_steps.is_empty());
    }
```

- [ ] **Step 2: Run to confirm failure**

```bash
cargo test engine::combat::tests::first_striker engine::combat::tests::double_striker engine::combat::tests::no_first_strikers
```

Expected: all 3 tests fail.

- [ ] **Step 3: Replace `deal_combat_damage` in `src/engine/combat.rs`**

Replace the entire `deal_combat_damage` function with this version, which handles first/double strike round selection. (Trample, lifelink, and deathtouch are added in the next task.)

```rust
pub fn deal_combat_damage(mut state: GameState) -> GameState {
    let defending_player = state.opponent_of(state.active_player);
    let attackers = state.combat.attackers.clone();
    let blocking_map = state.combat.blocking_map.clone();

    // Determine whether any participating creature has first or double strike.
    let any_first_or_double = attackers.iter()
        .chain(blocking_map.values().flatten())
        .any(|&id| state.objects.get(&id).map(|o|
            o.has_keyword(StaticAbility::FirstStrike) || o.has_keyword(StaticAbility::DoubleStrike)
        ).unwrap_or(false));

    let first_round  = any_first_or_double && !state.combat.first_strike_done;
    let second_round = any_first_or_double && state.combat.first_strike_done;

    // If this is the first-strike round, queue the regular damage round (CR 510.4).
    if first_round {
        state.combat.first_strike_done = true;
        state.extra_steps.push_back(Step::CombatDamage);
    }

    // Determine which creatures deal damage this round:
    //   first round  → first strike / double strike only
    //   second round → any creature WITHOUT first strike (double strikers included)
    //   no first/double strikers → all creatures deal (standard single round)
    let deals_this_round = |id: ObjectId| -> bool {
        let Some(obj) = state.objects.get(&id) else { return false };
        if first_round {
            obj.has_keyword(StaticAbility::FirstStrike) || obj.has_keyword(StaticAbility::DoubleStrike)
        } else if second_round {
            !obj.has_keyword(StaticAbility::FirstStrike)
        } else {
            true
        }
    };

    let mut damage_to_players: HashMap<PlayerId, i32> = HashMap::new();
    let mut damage_to_objects: HashMap<ObjectId, u32> = HashMap::new();

    for &attacker_id in &attackers {
        if !deals_this_round(attacker_id) { continue; }

        let attacker_power = state.objects.get(&attacker_id)
            .and_then(|o| o.effective_power())
            .map(|p| p.max(0) as u32)
            .unwrap_or(0);

        let blockers = blocking_map.get(&attacker_id).cloned().unwrap_or_default();

        if blockers.is_empty() {
            *damage_to_players.entry(defending_player).or_insert(0) += attacker_power as i32;
        } else {
            let mut remaining = attacker_power;
            for (i, &blocker_id) in blockers.iter().enumerate() {
                if remaining == 0 { break; }
                let is_last = i == blockers.len() - 1;
                let assign = if is_last {
                    remaining
                } else {
                    let toughness = state.objects.get(&blocker_id)
                        .and_then(|o| o.effective_toughness())
                        .map(|t| t.max(0) as u32)
                        .unwrap_or(0);
                    remaining.min(toughness.max(1))
                };
                *damage_to_objects.entry(blocker_id).or_insert(0) += assign;
                remaining -= assign;
            }
        }

        for &blocker_id in &blockers {
            if !deals_this_round(blocker_id) { continue; }
            let blocker_power = state.objects.get(&blocker_id)
                .and_then(|o| o.effective_power())
                .map(|p| p.max(0) as u32)
                .unwrap_or(0);
            *damage_to_objects.entry(attacker_id).or_insert(0) += blocker_power;
        }
    }

    for (pid, dmg) in damage_to_players {
        if let Some(p) = state.get_player_mut(pid) { p.life -= dmg; }
    }
    for (oid, dmg) in damage_to_objects {
        if let Some(obj) = state.objects.get_mut(&oid) { obj.damage_marked += dmg; }
    }

    check_and_apply_sbas(state)
}
```

Add `use crate::types::Step;` to the imports at the top of `combat.rs` if not already present.

- [ ] **Step 4: Run tests**

```bash
cargo test engine::combat
```

Expected: all combat tests pass.

- [ ] **Step 5: Commit**

```bash
git add src/engine/combat.rs
git commit -m "feat: first strike and double strike two-round combat damage (CR 510.4)"
```

---

## Task 9: Trample, Lifelink, and Deathtouch

**Files:**
- Modify: `src/engine/combat.rs`

- [ ] **Step 1: Write failing tests**

Add to the test module:

```rust
    #[test]
    fn trample_sends_excess_to_player() {
        // 5/5 Trample vs 2/2 blocker: 2 to blocker (lethal), 3 tramples through
        use crate::types::ability::StaticAbility;
        let mut gs = make_combat_state();
        let attacker = keyword_creature(&mut gs, PlayerId(0), 5, 5, vec![StaticAbility::Trample]);
        let blocker  = keyword_creature(&mut gs, PlayerId(1), 2, 2, vec![]);
        gs = declare_attackers(gs, PlayerId(0), &[attacker]).unwrap();
        gs.step = Step::DeclareBlockers;
        gs = declare_blockers(gs, PlayerId(1), &[(blocker, attacker)]).unwrap();
        gs.step = Step::CombatDamage;

        let gs = deal_combat_damage(gs);

        assert!(!gs.battlefield.contains(&blocker));
        assert_eq!(gs.get_player(PlayerId(1)).unwrap().life, 17); // 20 - 3
    }

    #[test]
    fn trample_deathtouch_one_damage_is_lethal_per_blocker() {
        // 5/5 Trample + Deathtouch vs 4/4 blocker: 1 damage is lethal (deathtouch),
        // 4 tramples through to defending player
        use crate::types::ability::StaticAbility;
        let mut gs = make_combat_state();
        let attacker = keyword_creature(
            &mut gs, PlayerId(0), 5, 5,
            vec![StaticAbility::Trample, StaticAbility::Deathtouch],
        );
        let blocker = keyword_creature(&mut gs, PlayerId(1), 4, 4, vec![]);
        gs = declare_attackers(gs, PlayerId(0), &[attacker]).unwrap();
        gs.step = Step::DeclareBlockers;
        gs = declare_blockers(gs, PlayerId(1), &[(blocker, attacker)]).unwrap();
        gs.step = Step::CombatDamage;

        let gs = deal_combat_damage(gs);

        assert!(!gs.battlefield.contains(&blocker)); // 1 deathtouch damage kills 4/4
        assert_eq!(gs.get_player(PlayerId(1)).unwrap().life, 16); // 20 - 4 trample
    }

    #[test]
    fn lifelink_attacker_gains_life_from_combat_damage() {
        // 3/3 Lifelink unblocked: controller gains 3 life
        use crate::types::ability::StaticAbility;
        let mut gs = make_combat_state();
        let attacker = keyword_creature(&mut gs, PlayerId(0), 3, 3, vec![StaticAbility::Lifelink]);
        gs = declare_attackers(gs, PlayerId(0), &[attacker]).unwrap();
        gs.step = Step::DeclareBlockers;
        gs = declare_blockers(gs, PlayerId(1), &[]).unwrap();
        gs.step = Step::CombatDamage;

        let gs = deal_combat_damage(gs);

        assert_eq!(gs.get_player(PlayerId(0)).unwrap().life, 23); // 20 + 3
        assert_eq!(gs.get_player(PlayerId(1)).unwrap().life, 17); // 20 - 3
    }

    #[test]
    fn deathtouch_marks_target_for_sba() {
        // 1/1 Deathtouch vs 4/4: deathtouch creature deals 1 damage, flag set on 4/4
        use crate::types::ability::StaticAbility;
        let mut gs = make_combat_state();
        let attacker = keyword_creature(&mut gs, PlayerId(0), 1, 1, vec![StaticAbility::Deathtouch]);
        let blocker  = keyword_creature(&mut gs, PlayerId(1), 4, 4, vec![]);
        gs = declare_attackers(gs, PlayerId(0), &[attacker]).unwrap();
        gs.step = Step::DeclareBlockers;
        gs = declare_blockers(gs, PlayerId(1), &[(blocker, attacker)]).unwrap();
        gs.step = Step::CombatDamage;

        let gs = deal_combat_damage(gs);

        // 4/4 received deathtouch damage → SBAs already ran → it should be dead
        assert!(!gs.battlefield.contains(&blocker));
        // 1/1 received 4 damage (lethal) → also dead
        assert!(!gs.battlefield.contains(&attacker));
    }
```

- [ ] **Step 2: Run to confirm failure**

```bash
cargo test engine::combat::tests::trample engine::combat::tests::lifelink engine::combat::tests::deathtouch
```

Expected: trample test fails (excess goes to last blocker, not player); lifelink test fails (no life gain); deathtouch test fails (4/4 survives 1 damage).

- [ ] **Step 3: Replace `deal_combat_damage` with the full implementation**

Replace the entire function:

```rust
pub fn deal_combat_damage(mut state: GameState) -> GameState {
    use std::collections::HashSet;

    let defending_player = state.opponent_of(state.active_player);
    let attackers = state.combat.attackers.clone();
    let blocking_map = state.combat.blocking_map.clone();

    let any_first_or_double = attackers.iter()
        .chain(blocking_map.values().flatten())
        .any(|&id| state.objects.get(&id).map(|o|
            o.has_keyword(StaticAbility::FirstStrike) || o.has_keyword(StaticAbility::DoubleStrike)
        ).unwrap_or(false));

    let first_round  = any_first_or_double && !state.combat.first_strike_done;
    let second_round = any_first_or_double && state.combat.first_strike_done;

    if first_round {
        state.combat.first_strike_done = true;
        state.extra_steps.push_back(Step::CombatDamage);
    }

    let deals_this_round = |id: ObjectId| -> bool {
        let Some(obj) = state.objects.get(&id) else { return false };
        if first_round {
            obj.has_keyword(StaticAbility::FirstStrike) || obj.has_keyword(StaticAbility::DoubleStrike)
        } else if second_round {
            !obj.has_keyword(StaticAbility::FirstStrike)
        } else {
            true
        }
    };

    let mut damage_to_players: HashMap<PlayerId, i32> = HashMap::new();
    let mut damage_to_objects: HashMap<ObjectId, u32>  = HashMap::new();
    let mut lifelink_gain:     HashMap<PlayerId, i32> = HashMap::new();
    let mut deathtouch_targets: HashSet<ObjectId>     = HashSet::new();

    for &attacker_id in &attackers {
        if !deals_this_round(attacker_id) { continue; }

        let (atk_power, has_trample, has_deathtouch, has_lifelink, atk_controller) = {
            let obj = match state.objects.get(&attacker_id) { Some(o) => o, None => continue };
            (
                obj.effective_power().map(|p| p.max(0) as u32).unwrap_or(0),
                obj.has_keyword(StaticAbility::Trample),
                obj.has_keyword(StaticAbility::Deathtouch),
                obj.has_keyword(StaticAbility::Lifelink),
                obj.controller,
            )
        };

        let blockers = blocking_map.get(&attacker_id).cloned().unwrap_or_default();
        let mut total_damage_dealt = 0u32;

        if blockers.is_empty() {
            *damage_to_players.entry(defending_player).or_insert(0) += atk_power as i32;
            total_damage_dealt = atk_power;
        } else {
            let mut remaining = atk_power;
            for &blocker_id in &blockers {
                if remaining == 0 { break; }
                // Lethal threshold: 1 if attacker has deathtouch (CR 702.2c), else remaining toughness.
                let lethal = if has_deathtouch {
                    1u32
                } else {
                    state.objects.get(&blocker_id)
                        .and_then(|o| o.effective_toughness())
                        .map(|t| (t.max(0) as u32)
                            .saturating_sub(state.objects.get(&blocker_id)
                                .map(|o| o.damage_marked).unwrap_or(0)))
                        .unwrap_or(0)
                        .max(1)
                };
                let assign = remaining.min(lethal);
                *damage_to_objects.entry(blocker_id).or_insert(0) += assign;
                remaining -= assign;
                total_damage_dealt += assign;
                if has_deathtouch && assign > 0 {
                    deathtouch_targets.insert(blocker_id);
                }
            }
            // Remaining damage: to player if trample, otherwise pile on last blocker.
            if remaining > 0 {
                if has_trample {
                    *damage_to_players.entry(defending_player).or_insert(0) += remaining as i32;
                    total_damage_dealt += remaining;
                } else if let Some(&last) = blockers.last() {
                    *damage_to_objects.entry(last).or_insert(0) += remaining;
                    if has_deathtouch {
                        deathtouch_targets.insert(last);
                    }
                    total_damage_dealt += remaining;
                }
            }
        }

        if has_lifelink && total_damage_dealt > 0 {
            *lifelink_gain.entry(atk_controller).or_insert(0) += total_damage_dealt as i32;
        }

        // Blockers deal their damage back to the attacker.
        for &blocker_id in &blockers {
            if !deals_this_round(blocker_id) { continue; }
            let (blk_power, blk_deathtouch, blk_lifelink, blk_controller) = {
                let obj = match state.objects.get(&blocker_id) { Some(o) => o, None => continue };
                (
                    obj.effective_power().map(|p| p.max(0) as u32).unwrap_or(0),
                    obj.has_keyword(StaticAbility::Deathtouch),
                    obj.has_keyword(StaticAbility::Lifelink),
                    obj.controller,
                )
            };
            if blk_power > 0 {
                *damage_to_objects.entry(attacker_id).or_insert(0) += blk_power;
                if blk_deathtouch { deathtouch_targets.insert(attacker_id); }
                if blk_lifelink {
                    *lifelink_gain.entry(blk_controller).or_insert(0) += blk_power as i32;
                }
            }
        }
    }

    // Apply all damage and effects simultaneously.
    for (pid, dmg) in &damage_to_players {
        if let Some(p) = state.get_player_mut(*pid) { p.life -= dmg; }
    }
    for (oid, dmg) in damage_to_objects {
        if let Some(obj) = state.objects.get_mut(&oid) { obj.damage_marked += dmg; }
    }
    for oid in deathtouch_targets {
        if let Some(obj) = state.objects.get_mut(&oid) { obj.damaged_by_deathtouch = true; }
    }
    for (pid, gain) in lifelink_gain {
        if let Some(p) = state.get_player_mut(pid) { p.life += gain; }
    }

    check_and_apply_sbas(state)
}
```

Add `use std::collections::HashSet;` at the top of the file (next to the existing `HashMap` import).

- [ ] **Step 4: Run tests**

```bash
cargo test engine::combat
```

Expected: all combat tests pass.

- [ ] **Step 5: Commit**

```bash
git add src/engine/combat.rs
git commit -m "feat: Trample, Lifelink, Deathtouch in deal_combat_damage"
```

---

## Task 10: SBA updates — Indestructible and Deathtouch (704.5h)

**Files:**
- Modify: `src/engine/state_based_actions.rs`
- Modify: `src/engine/turn.rs`

- [ ] **Step 1: Write failing SBA tests**

Add to the test module in `state_based_actions.rs`:

```rust
    fn keyword_creature_on_battlefield(
        state: &mut GameState,
        owner: PlayerId,
        power: i32,
        toughness: i32,
        keywords: Vec<crate::types::ability::StaticAbility>,
    ) -> ObjectId {
        use crate::types::{AbilityAST, CardDefinition, card::{CardType, TypeLine}};
        let id = state.alloc_id();
        let def = CardDefinition {
            name: "Test".into(),
            mana_cost: None,
            type_line: TypeLine {
                supertypes: vec![],
                card_types: vec![CardType::Creature],
                subtypes: vec![],
            },
            oracle_text: String::new(),
            abilities: keywords.into_iter().map(|k| AbilityAST::Static(k)).collect(),
            power: Some(power),
            toughness: Some(toughness),
        };
        let mut obj = CardObject::new(id, def, owner, Zone::Battlefield);
        obj.summoning_sick = false;
        state.battlefield.push(id);
        state.add_object(obj);
        id
    }

    #[test]
    fn indestructible_survives_lethal_damage() {
        use crate::types::ability::StaticAbility;
        let mut gs = make_state();
        let id = keyword_creature_on_battlefield(
            &mut gs, PlayerId(0), 2, 2, vec![StaticAbility::Indestructible],
        );
        gs.objects.get_mut(&id).unwrap().damage_marked = 5; // more than toughness

        let gs = check_and_apply_sbas(gs);

        assert!(gs.battlefield.contains(&id)); // survives
    }

    #[test]
    fn deathtouch_damage_kills_non_indestructible_creature() {
        let mut gs = make_state();
        let db = test_db();
        let id = add_creature_to_battlefield(&mut gs, PlayerId(0), db.get("Hill Giant").unwrap().clone());
        gs.objects.get_mut(&id).unwrap().damaged_by_deathtouch = true; // 1 deathtouch damage

        let gs = check_and_apply_sbas(gs);

        assert!(!gs.battlefield.contains(&id));
    }

    #[test]
    fn indestructible_survives_deathtouch_damage() {
        use crate::types::ability::StaticAbility;
        let mut gs = make_state();
        let id = keyword_creature_on_battlefield(
            &mut gs, PlayerId(0), 2, 2, vec![StaticAbility::Indestructible],
        );
        gs.objects.get_mut(&id).unwrap().damaged_by_deathtouch = true;

        let gs = check_and_apply_sbas(gs);

        assert!(gs.battlefield.contains(&id)); // indestructible ignores both 704.5g and 704.5h
    }
```

- [ ] **Step 2: Run to confirm failure**

```bash
cargo test engine::state_based_actions::tests::indestructible engine::state_based_actions::tests::deathtouch
```

Expected: `indestructible_survives_lethal_damage` fails (destroyed); `deathtouch_damage_kills` fails (survives); `indestructible_survives_deathtouch` fails.

- [ ] **Step 3: Update `find_sbas` in `src/engine/state_based_actions.rs`**

Add the import:
```rust
use crate::types::ability::StaticAbility;
```

Replace the creature SBA block (currently just 704.5g) with:

```rust
    // CR 704.5g / 704.5h: creature with lethal damage or deathtouch damage is destroyed,
    // unless it has indestructible (CR 702.12b).
    for &id in &state.battlefield {
        if let Some(obj) = state.objects.get(&id) {
            if obj.is_creature() && !obj.has_keyword(StaticAbility::Indestructible) {
                let lethal_damage = obj.effective_toughness()
                    .map(|t| t <= 0 || obj.damage_marked as i32 >= t)
                    .unwrap_or(false);
                if lethal_damage || obj.damaged_by_deathtouch {
                    sbas.push(Sba::MoveToGraveyard(id));
                }
            }
        }
    }
```

Also update `move_to_graveyard` to clear `damaged_by_deathtouch` when a creature leaves the battlefield:

```rust
pub fn move_to_graveyard(mut state: GameState, object_id: ObjectId) -> GameState {
    state.battlefield.retain(|&id| id != object_id);
    if let Some(obj) = state.objects.get_mut(&object_id) {
        let owner = obj.owner;
        obj.zone = Zone::Graveyard;
        obj.damage_marked = 0;
        obj.damaged_by_deathtouch = false;
        obj.tapped = false;
        if let Some(gy) = state.graveyards.get_mut(&owner) {
            gy.push(object_id);
        }
    }
    state
}
```

- [ ] **Step 4: Clear `damaged_by_deathtouch` in cleanup step in `src/engine/turn.rs`**

Update `cleanup_step`:

```rust
fn cleanup_step(mut state: GameState) -> GameState {
    // CR 514.2: remove damage from all permanents.
    for obj in state.objects.values_mut() {
        obj.damage_marked = 0;
        obj.damaged_by_deathtouch = false;
    }
    state
}
```

- [ ] **Step 5: Run all tests**

```bash
cargo test
```

Expected: all tests pass.

- [ ] **Step 6: Commit**

```bash
git add src/engine/state_based_actions.rs src/engine/turn.rs
git commit -m "feat: Indestructible exemption from SBAs; Deathtouch SBA (CR 704.5h)"
```

---

## Task 11: Test fixture additions and integration tests

**Files:**
- Modify: `tests/fixtures/oracle_cards_test.json`
- Modify: `tests/scripted_game.rs`

- [ ] **Step 1: Add keyword creatures to `tests/fixtures/oracle_cards_test.json`**

Append four new card objects to the JSON array (before the closing `]`). These use the same Scryfall-format structure but only the fields `parse_card` actually reads (`name`, `mana_cost`, `type_line`, `oracle_text`, `power`, `toughness`):

```json
  ,
  {
    "object": "card",
    "name": "Serra Angel",
    "mana_cost": "{3}{W}{W}",
    "type_line": "Creature — Angel",
    "oracle_text": "Flying, vigilance",
    "power": "4",
    "toughness": "4"
  },
  {
    "object": "card",
    "name": "Charging Badger",
    "mana_cost": "{G}",
    "type_line": "Creature — Badger",
    "oracle_text": "Trample",
    "power": "1",
    "toughness": "1"
  },
  {
    "object": "card",
    "name": "Typhoid Rats",
    "mana_cost": "{B}",
    "type_line": "Creature — Rat",
    "oracle_text": "Deathtouch (Any amount of damage this deals to a creature is enough to destroy it.)",
    "power": "1",
    "toughness": "1"
  },
  {
    "object": "card",
    "name": "Anaba Bodyguard",
    "mana_cost": "{3}{R}",
    "type_line": "Creature — Minotaur Warrior",
    "oracle_text": "First strike",
    "power": "3",
    "toughness": "2"
  }
```

- [ ] **Step 2: Verify the new cards parse correctly**

```bash
cargo test cards::
```

Expected: all `cards` tests pass. The `test_db()` helper now loads 7 cards without errors.

- [ ] **Step 3: Write integration tests in `tests/scripted_game.rs`**

Add these three new test functions at the bottom of the file:

```rust
#[test]
fn first_striker_kills_blocker_and_survives_unscathed() {
    // Anaba Bodyguard (3/2 First Strike) attacks; Grizzly Bears (2/2) blocks.
    // Round 1: Bodyguard deals 3 (kills Bears). Bears can't deal back.
    // Round 2: no blockers — Bodyguard untouched. Player takes no damage (creature was blocked).
    let db = card_db();
    let mut gs = GameState::new(vec![
        Player::new(PlayerId(0), "Alice"),
        Player::new(PlayerId(1), "Bob"),
    ]);
    gs.step = Step::DeclareAttackers;

    let bodyguard_id = {
        let id = gs.alloc_id();
        let mut obj = CardObject::new(
            id,
            db.get("Anaba Bodyguard").unwrap().clone(),
            PlayerId(0),
            Zone::Battlefield,
        );
        obj.summoning_sick = false;
        gs.battlefield.push(id);
        gs.add_object(obj);
        id
    };
    let bears_id = {
        let id = gs.alloc_id();
        let mut obj = CardObject::new(
            id,
            db.get("Grizzly Bears").unwrap().clone(),
            PlayerId(1),
            Zone::Battlefield,
        );
        obj.summoning_sick = false;
        gs.battlefield.push(id);
        gs.add_object(obj);
        id
    };

    gs = declare_attackers(gs, PlayerId(0), &[bodyguard_id]).unwrap();
    gs.step = Step::DeclareBlockers;
    gs = declare_blockers(gs, PlayerId(1), &[(bears_id, bodyguard_id)]).unwrap();
    gs.step = Step::CombatDamage;

    // Round 1: first strike
    gs = deal_combat_damage(gs);
    assert!(!gs.battlefield.contains(&bears_id), "Bears should be dead after round 1");
    assert_eq!(gs.objects[&bodyguard_id].damage_marked, 0, "Bodyguard takes no damage in round 1");

    // Advance to queued second round
    gs = advance_step(gs);
    assert_eq!(gs.step(), Step::CombatDamage);
    gs = deal_combat_damage(gs);

    assert!(gs.battlefield.contains(&bodyguard_id), "Bodyguard survives");
    assert_eq!(gs.get_player(PlayerId(1)).unwrap().life, 20, "No damage to player (blocker absorbed)");
}

#[test]
fn trample_excess_kills_player() {
    // Charging Badger is only 1/1 — not enough to demonstrate excess with a single blocker.
    // Use a manually-constructed 5/5 trampler instead.
    use mecha_oracle::types::{AbilityAST, ability::StaticAbility, CardDefinition, card::{CardType, TypeLine}};

    let db = card_db();
    let mut gs = GameState::new(vec![
        Player::new(PlayerId(0), "Alice"),
        Player::new(PlayerId(1), "Bob"),
    ]);
    gs.step = Step::DeclareAttackers;

    // Construct a 5/5 trampler inline
    let trampler_def = CardDefinition {
        name: "Big Trampler".into(),
        mana_cost: None,
        type_line: TypeLine {
            supertypes: vec![],
            card_types: vec![CardType::Creature],
            subtypes: vec![],
        },
        oracle_text: String::new(),
        abilities: vec![AbilityAST::Static(StaticAbility::Trample)],
        power: Some(5),
        toughness: Some(5),
    };

    let trampler_id = {
        let id = gs.alloc_id();
        let mut obj = CardObject::new(id, trampler_def, PlayerId(0), Zone::Battlefield);
        obj.summoning_sick = false;
        gs.battlefield.push(id);
        gs.add_object(obj);
        id
    };
    let blocker_id = {
        let id = gs.alloc_id();
        let mut obj = CardObject::new(
            id,
            db.get("Grizzly Bears").unwrap().clone(), // 2/2
            PlayerId(1),
            Zone::Battlefield,
        );
        obj.summoning_sick = false;
        gs.battlefield.push(id);
        gs.add_object(obj);
        id
    };

    gs = declare_attackers(gs, PlayerId(0), &[trampler_id]).unwrap();
    gs.step = Step::DeclareBlockers;
    gs = declare_blockers(gs, PlayerId(1), &[(blocker_id, trampler_id)]).unwrap();
    gs.step = Step::CombatDamage;
    gs = deal_combat_damage(gs);

    assert!(!gs.battlefield.contains(&blocker_id), "2/2 blocker dies");
    assert_eq!(gs.get_player(PlayerId(1)).unwrap().life, 17, "3 trample damage to player");
}

#[test]
fn deathtouch_rat_kills_hill_giant() {
    // Typhoid Rats (1/1 Deathtouch) attacks; Hill Giant (3/3) blocks.
    // Rats deal 1 deathtouch damage → Giant dies. Giant deals 3 → Rats die.
    let db = card_db();
    let mut gs = GameState::new(vec![
        Player::new(PlayerId(0), "Alice"),
        Player::new(PlayerId(1), "Bob"),
    ]);
    gs.step = Step::DeclareAttackers;

    let rats_id = {
        let id = gs.alloc_id();
        let mut obj = CardObject::new(
            id,
            db.get("Typhoid Rats").unwrap().clone(),
            PlayerId(0),
            Zone::Battlefield,
        );
        obj.summoning_sick = false;
        gs.battlefield.push(id);
        gs.add_object(obj);
        id
    };
    let giant_id = {
        let id = gs.alloc_id();
        let mut obj = CardObject::new(
            id,
            db.get("Hill Giant").unwrap().clone(),
            PlayerId(1),
            Zone::Battlefield,
        );
        obj.summoning_sick = false;
        gs.battlefield.push(id);
        gs.add_object(obj);
        id
    };

    gs = declare_attackers(gs, PlayerId(0), &[rats_id]).unwrap();
    gs.step = Step::DeclareBlockers;
    gs = declare_blockers(gs, PlayerId(1), &[(giant_id, rats_id)]).unwrap();
    gs.step = Step::CombatDamage;
    gs = deal_combat_damage(gs);

    assert!(!gs.battlefield.contains(&giant_id), "Hill Giant killed by deathtouch");
    assert!(!gs.battlefield.contains(&rats_id), "Typhoid Rats killed by 3 damage");
    assert_eq!(gs.get_player(PlayerId(1)).unwrap().life, 20, "No damage to player");
}
```

- [ ] **Step 4: Run the integration tests**

```bash
cargo test --test scripted_game
```

Expected: all 6 integration tests pass (3 original + 3 new).

- [ ] **Step 5: Run the full test suite**

```bash
cargo test
```

Expected: all tests pass (≥ 80 total).

- [ ] **Step 6: Commit**

```bash
git add tests/fixtures/oracle_cards_test.json tests/scripted_game.rs
git commit -m "feat: add keyword creatures to fixture and integration tests for Phase 2"
```

---

## Verification

After all tasks complete:

```bash
# All tests pass
cargo test

# No warnings
cargo build 2>&1 | grep "^warning" | wc -l

# Binary still runs
cargo run
```

The Phase 2 milestone: 11 evergreen keyword abilities parsed from Oracle text and enforced by the engine, with the two-round first-strike combat damage step implemented via a general dynamic step insertion mechanism.
