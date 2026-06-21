# Graveyard Zone-Change Hook + Persist/Undying — Design Spec

**Date:** 2026-06-21  
**Status:** Approved

---

## Overview

Implement a generic `MoveZone` effect step that moves a card object between any two zones
(graveyard → battlefield, battlefield → hand, exile → battlefield, etc.), then use it to
implement Persist (CR 702.79) and Undying (CR 702.93).

Scavenge (CR 702.97) is explicitly out of scope: it requires an activated-ability-from-graveyard
system that does not yet exist.

---

## New Types

### `ZoneOwner` (in `types/zone.rs`)

```rust
pub enum ZoneOwner {
    CardOwner,      // the player who started with the card
    CardController, // who controls it at the time of the move
}
```

Used only by the `to` side of `MoveZone`. The `from` side is always deterministic: for
player-specific zones (Hand, Library, Graveyard) a card always lives in its owner's pile;
for shared zones (Battlefield, Exile, Stack) there is no player component.

### `TriggerCondition::SubjectLacksCounter(CounterKind)` (in `types/ability.rs`)

New arm on the existing `TriggerCondition` enum. Satisfied when the subject permanent carries
zero counters of the given kind. Evaluated at trigger-collection time — before the zone change —
so the creature is still visible in `state.battlefield` (per the existing LKI pattern in SBA code).

### `EffectStep::MoveZone` (in `types/effect.rs`)

```rust
MoveZone {
    from: Zone,          // expected source zone; step is a no-op if object is elsewhere
    to: Zone,            // destination zone
    to_player: ZoneOwner, // whose pile / who controls (for player-specific or Battlefield dest)
}
```

Sequential composition with `AddCounter` covers Persist and Undying without adding a
`with_counter` field to `MoveZone` itself.

### `StaticAbility::Persist` and `StaticAbility::Undying` (in `types/ability.rs`)

Added for display (`display_name()`) and eventual parser emission. A shim in
`collect_triggers_for_event` converts these static entries into trigger dispatch at runtime,
mirroring the existing Evolve shim pattern.

---

## Trigger Infrastructure

### `trigger_condition_satisfied` (in `engine/triggered.rs`)

New match arm:

```rust
TriggerCondition::SubjectLacksCounter(kind) => {
    let sid = subject_id?;
    state.battlefield.get(&sid)
        .map_or(true, |p| p.counter_count(kind) == 0)
}
```

Returns `true` (condition met) when the dying permanent has zero of the given counter kind.

### Shims in `collect_triggers_for_event`

Two shims, one each for `StaticAbility::Persist` and `StaticAbility::Undying`, keyed on
`GameEvent::Dies { subject_id }`. Each shim:
1. Checks that `source_id == subject_id` (it's dying creature's own ability).
2. Checks that the subject is in `state.battlefield` (still visible before zone change).
3. Evaluates `SubjectLacksCounter` inline.
4. If satisfied, pushes a `StackObject` with `TriggerTargetMode::Source` targeting the dying ID.

### Helper functions

```rust
pub fn persist_triggered_ability() -> TriggeredAbility { ... }
pub fn undying_triggered_ability() -> TriggeredAbility { ... }
```

Both use `TriggerEvent::Dies { subject: TriggerSubjectFilter { is_self: Some(true), .. } }`,
`TriggerTargetMode::Source`, and the two-step effect below.

---

## Effect Steps

### Persist

```
CR 702.79: "When a creature with Persist dies, if it had no -1/-1 counters on it,
return it to the battlefield under its owner's control with a -1/-1 counter on it."
```

```rust
vec![
    EffectStep::MoveZone {
        from: Zone::Graveyard,
        to: Zone::Battlefield,
        to_player: ZoneOwner::CardOwner,
    },
    EffectStep::AddCounter {
        kind: CounterKind::PtModifier { power: -1, toughness: -1 },
        count: 1,
    },
]
```

Condition: `TriggerCondition::SubjectLacksCounter(CounterKind::PtModifier { power: -1, toughness: -1 })`

### Undying

```
CR 702.93: "When a creature with Undying dies, if it had no +1/+1 counters on it,
return it to the battlefield under its owner's control with a +1/+1 counter on it."
```

```rust
vec![
    EffectStep::MoveZone {
        from: Zone::Graveyard,
        to: Zone::Battlefield,
        to_player: ZoneOwner::CardOwner,
    },
    EffectStep::AddCounter {
        kind: CounterKind::PtModifier { power: 1, toughness: 1 },
        count: 1,
    },
]
```

Condition: `TriggerCondition::SubjectLacksCounter(CounterKind::PtModifier { power: 1, toughness: 1 })`

---

## `MoveZone` Execution (in `engine/stack.rs`)

For each `EffectTarget::Object { id }` in the trigger's target list:

1. **Validate zone**: if `state.objects[id].zone != from`, skip (no-op).
2. **Remove from source**:
   - Graveyard: `state.graveyards[owner].retain(|&x| x != id)`
   - Battlefield: `state.battlefield.remove(id)`
   - Exile: `state.exile.retain(|&x| x != id)`
   - Hand: `state.hands[owner].retain(|&x| x != id)`
3. **Update object**:
   - `obj.zone = to`
   - If `to == Battlefield`: set `obj.controller` per `to_player` (`obj.owner` or current `obj.controller`)
4. **Insert into destination**:
   - Graveyard: push to `state.graveyards[obj.owner]`
   - Battlefield: create `PermanentState::new(&obj.definition)` with `controller_since_turn = state.turn_number`; insert into `state.battlefield`
   - Exile: push to `state.exile`
   - Hand: push to `state.hands[owner]`
5. **ETB triggers** (only when `to == Battlefield`): collect via `collect_triggers_for_event(GameEvent::EntersTheBattlefield { subject_id: id })` and push onto `state.stack`.

The `AddCounter` step after `MoveZone` operates on `state.battlefield[id]`, which is now populated, so no special sequencing is needed.

---

## `docs/todo.md` changes

- Remove Persist and Undying from the "Counter system block" and "Graveyard / zone-change block" sections.
- Add a note in a new "Activated abilities from non-battlefield zones" section covering:
  - Cycling is currently a special case; generalise it.
  - Graveyard activations: Scavenge (702.97), Unearth (702.84), Escape (702.138), Flashback (702.34), Dredge (702.52), Delve (702.66).
  - Hand activations: Foretell (702.143).
  - Exile activations: Cascade (702.85), Suspend (702.62).

---

## Testing plan

- `TriggerCondition::SubjectLacksCounter`: unit tests for both true/false cases.
- `MoveZone` graveyard→battlefield: unit test that object appears in `state.battlefield` and not in graveyard; ETB triggers fire.
- Persist end-to-end: creature with Persist and no -1/-1 counter dies → trigger collected → resolves → back on battlefield with -1/-1 counter.
- Persist suppressed when already has -1/-1 counter: no trigger fires.
- Undying end-to-end: parallel to Persist.
- Undying suppressed when already has +1/+1 counter.
- `MoveZone` no-op when object not in `from` zone.
