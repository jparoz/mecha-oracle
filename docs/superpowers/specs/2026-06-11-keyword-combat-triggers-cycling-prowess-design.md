---
name: keyword-combat-triggers-cycling-prowess
description: Design for implementing Exalted, Flanking, Bushido N, Melee, Prowess, and Cycling keyword abilities
metadata:
  type: design
---

# Keyword Abilities: Combat Triggers, Cycling, Prowess

**Date:** 2026-06-11  
**Keywords covered:** Exalted (702.83), Flanking (702.25), Bushido N (702.45), Melee (702.121), Prowess (702.108), Cycling (702.29)  
**Out of scope this pass:** Battle Cry, Annihilator, Provoke, any keyword requiring counters or graveyard zone changes

---

## Overview

All six keywords share a common dependency: **until-end-of-turn P/T modification tracking** stored per-permanent and cleared in the cleanup step. Combat triggers (Exalted, Flanking, Bushido N, Melee) are collected immediately after attackers/blockers are declared and pushed onto the stack as `TriggeredAbility` stack objects — consistent with the existing ETB trigger pattern. Prowess fires from a new hook in `cast_spell`. Cycling is a hand-activated ability handled by a dedicated engine function.

---

## 1. Until-EOT P/T Boost Infrastructure

### `types/permanent.rs` — new `PTDelta` struct

Add a named struct to replace raw tuples throughout:
```rust
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct PTDelta {
    pub power: i32,
    pub toughness: i32,
}
```

### `types/permanent.rs` — `PermanentState`

Add field:
```rust
pub pt_boost_until_eot: PTDelta,  // cleared at cleanup step
```
Initialize to `PTDelta::default()` (i.e. `{power: 0, toughness: 0}`) in `PermanentState::new`.

Update accessors to apply the boost:
```rust
pub fn effective_power(&self) -> Option<i32> {
    self.current_power.map(|p| p + self.pt_boost_until_eot.power)
}
pub fn effective_toughness(&self) -> Option<i32> {
    self.current_toughness.map(|t| t + self.pt_boost_until_eot.toughness)
}
```

Add helper for parameterized keyword lookup:
```rust
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

### `engine/turn.rs` — `cleanup_step`

After clearing damage, also clear EOT boosts for all permanents:
```rust
perm.pt_boost_until_eot = PTDelta::default();
```

---

## 2. New EffectStep

### `types/effect.rs`

```rust
BoostPermanentPT { target_id: ObjectId, delta: PTDelta },
```

`PTDelta` is imported from `types/permanent.rs` (or moved to `types/mod.rs` for shared use).

### `engine/stack.rs` — `execute_effect_steps`

Add match arm:
```rust
EffectStep::BoostPermanentPT { target_id, delta } => {
    if let Some(perm) = state.battlefield.get_mut(target_id) {
        perm.pt_boost_until_eot.power += delta.power;
        perm.pt_boost_until_eot.toughness += delta.toughness;
    }
}
```

---

## 3. New Static Abilities and Cycling Ability

### `types/ability.rs`

Add to `StaticAbility`:
```rust
Exalted,
Flanking,
BushidoN(u32),
Melee,
Prowess,
```

Change `display_name()` return type from `&'static str` to `String` to accommodate parameterized variants. Update all existing match arms to call `.to_string()`. Add new arms:
```rust
Self::Exalted => "Exalted".to_string(),
Self::Flanking => "Flanking".to_string(),
Self::BushidoN(n) => format!("Bushido {n}"),
Self::Melee => "Melee".to_string(),
Self::Prowess => "Prowess".to_string(),
```

Add new `Ability` variant:
```rust
Cycling(ManaCost),   // hand-activated: pay {ManaCost}, discard self → draw a card
```

Note: `StaticAbility` must derive `PartialEq`. `BushidoN(u32)` compares equal only when N matches — a `bushido_n()` accessor on `PermanentState` is used instead of `has_keyword` for Bushido lookups.

---

## 4. Combat Trigger Collection

### `engine/triggered.rs` — two new functions

#### `collect_attack_triggers`

Called from `declare_attackers` after `state.combat.attackers` is set.

