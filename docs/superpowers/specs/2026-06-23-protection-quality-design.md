# Protection Quality & Hexproof-From Design

**Date:** 2026-06-23  
**Status:** Approved

## Goal

Complete the "Unblocked" items remaining in `docs/todo.md` under "Protection from X — partial":

1. Damage prevention (D in DEBT) — CR 702.16e
2. `ProtectionQuality` enum replacing the colour-only `ProtectionFromColor(ManaColor)` — CR 702.16a
3. `ProtectionFrom(Everything)` — CR 702.16j
4. `HexproofFromColor(ManaColor)` — CR 702.11d

The enchant/equip items (E in DEBT) remain future work as noted in the todo.

---

## Section 1 — New types in `types/ability.rs`

### `ProtectionQuality` enum

```rust
// CR 702.16a
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ProtectionQuality {
    Color(ManaColor),
    CardType(CardType),    // e.g. Artifact, Instant
    CreatureType(String),  // e.g. "Eldrazi", "Vampire"
    Everything,            // CR 702.16j
}
```

### `source_matches_quality` free function

```rust
pub fn source_matches_quality(
    quality: &ProtectionQuality,
    source_colors: &[ManaColor],
    source_card_types: &[CardType],
    source_subtypes: &[String],
) -> bool
```

- `Color(c)` → `source_colors.contains(c)`
- `CardType(ct)` → `source_card_types.contains(ct)`
- `CreatureType(st)` → `source_subtypes` contains `st` (case-insensitive)
- `Everything` → always `true`

### `KeywordAbility` changes

- Rename `ProtectionFromColor(ManaColor)` → `ProtectionFrom(ProtectionQuality)`  
  Update `display_name`: "Protection from white", "Protection from artifacts", "Protection from Eldrazi", "Protection from everything"
- Add `HexproofFromColor(ManaColor)` (CR 702.11d)  
  `display_name`: "Hexproof from white", etc.

---

## Section 2 — `DamageStep` expansion in `types/effect.rs`

Add three new fields (all default to empty `vec![]`):

```rust
pub source_colors: Vec<ManaColor>,
pub source_card_types: Vec<CardType>,
pub source_subtypes: Vec<String>,
```

These capture the source's characteristics at stack-push time (LKI pattern matching existing `lifelink`, `wither`, etc. fields). Relevant CR: 702.16e.

### DealDamage protection check in `execute_effect_steps` (`engine/stack.rs`)

When resolving `EffectStep::DealDamage` against an `EffectTarget::Object`, before applying damage, call `protection_prevents_damage` using the snapshotted `source_colors`, `source_card_types`, `source_subtypes` from `DamageStep`. If it returns `true`, skip damage application entirely (damage is prevented — CR 702.16e). Player targets are not checked (players have no keyword protection in this engine).

### `inject_source_flags` signature change (`engine/stack.rs`)

```rust
pub(crate) fn inject_source_flags(
    effect: Effect,
    source_rules_text: &[RulesText],
    source_colors: &[ManaColor],
    source_card_types: &[CardType],
    source_subtypes: &[String],
) -> Effect
```

The three new params are snapshotted into `DamageStep`. All three call sites (stack.rs, triggered.rs, activated.rs) gain the extra slices from the source object they already hold.

---

## Section 3 — Targeting (`engine/targeting.rs`)

### Signature changes

```rust
pub fn is_legal_target(
    state: &GameState,
    target: &EffectTarget,
    filter: &TargetFilter,
    caster: PlayerId,
    source_colors: &[ManaColor],
    source_card_types: &[CardType],   // new
    source_subtypes: &[String],        // new
) -> bool

pub fn legal_targets(
    state: &GameState,
    filter: &TargetFilter,
    caster: PlayerId,
    source_colors: &[ManaColor],
    source_card_types: &[CardType],   // new
    source_subtypes: &[String],        // new
) -> Vec<EffectTarget>
```

Callers: serve.rs (2 places), state_based_actions.rs (1 place), internal tests.

### Logic changes

- `ProtectionFrom(q)` check: `source_matches_quality(q, source_colors, source_card_types, source_subtypes)`
- `HexproofFromColor(c)` check: `source_colors.contains(c) && obj.controller != caster` (analogous to `Hexproof` but colour-gated)

