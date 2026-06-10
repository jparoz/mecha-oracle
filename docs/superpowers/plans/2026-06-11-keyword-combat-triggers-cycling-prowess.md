# Keyword Abilities Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Implement Exalted, Flanking, Bushido N, Melee, Prowess (combat/cast triggers that grant until-EOT P/T boosts), and Cycling (hand-activated draw ability).

**Architecture:** A shared `PTDelta` struct and `pt_boost_until_eot` field on `PermanentState` track temporary P/T modifications; `cleanup_step` clears them. Combat triggers fire as proper stack objects collected in `declare_attackers`/`declare_blockers`. Prowess fires via a general `collect_cast_triggers(state, caster, spell_id, filter)` hooked into `cast_spell`. Cycling is a dedicated `cycle_card` engine function (not the general `activate_ability` path) that discards the card as cost and puts DrawCard on the stack.

**Tech Stack:** Rust, no new dependencies.

---

## File Map

| File | Change |
|------|--------|
| `src/types/permanent.rs` | Add `PTDelta` struct, `pt_boost_until_eot` field, update accessors, add `bushido_n()` |
| `src/types/mod.rs` | Re-export `PTDelta` and `CastFilter` |
| `src/types/effect.rs` | Add `BoostPermanentPT { target_id, delta }` variant |
| `src/types/ability.rs` | Add `Exalted`/`Flanking`/`BushidoN`/`Melee`/`Prowess` to `StaticAbility`; add `Ability::Cycling(ManaCost)`; change `display_name` to return `String`; add `CastFilter` struct |
| `src/engine/stack.rs` | Handle `BoostPermanentPT` in `execute_effect_steps` |
| `src/engine/turn.rs` | Clear `pt_boost_until_eot` in `cleanup_step` |
| `src/parser/oracle.rs` | Parse new keywords and Cycling; promote from `ParsedUnimplemented` |
| `src/engine/triggered.rs` | Add `collect_attack_triggers`, `collect_block_triggers`, `collect_cast_triggers` |
| `src/engine/combat.rs` | Wire attack/block trigger collection into declare functions |
| `src/engine/casting.rs` | Wire `collect_cast_triggers` into `cast_spell` |
| `src/engine/cycling.rs` | New: `cycle_card` function |
| `src/engine/mod.rs` | Expose `pub mod cycling` |
| `src/serve.rs` | Add `Ability::Cycling` arm; fix power/toughness to use `effective_power()`/`effective_toughness()` |

---

## Task 1: PTDelta struct and PermanentState EOT boost field

**Files:**
- Modify: `src/types/permanent.rs`
- Modify: `src/types/mod.rs`

- [ ] **Write the failing tests** (add inside the existing `#[cfg(test)] mod tests` at the bottom of `permanent.rs`):

```rust
#[test]
fn pt_delta_default_is_zero() {
    let delta = PTDelta::default();
    assert_eq!(delta.power, 0);
    assert_eq!(delta.toughness, 0);
}

#[test]
fn pt_boost_until_eot_initialises_to_zero() {
    let perm = grizzly_bears_perm();
    assert_eq!(perm.pt_boost_until_eot.power, 0);
    assert_eq!(perm.pt_boost_until_eot.toughness, 0);
}

#[test]
fn effective_power_includes_eot_boost() {
    let mut perm = grizzly_bears_perm();
    perm.summoning_sick = false;
    perm.pt_boost_until_eot.power = 3;
    assert_eq!(perm.effective_power(), Some(5)); // 2 base + 3
}

#[test]
fn effective_toughness_includes_eot_boost() {
    let mut perm = grizzly_bears_perm();
    perm.pt_boost_until_eot.toughness = -1;
    assert_eq!(perm.effective_toughness(), Some(1)); // 2 base - 1
}

#[test]
fn effective_power_with_negative_boost_does_not_panic() {
    let mut perm = grizzly_bears_perm();
    perm.pt_boost_until_eot.power = -5;
    assert_eq!(perm.effective_power(), Some(-3)); // 2 base - 5
}
```

- [ ] **Run to confirm they fail:**
```
cargo test 2>&1 | grep -E "^test result|FAILED|error\["
```
Expected: compile errors about `PTDelta` not found.

- [ ] **Implement `PTDelta` and add field to `PermanentState`.**

In `src/types/permanent.rs`, above the `PermanentState` struct definition, add:

```rust
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct PTDelta {
    pub power: i32,
    pub toughness: i32,
}
```

Add the field to `PermanentState`:
```rust
pub struct PermanentState {
    pub definition: CardDefinition,
    pub current_power: Option<i32>,
    pub current_toughness: Option<i32>,
    pub tapped: bool,
    pub summoning_sick: bool,
    pub damage_marked: u32,
    pub damaged_by_deathtouch: bool,
    pub pt_boost_until_eot: PTDelta,   // ← add this line
}
```

Initialize in `PermanentState::new`:
```rust
pub fn new(definition: &CardDefinition) -> Self {
    Self {
        definition: definition.clone(),
        current_power: definition.power,
        current_toughness: definition.toughness,
        tapped: false,
        summoning_sick: true,
        damage_marked: 0,
        damaged_by_deathtouch: false,
        pt_boost_until_eot: PTDelta::default(),   // ← add this line
    }
}
```

Update `effective_power` and `effective_toughness`:
```rust
pub fn effective_power(&self) -> Option<i32> {
    self.current_power.map(|p| p + self.pt_boost_until_eot.power)
}

pub fn effective_toughness(&self) -> Option<i32> {
    self.current_toughness.map(|t| t + self.pt_boost_until_eot.toughness)
}
```

- [ ] **Re-export `PTDelta` from `src/types/mod.rs`.**

Change the `permanent` re-export line from:
```rust
pub use permanent::PermanentState;
```
to:
```rust
pub use permanent::{PTDelta, PermanentState};
```

- [ ] **Run tests to confirm they pass:**
```
cargo test 2>&1 | grep -E "^test result|FAILED|error\["
```
Expected: `test result: ok`

- [ ] **Commit:**
```bash
git add src/types/permanent.rs src/types/mod.rs
git commit -m "feat: add PTDelta struct and pt_boost_until_eot field to PermanentState"
```

---

## Task 2: BoostPermanentPT EffectStep resolves via stack

**Files:**
- Modify: `src/types/effect.rs`
- Modify: `src/engine/stack.rs`

- [ ] **Add the new `EffectStep` variant** to `src/types/effect.rs`:

```rust
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EffectStep {
    AddMana(ManaPool),
    Mill(u32),
    DrawCard(u32),
    GainLife(u32),
    BoostPermanentPT { target_id: ObjectId, delta: PTDelta },   // ← new
    Unimplemented(String),
}
```

Add the import at the top of the file (after the existing `use super::ids::{ObjectId, PlayerId};`):
```rust
use super::permanent::PTDelta;
```

- [ ] **Write the failing test** in the existing `#[cfg(test)] mod tests` of `src/engine/stack.rs`:

```rust
#[test]
fn boost_permanent_pt_effect_applies_delta() {
    use crate::types::ability::{Ability, StaticAbility};
    use crate::types::card::{CardDefinition, CardType, TypeLine};
    use crate::types::{CardObject, PTDelta, PermanentState};
    let mut gs = make_state();
    // Put a 2/2 creature on the battlefield.
    let def = CardDefinition {
        name: "Target".into(),
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
    let id = gs.alloc_id();
    let obj = CardObject::new(id, def, PlayerId(0), Zone::Battlefield);
    let mut perm = PermanentState::new(&obj.definition);
    perm.summoning_sick = false;
    gs.battlefield.insert(id, perm);
    gs.add_object(obj);

    // Push a BoostPermanentPT trigger onto the stack.
    let stack_id = gs.alloc_stack_id();
    let stack_obj = StackObject {
        id: stack_id,
        payload: StackPayload::TriggeredAbility {
            source_id: id,
            effect: vec![EffectStep::BoostPermanentPT {
                target_id: id,
                delta: PTDelta { power: 1, toughness: 1 },
            }],
            label: "test boost".into(),
        },
        controller: PlayerId(0),
    };
    gs.stack.push(stack_id);
    gs.stack_objects.insert(stack_id, stack_obj);

    let gs = resolve_top(gs);

    assert_eq!(gs.battlefield[&id].effective_power(), Some(3));
    assert_eq!(gs.battlefield[&id].effective_toughness(), Some(3));
    assert!(gs.stack.is_empty());
}

#[test]
fn boost_permanent_pt_noop_if_not_on_battlefield() {
    use crate::types::{PTDelta};
    let mut gs = make_state();
    let nonexistent_id = ObjectId(999);
    let stack_id = gs.alloc_stack_id();
    let stack_obj = StackObject {
        id: stack_id,
        payload: StackPayload::TriggeredAbility {
            source_id: nonexistent_id,
            effect: vec![EffectStep::BoostPermanentPT {
                target_id: nonexistent_id,
                delta: PTDelta { power: 5, toughness: 5 },
            }],
            label: "noop boost".into(),
        },
        controller: PlayerId(0),
    };
    gs.stack.push(stack_id);
    gs.stack_objects.insert(stack_id, stack_obj);

    // Should not panic.
    let gs = resolve_top(gs);
    assert!(gs.stack.is_empty());
}
```