**Exalted (CR 702.83b):** If exactly one attacker, iterate all battlefield permanents controlled by the attacking player. For each one with `StaticAbility::Exalted`, generate a `TriggeredAbility` stack object: `BoostPermanentPT { target_id: attacker_id, delta: PTDelta { power: 1, toughness: 1 } }`.

**Melee (CR 702.121b):** For each attacker with `StaticAbility::Melee`, generate one trigger per opponent attacked. In 2-player, this is always 1 opponent → `BoostPermanentPT { target_id: attacker_id, delta: PTDelta { power: 1, toughness: 1 } }`.

#### `collect_block_triggers`

Called from `declare_blockers` after `state.combat.blocking_map` is set.

**Flanking (CR 702.25b):** For each attacker with `StaticAbility::Flanking`, for each blocker of that attacker that does **not** have `StaticAbility::Flanking`, generate: `BoostPermanentPT { target_id: blocker_id, delta: PTDelta { power: -1, toughness: -1 } }`. Controller is the attacking player (controller of the Flanking creature).

**Bushido N (CR 702.45b):** 
- For each attacker where `perm.bushido_n()` is `Some(n)` and the attacker has at least one blocker: `BoostPermanentPT { target_id: attacker_id, delta: PTDelta { power: n as i32, toughness: n as i32 } }`.
- For each blocker where `perm.bushido_n()` is `Some(n)`: `BoostPermanentPT { target_id: blocker_id, delta: PTDelta { power: n as i32, toughness: n as i32 } }`.

### `engine/combat.rs` — wire up triggers

In `declare_attackers`, after setting combat state:
```rust
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

Same pattern in `declare_blockers` using `collect_block_triggers`.

---

## 5. Prowess — General Cast-Triggered Ability Collection

### `types/ability.rs` — new `CastFilter` struct

```rust
/// Describes which spells satisfy the "whenever you cast a spell" trigger condition.
/// An empty `excluded_card_types` means any spell qualifies (e.g. Extort).
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct CastFilter {
    /// The cast spell must not have any of these card types for the trigger to fire.
    pub excluded_card_types: Vec<CardType>,
}

impl CastFilter {
    /// Matches any spell (no restrictions).
    pub fn any() -> Self { Self::default() }

    /// Matches only noncreature spells (Prowess).
    pub fn noncreature() -> Self {
        Self { excluded_card_types: vec![CardType::Creature] }
    }

