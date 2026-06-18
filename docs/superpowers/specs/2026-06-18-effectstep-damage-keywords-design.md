# Design Spec: EffectStep::DealDamage Source Keyword Propagation

**Date:** 2026-06-18
**Status:** Approved for implementation

---

## Problem Statement

`EffectStep::DealDamage(u32)` carries only an amount. When it resolves through the
stack, keyword properties of the damage source — Lifelink, Deathtouch, Wither, Infect
— are never consulted. Combat damage handles these correctly via a parallel code path
that bypasses `EffectStep` entirely. Any activated ability, triggered ability, or spell
that deals damage from a source with these keywords silently behaves as vanilla damage.

### Concrete breakage

| Keyword | CR | Bug |
|---------|----|-----|
| Lifelink | 702.15b | Controller gains no life when ability resolves through stack |
| Deathtouch | 702.2b | `damaged_by_deathtouch` never set; SBA does not destroy target creature |
| Wither | 702.80a | `damage_marked += n` instead of -1/-1 counters on creature |
| Infect | 702.90b/c | `damage_marked` on creature; `life -= n` on player — both wrong |

Toxic N (CR 702.164a) is **not** broken here. It is a combat-damage-only triggered
ability and is correctly handled in `deal_combat_damage`. It does not belong in the
stack damage path.

---

## Rules Basis

- **CR 702.15b** — Lifelink: damage dealt causes the source's controller to gain that
  much life.
- **CR 702.2b** — Deathtouch: any nonzero damage from a deathtouch source is lethal;
  creature is destroyed by SBA.
- **CR 702.2e** — Deathtouch (and by analogy other keywords): if the source changes
  zones before dealing damage, last-known information is used.
- **CR 702.80a/b** — Wither: damage to a creature becomes -1/-1 counters; damage to a
  player is still regular life loss.
- **CR 702.90b/c** — Infect: damage to a creature becomes -1/-1 counters; damage to a
  player becomes poison counters (no life loss).

---

## Why Snapshot at Stack-Push Time

The correct fix is to read source keyword flags when the ability goes on the stack, not
at resolution. Two alternatives were considered and rejected:

**LKI lookup at resolution**: look up the source's current keywords when `DealDamage`
resolves. Fails when the source has left the battlefield — CR 702.2e requires
last-known information in that case, and there is no LKI cache in the engine.

**Dual step variants**: leave `DealDamage(u32)` for vanilla cases and add
`DealDamageFromSource { amount, source_id }` for keyword-aware cases. Same LKI
requirement; adds a variant without reducing complexity.

**Snapshot flags (chosen)**: read keywords from the source at activation/cast time and
bake them into the step. This is rules-correct because CR 702.2e pins these properties
to the source's state when the ability was put on the stack — exactly what snapshotting
captures. No LKI cache needed.

---

## Design

### 1. Data Model (`src/types/effect.rs`)

Replace `DealDamage(u32)` with `DealDamage(DamageStep)`:

```rust
/// CR 702.15b, 702.2b, 702.80a, 702.90b/c — source keyword flags snapshotted at
/// stack-push time. All flags default to false; the parser always produces
/// flag-less steps and injection fills them in at runtime.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct DamageStep {
    pub amount: u32,
    pub lifelink: bool,
    pub deathtouch: bool,
    pub wither: bool,
    pub infect: bool,
}

pub enum EffectStep {
    // ...
    DealDamage(DamageStep),
    // ...
}
```

`DamageStep` derives `Default` so existing call sites migrate cleanly:

```rust
// Before
EffectStep::DealDamage(3)

// After
EffectStep::DealDamage(DamageStep { amount: 3, ..Default::default() })
```

No behaviour change at existing call sites — all flags default to `false`.

**Toxic N is excluded.** CR 702.164a makes it a combat-damage-only triggered ability,
already handled in `deal_combat_damage`. Including it here would incorrectly apply it
to non-combat damage.

**`combat: bool` is excluded.** Combat damage never flows through `EffectStep::DealDamage`
— `deal_combat_damage` applies damage directly, bypassing the step entirely. The field
would always be `false` and suggest a unification that isn't planned.

---

### 2. Flag Injection (`src/engine/stack.rs`)

