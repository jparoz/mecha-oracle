# Design Notes: EffectStep::DealDamage Source Context

## Problem Statement

`EffectStep::DealDamage(u32)` carries only an amount. When it resolves through the stack, keyword properties of the damage source — Lifelink, Deathtouch, Wither, Infect, Toxic N — are never consulted. Combat damage handles these correctly via a parallel code path that bypasses `EffectStep` entirely. Any activated ability, triggered ability, or spell that deals damage from a source with these keywords will silently behave as vanilla damage.

---

## Current State

### The step definition (`effect.rs:30`)

```rust
pub enum EffectStep {
    DealDamage(u32),
    // ...
}
```

### Resolution (`stack.rs:130-142`)

```rust
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
```

Always marks damage or subtracts life, nothing else.

### The acknowledged TODO (`stack.rs:128-130`)

```rust
// TODO CR 702.2c/702.15a: deathtouch and lifelink propagation not yet
// implemented; DealDamage carries no source-keyword context.
```

### How combat damage does it correctly (`combat.rs:342-553`)

`deal_combat_damage` reads keywords directly from the attacking/blocking permanent and routes into separate accumulator maps before applying anything:

```rust
let mut damage_to_players: HashMap<PlayerId, i32> = HashMap::new();
let mut damage_to_objects: HashMap<ObjectId, u32> = HashMap::new();
let mut lifelink_gain: HashMap<PlayerId, i32> = HashMap::new();
let mut deathtouch_targets: HashSet<ObjectId> = HashSet::new();
let mut wither_to_objects: HashMap<ObjectId, u32> = HashMap::new();
let mut poison_to_players: HashMap<PlayerId, u32> = HashMap::new();
```

Combat damage is a closed special case. It does not go through `EffectStep::DealDamage` at all.

---

## Concrete Breakage

### Lifelink (CR 702.15)

A creature with Lifelink and `{T}: This deals 2 damage to any target` — the controller gains no life when the ability resolves through the stack. The source's Lifelink is never read.

### Deathtouch (CR 702.2c)

`perm.damaged_by_deathtouch` is never set to `true` by `DealDamage`. SBAs won't kill the target creature even if it received any damage from a Deathtouch source. The creature would need to receive damage >= its toughness through normal marking.

### Wither (CR 702.80) and Infect (CR 702.90) on creatures

`DealDamage` always calls `perm.damage_marked += n`. For a Wither or Infect source, the damage should instead become -1/-1 counters via `perm.add_counters(CounterKind::PtModifier { power: -1, toughness: -1 }, n)`. A 1/1 creature receiving 1 damage from an Infect spell would survive (1 marked damage vs 1 toughness triggers SBA, which is actually lethal — but only coincidentally, not through the correct counter path).

### Infect on players (CR 702.90)

`DealDamage` to a player always calls `player.life -= n`. An Infect source should call `player.add_counters(CounterKind::Poison, n)` instead, without reducing life.

### Toxic N (CR 702.164)

No poison counters are added when `DealDamage` resolves, even if the source has Toxic N. The `toxic_n()` method on `PermanentState` is never called during stack resolution.

---

## Why a Simple Fix Doesn't Work

### Option 1: Look up the source at resolution time

Pass the source `ObjectId` through `EffectStep` and look up its keywords when `DealDamage` resolves. Problem: the source may have left the battlefield by then. CR 702.15a, 702.2c, 702.80, etc. specify that these keywords apply based on whether the *source* had them — and the rules use last-known information when the source has left. Looking up a live permanent would give wrong results for sources that died in response to their own activated ability.

### Option 2: Look up keywords when the ability goes on the stack

Snapshot the relevant boolean flags at activation/cast time and store them in the step. This is correct per the rules: the source's properties are evaluated at the time the ability is created, not at resolution.

---

## Proposed Fix: Snapshot Flags in the Step

Replace the bare amount with a struct carrying the source's relevant keyword flags:

```rust
pub struct DamageStep {
    pub amount: u32,
    pub lifelink: bool,
    pub deathtouch: bool,
    pub wither: bool,         // routes creature damage as -1/-1 counters
    pub infect: bool,         // routes creature damage as -1/-1 counters; player damage as poison
    pub toxic_n: Option<u32>, // additional poison counters on player damage
}

pub enum EffectStep {
    DealDamage(DamageStep),
    // ...
}
```