---

## Section 4 — Combat damage (`engine/combat.rs`)

### Helper function

Defined in `engine/mod.rs` (already hosts `continuous_pt_bonus` and similar shared helpers) so both `combat.rs` and `stack.rs` can call it:

```rust
pub(crate) fn protection_prevents_damage(
    target_obj: &CardObject,
    source_colors: &[ManaColor],
    source_card_types: &[CardType],
    source_subtypes: &[String],
) -> bool
```

Iterates `target_obj.definition.rules_text` looking for `Active(Static(ProtectionFrom(q)))`. Returns true if any quality matches via `source_matches_quality`.

### Integration points

1. **Attacker → player**: players have no keyword protection in this engine; no check needed.
2. **Attacker → blocker**: before accumulating `damage_to_objects[blocker_id]` (and `wither_to_objects`), call `protection_prevents_damage` with the attacker's colors/card_types/subtypes. If true, skip.
3. **Blocker → attacker**: same pattern before `damage_to_objects[attacker_id]`.
4. **Blocking legality**: rename existing `ProtectionFromColor` match arm to use `source_matches_quality` via `ProtectionFrom(q)`.

---

## Section 5 — Parser (`parser/oracle.rs`)

`protection from [quality]` dispatch table (after stripping prefix and trailing `.`):

| Input text | Result |
|---|---|
| `white` / `blue` / `black` / `red` / `green` | `ProtectionFrom(Color(c))` |
| `everything` | `ProtectionFrom(Everything)` |
| `artifacts` / `artifact` | `ProtectionFrom(CardType(Artifact))` |
| `creatures` / `creature` | `ProtectionFrom(CardType(Creature))` |
| `instants` / `instant` | `ProtectionFrom(CardType(Instant))` |
| `[word] creatures` (e.g. `vampire creatures`) | `ProtectionFrom(CreatureType("Vampire"))` |
| anything else | `ParsedUnimplemented` (unchanged) |

`hexproof from [color]` dispatch: strip prefix `"hexproof from "`, match colour words → `HexproofFromColor(c)`. This replaces the existing `ParsedUnimplemented` path for `s.starts_with("hexproof from ")`.

---

## Section 6 — Tests

### `engine/combat.rs`

- Protection prevents attacker from dealing damage to blocker
- Protection prevents blocker from dealing damage back to attacker
- `ProtectionFrom(Everything)` prevents all combat damage
- Blocking legality: `ProtectionFrom(CardType(Artifact))` blocks artifact blockers

### `engine/targeting.rs`

- `HexproofFromColor(Blue)` blocks blue spell from opponent, allows red spell, allows blue spell from controller
- `ProtectionFrom(CardType(Artifact))` with `source_card_types=[Artifact]` prevents targeting

### `parser/oracle.rs`

- `"protection from everything"` parses to `ProtectionFrom(Everything)`
- `"protection from artifacts"` parses to `ProtectionFrom(CardType(Artifact))`
- `"protection from vampire creatures"` parses to `ProtectionFrom(CreatureType("Vampire"))`
- `"hexproof from black"` parses to `HexproofFromColor(Black)`

---

## Files touched

| File | Change |
|---|---|
| `src/types/ability.rs` | Add `ProtectionQuality`, `source_matches_quality`; rename variant; add `HexproofFromColor` |
| `src/types/effect.rs` | Add 3 fields to `DamageStep` |
| `src/engine/stack.rs` | Expand `inject_source_flags` signature; add protection prevention to `DealDamage` resolution |
| `src/engine/targeting.rs` | Expand `is_legal_target`/`legal_targets`; add `HexproofFromColor` check |
| `src/engine/combat.rs` | Add `protection_prevents_damage` helper; guard damage paths; update blocking check |
| `src/parser/oracle.rs` | Expand protection parsing; add hexproof-from-color parsing |
| `src/serve.rs` | Update `legal_targets` call sites (2) |
| `src/engine/state_based_actions.rs` | Update `is_legal_target` call site (1) |
| `docs/todo.md` | Remove completed bullets |