- [ ] **Run to confirm they fail:**
```
cargo test 2>&1 | grep -E "^test result|FAILED|error\["
```
Expected: compile errors about missing match arm for `BoostPermanentPT`.

- [ ] **Handle `BoostPermanentPT` in `execute_effect_steps`** in `src/engine/stack.rs`. In the `match step` block, add after the `EffectStep::Unimplemented(_) => {}` arm:

```rust
EffectStep::BoostPermanentPT { target_id, delta } => {
    if let Some(perm) = state.battlefield.get_mut(target_id) {
        perm.pt_boost_until_eot.power += delta.power;
        perm.pt_boost_until_eot.toughness += delta.toughness;
    }
}
```

Also add the import at the top of `src/engine/stack.rs`:
```rust
use crate::types::PTDelta;
```

- [ ] **Run tests:**
```
cargo test 2>&1 | grep -E "^test result|FAILED|error\["
```
Expected: `test result: ok`

- [ ] **Commit:**
```bash
git add src/types/effect.rs src/engine/stack.rs
git commit -m "feat: add BoostPermanentPT effect step, resolves via stack"
```

---

## Task 3: Cleanup step clears EOT boosts

**Files:**
- Modify: `src/engine/turn.rs`

- [ ] **Write the failing test** in the existing `#[cfg(test)] mod tests` of `src/engine/turn.rs`:

```rust
#[test]
fn cleanup_step_clears_pt_boost_until_eot() {
    use crate::types::{CardObject, PTDelta, PermanentState, Zone};
    let db = test_db();
    let mut gs = make_state();
    gs.step = Step::Cleanup;
    let id = gs.alloc_id();
    let obj = CardObject::new(
        id,
        db.get("Grizzly Bears").unwrap().clone(),
        PlayerId(0),
        Zone::Battlefield,
    );
    let mut perm = PermanentState::new(&obj.definition);
    perm.pt_boost_until_eot = PTDelta { power: 3, toughness: 3 };
    gs.battlefield.insert(id, perm);
    gs.add_object(obj);

    let gs = apply_step_start(gs);

    assert_eq!(gs.battlefield[&id].pt_boost_until_eot, PTDelta::default());
    assert_eq!(gs.battlefield[&id].effective_power(), Some(2)); // back to 2/2 base
}
```

- [ ] **Run to confirm it fails:**
```
cargo test engine::turn::tests::cleanup_step_clears_pt_boost_until_eot 2>&1 | grep -E "^test result|FAILED|error\["
```
Expected: FAILED (boost is not cleared).

- [ ] **Update `cleanup_step`** in `src/engine/turn.rs`. Add `PTDelta` import at the top:
```rust
use crate::types::PTDelta;
```

In `cleanup_step`, extend the loop over permanents:
```rust
fn cleanup_step(mut state: GameState) -> GameState {
    for perm in state.battlefield.values_mut() {
        perm.damage_marked = 0;
        perm.damaged_by_deathtouch = false;
        perm.pt_boost_until_eot = PTDelta::default();   // ← add this line
    }
    state
}
```

- [ ] **Run tests:**
```
cargo test 2>&1 | grep -E "^test result|FAILED|error\["
```
Expected: `test result: ok`

- [ ] **Commit:**
```bash
git add src/engine/turn.rs
git commit -m "feat: cleanup_step clears pt_boost_until_eot on all permanents"
```

---

## Task 4: New StaticAbility variants and display_name returns String

**Files:**
- Modify: `src/types/ability.rs`
- Modify: `src/serve.rs` (minor: remove redundant `.to_string()`)

- [ ] **Add the new variants** to `StaticAbility` in `src/types/ability.rs`. Extend the enum:

```rust
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
    Defender,
    Shadow,
    Horsemanship,
    Skulk,
    Decayed,
    Flash,
    Exalted,          // ← new
    Flanking,         // ← new
    BushidoN(u32),    // ← new
    Melee,            // ← new
    Prowess,          // ← new
}
```

- [ ] **Change `display_name` to return `String`** and add new arms. Replace the entire `display_name` method:

```rust
pub fn display_name(&self) -> String {
    match self {
        Self::Flying => "Flying".to_string(),
        Self::Reach => "Reach".to_string(),
        Self::Trample => "Trample".to_string(),
        Self::FirstStrike => "First strike".to_string(),
        Self::DoubleStrike => "Double strike".to_string(),
        Self::Vigilance => "Vigilance".to_string(),
        Self::Haste => "Haste".to_string(),
        Self::Lifelink => "Lifelink".to_string(),
        Self::Deathtouch => "Deathtouch".to_string(),
        Self::Menace => "Menace".to_string(),
        Self::Indestructible => "Indestructible".to_string(),
        Self::Defender => "Defender".to_string(),
        Self::Shadow => "Shadow".to_string(),
        Self::Horsemanship => "Horsemanship".to_string(),
        Self::Skulk => "Skulk".to_string(),
        Self::Decayed => "Decayed".to_string(),
        Self::Flash => "Flash".to_string(),
        Self::Exalted => "Exalted".to_string(),
        Self::Flanking => "Flanking".to_string(),
        Self::BushidoN(n) => format!("Bushido {n}"),
        Self::Melee => "Melee".to_string(),
        Self::Prowess => "Prowess".to_string(),
    }
}
```

- [ ] **In `src/serve.rs` line 426**, `display_name()` now returns `String`, so `.to_string()` is redundant. Change:
```rust
text: kw.display_name().to_string(),
```
to:
```rust
text: kw.display_name(),
```

- [ ] **Write tests** for new display names. Add to the existing `#[cfg(test)] mod tests` in `ability.rs`:

```rust
#[test]
fn display_name_new_keywords() {
    assert_eq!(StaticAbility::Exalted.display_name(), "Exalted");
    assert_eq!(StaticAbility::Flanking.display_name(), "Flanking");
    assert_eq!(StaticAbility::BushidoN(2).display_name(), "Bushido 2");
    assert_eq!(StaticAbility::Melee.display_name(), "Melee");
    assert_eq!(StaticAbility::Prowess.display_name(), "Prowess");
}
```

- [ ] **Run tests:**
```
cargo test 2>&1 | grep -E "^test result|FAILED|error\["
```
Expected: `test result: ok`

- [ ] **Commit:**
```bash
git add src/types/ability.rs src/serve.rs
git commit -m "feat: add Exalted/Flanking/BushidoN/Melee/Prowess StaticAbility variants"
```

---

## Task 5: Ability::Cycling and bushido_n accessor

**Files:**
- Modify: `src/types/ability.rs`
- Modify: `src/types/permanent.rs`
- Modify: `src/types/mod.rs`

- [ ] **Add `Ability::Cycling`** to the `Ability` enum in `src/types/ability.rs`:

```rust
pub enum Ability {
    Static(StaticAbility),
    Triggered(TriggeredAbility),
    Activated(ActivatedAbility),
    SpellEffect(Effect),
    Cycling(ManaCost),   // ← new: hand-activated; cost = pay ManaCost + discard self
}
```

- [ ] **Add `bushido_n` accessor** to `PermanentState` in `src/types/permanent.rs`. Add after the existing `has_keyword` method:

```rust
/// Returns the Bushido parameter N if this permanent has Bushido N, otherwise None.
pub fn bushido_n(&self) -> Option<u32> {
    self.definition.abilities.iter().find_map(|span| {
        if let OracleSpan::Parsed(Ability::Static(StaticAbility::BushidoN(n))) = span {
            Some(*n)
        } else {
            None
        }
    })
}
```

- [ ] **Write tests** for `bushido_n`. Add to the existing `#[cfg(test)] mod tests` in `permanent.rs`:

```rust
#[test]
fn bushido_n_returns_some_for_bushido_creature() {
    use crate::types::{Ability, OracleSpan, ability::StaticAbility};
    let mut def = test_db().get("Grizzly Bears").unwrap().clone();
    def.abilities = vec![OracleSpan::Parsed(Ability::Static(StaticAbility::BushidoN(3)))];
    let perm = PermanentState::new(&def);
    assert_eq!(perm.bushido_n(), Some(3));
}

#[test]
fn bushido_n_returns_none_for_vanilla_creature() {
    let perm = grizzly_bears_perm();
    assert_eq!(perm.bushido_n(), None);
}

#[test]
fn ability_cycling_roundtrips() {
    use crate::types::{Ability, OracleSpan};
    use crate::types::mana::{ManaCost, ManaPip};
    let cost = ManaCost { pips: vec![ManaPip::Generic(2)] };
    let span = OracleSpan::Parsed(Ability::Cycling(cost.clone()));
    assert_eq!(span, OracleSpan::Parsed(Ability::Cycling(cost)));
}
```

