# Alternative Casting Cost Framework — Kicker, Multikicker, Dash, Evoke

**Date:** 2026-06-24  
**Scope:** Framework for alternative/additional casting costs, with first implementations: Kicker (702.33), Multikicker (702.33c), Dash (702.109), Evoke (702.74).  
**Out of scope (future phases):** Morph, Bestow, Emerge, Mutate.

---

## Background

`cast_spell` currently only handles a card's standard `mana_cost`. The "Alternative casting block" section of `docs/todo.md` lists several mechanics that require a general framework for communicating, validating, paying, and recording non-standard casting costs at the stack level.

The four mechanics in scope divide into two CR categories:

| Mechanic | CR | Category | Effect |
|---|---|---|---|
| Kicker | 702.33a | Additional cost | Pay mana_cost + kicker_cost; conditional effects in rules text |
| Multikicker | 702.33c | Additional cost (repeatable) | Pay mana_cost + N × kicker_cost; N ≥ 1 |
| Dash | 702.109a | Alternative cost | Replaces mana_cost; grants Haste; returns to hand at next end step |
| Evoke | 702.74a | Alternative cost | Replaces mana_cost; ETB triggered ability sacrifices the permanent |

---

## 1. Type System — `types/ability.rs`

Four new `Rule` variants, using explicit field names to distinguish additional vs. alternative costs:

```rust
Rule::Kicker      { additional_cost: Cost },   // 702.33a
Rule::Multikicker { additional_cost: Cost },   // 702.33c
Rule::Dash        { alternative_cost: Cost },  // 702.109a
Rule::Evoke       { alternative_cost: Cost },  // 702.74a
```

New `CastMode` enum (same file or a new `types/cast_mode.rs`):

```rust
pub enum CastMode {
    Standard,
    Kicked,            // Kicker paid exactly once
    Multikicked(u32),  // Multikicker paid N times; N >= 1
    Dashed,
    Evoked,
}
```

`Multikicked(0)` is invalid — rejected at cast time with `EngineError::InvalidCastMode`.

---

## 2. `cast_spell` — `engine/casting.rs`

### Signature

```rust
pub fn cast_spell(
    state: GameState,
    player_id: PlayerId,
    object_id: ObjectId,
    declared_targets: Vec<EffectTarget>,
    x_value: Option<u32>,
    cast_mode: CastMode,        // NEW — use CastMode::Standard at all existing call sites
) -> Result<GameState, EngineError>
```

### Cost selection logic

| `cast_mode` | Requires | Cost paid |
|---|---|---|
| `Standard` | `mana_cost` present | `mana_cost` |
| `Kicked` | `Rule::Kicker` present | `mana_cost + additional_cost` |
| `Multikicked(n)` | `Rule::Multikicker` present, n ≥ 1 | `mana_cost + additional_cost × n` |
| `Dashed` | `Rule::Dash` present | `alternative_cost` (replaces mana_cost) |
| `Evoked` | `Rule::Evoke` present | `alternative_cost` (replaces mana_cost) |

If the declared mode has no matching `Rule` on the card, return `EngineError::InvalidCastMode`.

### `StackObject` — `types/stack.rs`

```rust
pub struct StackObject {
    pub id: StackId,
    pub payload: StackPayload,
    pub controller: PlayerId,
    pub targets: Vec<EffectTarget>,
    pub x_value: Option<u32>,
    pub cast_mode: CastMode,    // NEW — records how this spell was cast
}
```

All existing construction sites use `CastMode::Standard`.

---

## 3. Resolution Effects — `engine/stack.rs`

### Kicker / Multikicker

No resolution side-effects. The `cast_mode` is recorded on the `StackObject` so that "if this spell was kicked" conditional rules text can check it in future. For now those conditional paragraphs remain `ParsedUnimplemented`.

### Evoke (702.74a)

When `resolve_top` moves an Evoked spell to the battlefield, it synthesises and queues an ETB triggered ability before returning:

```
EffectStep::Sacrifice { target: self_id }
```

This avoids parsing the conditional clause ("if its evoke cost was paid, sacrifice it") from oracle text. The sacrifice trigger is placed on the stack and resolves immediately (both players will then pass priority).

### Dash (702.109a)

When `resolve_top` moves a Dashed spell to the battlefield, two things happen:

1. **Haste injection** — push `RulesText::Active(Rule::Static(KeywordAbility::Haste))` onto the newly-created `PermanentState.definition.rules_text`. This modifies only the battlefield copy; `CardObject.definition` (the canonical card identity) is left untouched. When the permanent leaves the battlefield the `PermanentState` is dropped and the injected Haste disappears with it.

