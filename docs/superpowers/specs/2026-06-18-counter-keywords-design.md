# Counter-Unblocked Keywords Design

**Date:** 2026-06-18
**Keywords:** Wither, Infect, Toxic N, Evolve, Training

## Background

The counter infrastructure (`PermanentState.counters`, `Player.counters`, `CounterKind`, `EffectStep::AddCounter`, SBAs for 10-poison loss and +1/+1 vs -1/-1 cancellation) was merged June 2026. Five CR 702 keywords were previously parsed as `ParsedUnimplemented` pending that work. This spec covers their full enforcement.

---

## Section 1 — Types and Parser

### `types/ability.rs`

Add five new `StaticAbility` variants:

```rust
Wither,        // CR 702.80
Infect,        // CR 702.90
ToxicN(u32),   // CR 702.164
Evolve,        // CR 702.100
Training,      // CR 702.149
```

Add display names to `display_name()`:
- `Wither` → `"Wither"`
- `Infect` → `"Infect"`
- `ToxicN(n)` → `"Toxic {n}"`
- `Evolve` → `"Evolve"`
- `Training` → `"Training"`

### `types/permanent.rs`

Add `toxic_n()` helper mirroring the existing `bushido_n()` pattern:

```rust
pub fn toxic_n(&self) -> Option<u32> {
    self.definition.abilities.iter().find_map(|span| {
        if let OracleSpan::Parsed(Ability::Static(StaticAbility::ToxicN(n))) = span {
            Some(*n)
        } else {
            None
        }
    })
}
```

### `parser/oracle.rs`

- Remove `"wither"`, `"infect"`, `"evolve"`, `"training"` from the simple `ParsedUnimplemented` match arm.
- Add those four to `parse_keyword`, returning `OracleSpan::Parsed(Ability::Static(StaticAbility::Wither))` etc.
- Remove `"toxic "` from the parameterized `ParsedUnimplemented` list.
- Add a `ToxicN` parse branch immediately before or after the BushidoN branch, using the same `strip_prefix` + `parse::<u32>()` pattern:

```rust
// CR 702.164 Toxic N
if let Some(rest) = s.strip_prefix("toxic ")
    && let Ok(n) = rest.trim().parse::<u32>()
{
    return OracleSpan::Parsed(Ability::Static(StaticAbility::ToxicN(n)));
}
```

---

## Section 2 — Wither / Infect / Toxic in `deal_combat_damage`

### Rules basis

- CR 702.80a: Wither — damage dealt to creatures arrives as -1/-1 counters instead of marked damage.
- CR 702.90a: Infect — damage to creatures as -1/-1 counters; damage to players as poison counters.
- CR 702.164a: Toxic N — whenever this creature deals combat damage to a player, that player gets N poison counters (in addition to any other damage effects).

Lifelink still counts total damage dealt regardless of routing form (CR 702.15a). Deathtouch is unchanged.

### New accumulators in `deal_combat_damage`

Alongside the existing `damage_to_players` and `damage_to_objects`:

```rust
let mut wither_to_objects: HashMap<ObjectId, u32> = HashMap::new();
let mut poison_to_players: HashMap<PlayerId, u32> = HashMap::new();
```

### Per-source keyword extraction

Extend the existing `(atk_power, has_trample, has_deathtouch, has_lifelink, atk_controller)` tuple:

```rust
let has_wither = obj.has_keyword(StaticAbility::Wither);
let has_infect = obj.has_keyword(StaticAbility::Infect);
let toxic_n = state.battlefield.get(&attacker_id).and_then(|p| p.toxic_n());
```