- [ ] **Run tests:**
```
cargo test 2>&1 | grep -E "^test result|FAILED|error\["
```
Expected: `test result: ok`

- [ ] **Commit:**
```bash
git add src/types/ability.rs src/types/permanent.rs
git commit -m "feat: add Ability::Cycling variant and bushido_n accessor"
```

---

## Task 6: Parser — promote new keywords from ParsedUnimplemented

**Files:**
- Modify: `src/parser/oracle.rs`

- [ ] **Write failing parser tests.** Add to the existing `#[cfg(test)] mod tests` in `oracle.rs`:

```rust
#[test]
fn parse_exalted_keyword() {
    let spans = parse_permanent("Exalted", "");
    assert_eq!(spans, vec![parsed(StaticAbility::Exalted)]);
}

#[test]
fn parse_flanking_keyword() {
    let spans = parse_permanent("Flanking", "");
    assert_eq!(spans, vec![parsed(StaticAbility::Flanking)]);
}

#[test]
fn parse_bushido_n_keyword() {
    use crate::types::ability::StaticAbility;
    let spans = parse_permanent("Bushido 2", "");
    assert_eq!(spans, vec![OracleSpan::Parsed(Ability::Static(StaticAbility::BushidoN(2)))]);
}

#[test]
fn parse_melee_keyword() {
    let spans = parse_permanent("Melee", "");
    assert_eq!(spans, vec![parsed(StaticAbility::Melee)]);
}

#[test]
fn parse_prowess_keyword() {
    let spans = parse_permanent("Prowess", "");
    assert_eq!(spans, vec![parsed(StaticAbility::Prowess)]);
}

#[test]
fn parse_cycling_keyword() {
    use crate::types::mana::{ManaCost, ManaPip};
    let spans = parse_permanent("Cycling {2}", "");
    assert_eq!(
        spans,
        vec![OracleSpan::Parsed(Ability::Cycling(ManaCost {
            pips: vec![ManaPip::Generic(2)],
        }))]
    );
}

#[test]
fn parse_cycling_with_reminder_text() {
    use crate::types::mana::{ManaCost, ManaPip};
    let spans = parse_permanent(
        "Cycling {2} ({2}, Discard this card: Draw a card.)",
        "",
    );
    // First span is the cycling ability; second is reminder text (ignored).
    assert_eq!(spans.len(), 2);
    assert_eq!(
        spans[0],
        OracleSpan::Parsed(Ability::Cycling(ManaCost {
            pips: vec![ManaPip::Generic(2)],
        }))
    );
    assert!(matches!(
        &spans[1],
        OracleSpan::Ignored(crate::types::ability::IgnoredKind::ReminderText, _)
    ));
}

#[test]
fn mountaincycling_stays_parsed_unimplemented() {
    let spans = parse_permanent("Mountaincycling {2}", "");
    assert!(matches!(&spans[0], OracleSpan::ParsedUnimplemented(_)));
}
```

- [ ] **Run to confirm they fail:**
```
cargo test 2>&1 | grep -E "^test result|FAILED|error\["
```
Expected: FAILED (keywords still ParsedUnimplemented).

- [ ] **Implement keyword promotion** in `src/parser/oracle.rs`. In the `match_keyword` function, add new fully-implemented keyword cases **before** the `if is_cr702_keyword(s)` line. Add after the `"flash" => return parsed!(Flash),` arm in the `match s` block:

```rust
"exalted" => return parsed!(Exalted),
"flanking" => return parsed!(Flanking),
"melee" => return parsed!(Melee),
"prowess" => return parsed!(Prowess),
```

Then add these two `if let` checks between the match block and the `is_cr702_keyword` call:

```rust
// BushidoN: "bushido N"
if let Some(rest) = s.strip_prefix("bushido ") {
    if let Some(n) = parse_number_word(rest.trim()) {
        return OracleSpan::Parsed(Ability::Static(StaticAbility::BushidoN(n)));
    }
}

// Plain cycling (not type-cycling variants like mountaincycling):
if let Some(cost_str) = s.strip_prefix("cycling ") {
    if let Some(cost) = try_parse_mana_cost(cost_str.trim()) {
        return OracleSpan::Parsed(Ability::Cycling(cost));
    }
}
```

- [ ] **Run tests:**
```
cargo test 2>&1 | grep -E "^test result|FAILED|error\["
```
Expected: `test result: ok`

- [ ] **Commit:**
```bash
git add src/parser/oracle.rs
git commit -m "feat: parser promotes Exalted/Flanking/BushidoN/Melee/Prowess/Cycling from ParsedUnimplemented"
```

---

## Task 7: CastFilter, collect_cast_triggers, and Prowess in cast_spell

**Files:**
- Modify: `src/types/ability.rs`
- Modify: `src/types/mod.rs`
- Modify: `src/engine/triggered.rs`
- Modify: `src/engine/casting.rs`

- [ ] **Add `CastFilter`** to `src/types/ability.rs`. Add the import at the top of the file:
```rust
use super::card::CardType;
```

Then add the struct below the `IgnoredKind` definition:

```rust
/// Describes which cast spells activate "whenever you cast a spell" triggers.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct CastFilter {
    /// The spell must not have any of these card types for the trigger to fire.
    pub excluded_card_types: Vec<CardType>,
}

impl CastFilter {
    /// Matches any spell (e.g. Extort — no restriction).
    pub fn any() -> Self { Self::default() }

    /// Matches only noncreature spells (e.g. Prowess).
    pub fn noncreature() -> Self {
        Self { excluded_card_types: vec![CardType::Creature] }
    }

    pub fn matches(&self, card_types: &[CardType]) -> bool {
        self.excluded_card_types
            .iter()
            .all(|t| !card_types.contains(t))
    }
}
```

- [ ] **Re-export `CastFilter`** from `src/types/mod.rs`. Change the ability re-export:
```rust
pub use ability::{
    Ability, ActivatedAbility, ActivationCost, CardFilter, CastFilter, CostComponent,
    IgnoredKind, OracleSpan, PermanentFilter, StaticAbility, TriggerEvent, TriggeredAbility,
};
```

- [ ] **Write failing Prowess tests** in the existing `#[cfg(test)] mod tests` of `src/engine/triggered.rs`:

```rust
#[test]
fn collect_cast_triggers_prowess_fires_on_noncreature() {
    use crate::engine::triggered::collect_cast_triggers;
    use crate::types::ability::{Ability, CastFilter, StaticAbility};
    use crate::types::card::{CardDefinition, CardType, TypeLine};
    use crate::types::mana::ManaCost;
    use crate::types::{CardObject, OracleSpan, PermanentState, Zone};

    let mut gs = two_player_state();

    // A creature with Prowess on the battlefield.
    let prowess_def = CardDefinition {
        name: "Prowess Monk".into(),
        mana_cost: None,
        type_line: TypeLine {
            supertypes: vec![],
            card_types: vec![CardType::Creature],
            subtypes: vec![],
        },
        oracle_text: "Prowess".into(),
        abilities: vec![OracleSpan::Parsed(Ability::Static(StaticAbility::Prowess))],
        power: Some(1),
        toughness: Some(1),
    };
    let creature_id = place_on_battlefield(&mut gs, prowess_def, PlayerId(0));

    // A noncreature spell on the stack (instant).
    let instant_def = CardDefinition {
        name: "Lightning Bolt".into(),
        mana_cost: Some(ManaCost { pips: vec![] }),
        type_line: TypeLine {
            supertypes: vec![],
            card_types: vec![CardType::Instant],
            subtypes: vec![],
        },
        oracle_text: String::new(),
        abilities: vec![],
        power: None,
        toughness: None,
    };
    let spell_id = gs.alloc_id();
    let spell_obj = CardObject::new(spell_id, instant_def, PlayerId(0), Zone::Stack);
    gs.add_object(spell_obj);

    let triggers = collect_cast_triggers(&mut gs, PlayerId(0), spell_id, &CastFilter::noncreature());

    assert_eq!(triggers.len(), 1);
    assert_eq!(triggers[0].controller, PlayerId(0));
    use crate::types::stack::StackPayload;
    let StackPayload::TriggeredAbility { source_id, effect, .. } = &triggers[0].payload else {
        panic!("expected TriggeredAbility");
    };
    assert_eq!(source_id, &creature_id);
    use crate::types::{PTDelta};
    use crate::types::effect::EffectStep;
    assert_eq!(*effect, vec![EffectStep::BoostPermanentPT {
        target_id: creature_id,
        delta: PTDelta { power: 1, toughness: 1 },
    }]);
}

#[test]
fn collect_cast_triggers_prowess_silent_on_creature_spell() {
    use crate::engine::triggered::collect_cast_triggers;
    use crate::types::ability::{Ability, CastFilter, StaticAbility};
    use crate::types::card::{CardDefinition, CardType, TypeLine};
    use crate::types::{CardObject, OracleSpan, PermanentState, Zone};

    let mut gs = two_player_state();

    let prowess_def = CardDefinition {
        name: "Prowess Monk".into(),
        mana_cost: None,
        type_line: TypeLine {
            supertypes: vec![],
            card_types: vec![CardType::Creature],
            subtypes: vec![],
        },
        oracle_text: String::new(),
        abilities: vec![OracleSpan::Parsed(Ability::Static(StaticAbility::Prowess))],
        power: Some(1),
        toughness: Some(1),
    };
    place_on_battlefield(&mut gs, prowess_def, PlayerId(0));

    // A creature spell.
    let creature_spell_def = CardDefinition {
        name: "Bear".into(),
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
    let spell_id = gs.alloc_id();
    let spell_obj = CardObject::new(spell_id, creature_spell_def, PlayerId(0), Zone::Stack);
    gs.add_object(spell_obj);

    let triggers = collect_cast_triggers(&mut gs, PlayerId(0), spell_id, &CastFilter::noncreature());
    assert!(triggers.is_empty());
}
```