    pub fn matches(&self, obj: &CardObject) -> bool {
        self.excluded_card_types
            .iter()
            .all(|t| !obj.definition.type_line.card_types.contains(t))
    }
}
```

`CardType` is already in scope from `types/card.rs`.

### `engine/triggered.rs` — `collect_cast_triggers`

```rust
pub fn collect_cast_triggers(
    state: &mut GameState,
    caster: PlayerId,
    spell_id: ObjectId,
    filter: &CastFilter,
) -> Vec<StackObject>
```

- If the cast spell does not satisfy `filter.matches(obj)`, return empty vec immediately.
- For each permanent on the battlefield controlled by `caster` where `perm.is_creature() && perm.has_keyword(StaticAbility::Prowess)`:
  - Generate `TriggeredAbility`: `BoostPermanentPT { target_id: creature_id, delta: PTDelta { power: 1, toughness: 1 } }`.

The function is intentionally structured so that future cast-triggered abilities (e.g. Extort with `CastFilter::any()`) add a new `has_keyword` branch inside this function, without changing its signature.

### `engine/casting.rs` — `cast_spell`

After pushing the spell onto the stack:
```rust
let cast_triggers = collect_cast_triggers(
    &mut state,
    player_id,
    object_id,
    &CastFilter::noncreature(),
);
for t in cast_triggers {
    let id = t.id;
    state.stack.push(id);
    state.stack_objects.insert(id, t);
}
```

---

## 6. Cycling

### `engine/cycling.rs` (new file)

```rust
pub fn cycle_card(
    mut state: GameState,
    card_id: ObjectId,
    player_id: PlayerId,
    payment_plan: Option<PaymentPlan>,
) -> Result<GameState, EngineError>
```

Steps:
1. Validate `player_id` has priority.
2. Validate `card_id` is in `player_id`'s hand.
3. Find `Ability::Cycling(cost)` on the card — if none, return `EngineError::AbilityIndexOutOfRange`.
4. Pay the mana cost (using `can_pay_mana` / `greedy_payment_plan` / `pay_mana_cost`).
5. Move card from hand to graveyard (discard it — this is the cost, not the effect).
6. Create a `StackObject` with `StackPayload::ActivatedAbility { source_id: card_id, effect: vec![EffectStep::DrawCard(1)], label: "Cycling".into() }`.
7. Push onto stack.
8. Reset `consecutive_passes = 0`; player retains priority.

### `engine/mod.rs`

Expose `pub mod cycling;` and re-export `cycle_card`.

---

## 7. Parser Changes

### `parser/oracle.rs` — `match_keyword`

Add before the `is_cr702_keyword` check (all on the lowercased `s`):

```rust
"exalted" => return Parsed(Ability::Static(StaticAbility::Exalted)),
"flanking" => return Parsed(Ability::Static(StaticAbility::Flanking)),
"melee" => return Parsed(Ability::Static(StaticAbility::Melee)),
"prowess" => return Parsed(Ability::Static(StaticAbility::Prowess)),
```

For `BushidoN`:
```rust
if let Some(rest) = s.strip_prefix("bushido ") {
    if let Some(n) = parse_number_word(rest.trim()) {
        return Parsed(Ability::Static(StaticAbility::BushidoN(n)));
    }
}
```

For plain Cycling (not type-cycling variants):
```rust
if let Some(cost_str) = s.strip_prefix("cycling ") {
    if let Some(cost) = try_parse_mana_cost(cost_str.trim()) {
        return Parsed(Ability::Cycling(cost));
    }
}
```

Type-cycling variants ("mountaincycling {2}", "basic landcycling {2}") don't start with "cycling " so they continue to be matched by `kw_part.ends_with("cycling")` in `is_cr702_keyword`.

---

## 8. Serialization / UI

`serve.rs` serializes card abilities to JSON. The new `StaticAbility` variants and `Ability::Cycling` must be handled in `OracleSpan`'s Serialize impl (or wherever ability rendering happens). Specifically:
- New `StaticAbility` variants need `display_name` entries.
- `Ability::Cycling(cost)` needs a serializable representation (e.g., `{ "type": "cycling", "cost": ... }`).

---

## 9. Tests

### Infrastructure
- `pt_boost_until_eot` initializes to `PTDelta::default()` and `effective_power/toughness` reflect the boost
- `cleanup_step` resets all `pt_boost_until_eot` to `PTDelta::default()`
- `BoostPermanentPT` effect step applies cumulatively; silently no-ops if permanent not found

### Exalted
- Single attacker with no Exalted permanents → no triggers
- Single attacker, one Exalted permanent → +1/+1 on attacker
- Single attacker, two Exalted permanents → +2/+2 on attacker (two triggers)
- Multiple attackers, Exalted permanents → no triggers (not attacking alone)
- Exalted boost cleared at cleanup

### Flanking
- Attacker with Flanking blocked by non-Flanking creature → blocker gets -1/-1
- Attacker with Flanking blocked by Flanking creature → no trigger
- Multiple blockers, some with Flanking → only non-Flanking blockers receive -1/-1

### Bushido N
- Attacker with Bushido 2 blocked → attacker gets +2/+2
- Blocker with Bushido 2 → blocker gets +2/+2
- Unblocked attacker with Bushido → no trigger

### Melee
- Attacker with Melee in 2-player game → gets +1/+1

### Prowess
- Cast noncreature spell with Prowess creature on battlefield → creature gets +1/+1
- Cast creature spell → no Prowess trigger
- Two Prowess creatures → each gets +1/+1 independently

### Cycling
- `cycle_card` with insufficient mana → `EngineError::InsufficientMana`
- `cycle_card` with card not in hand → error
- `cycle_card` on card without Cycling ability → error
- Successful cycle: card goes to graveyard, DrawCard goes on stack
- Resolving the stack object draws a card