2. **Delayed return-to-hand trigger** — register a one-shot delayed trigger targeting the permanent's `ObjectId`: fires at `PhaseStep { step: Step::EndStep }`, effect `EffectStep::ReturnToHand(object_id)`.

**Why inject into `PermanentState.definition` and not `CardObject.definition`?**  
`PermanentState` clones `CardDefinition` on ETB and is the authoritative source for all battlefield keyword lookups. `CardObject` persists across zone changes; injecting Haste there would cause the card to retain Haste across future casts. Injecting into the `PermanentState` copy creates a battlefield-local modification that is discarded on zone change — no special-casing of `has_keyword` needed.  
(See the existing comment in `types/permanent.rs` on this clone pattern.)

---

## 4. `serve.rs` Changes

### `CastSpell` action

```rust
CastSpell {
    object_id: u64,
    #[serde(default)]
    targets: Vec<EffectTarget>,
    #[serde(default)]
    x_value: Option<u32>,
    #[serde(default)]
    cast_mode: Option<CastMode>,  // None deserialises as Standard
}
```

Existing JSON clients and tests that omit `cast_mode` continue to work unchanged.

### Action generation

For each card in hand that can be cast, one action is generated per available mode. Examples:

- A Dashed creature at sorcery speed → "Cast Hellspark Elemental" (Standard) + "Cast Hellspark Elemental (Dash {R})"
- A Kicked spell → "Cast Kor Sanctifiers" (Standard) + "Cast Kor Sanctifiers (Kicked {2})"
- A Multikicker spell → Standard + one Multikicked action per affordable `times` value (1..N)
- An Evoked creature → "Cast Shriekmaw" (Standard) + "Cast Shriekmaw (Evoke {B})"

The `cost_label` in each action JSON reflects the actual cost being paid:
- Standard: standard mana cost string
- Kicked: mana cost + kicker cost
- Multikicked(n): mana cost + n × kicker cost
- Dashed / Evoked: alternative cost string only

---

## 5. Parser — `parser/oracle.rs`

Four new parse branches replacing the existing `ParsedUnimplemented` fallbacks:

```
"kicker ..."        → Rule::Kicker      { additional_cost: ManaCost }
"multikicker ..."   → Rule::Multikicker { additional_cost: ManaCost }
"dash ..."          → Rule::Dash        { alternative_cost: ManaCost }
"evoke ..."         → Rule::Evoke       { alternative_cost: ManaCost }
```

Each branch extracts the cost string following the keyword and parses it with the existing mana cost parser. If the cost is malformed or unparseable, the line falls back to `ParsedUnimplemented` with the full text.

`display_name` / annotation infrastructure in `ability.rs` and `serve.rs` gets corresponding arms so these rules render correctly in the UI rather than as cyan+underlined unimplemented keywords.

---

## 6. Testing

### Parser (`parser/oracle.rs`)
- Each keyword round-trips: `"kicker {1}{U}"` → `Rule::Kicker { additional_cost: ManaCost([Generic(1), Blue]) }`, etc.
- Malformed cost stays `ParsedUnimplemented`.

### `cast_spell` (`engine/casting.rs`)
- Kicked: deducts `mana_cost + kicker_cost`.
- Multikicked(2): deducts `mana_cost + 2 × kicker_cost`.
- Dashed: deducts only `alternative_cost` (not `mana_cost`).
- Evoked: deducts only `alternative_cost`.
- `InvalidCastMode` when declared mode has no matching Rule.
- `Multikicked(0)` rejected.
- `cast_mode` recorded correctly on `StackObject`.

### Dash resolution (`engine/stack.rs`)
- After both players pass on a Dashed creature: permanent has Haste in `PermanentState.definition`; EOT delayed trigger is registered; `CardObject.definition` does not have Haste.

### Evoke resolution (`engine/stack.rs`)
- After both players pass on an Evoked creature: ETB sacrifice trigger is on the stack; after it resolves the permanent is in the graveyard.

### `serve.rs` action generation
- Card with Dash in hand at sorcery speed generates both Standard and Dashed actions.
- Card with Kicker generates both Standard and Kicked actions.

---

## Out of Scope

The following mechanics require this framework as a prerequisite and are tracked in `docs/todo.md` under "Alternative casting block":

- **Morph** (702.37): face-down permanents, turn-face-up action
- **Bestow** (702.103): spell type changes based on cast choice
- **Emerge** (702.119): sacrifice a creature during casting to reduce cost
- **Mutate** (702.140): merged permanent state