- [ ] **Run to confirm they fail:**
```
cargo test 2>&1 | grep -E "^test result|FAILED|error\["
```
Expected: compile error — `collect_cast_triggers` not defined.

- [ ] **Implement `collect_cast_triggers`** in `src/engine/triggered.rs`. Add these imports at the top:

```rust
use crate::types::ability::CastFilter;
use crate::types::{PTDelta};
use crate::types::effect::EffectStep;
```

Then add the function (after `collect_etb_triggers`):

```rust
/// CR 702.108b: collect triggered abilities that fire when a spell is cast.
/// Currently handles: Prowess (noncreature filter → +1/+1 until EOT on each Prowess creature).
/// Add additional StaticAbility branches here as new cast-triggered keywords are implemented.
pub fn collect_cast_triggers(
    state: &mut GameState,
    caster: PlayerId,
    spell_id: ObjectId,
    filter: &CastFilter,
) -> Vec<StackObject> {
    // Check whether the cast spell satisfies the filter.
    let spell_types: Vec<crate::types::card::CardType> = state
        .objects
        .get(&spell_id)
        .map(|o| o.definition.type_line.card_types.clone())
        .unwrap_or_default();
    if !filter.matches(&spell_types) {
        return vec![];
    }

    // Collect permanents that have cast-triggered abilities.
    let prowess_creature_ids: Vec<(ObjectId, PlayerId)> = state
        .battlefield
        .keys()
        .filter_map(|&id| {
            let obj = state.objects.get(&id)?;
            if obj.controller != caster {
                return None;
            }
            let perm = state.battlefield.get(&id)?;
            if perm.is_creature() && perm.has_keyword(StaticAbility::Prowess) {
                Some((id, obj.controller))
            } else {
                None
            }
        })
        .collect();

    prowess_creature_ids
        .into_iter()
        .map(|(creature_id, controller)| {
            let sid = state.alloc_stack_id();
            StackObject {
                id: sid,
                payload: StackPayload::TriggeredAbility {
                    source_id: creature_id,
                    effect: vec![EffectStep::BoostPermanentPT {
                        target_id: creature_id,
                        delta: PTDelta { power: 1, toughness: 1 },
                    }],
                    label: "Prowess".into(),
                },
                controller,
            }
        })
        .collect()
}
```

- [ ] **Wire into `cast_spell`** in `src/engine/casting.rs`. Add imports:
```rust
use crate::engine::triggered::collect_cast_triggers;
use crate::types::ability::CastFilter;
```

After the two lines that push the spell onto the stack and reset `consecutive_passes`, add:
```rust
// CR 702.108b: collect Prowess and other cast-triggered ability triggers.
let cast_triggers = collect_cast_triggers(&mut state, player_id, object_id, &CastFilter::noncreature());
for t in cast_triggers {
    let id = t.id;
    state.stack.push(id);
    state.stack_objects.insert(id, t);
}
```

- [ ] **Write an integration test** in `src/engine/casting.rs` `#[cfg(test)] mod tests`:

```rust
#[test]
fn cast_noncreature_with_prowess_creature_puts_boost_on_stack() {
    use crate::types::ability::{Ability, StaticAbility};
    use crate::types::card::{CardDefinition, CardType, TypeLine};
    use crate::types::mana::{ManaCost, ManaPip};
    use crate::types::{CardObject, OracleSpan, PermanentState, Zone};

    let mut gs = make_state();
    // Prowess creature on battlefield.
    let prowess_def = CardDefinition {
        name: "Monk".into(),
        mana_cost: None,
        type_line: TypeLine {
            supertypes: vec![],
            card_types: vec![CardType::Creature],
            subtypes: vec![],
        },
        oracle_text: String::new(),
        abilities: vec![OracleSpan::Parsed(Ability::Static(StaticAbility::Prowess))],
        power: Some(1),
        toughness: Some(1),
    };
    let monk_id = gs.alloc_id();
    let obj = CardObject::new(monk_id, prowess_def, PlayerId(0), Zone::Battlefield);
    let mut perm = PermanentState::new(&obj.definition);
    perm.summoning_sick = false;
    gs.battlefield.insert(monk_id, perm);
    gs.add_object(obj);

    // Cast an instant.
    gs.get_player_mut(PlayerId(0)).unwrap().mana_pool.blue += 1;
    let spell_id = put_in_hand(
        &mut gs,
        PlayerId(0),
        make_instant_def("Opt", vec![ManaPip::Blue]),
    );
    let gs = cast_spell(gs, PlayerId(0), spell_id).unwrap();

    // Spell on stack + 1 Prowess trigger on stack.
    assert_eq!(gs.stack.len(), 2);
}
```

- [ ] **Run tests:**
```
cargo test 2>&1 | grep -E "^test result|FAILED|error\["
```
Expected: `test result: ok`

- [ ] **Commit:**
```bash
git add src/types/ability.rs src/types/mod.rs src/engine/triggered.rs src/engine/casting.rs
git commit -m "feat: CastFilter, collect_cast_triggers, wire Prowess into cast_spell"
```

---

## Task 8: collect_attack_triggers (Exalted + Melee) and wire into declare_attackers

**Files:**
- Modify: `src/engine/triggered.rs`
- Modify: `src/engine/combat.rs`

- [ ] **Write failing tests** in the existing `#[cfg(test)] mod tests` of `src/engine/triggered.rs`:

```rust
#[test]
fn collect_attack_triggers_exalted_single_attacker() {
    use crate::engine::triggered::collect_attack_triggers;
    use crate::types::ability::{Ability, StaticAbility};
    use crate::types::card::{CardDefinition, CardType, TypeLine};
    use crate::types::{CardObject, OracleSpan, PermanentState, Zone};

    let mut gs = two_player_state();
    // A 2/2 attacker (no Exalted).
    let attacker_def = CardDefinition {
        name: "Attacker".into(),
        mana_cost: None,
        type_line: TypeLine { supertypes: vec![], card_types: vec![CardType::Creature], subtypes: vec![] },
        oracle_text: String::new(),
        abilities: vec![],
        power: Some(2),
        toughness: Some(2),
    };
    let attacker_id = place_on_battlefield(&mut gs, attacker_def, PlayerId(0));
    // An Exalted land/creature also controlled by P0.
    let exalted_def = CardDefinition {
        name: "Exalted Permanent".into(),
        mana_cost: None,
        type_line: TypeLine { supertypes: vec![], card_types: vec![CardType::Creature], subtypes: vec![] },
        oracle_text: String::new(),
        abilities: vec![OracleSpan::Parsed(Ability::Static(StaticAbility::Exalted))],
        power: Some(1),
        toughness: Some(1),
    };
    let _exalted_id = place_on_battlefield(&mut gs, exalted_def, PlayerId(0));
    gs.combat.attackers = vec![attacker_id];

    let triggers = collect_attack_triggers(&mut gs);

    assert_eq!(triggers.len(), 1);
    use crate::types::stack::StackPayload;
    let StackPayload::TriggeredAbility { effect, .. } = &triggers[0].payload else {
        panic!("expected TriggeredAbility");
    };
    use crate::types::{PTDelta, effect::EffectStep};
    assert_eq!(*effect, vec![EffectStep::BoostPermanentPT {
        target_id: attacker_id,
        delta: PTDelta { power: 1, toughness: 1 },
    }]);
}

#[test]
fn collect_attack_triggers_exalted_multiple_attackers_no_trigger() {
    use crate::engine::triggered::collect_attack_triggers;
    use crate::types::ability::{Ability, StaticAbility};
    use crate::types::card::{CardDefinition, CardType, TypeLine};
    use crate::types::{CardObject, OracleSpan, PermanentState, Zone};

    let mut gs = two_player_state();
    let make_def = |name: &str| CardDefinition {
        name: name.into(),
        mana_cost: None,
        type_line: TypeLine { supertypes: vec![], card_types: vec![CardType::Creature], subtypes: vec![] },
        oracle_text: String::new(),
        abilities: vec![OracleSpan::Parsed(Ability::Static(StaticAbility::Exalted))],
        power: Some(1),
        toughness: Some(1),
    };
    let a = place_on_battlefield(&mut gs, make_def("A"), PlayerId(0));
    let b = place_on_battlefield(&mut gs, make_def("B"), PlayerId(0));
    gs.combat.attackers = vec![a, b]; // two attackers

    let triggers = collect_attack_triggers(&mut gs);
    assert!(triggers.is_empty()); // not attacking alone
}

#[test]
fn collect_attack_triggers_two_exalted_permanents_give_two_triggers() {
    use crate::engine::triggered::collect_attack_triggers;
    use crate::types::ability::{Ability, StaticAbility};
    use crate::types::card::{CardDefinition, CardType, TypeLine};
    use crate::types::{CardObject, OracleSpan, PermanentState, Zone};

    let mut gs = two_player_state();
    let plain_def = CardDefinition {
        name: "Attacker".into(),
        mana_cost: None,
        type_line: TypeLine { supertypes: vec![], card_types: vec![CardType::Creature], subtypes: vec![] },
        oracle_text: String::new(),
        abilities: vec![],
        power: Some(2), toughness: Some(2),
    };
    let attacker_id = place_on_battlefield(&mut gs, plain_def, PlayerId(0));
    let exalted_def = CardDefinition {
        name: "Exalted".into(),
        mana_cost: None,
        type_line: TypeLine { supertypes: vec![], card_types: vec![CardType::Creature], subtypes: vec![] },
        oracle_text: String::new(),
        abilities: vec![OracleSpan::Parsed(Ability::Static(StaticAbility::Exalted))],
        power: Some(1), toughness: Some(1),
    };
    place_on_battlefield(&mut gs, exalted_def.clone(), PlayerId(0));
    place_on_battlefield(&mut gs, exalted_def, PlayerId(0));
    gs.combat.attackers = vec![attacker_id];

    let triggers = collect_attack_triggers(&mut gs);
    assert_eq!(triggers.len(), 2); // one per Exalted permanent
}

#[test]
fn collect_attack_triggers_melee_in_two_player_gives_one_boost() {
    use crate::engine::triggered::collect_attack_triggers;
    use crate::types::ability::{Ability, StaticAbility};
    use crate::types::card::{CardDefinition, CardType, TypeLine};
    use crate::types::{CardObject, OracleSpan, PermanentState, Zone};

    let mut gs = two_player_state();
    let melee_def = CardDefinition {
        name: "Melee Creature".into(),
        mana_cost: None,
        type_line: TypeLine { supertypes: vec![], card_types: vec![CardType::Creature], subtypes: vec![] },
        oracle_text: String::new(),
        abilities: vec![OracleSpan::Parsed(Ability::Static(StaticAbility::Melee))],
        power: Some(2), toughness: Some(2),
    };
    let attacker_id = place_on_battlefield(&mut gs, melee_def, PlayerId(0));
    gs.combat.attackers = vec![attacker_id];

    let triggers = collect_attack_triggers(&mut gs);

    assert_eq!(triggers.len(), 1);
    use crate::types::{PTDelta, effect::EffectStep};
    use crate::types::stack::StackPayload;
    let StackPayload::TriggeredAbility { effect, .. } = &triggers[0].payload else {
        panic!();
    };
    assert_eq!(*effect, vec![EffectStep::BoostPermanentPT {
        target_id: attacker_id,
        delta: PTDelta { power: 1, toughness: 1 },
    }]);
}
```

- [ ] **Run to confirm they fail:**
```
cargo test 2>&1 | grep -E "^test result|FAILED|error\["
```
Expected: compile error — `collect_attack_triggers` not defined.

- [ ] **Implement `collect_attack_triggers`** in `src/engine/triggered.rs` (add after `collect_etb_triggers`):

```rust
/// Collect triggered abilities that fire when creatures are declared as attackers.
/// Handles: Exalted (CR 702.83b), Melee (CR 702.121b).
pub fn collect_attack_triggers(state: &mut GameState) -> Vec<StackObject> {
    let attackers = state.combat.attackers.clone();
    let attacking_player = state.active_player;
    let mut result = Vec::new();

    // Exalted (CR 702.83b): fires once per Exalted permanent when exactly one creature attacks.
    if attackers.len() == 1 {
        let attacker_id = attackers[0];
        let exalted_sources: Vec<ObjectId> = state
            .battlefield
            .keys()
            .filter(|&&id| {
                state.objects.get(&id).map(|o| o.controller == attacking_player).unwrap_or(false)
                    && state.battlefield.get(&id)
                        .map(|p| p.has_keyword(StaticAbility::Exalted))
                        .unwrap_or(false)
            })
            .copied()
            .collect();
        for source_id in exalted_sources {
            let sid = state.alloc_stack_id();
            result.push(StackObject {
                id: sid,
                payload: StackPayload::TriggeredAbility {
                    source_id,
                    effect: vec![EffectStep::BoostPermanentPT {
                        target_id: attacker_id,
                        delta: PTDelta { power: 1, toughness: 1 },
                    }],
                    label: "Exalted".into(),
                },
                controller: attacking_player,
            });
        }
    }

    // Melee (CR 702.121b): +1/+1 per opponent attacked; 2-player = always 1 opponent.
    let melee_attackers: Vec<ObjectId> = attackers
        .iter()
        .filter(|&&id| {
            state.battlefield.get(&id)
                .map(|p| p.has_keyword(StaticAbility::Melee))
                .unwrap_or(false)
        })
        .copied()
        .collect();
    for attacker_id in melee_attackers {
        let sid = state.alloc_stack_id();
        result.push(StackObject {
            id: sid,
            payload: StackPayload::TriggeredAbility {
                source_id: attacker_id,
                effect: vec![EffectStep::BoostPermanentPT {
                    target_id: attacker_id,
                    delta: PTDelta { power: 1, toughness: 1 },
                }],
                label: "Melee".into(),
            },
            controller: attacking_player,
        });
    }

    result
}
```

- [ ] **Wire into `declare_attackers`** in `src/engine/combat.rs`. Add import:
```rust
use crate::engine::triggered::{collect_attack_triggers, collect_block_triggers};
```

At the end of `declare_attackers`, after `state.combat.attackers_declared = true;` and before `Ok(state)`:
```rust
// Collect attack-triggered abilities (Exalted, Melee).
let triggers = collect_attack_triggers(&mut state);
for t in triggers {
    let id = t.id;
    state.stack.push(id);
    state.stack_objects.insert(id, t);
}
if !state.stack.is_empty() {
    state.consecutive_passes = 0;
    state.priority_player = state.active_player;
}
```

- [ ] **Add an integration test** in `src/engine/combat.rs` `#[cfg(test)] mod tests`:

```rust
#[test]
fn declare_attackers_exalted_puts_trigger_on_stack() {
    use crate::types::ability::{Ability, StaticAbility};
    use crate::types::card::{CardDefinition, CardType, TypeLine};
    use crate::types::{CardObject, OracleSpan, PermanentState};

    let mut gs = make_combat_state();
    let plain_def = CardDefinition {
        name: "Attacker".into(),
        mana_cost: None,
        type_line: TypeLine { supertypes: vec![], card_types: vec![CardType::Creature], subtypes: vec![] },
        oracle_text: String::new(),
        abilities: vec![],
        power: Some(2), toughness: Some(2),
    };
    let attacker_id = add_creature(&mut gs, PlayerId(0), plain_def);
    let exalted_def = CardDefinition {
        name: "Exalted Elf".into(),
        mana_cost: None,
        type_line: TypeLine { supertypes: vec![], card_types: vec![CardType::Creature], subtypes: vec![] },
        oracle_text: String::new(),
        abilities: vec![OracleSpan::Parsed(Ability::Static(StaticAbility::Exalted))],
        power: Some(1), toughness: Some(1),
    };
    add_creature(&mut gs, PlayerId(0), exalted_def);

    let gs = declare_attackers(gs, PlayerId(0), &[attacker_id]).unwrap();

    assert_eq!(gs.stack.len(), 1); // one Exalted trigger
    assert_eq!(gs.consecutive_passes, 0);
}
```