Flags are set when the `StackObject` is constructed — i.e., when the ability is activated or the spell is cast — by reading the source permanent's current state.

### Construction sites

Wherever a `DealDamage` step is built, the source's flags must be snapshotted. Currently this happens in:

- `src/parser/oracle.rs` — when parsing "deals N damage" from oracle text, the source is not yet known (it's a card definition, not a game object). **The flags cannot be set here.** This is the key structural issue: `EffectStep` is defined at parse time but needs runtime data.

This means the flags cannot live in the step as parsed oracle data. They must be injected when the ability goes on the stack.

### Better model: inject at stack-push time

The `StackObject` already has a `source_id` field (on `TriggeredAbility` and `ActivatedAbility` payloads). When pushing a `DealDamage`-containing effect onto the stack, the engine should walk the effect's steps and substitute concrete flags for any `DealDamage` steps using the source's current keywords:

```rust
fn inject_source_flags(effect: Effect, source_id: ObjectId, state: &GameState) -> Effect {
    let source_perm = state.battlefield.get(&source_id);
    effect.into_iter().map(|step| match step {
        EffectStep::DealDamage(mut s) => {
            if let Some(perm) = source_perm {
                s.lifelink   = perm.has_keyword(StaticAbility::Lifelink);
                s.deathtouch = perm.has_keyword(StaticAbility::Deathtouch);
                s.wither     = perm.has_keyword(StaticAbility::Wither);
                s.infect     = perm.has_keyword(StaticAbility::Infect);
                s.toxic_n    = perm.toxic_n();
            }
            EffectStep::DealDamage(s)
        }
        other => other,
    }).collect()
}
```

Called in `engine/activated.rs` and `engine/stack.rs` (triggered abilities) when the `StackObject` is constructed.

If the source is not on the battlefield (e.g. a spell dealing damage — the source is the spell itself), flags come from the spell's card definition at cast time, which is available.

### Updated resolution in `execute_effect_steps`

```rust
EffectStep::DealDamage(s) => {
    let amount = s.amount;
    match targets.first() {
        Some(EffectTarget::Object { id }) => {
            if let Some(perm) = state.battlefield.get_mut(id) {
                if s.wither || s.infect {
                    perm.add_counters(CounterKind::PtModifier { power: -1, toughness: -1 }, amount);
                } else {
                    perm.damage_marked += amount;
                }
                if s.deathtouch && amount > 0 {
                    perm.damaged_by_deathtouch = true;
                }
            }
        }
        Some(EffectTarget::Player { id }) => {
            if let Some(player) = state.get_player_mut(*id) {
                if s.infect {
                    player.add_counters(CounterKind::Poison, amount);
                } else {
                    player.life -= amount as i32;
                }
                if let Some(n) = s.toxic_n {
                    player.add_counters(CounterKind::Poison, n);
                }
            }
        }
        _ => {}
    }
    if s.lifelink && amount > 0 {
        if let Some(player) = state.get_player_mut(controller) {
            player.life += amount as i32;
        }
    }
}
```

This mirrors the accumulate-then-apply logic in `deal_combat_damage`, but since stack resolution is inherently sequential (one step at a time, one target), the simultaneous-application concern doesn't apply.

---

## Interaction: Multi-Target Damage

`DealDamage` currently only handles `targets.first()`. If a future spell deals damage to multiple targets ("deals 2 damage to each of up to two target creatures"), the step would need to iterate all targets, not just the first. This is a separate issue (see design notes on AOE effects) but the flag-injection approach above works per-target: the source's flags apply uniformly to all damage dealt by the step.

---

## Files to Change

- `src/types/effect.rs` — replace `DealDamage(u32)` with `DealDamage(DamageStep)`; define `DamageStep`
- `src/engine/stack.rs` — update `execute_effect_steps` to route damage per flags; add `inject_source_flags` helper
- `src/engine/activated.rs` — call `inject_source_flags` when pushing activated abilities onto the stack
- `src/engine/casting.rs` — call `inject_source_flags` (or equivalent) for spell DealDamage steps at cast time
- `src/engine/triggered.rs` — call `inject_source_flags` when constructing triggered ability stack objects with DealDamage
- All existing `DealDamage(n)` construction sites — update to `DealDamage(DamageStep { amount: n, ..Default::default() })`