Same extension needed for blocker keyword extraction (Wither/Infect only; blockers don't deal player damage).

### Damage routing

**Creature targets** (blockers hit by attacker, or attacker hit by blockers):
- If source has `Wither` or `Infect` → accumulate in `wither_to_objects`
- Otherwise → accumulate in `damage_to_objects` (unchanged)

**Player targets** (unblocked attacker, or trample remainder):
- If source has `Infect` → accumulate in `poison_to_players`
- Otherwise → accumulate in `damage_to_players` (unchanged)

**Toxic N** — applied per-attacker after player damage is determined:
```rust
// Track whether this attacker reached a player this iteration
let mut attacked_player: Option<PlayerId> = None;
// ... (set to Some(defending_player) in the unblocked/trample paths above)
if let (Some(n), Some(pid)) = (toxic_n, attacked_player) {
    *poison_to_players.entry(pid).or_insert(0) += n;
}
```

### Application (end of function, before `check_and_apply_sbas`)

```rust
for (oid, n) in wither_to_objects {
    if let Some(perm) = state.battlefield.get_mut(&oid) {
        perm.add_counters(CounterKind::PtModifier { power: -1, toughness: -1 }, n);
    }
}
for (pid, n) in poison_to_players {
    if let Some(p) = state.get_player_mut(pid) {
        p.add_counters(CounterKind::Poison, n);
    }
}
```

---

## Section 3 — Training in `collect_attack_triggers`

### Rules basis

CR 702.149a: "Whenever this creature and at least one other creature attack, if the other creature has greater power, put a +1/+1 counter on this creature."

### Implementation

New branch at the end of `collect_attack_triggers`, following the BattleCry/Melee/Exalted pattern:

```rust
// Training (CR 702.149a)
for &attacker_id in &attackers {
    if !state.battlefield.get(&attacker_id)
        .map(|p| p.has_keyword(StaticAbility::Training))
        .unwrap_or(false) { continue; }

    let my_power = state.battlefield.get(&attacker_id)
        .and_then(|p| p.effective_power())
        .unwrap_or(0);

    let has_ally_with_greater = attackers.iter()
        .filter(|&&id| id != attacker_id)
        .any(|&id| {
            state.battlefield.get(&id)
                .and_then(|p| p.effective_power())
                .map(|p| p > my_power)
                .unwrap_or(false)
        });

    if has_ally_with_greater {
        let sid = state.alloc_stack_id();
        result.push(StackObject {
            id: sid,
            payload: StackPayload::TriggeredAbility {
                source_id: attacker_id,
                effect: vec![EffectStep::AddCounter {
                    kind: CounterKind::PtModifier { power: 1, toughness: 1 },
                    count: 1,
                }],
                label: "Training".into(),
            },
            controller: attacking_player,
            targets: vec![EffectTarget::Object { id: attacker_id }],
            x_value: None,
        });
    }
}
```

The trigger resolves through the stack using the existing `EffectStep::AddCounter` handler (`stack.rs:115–127`), which already handles `EffectTarget::Object`.

---

## Section 4 — Evolve via `collect_evolve_triggers`

### Rules basis

CR 702.100b: "Whenever a creature enters the battlefield under your control, if that creature has greater power or greater toughness than this creature, put a +1/+1 counter on this creature."

The comparison is against the Evolve creature's stats (not the entering creature). Checked at trigger-collection time (consistent with how other triggers are handled in this engine).

### New function in `triggered.rs`

```rust
/// CR 702.100b: collect Evolve triggers for battlefield permanents when `entering_id` ETBs.
pub fn collect_evolve_triggers(state: &mut GameState, entering_id: ObjectId) -> Vec<StackObject> {
    let Some(entering_obj) = state.objects.get(&entering_id) else { return vec![]; };
    let controller = entering_obj.controller;
    let entering_power = state.battlefield.get(&entering_id)
        .and_then(|p| p.effective_power());
    let entering_toughness = state.battlefield.get(&entering_id)
        .and_then(|p| p.effective_toughness());

    // Collect ids of friendly Evolve permanents (excluding the entering creature itself)
    let evolve_ids: Vec<ObjectId> = state.battlefield.keys()
        .filter(|&&id| {
            id != entering_id
                && state.objects.get(&id).map(|o| o.controller == controller).unwrap_or(false)
                && state.battlefield.get(&id).map(|p| p.has_keyword(StaticAbility::Evolve)).unwrap_or(false)
        })
        .copied()
        .collect();

    evolve_ids.into_iter().filter_map(|evolve_id| {
        let perm = state.battlefield.get(&evolve_id)?;
        let my_power = perm.effective_power().unwrap_or(0);
        let my_toughness = perm.effective_toughness().unwrap_or(0);
        let qualifies = entering_power.map(|ep| ep > my_power).unwrap_or(false)
            || entering_toughness.map(|et| et > my_toughness).unwrap_or(false);
        if !qualifies { return None; }
        let sid = state.alloc_stack_id();
        Some(StackObject {
            id: sid,
            payload: StackPayload::TriggeredAbility {
                source_id: evolve_id,
                effect: vec![EffectStep::AddCounter {
                    kind: CounterKind::PtModifier { power: 1, toughness: 1 },
                    count: 1,
                }],
                label: "Evolve".into(),
            },
            controller,
            targets: vec![EffectTarget::Object { id: evolve_id }],
            x_value: None,
        })
    }).collect()
}
```

### Call sites

Both `casting.rs:63` and `stack.rs:249` call `collect_etb_triggers`. Immediately after each, add:

```rust
let evolve_triggers = collect_evolve_triggers(&mut state, object_id);
for t in evolve_triggers {
    let id = t.id;
    state.stack.push(id);
    state.stack_objects.insert(id, t);
}
```

---

## Files Changed

| File | Change |
|------|--------|
| `src/types/ability.rs` | 5 new `StaticAbility` variants + display names |
| `src/types/permanent.rs` | `toxic_n()` helper |
| `src/engine/combat.rs` | Wither/Infect/Toxic routing in `deal_combat_damage` |
| `src/engine/triggered.rs` | Training branch in `collect_attack_triggers`; new `collect_evolve_triggers` |
| `src/engine/casting.rs` | Call `collect_evolve_triggers` after `collect_etb_triggers` |
| `src/engine/stack.rs` | Call `collect_evolve_triggers` after `collect_etb_triggers` |
| `src/parser/oracle.rs` | Promote 5 keywords from `ParsedUnimplemented` |

## Out of Scope

- Persist, Undying, Scavenge — these share the counter prerequisite but also need the graveyard zone-change hook; deferred to the Graveyard/zone-change block.
- Protection-D (damage prevention) — separate todo item under Protection from X.
- Non-combat Infect/Wither (e.g. from spell effects via `EffectStep::DealDamage`) — deferred; `DealDamage` currently carries no source-keyword context (noted in `stack.rs:128`).