A `pub(crate)` helper reads keyword flags from a source's `OracleSpan` list and
rewrites any `DealDamage` steps in an effect:

```rust
pub(crate) fn inject_source_flags(effect: Effect, source_abilities: &[OracleSpan]) -> Effect {
    effect.into_iter().map(|step| match step {
        EffectStep::DealDamage(s) => EffectStep::DealDamage(DamageStep {
            lifelink:   has_damage_kw(source_abilities, &StaticAbility::Lifelink),
            deathtouch: has_damage_kw(source_abilities, &StaticAbility::Deathtouch),
            wither:     has_damage_kw(source_abilities, &StaticAbility::Wither),
            infect:     has_damage_kw(source_abilities, &StaticAbility::Infect),
            ..s
        }),
        other => other,
    }).collect()
}

fn has_damage_kw(abilities: &[OracleSpan], kw: &StaticAbility) -> bool {
    abilities.iter().any(|span| {
        matches!(span, OracleSpan::Parsed(Ability::Static(k)) if k == kw)
    })
}
```

The function takes `&[OracleSpan]` (not `&PermanentState`) so it works uniformly for
both battlefield sources (activated/triggered abilities) and card-definition sources
(spells). Both expose `definition.abilities: Vec<OracleSpan>`.

**Call sites:**

| File | Source | When |
|------|--------|------|
| `engine/activated.rs` | `state.battlefield.get(&object_id)` | After `ability.effect.clone()`, before inserting `StackObject` |
| `engine/triggered.rs` — all `collect_*_triggers` functions | `state.battlefield.get(&source_id)` | When building `StackPayload::TriggeredAbility { effect, .. }` |
| `engine/stack.rs` `resolve_top` — spell branch | `state.objects.get(&card_id)` | After extracting steps from card definition, before `execute_effect_steps` |

The spell case is a no-op in practice — instants and sorceries do not carry these
keywords as static abilities. Injecting uniformly keeps the call sites consistent and
handles the hypothetical future case of continuous effects granting keywords to spells.

**Known limitation:** if `state.battlefield.get(&source_id)` returns `None` (source
left the battlefield before its trigger was collected), injection is called with `&[]`
and all flags remain `false`. CR 702.2e requires using last-known information in this
case. Implementing LKI correctly requires a dedicated cache and is out of scope here;
this limitation is acceptable for the current engine phase.

---

### 3. Resolution (`src/engine/stack.rs` — `execute_effect_steps`)

The `DealDamage` arm is replaced with:

```rust
// CR 702.15b, 702.2b, 702.80a/b, 702.90b/c
EffectStep::DealDamage(s) => {
    let amount = s.amount;
    match targets.first() {
        Some(EffectTarget::Object { id }) => {
            if let Some(perm) = state.battlefield.get_mut(id) {
                if s.wither || s.infect {
                    // CR 702.80a / 702.90b: damage to a creature from a wither/infect
                    // source becomes -1/-1 counters instead of marked damage.
                    perm.add_counters(
                        CounterKind::PtModifier { power: -1, toughness: -1 },
                        amount,
                    );
                } else {
                    perm.damage_marked += amount;
                }
                if s.deathtouch && amount > 0 {
                    // CR 702.2b: any nonzero damage from a deathtouch source is lethal.
                    perm.damaged_by_deathtouch = true;
                }
            }
        }
        Some(EffectTarget::Player { id }) => {
            if let Some(player) = state.get_player_mut(*id) {
                if s.infect {
                    // CR 702.90c: infect damage to a player is poison counters, not life loss.
                    player.add_counters(CounterKind::Poison, amount);
                } else {
                    // CR 702.80b: wither damage to a player is regular life loss.
                    player.life -= amount as i32;
                }
            }
        }
        _ => {}
    }
    if s.lifelink && amount > 0 {
        // CR 702.15b: source's controller gains life equal to damage dealt.
        if let Some(player) = state.get_player_mut(controller) {
            player.life += amount as i32;
        }
    }
}
```

Key rules-derived decisions:

- **Wither on players is regular life loss** (CR 702.80b). Only `infect` routes player
  damage to poison counters. The `infect`-only branch in the player arm is correct.
- **Wither + Deathtouch on creatures**: both apply. The creature gets -1/-1 counters
  and `damaged_by_deathtouch` is set. This is correct — Deathtouch still marks the
  damage as lethal for overflow purposes (CR 702.2c) even though no `damage_marked`
  is set.
