# Counters System — Data Model & Infrastructure

**Date:** 2026-06-17  
**Scope:** Counter data model, P/T integration, `EffectStep::AddCounter`, SBAs. No keyword abilities (Wither, Infect, Toxic, Evolve, Training, Persist, Undying, Scavenge) — those are a follow-on pass.

---

## 1. `CounterKind` (new `src/types/counter.rs`)

```rust
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum CounterKind {
    /// CR 122.1a: per-counter P/T delta. +1/+1 → { power: 1, toughness: 1 }.
    PtModifier { power: i32, toughness: i32 },
    /// CR 122.1f: poison counter placed on a player.
    Poison,
    /// Fallback for named counters with no specific rules meaning (charge, time, age, …).
    Named(String),
}
```

`Hash + Eq` makes it a valid `HashMap` key. Named fields on `PtModifier` follow the CR naming ("X" and "Y" per 122.1a) and aid readability at call sites.

**Parser contract:** the parser must produce `CounterKind::Poison` specifically for poison counters — not `Named("poison")`. The same principle extends to any future recognized variant. `Named` is purely a fallback for unrecognized counter names.

---

## 2. `counters` field on `PermanentState` and `Player`

Both types gain:

```rust
pub counters: HashMap<CounterKind, u32>,
```

Initialized to an empty `HashMap` in their respective `new()` constructors.

Both types gain the same two convenience methods:

```rust
pub fn counter_count(&self, kind: &CounterKind) -> u32 { … }
pub fn add_counters(&mut self, kind: CounterKind, n: u32) { … }
```

`counter_count` returns 0 for absent keys. `add_counters` uses `entry().or_insert(0) += n`.

---

## 3. `effective_power` / `effective_toughness` updated

Both methods in `PermanentState` are extended to sum `PtModifier` counter contributions on top of `current_power`/`current_toughness` + `pt_boost_until_eot`:

```
effective_power = current_power + pt_boost_until_eot.power
                + Σ (power_delta × count) for each PtModifier counter
```

The iteration filters only `PtModifier` variants; `Poison` and `Named` counters have no P/T effect.

---

## 4. `EffectStep::AddCounter`

New variant in `src/types/effect.rs`:

```rust
/// CR 122.6: put one or more counters of a given kind onto the target.
AddCounter { kind: CounterKind, count: u32 },
```

Resolved in `execute_effect_steps` (`src/engine/stack.rs`) using `targets.first()`:

- `EffectTarget::Object { id }` → calls `perm.add_counters(kind, count)` on the battlefield permanent.
- `EffectTarget::Player { id }` → calls `player.add_counters(kind, count)` on the player.
- Any other target shape → no-op (consistent with `DealDamage` and `BoostPermanentPT` behaviour).

This allows spells parsed as e.g. "Put a +1/+1 counter on target creature" to be represented as:

```rust
SpellAbility {
    target_requirements: vec![TargetFilter::Creature],
    steps: vec![EffectStep::AddCounter {
        kind: CounterKind::PtModifier { power: 1, toughness: 1 },
        count: 1,
    }],
}
```

---

## 5. State-Based Actions

Two new SBA checks added to `find_sbas` in `src/engine/state_based_actions.rs`:

### CR 122.3 — +1/+1 / -1/-1 counter cancellation

> "If a permanent has both a +1/+1 counter and a -1/-1 counter on it, N +1/+1 and N -1/-1 counters are removed from it as a state-based action, where N is the smaller of the number of +1/+1 and -1/-1 counters on it."

For each battlefield permanent, if both `PtModifier { power: 1, toughness: 1 }` and `PtModifier { power: -1, toughness: -1 }` counts are > 0, enqueue `Sba::CancelCounters(id, n)` where n = min of the two counts.

New SBA variant:
```rust
CancelCounters(ObjectId, u32),
```

Application: subtract `n` from each of the two counter kinds (using saturating subtraction; remove the key if the count reaches 0).

### CR 122.1f — Poison loss condition

> "If a player has ten or more poison counters, that player loses the game as a state-based action."

Checked alongside the existing CR 704.5a life-loss check. Reuses the existing `Sba::PlayerLoses(PlayerId)` variant — no new variant needed.

---

## 6. Files changed

| File | Change |
|---|---|
| `src/types/counter.rs` | New — `CounterKind` |
| `src/types/mod.rs` | Add `pub mod counter` + re-export `CounterKind` |
| `src/types/permanent.rs` | Add `counters` field, update `new`, update `effective_power`/`toughness`, add helpers |
| `src/types/player.rs` | Add `counters` field, update `new`, add helpers |
| `src/types/effect.rs` | Add `EffectStep::AddCounter` |
| `src/engine/stack.rs` | Handle `AddCounter` in `execute_effect_steps` |
| `src/engine/state_based_actions.rs` | Add CR 122.3 and CR 122.1f SBA checks |

---

## 7. Testing

Unit tests to add or extend:

- `PermanentState::effective_power/toughness` with `PtModifier` counters present
- `PermanentState::effective_power/toughness` with both +1/+1 and -1/-1 counters (before SBA runs)
- `counter_count` returns 0 for absent keys
- `add_counters` accumulates correctly across multiple calls
- `execute_effect_steps` with `AddCounter` targeting a creature — counter appears on permanent
- `execute_effect_steps` with `AddCounter` targeting a player — counter appears on player
- SBA CR 122.3: +1/+1 and -1/-1 cancel correctly (equal counts → both removed; unequal → remainder stays)
- SBA CR 122.3: non-(+1/+1)/(−1/−1) PtModifier counters are not cancelled
- SBA CR 122.1f: player with exactly 10 poison counters loses; 9 does not
- End-to-end: a spell with `EffectStep::AddCounter { kind: PtModifier(1,1), count: 1 }` resolves and the targeted creature's `effective_power` increases by 1