- [ ] **Run tests:**
```
cargo test 2>&1 | grep -E "^test result|FAILED|error\["
```
Expected: `test result: ok`

- [ ] **Commit:**
```bash
git add src/engine/triggered.rs src/engine/combat.rs
git commit -m "feat: collect_attack_triggers (Exalted, Melee) wired into declare_attackers"
```

---

## Task 9: collect_block_triggers (Flanking + Bushido) and wire into declare_blockers

**Files:**
- Modify: `src/engine/triggered.rs`
- Modify: `src/engine/combat.rs`

- [ ] **Write failing tests** in the existing `#[cfg(test)] mod tests` of `src/engine/triggered.rs`:

```rust
#[test]
fn collect_block_triggers_flanking_gives_minus_one_to_non_flanking_blocker() {
    use crate::engine::triggered::collect_block_triggers;
    use crate::types::ability::{Ability, StaticAbility};
    use crate::types::card::{CardDefinition, CardType, TypeLine};
    use crate::types::{CardObject, OracleSpan, PermanentState, Zone};

    let mut gs = two_player_state();
    let flanking_def = CardDefinition {
        name: "Flanking Attacker".into(),
        mana_cost: None,
        type_line: TypeLine { supertypes: vec![], card_types: vec![CardType::Creature], subtypes: vec![] },
        oracle_text: String::new(),
        abilities: vec![OracleSpan::Parsed(Ability::Static(StaticAbility::Flanking))],
        power: Some(2), toughness: Some(2),
    };
    let attacker_id = place_on_battlefield(&mut gs, flanking_def, PlayerId(0));
    let plain_def = CardDefinition {
        name: "Plain Blocker".into(),
        mana_cost: None,
        type_line: TypeLine { supertypes: vec![], card_types: vec![CardType::Creature], subtypes: vec![] },
        oracle_text: String::new(),
        abilities: vec![],
        power: Some(2), toughness: Some(2),
    };
    let blocker_id = place_on_battlefield(&mut gs, plain_def, PlayerId(1));

    gs.combat.attackers = vec![attacker_id];
    gs.combat.blocking_map = [(attacker_id, vec![blocker_id])].into();

    let triggers = collect_block_triggers(&mut gs);

    assert_eq!(triggers.len(), 1);
    use crate::types::{PTDelta, effect::EffectStep};
    use crate::types::stack::StackPayload;
    let StackPayload::TriggeredAbility { effect, .. } = &triggers[0].payload else { panic!(); };
    assert_eq!(*effect, vec![EffectStep::BoostPermanentPT {
        target_id: blocker_id,
        delta: PTDelta { power: -1, toughness: -1 },
    }]);
}

#[test]
fn collect_block_triggers_flanking_no_trigger_for_flanking_blocker() {
    use crate::engine::triggered::collect_block_triggers;
    use crate::types::ability::{Ability, StaticAbility};
    use crate::types::card::{CardDefinition, CardType, TypeLine};
    use crate::types::{CardObject, OracleSpan, PermanentState, Zone};

    let mut gs = two_player_state();
    let flanking_def = |name: &str| CardDefinition {
        name: name.into(),
        mana_cost: None,
        type_line: TypeLine { supertypes: vec![], card_types: vec![CardType::Creature], subtypes: vec![] },
        oracle_text: String::new(),
        abilities: vec![OracleSpan::Parsed(Ability::Static(StaticAbility::Flanking))],
        power: Some(2), toughness: Some(2),
    };
    let attacker_id = place_on_battlefield(&mut gs, flanking_def("Attacker"), PlayerId(0));
    let blocker_id = place_on_battlefield(&mut gs, flanking_def("Blocker"), PlayerId(1));
    gs.combat.attackers = vec![attacker_id];
    gs.combat.blocking_map = [(attacker_id, vec![blocker_id])].into();

    let triggers = collect_block_triggers(&mut gs);
    assert!(triggers.is_empty()); // blocker also has Flanking → no trigger
}

#[test]
fn collect_block_triggers_bushido_boosts_attacker_and_blocker() {
    use crate::engine::triggered::collect_block_triggers;
    use crate::types::ability::{Ability, StaticAbility};
    use crate::types::card::{CardDefinition, CardType, TypeLine};
    use crate::types::{CardObject, OracleSpan, PermanentState, Zone};

    let mut gs = two_player_state();
    let bushido_def = CardDefinition {
        name: "Bushido Attacker".into(),
        mana_cost: None,
        type_line: TypeLine { supertypes: vec![], card_types: vec![CardType::Creature], subtypes: vec![] },
        oracle_text: String::new(),
        abilities: vec![OracleSpan::Parsed(Ability::Static(StaticAbility::BushidoN(2)))],
        power: Some(3), toughness: Some(3),
    };
    let attacker_id = place_on_battlefield(&mut gs, bushido_def, PlayerId(0));
    let bushido_blocker_def = CardDefinition {
        name: "Bushido Blocker".into(),
        mana_cost: None,
        type_line: TypeLine { supertypes: vec![], card_types: vec![CardType::Creature], subtypes: vec![] },
        oracle_text: String::new(),
        abilities: vec![OracleSpan::Parsed(Ability::Static(StaticAbility::BushidoN(1)))],
        power: Some(2), toughness: Some(2),
    };
    let blocker_id = place_on_battlefield(&mut gs, bushido_blocker_def, PlayerId(1));
    gs.combat.attackers = vec![attacker_id];
    gs.combat.blocking_map = [(attacker_id, vec![blocker_id])].into();

    let triggers = collect_block_triggers(&mut gs);
    // 2 triggers: one for attacker (Bushido 2), one for blocker (Bushido 1).
    assert_eq!(triggers.len(), 2);

    use crate::types::{PTDelta, effect::EffectStep};
    use crate::types::stack::StackPayload;
    let effects: Vec<_> = triggers.iter().map(|t| {
        let StackPayload::TriggeredAbility { effect, .. } = &t.payload else { panic!(); };
        effect.clone()
    }).collect();
    assert!(effects.contains(&vec![EffectStep::BoostPermanentPT {
        target_id: attacker_id,
        delta: PTDelta { power: 2, toughness: 2 },
    }]));
    assert!(effects.contains(&vec![EffectStep::BoostPermanentPT {
        target_id: blocker_id,
        delta: PTDelta { power: 1, toughness: 1 },
    }]));
}

#[test]
fn collect_block_triggers_bushido_no_trigger_when_unblocked() {
    use crate::engine::triggered::collect_block_triggers;
    use crate::types::ability::{Ability, StaticAbility};
    use crate::types::card::{CardDefinition, CardType, TypeLine};
    use crate::types::{CardObject, OracleSpan, PermanentState, Zone};

    let mut gs = two_player_state();
    let bushido_def = CardDefinition {
        name: "Bushido".into(),
        mana_cost: None,
        type_line: TypeLine { supertypes: vec![], card_types: vec![CardType::Creature], subtypes: vec![] },
        oracle_text: String::new(),
        abilities: vec![OracleSpan::Parsed(Ability::Static(StaticAbility::BushidoN(2)))],
        power: Some(3), toughness: Some(3),
    };
    let attacker_id = place_on_battlefield(&mut gs, bushido_def, PlayerId(0));
    gs.combat.attackers = vec![attacker_id];
    gs.combat.blocking_map = [(attacker_id, vec![])].into(); // unblocked

    let triggers = collect_block_triggers(&mut gs);
    assert!(triggers.is_empty());
}
```

- [ ] **Run to confirm they fail:**
```
cargo test 2>&1 | grep -E "^test result|FAILED|error\["
```

- [ ] **Implement `collect_block_triggers`** in `src/engine/triggered.rs` (add after `collect_attack_triggers`):

