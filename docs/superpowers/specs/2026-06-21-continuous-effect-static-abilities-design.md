# Continuous Effect Static Abilities (Glorious Anthem)

**Date:** 2026-06-21  
**CR refs:** 113.3d (static abilities), 611.1 (continuous effects), 611.3b (continuous effects from static abilities)

## Overview

Add support for enchantments (and other permanents) whose static abilities generate continuous P/T modifications to groups of other permanents — e.g. Glorious Anthem ("Creatures you control get +1/+1."). The design introduces a `ContinuousEffect` type at the `Rule` level, a fleshed-out `PermanentFilter` for subject constraints, a battlefield-scanning helper for computing bonuses at query time, and updates to the three engine sites that consult P/T. A G/W test deck is added to exercise the feature.

## Type System

### `PermanentFilter` (types/ability.rs)

Currently an empty struct. Replaced with:

```rust
pub enum ControllerFilter { You, Opponent, Any }

pub struct PermanentFilter {
    pub controller: ControllerFilter,   // "you control" vs "all" vs opponent's
    pub card_types: Vec<CardType>,      // empty = no constraint; [Creature] for creature anthems
    pub subtypes: Vec<String>,          // empty = no constraint; e.g. ["Elf"] for lord effects
    pub colors: Vec<ManaColor>,         // empty = no constraint; e.g. [White] for Crusade
}
```

`ControllerFilter::default()` is `Any` so existing uses of the empty `PermanentFilter` (none currently exist in live code — the struct is a placeholder) remain valid.

### `ContinuousEffect` (types/ability.rs)

```rust
pub struct ContinuousEffect {
    pub subject_filter: PermanentFilter,
    pub pt_modification: Option<PTDelta>,  // None reserved for future non-PT effects
}
```

`PTDelta` is already defined in `types/permanent.rs` and re-exported via `types/mod.rs`.

### `Rule` enum (types/ability.rs)

New arm:

```rust
pub enum Rule {
    Static(StaticAbility),
    Triggered(TriggeredAbility),
    Activated(ActivatedAbility),
    SpellEffect(SpellEffect),
    Cycling(ManaCost),
    Continuous(ContinuousEffect),   // ← new (CR 611.3b)
}
```

## Parser

In `parser/oracle.rs`, `parse_permanent` gains a new paragraph-level check after the ETB trigger check. Patterns (case-insensitive, trailing period stripped):

| Oracle text form | Filter produced |
|---|---|
| `"Creatures you control get +N/+M"` | controller=You, card_types=[Creature] |
| `"[Color] creatures get +N/+M"` | controller=Any, colors=[color] |
| `"Creatures you control with [subtype] get +N/+M"` | controller=You, card_types=[Creature], subtypes=[subtype] |

The `+N/+M` extraction reuses the existing sign-aware i32 parser. Text matching the subject shape but with an unrecognised predicate falls through to `Unparsed`.

`Glorious Anthem` oracle text `"Creatures you control get +1/+1."` parses as:
```
Rule::Continuous(ContinuousEffect {
    subject_filter: PermanentFilter { controller: You, card_types: [Creature], .. },
    pt_modification: Some(PTDelta { power: 1, toughness: 1 }),
})
```

## Engine — Bonus Computation

### Helper function

A new `pub fn continuous_pt_bonus(state: &GameState, target_id: ObjectId) -> PTDelta` in `engine/mod.rs`:

1. Look up `target_id` in `state.objects` to get its controller and card types. If absent, return zero delta.
2. Check that the target is on the battlefield (`state.battlefield.get(&target_id)` is `Some`). If not, return zero delta.
3. For each other permanent `(id, perm)` in `state.battlefield`:
   - Look up its controller in `state.objects`.
   - For each `RulesText::Active(Rule::Continuous(effect))` in `perm.definition.rules_text`:
     - Evaluate `effect.subject_filter` against the target (controller constraint relative to the effect source's controller, card type set, subtypes, colors).
     - If it matches and `effect.pt_modification` is `Some(delta)`, accumulate `delta` into the running total.
4. Return the accumulated `PTDelta`.

### `effective_power` / `effective_toughness` signature change

`PermanentState::effective_power()` and `effective_toughness()` gain a `continuous_bonus: i32` parameter:

```rust
pub fn effective_power(&self, continuous_bonus: i32) -> Option<i32>
pub fn effective_toughness(&self, continuous_bonus: i32) -> Option<i32>
```

Each method folds the bonus into its existing calculation alongside `pt_boost_until_eot` and counter modifications. This keeps all P/T summation logic inside the method rather than scattered across call sites.

Passing a precomputed `i32` (not `&GameState`) avoids the borrow-checker problem that would arise from borrowing `state.battlefield` to get the `PermanentState` and then trying to borrow `state` again as a method argument.

### Call sites

At every call site, callers first compute `continuous_pt_bonus(state, id)` and extract `.power` or `.toughness` from the result, then pass it into the method:

| Site | File | Change |
|---|---|---|
| P/T display | `serve.rs:722-723` | Compute bonus; pass `.power`/`.toughness` to `effective_power`/`effective_toughness` |
| Toughness ≤ 0 SBA | `state_based_actions.rs:55-56` | Compute bonus; pass `.toughness` to `effective_toughness` |
| Combat power (damage dealt) | `combat.rs` (attacker/blocker power reads) | Compute bonus; pass `.power` to `effective_power` |
| Combat toughness (lethal threshold) | `combat.rs` (toughness/remaining-damage reads) | Compute bonus; pass `.toughness` to `effective_toughness` |

## Test Deck Update

`docs/test-decks/green_abilities.json` player 2 deck becomes a G/W build:

- **Mana:** 4× Plains, 2× Savannah, 5× Forest (Savannah taps for Green only in the current engine — see todo.md)
- **Anthem:** 2× Glorious Anthem
- **Creatures:** Elvish Mystic, Giant Growth, Dungrove Elder, Ripjaw Raptor, Kalonian Tusker, Kalonian Hydra, Garruk's Packleader (carried from original; these are Glorious Anthem targets)

## Error Handling

`continuous_pt_bonus` is pure / infallible. If `target_id` is not in `objects` or not on the battlefield it returns `PTDelta::default()` (zero), which is a safe no-op at every call site.

## Testing

- Unit test in `engine/continuous.rs` (or `engine/mod.rs`): two creatures on the battlefield, one anthem source controlled by the same player — verify bonus is applied to the creature but not to the opponent's creature.
- Unit test: anthem LTBs (removed from battlefield map) — verify bonus drops to zero.
- Existing combat and SBA tests must continue to pass unchanged (regression guard).