- **Lifelink outside the match**: life gain is applied once per step, regardless of
  target type, after both target branches. Amount-zero short-circuit (`amount > 0`)
  avoids a no-op life event.

---

### 4. Tests

#### `inject_source_flags` unit tests (in `stack.rs` tests block)

| Test | Input abilities | Expected DamageStep |
|------|----------------|---------------------|
| Lifelink source | `[StaticAbility::Lifelink]` | `lifelink: true`, others false |
| Wither + Infect source | both keywords | `wither: true, infect: true` |
| Vanilla source | `[]` | all flags false |
| Empty slice (absent source) | `&[]` | all flags false |
| Non-DealDamage step passes through | `DrawCard(1)` in effect | `DrawCard(1)` unchanged |

#### Resolution behavior tests (in `stack.rs` tests block)

Each test pushes a `TriggeredAbility` or `ActivatedAbility` `StackObject` whose
`DamageStep` has flags pre-set, then calls `resolve_top`.

| Test | Flags | Target | Assert |
|------|-------|--------|--------|
| Lifelink to creature | `lifelink: true` | creature | controller life += amount |
| Lifelink to player | `lifelink: true` | player | target life decreases AND controller life increases |
| Lifelink with amount 0 | `lifelink: true, amount: 0` | player | no life change either side |
| Deathtouch nonzero | `deathtouch: true, amount: 1` | 1/1 creature | `damaged_by_deathtouch == true`; SBA removes it from battlefield |
| Deathtouch zero | `deathtouch: true, amount: 0` | creature | `damaged_by_deathtouch` stays false |
| Wither to creature | `wither: true, amount: 2` | creature | 2× `PtModifier(-1,-1)` counter; `damage_marked == 0` |
| Wither to player | `wither: true, amount: 2` | player | life -= 2; no poison counters |
| Infect to creature | `infect: true, amount: 3` | creature | 3× `PtModifier(-1,-1)` counter; `damage_marked == 0` |
| Infect to player | `infect: true, amount: 3` | player | 3 poison counters; life unchanged |
| Wither + Deathtouch to creature | both true, `amount: 1` | creature | `PtModifier(-1,-1)` counter AND `damaged_by_deathtouch == true` |
| Vanilla (all false) to creature | all false, `amount: 3` | creature | `damage_marked == 3`; no counters (regression) |
| Vanilla (all false) to player | all false, `amount: 3` | player | life -= 3; no counters (regression) |

The Deathtouch test must use a 1/1 creature target so that `check_and_apply_sbas`
(which runs inside `resolve_top`) can destroy it, validating the full
deathtouch → flag → SBA destroy chain.

---

## Files to Change

| File | Change |
|------|--------|
| `src/types/effect.rs` | Define `DamageStep`; replace `DealDamage(u32)` with `DealDamage(DamageStep)` |
| `src/engine/stack.rs` | Add `inject_source_flags` + `has_damage_kw`; update `execute_effect_steps` arm; inject in `resolve_top` spell branch; add all tests |
| `src/engine/activated.rs` | Call `inject_source_flags` when building `ActivatedAbility` payload |
| `src/engine/triggered.rs` | Call `inject_source_flags` in each `collect_*_triggers` function |
| All existing `DealDamage(n)` construction sites | Migrate to `DealDamage(DamageStep { amount: n, ..Default::default() })` |

---

## Known Limitations / Out of Scope

- **LKI for absent sources**: if a permanent leaves the battlefield before its triggered
  ability is collected, the engine cannot recover its keywords. Flags default to false.
  Full LKI support requires a dedicated cache and is deferred.
- **Non-creature permanent targets** (planeswalkers, battles): the engine does not yet
  distinguish creature vs non-creature Object targets. Wither/Infect counters would be
  placed on any Object target. This is a pre-existing limitation, not introduced here.
- **Multi-target DealDamage**: the step currently handles only `targets.first()`. AOE
  damage ("deals 2 damage to each of up to two targets") is a separate future concern;
  the flag-injection approach works per-source, so flags apply uniformly when multi-target
  is eventually added.
- **Continuous effects granting keywords to spells**: not modelled. Spells always inject
  all-false flags.