```rust
/// Collect triggered abilities that fire when blockers are declared.
/// Handles: Flanking (CR 702.25b), Bushido N (CR 702.45b).
pub fn collect_block_triggers(state: &mut GameState) -> Vec<StackObject> {
    let attacking_player = state.active_player;
    let defending_player = state.opponent_of(attacking_player);
    let blocking_map: Vec<(ObjectId, Vec<ObjectId>)> = state
        .combat
        .blocking_map
        .iter()
        .map(|(&a, bs)| (a, bs.clone()))
        .collect();
    let mut result = Vec::new();

    for (attacker_id, blockers) in &blocking_map {
        // Flanking (CR 702.25b): each non-Flanking blocker gets -1/-1.
        if state.battlefield.get(attacker_id)
            .map(|p| p.has_keyword(StaticAbility::Flanking))
            .unwrap_or(false)
        {
            for &blocker_id in blockers {
                let blocker_has_flanking = state.battlefield.get(&blocker_id)
                    .map(|p| p.has_keyword(StaticAbility::Flanking))
                    .unwrap_or(false);
                if !blocker_has_flanking {
                    let sid = state.alloc_stack_id();
                    result.push(StackObject {
                        id: sid,
                        payload: StackPayload::TriggeredAbility {
                            source_id: *attacker_id,
                            effect: vec![EffectStep::BoostPermanentPT {
                                target_id: blocker_id,
                                delta: PTDelta { power: -1, toughness: -1 },
                            }],
                            label: "Flanking".into(),
                        },
                        controller: attacking_player,
                    });
                }
            }
        }

        // Bushido N on attacker: fires if attacker has at least one blocker.
        if let Some(n) = state.battlefield.get(attacker_id).and_then(|p| p.bushido_n()) {
            if !blockers.is_empty() {
                let sid = state.alloc_stack_id();
                result.push(StackObject {
                    id: sid,
                    payload: StackPayload::TriggeredAbility {
                        source_id: *attacker_id,
                        effect: vec![EffectStep::BoostPermanentPT {
                            target_id: *attacker_id,
                            delta: PTDelta { power: n as i32, toughness: n as i32 },
                        }],
                        label: format!("Bushido {n}"),
                    },
                    controller: attacking_player,
                });
            }
        }

        // Bushido N on each blocker: fires for every blocker with Bushido.
        for &blocker_id in blockers {
            if let Some(n) = state.battlefield.get(&blocker_id).and_then(|p| p.bushido_n()) {
                let sid = state.alloc_stack_id();
                result.push(StackObject {
                    id: sid,
                    payload: StackPayload::TriggeredAbility {
                        source_id: blocker_id,
                        effect: vec![EffectStep::BoostPermanentPT {
                            target_id: blocker_id,
                            delta: PTDelta { power: n as i32, toughness: n as i32 },
                        }],
                        label: format!("Bushido {n}"),
                    },
                    controller: defending_player,
                });
            }
        }
    }

    result
}
```

- [ ] **Wire into `declare_blockers`** in `src/engine/combat.rs`. At the end of `declare_blockers`, after `state.combat.blockers_declared = true;` and before `Ok(state)`:

```rust
// Collect block-triggered abilities (Flanking, Bushido N).
let triggers = collect_block_triggers(&mut state);
for t in triggers {
    let id = t.id;
    state.stack.push(id);
    state.stack_objects.insert(id, t);
}
if !state.stack.is_empty() {
    state.consecutive_passes = 0;
    state.priority_player = state.active_player;
}
```

- [ ] **Add an integration test** in `src/engine/combat.rs` `#[cfg(test)] mod tests`:

```rust
#[test]
fn declare_blockers_flanking_attacker_puts_trigger_on_stack() {
    use crate::types::ability::{Ability, StaticAbility};
    use crate::types::card::{CardDefinition, CardType, TypeLine};
    use crate::types::{CardObject, OracleSpan, PermanentState};

    let mut gs = make_combat_state();
    let flanking_def = CardDefinition {
        name: "Flanking Attacker".into(),
        mana_cost: None,
        type_line: TypeLine { supertypes: vec![], card_types: vec![CardType::Creature], subtypes: vec![] },
        oracle_text: String::new(),
        abilities: vec![OracleSpan::Parsed(Ability::Static(StaticAbility::Flanking))],
        power: Some(2), toughness: Some(2),
    };
    let plain_def = CardDefinition {
        name: "Blocker".into(),
        mana_cost: None,
        type_line: TypeLine { supertypes: vec![], card_types: vec![CardType::Creature], subtypes: vec![] },
        oracle_text: String::new(),
        abilities: vec![],
        power: Some(2), toughness: Some(2),
    };
    let attacker = add_creature(&mut gs, PlayerId(0), flanking_def);
    let blocker = add_creature(&mut gs, PlayerId(1), plain_def);
    gs = declare_attackers(gs, PlayerId(0), &[attacker]).unwrap();
    gs.stack.clear(); gs.stack_objects.clear(); // discard any attack triggers for this test
    gs.step = crate::types::Step::DeclareBlockers;

    let gs = declare_blockers(gs, PlayerId(1), &[(blocker, attacker)]).unwrap();

    assert_eq!(gs.stack.len(), 1); // Flanking trigger
}
```

- [ ] **Run tests:**
```
cargo test 2>&1 | grep -E "^test result|FAILED|error\["
```
Expected: `test result: ok`

- [ ] **Commit:**
```bash
git add src/engine/triggered.rs src/engine/combat.rs
git commit -m "feat: collect_block_triggers (Flanking, Bushido N) wired into declare_blockers"
```

---

## Task 10: cycle_card engine function

**Files:**
- Create: `src/engine/cycling.rs`
- Modify: `src/engine/mod.rs`

- [ ] **Create `src/engine/cycling.rs`** with the tests first:

```rust
use super::EngineError;
use crate::engine::mana::{can_pay_mana, greedy_payment_plan, pay_mana_cost};
use crate::types::ability::Ability;
use crate::types::{GameState, ObjectId, PaymentPlan, PlayerId, Zone};
use crate::types::stack::{StackObject, StackPayload};
use crate::types::effect::EffectStep;

/// CR 702.29: Cycling — pay the cycling cost and discard this card (cost), then draw a card (effect).
/// The draw effect is placed on the stack; cycling is treated as an activated ability from hand.
pub fn cycle_card(
    mut state: GameState,
    card_id: ObjectId,
    player_id: PlayerId,
    payment_plan: Option<PaymentPlan>,
) -> Result<GameState, EngineError> {
    todo!()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::ability::{Ability, OracleSpan};
    use crate::types::card::{CardDefinition, CardType, TypeLine};
    use crate::types::mana::{ManaCost, ManaPip, ManaPool};
    use crate::types::{CardObject, PermanentState, Player, Step};

    fn two_player_state() -> GameState {
        let mut gs = GameState::new(vec![
            Player::new(PlayerId(0), "Alice"),
            Player::new(PlayerId(1), "Bob"),
        ]);
        gs.step = Step::PreCombatMain;
        gs
    }

    fn put_in_hand(state: &mut GameState, owner: PlayerId, def: CardDefinition) -> ObjectId {
        let id = state.alloc_id();
        let obj = CardObject::new(id, def, owner, Zone::Hand);
        state.hands.get_mut(&owner).unwrap().push(id);
        state.add_object(obj);
        id
    }

    fn put_in_library(state: &mut GameState, owner: PlayerId) -> ObjectId {
        let def = CardDefinition {
            name: "Dummy".into(),
            mana_cost: None,
            type_line: TypeLine { supertypes: vec![], card_types: vec![CardType::Creature], subtypes: vec![] },
            oracle_text: String::new(),
            abilities: vec![],
            power: Some(1), toughness: Some(1),
        };
        let id = state.alloc_id();
        let obj = CardObject::new(id, def, owner, Zone::Library);
        state.libraries.get_mut(&owner).unwrap().push(id);
        state.add_object(obj);
        id
    }

    fn cycling_card_def(cost: ManaCost) -> CardDefinition {
        CardDefinition {
            name: "Desert".into(),
            mana_cost: None,
            type_line: TypeLine {
                supertypes: vec![],
                card_types: vec![CardType::Land],
                subtypes: vec![],
            },
            oracle_text: format!("Cycling {{{}}}", cost.pips.len()),
            abilities: vec![OracleSpan::Parsed(Ability::Cycling(cost))],
            power: None,
            toughness: None,
        }
    }

    #[test]
    fn cycle_card_discards_card_and_puts_draw_on_stack() {
        let mut gs = two_player_state();
        put_in_library(&mut gs, PlayerId(0));
        gs.get_player_mut(PlayerId(0)).unwrap().mana_pool.colorless = 2;
        let cost = ManaCost { pips: vec![ManaPip::Generic(2)] };
        let card_id = put_in_hand(&mut gs, PlayerId(0), cycling_card_def(cost));

        let gs = cycle_card(gs, card_id, PlayerId(0), None).unwrap();

        // Card moved to graveyard (cost).
        assert!(!gs.hands[&PlayerId(0)].contains(&card_id));
        assert!(gs.graveyards[&PlayerId(0)].contains(&card_id));
        assert_eq!(gs.objects[&card_id].zone, Zone::Graveyard);
        // DrawCard effect on stack.
        assert_eq!(gs.stack.len(), 1);
        // Mana was spent.
        assert!(gs.get_player(PlayerId(0)).unwrap().mana_pool.is_empty());
    }

    #[test]
    fn cycle_card_draw_resolves_after_stack_resolves() {
        use crate::engine::stack::resolve_top;
        let mut gs = two_player_state();
        put_in_library(&mut gs, PlayerId(0));
        gs.get_player_mut(PlayerId(0)).unwrap().mana_pool.colorless = 2;
        let cost = ManaCost { pips: vec![ManaPip::Generic(2)] };
        let card_id = put_in_hand(&mut gs, PlayerId(0), cycling_card_def(cost));

        let gs = cycle_card(gs, card_id, PlayerId(0), None).unwrap();
        let gs = resolve_top(gs);

        assert_eq!(gs.hands[&PlayerId(0)].len(), 1); // drew a card
        assert!(gs.stack.is_empty());
    }

    #[test]
    fn cycle_card_insufficient_mana_returns_error() {
        let mut gs = two_player_state();
        // No mana in pool.
        let cost = ManaCost { pips: vec![ManaPip::Generic(2)] };
        let card_id = put_in_hand(&mut gs, PlayerId(0), cycling_card_def(cost));
        assert!(matches!(
            cycle_card(gs, card_id, PlayerId(0), None),
            Err(EngineError::InsufficientMana)
        ));
    }

    #[test]
    fn cycle_card_not_in_hand_returns_error() {
        let mut gs = two_player_state();
        let cost = ManaCost { pips: vec![ManaPip::Generic(2)] };
        let def = cycling_card_def(cost);
        // Put the card in the library, not the hand.
        let id = gs.alloc_id();
        let obj = CardObject::new(id, def, PlayerId(0), Zone::Library);
        gs.libraries.get_mut(&PlayerId(0)).unwrap().push(id);
        gs.add_object(obj);
        assert!(matches!(
            cycle_card(gs, id, PlayerId(0), None),
            Err(EngineError::CardNotInHand)
        ));
    }

    #[test]
    fn cycle_card_no_cycling_ability_returns_error() {
        let mut gs = two_player_state();
        let def = CardDefinition {
            name: "Plain Card".into(),
            mana_cost: None,
            type_line: TypeLine { supertypes: vec![], card_types: vec![CardType::Land], subtypes: vec![] },
            oracle_text: String::new(),
            abilities: vec![],
            power: None, toughness: None,
        };
        let card_id = put_in_hand(&mut gs, PlayerId(0), def);
        assert!(matches!(
            cycle_card(gs, card_id, PlayerId(0), None),
            Err(EngineError::AbilityIndexOutOfRange)
        ));
    }

    #[test]
    fn cycle_card_not_your_priority_returns_error() {
        let mut gs = two_player_state();
        gs.priority_player = PlayerId(1);
        let cost = ManaCost { pips: vec![] };
        let card_id = put_in_hand(&mut gs, PlayerId(0), cycling_card_def(cost));
        assert!(matches!(
            cycle_card(gs, card_id, PlayerId(0), None),
            Err(EngineError::NotYourPriority)
        ));
    }
}
```

- [ ] **Run to confirm tests fail** (they will because of the `todo!()`):
```
cargo test engine::cycling 2>&1 | grep -E "^test result|FAILED|error\["
```

- [ ] **Implement `cycle_card`** — replace the `todo!()` with:

```rust
pub fn cycle_card(
    mut state: GameState,
    card_id: ObjectId,
    player_id: PlayerId,
    payment_plan: Option<PaymentPlan>,
) -> Result<GameState, EngineError> {
    if state.priority_player != player_id {
        return Err(EngineError::NotYourPriority);
    }

    // Validate card is in player's hand.
    {
        let hand = state.hands.get(&player_id).ok_or(EngineError::CardNotFound)?;
        if !hand.contains(&card_id) {
            return Err(EngineError::CardNotInHand);
        }
    }

    // Find the cycling cost.
    let cycling_cost = state
        .objects
        .get(&card_id)
        .and_then(|obj| {
            obj.definition.abilities.iter().find_map(|span| {
                if let OracleSpan::Parsed(Ability::Cycling(cost)) = span {
                    Some(cost.clone())
                } else {
                    None
                }
            })
        })
        .ok_or(EngineError::AbilityIndexOutOfRange)?;

    // Check and pay mana cost.
    {
        let player = state.get_player(player_id).ok_or(EngineError::CardNotFound)?;
        if !can_pay_mana(&cycling_cost, &player.mana_pool, player.life) {
            return Err(EngineError::InsufficientMana);
        }
    }
    let plan = match payment_plan {
        Some(p) => p,
        None => {
            let player = state.get_player(player_id).ok_or(EngineError::CardNotFound)?;
            greedy_payment_plan(&cycling_cost, &player.mana_pool, player.life)
                .ok_or(EngineError::InsufficientMana)?
        }
    };
    state = pay_mana_cost(state, player_id, &cycling_cost, &plan)?;

    // Pay the discard cost: move the card from hand to graveyard.
    state.hands.get_mut(&player_id).unwrap().retain(|&id| id != card_id);
    if let Some(obj) = state.objects.get_mut(&card_id) {
        obj.zone = Zone::Graveyard;
    }
    state.graveyards.get_mut(&player_id).unwrap().push(card_id);

    // Put the draw effect on the stack.
    let stack_id = state.alloc_stack_id();
    let stack_obj = StackObject {
        id: stack_id,
        payload: StackPayload::ActivatedAbility {
            source_id: card_id,
            effect: vec![EffectStep::DrawCard(1)],
            label: "Cycling".into(),
        },
        controller: player_id,
    };
    state.stack.push(stack_id);
    state.stack_objects.insert(stack_id, stack_obj);

    state.consecutive_passes = 0;
    state.priority_player = player_id;
    Ok(state)
}
```

- [ ] **Expose the module** in `src/engine/mod.rs`. Add:
```rust
pub mod cycling;
```

- [ ] **Run tests:**
```
cargo test 2>&1 | grep -E "^test result|FAILED|error\["
```
Expected: `test result: ok`

- [ ] **Commit:**
```bash
git add src/engine/cycling.rs src/engine/mod.rs
git commit -m "feat: cycle_card engine function — discard cost, DrawCard on stack"
```

---

## Task 11: Serialization — new ability types and effective P/T in UI

**Files:**
- Modify: `src/serve.rs`

- [ ] **Fix P/T display to use effective values.** In `src/serve.rs`, locate the `to_card_view` closure (~line 463). Change:
```rust
power: perm.and_then(|p| p.current_power),
toughness: perm.and_then(|p| p.current_toughness),
```
to:
```rust
power: perm.and_then(|p| p.effective_power()),
toughness: perm.and_then(|p| p.effective_toughness()),
```

- [ ] **Add `Ability::Cycling` arm** to the `oracle_text` match in `src/serve.rs`. After the `Ability::SpellEffect` arm, add:
```rust
OracleSpan::Parsed(Ability::Cycling(cost)) => OracleSpanView {
    kind: SpanKind::Parsed,
    text: format!("Cycling {}", format_mana_cost(cost)),
    ignored_kind: None,
},
```

- [ ] **Verify compilation succeeds:**
```
cargo build 2>&1 | grep -E "^error"
```
Expected: no output (clean build).

- [ ] **Run full test suite:**
```
cargo test 2>&1 | grep -E "^test result|FAILED|error\["
```
Expected: `test result: ok`

- [ ] **Commit:**
```bash
git add src/serve.rs
git commit -m "fix: serve.rs — use effective_power/toughness, handle Ability::Cycling in UI"
```

---

## Self-Review

**Spec coverage check:**

| Spec requirement | Task |
|---|---|
| PTDelta struct with named fields | Task 1 |
| pt_boost_until_eot on PermanentState | Task 1 |
| effective_power/toughness apply boost | Task 1 |
| cleanup_step clears boost | Task 3 |
| BoostPermanentPT EffectStep | Task 2 |
| BoostPermanentPT resolves via stack | Task 2 |
| StaticAbility::Exalted/Flanking/BushidoN/Melee/Prowess | Task 4 |
| display_name returns String | Task 4 |
| Ability::Cycling(ManaCost) | Task 5 |
| bushido_n() accessor | Task 5 |
| Parser promotes all new keywords | Task 6 |
| CastFilter with excluded_card_types | Task 7 |
| collect_cast_triggers with filter param | Task 7 |
| Prowess wired into cast_spell | Task 7 |
| collect_attack_triggers (Exalted, Melee) | Task 8 |
| Wire into declare_attackers | Task 8 |
| collect_block_triggers (Flanking, Bushido) | Task 9 |
| Wire into declare_blockers | Task 9 |
| cycle_card function | Task 10 |
| Cycling serialized in UI | Task 11 |
| effective_power shown in UI | Task 11 |
