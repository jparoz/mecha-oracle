# Protection Quality & Hexproof-From Design

**Date:** 2026-06-23  
**Status:** Approved

## Goal

Complete all "Unblocked" items remaining in `docs/todo.md` under "Protection from X — partial":

1. Damage prevention (D in DEBT) — CR 702.16e
2. Enchant/Equip prevention (E in DEBT) — CR 702.16c/d
3. `ProtectionQuality` enum replacing the colour-only `ProtectionFromColor(ManaColor)` — CR 702.16a
4. `ProtectionFrom(Everything)` — CR 702.16j
5. `HexproofFromColor(ManaColor)` — CR 702.11d

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
- Add `HexproofFrom(ProtectionQuality)` (CR 702.11d) — reuses `ProtectionQuality` since 702.11d uses the same quality concept as 702.16a.  
  `display_name`: "Hexproof from white", "Hexproof from artifacts", "Hexproof from everything", etc.

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
- `HexproofFrom(q)` check: `source_matches_quality(q, source_colors, source_card_types, source_subtypes) && obj.controller != caster` (analogous to `Hexproof` but quality-gated per CR 702.11d)

---

## Section 4 — Combat damage (`engine/combat.rs`)

### Shared helper function

Defined in `engine/mod.rs` (already hosts `continuous_pt_bonus` and similar shared helpers) so `combat.rs`, `stack.rs`, and `state_based_actions.rs` can all call it:

```rust
pub(crate) fn has_protection_from(
    target_obj: &CardObject,
    source_colors: &[ManaColor],
    source_card_types: &[CardType],
    source_subtypes: &[String],
) -> bool
```

Iterates `target_obj.definition.rules_text` looking for `Active(Static(ProtectionFrom(q)))`. Returns true if any quality matches via `source_matches_quality`.

### Integration points

1. **Attacker → player**: players have no keyword protection in this engine; no check needed.
2. **Attacker → blocker**: before accumulating `damage_to_objects[blocker_id]` (and `wither_to_objects`), call `has_protection_from` with the attacker's colors/card_types/subtypes. If true, skip.
3. **Blocker → attacker**: same pattern before `damage_to_objects[attacker_id]`.
4. **Blocking legality**: rename existing `ProtectionFromColor` match arm to use `source_matches_quality` via `ProtectionFrom(q)`.

---

## Section 5 — Enchant/Equip prevention (E in DEBT)

### Aura SBA (`engine/state_based_actions.rs` — CR 704.5m / 702.16c)

The existing `AuraToGraveyard` SBA at line 114 already delegates to `is_legal_target` with the aura's colors. After the Section 3 signature expansion, we also pass the aura's `card_types` and `subtypes`. This makes "aura with the protected quality attached to a protected permanent" automatically trigger `AuraToGraveyard` with no further logic change.

### Aura ETB attachment (`engine/stack.rs` — CR 303.4 / 702.16c)

At the attachment step (line 481–492 of stack.rs), before `perm.attached_to = Some(host_id)`, call `has_protection_from(host_obj, aura_colors, aura_card_types, aura_subtypes)`. If true, skip setting `attached_to`. The aura is now on the battlefield unattached; the `AuraToGraveyard` SBA fires at the next SBA check.

### Equipment SBA (`engine/state_based_actions.rs` — CR 704.5n / 702.16d)

Extend the `DetachEquipment` SBA: after the existing `!host_on_battlefield || !host_is_creature` guard, add a second condition — if the host creature has `has_protection_from(host_obj, equip_colors, equip_card_types, equip_subtypes)`, also push `DetachEquipment`. Per CR 702.16d, equipment stays on the battlefield (just detaches); the existing `Sba::DetachEquipment` handler already does this (`perm.attached_to = None`).

### Equipment Attach prevention (`engine/stack.rs` — CR 702.16d)

In `EffectStep::Attach` resolution (line 364–374), before setting `perm.attached_to = Some(target_id)`, look up the equipment's `colors`, `card_types`, and `subtypes` from `state.objects.get(source_id)` and call `has_protection_from` on the target. If protected, skip attachment entirely.

---

## Section 6 — Parser (`parser/oracle.rs`)

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

`hexproof from [quality]` dispatch: strip prefix `"hexproof from "`, parse the remainder using the same quality dispatch table as `protection from` (colours, "everything", card types, creature types). Produces `HexproofFrom(ProtectionQuality)`. Unrecognised qualities remain `ParsedUnimplemented`. This replaces the existing `ParsedUnimplemented` path for `s.starts_with("hexproof from ")`.

---

## Section 7 — Tests

### `engine/combat.rs`

- Protection prevents attacker from dealing damage to blocker
- Protection prevents blocker from dealing damage back to attacker
- `ProtectionFrom(Everything)` prevents all combat damage
- Blocking legality: `ProtectionFrom(CardType(Artifact))` blocks artifact blockers

### `engine/targeting.rs`

- `HexproofFrom(Color(Blue))` blocks blue spell from opponent, allows red spell, allows blue spell from controller
- `HexproofFrom(CardType(Artifact))` blocks artifact-source ability from opponent
- `ProtectionFrom(CardType(Artifact))` with `source_card_types=[Artifact]` prevents targeting

### `engine/state_based_actions.rs`

- Blue aura attached to creature with `ProtectionFrom(Color(Blue))` → `AuraToGraveyard`
- Equipment (Artifact subtype) attached to creature with `ProtectionFrom(CardType(Artifact))` → `DetachEquipment`

### `parser/oracle.rs`

- `"protection from everything"` parses to `ProtectionFrom(Everything)`
- `"protection from artifacts"` parses to `ProtectionFrom(CardType(Artifact))`
- `"protection from vampire creatures"` parses to `ProtectionFrom(CreatureType("Vampire"))`
- `"hexproof from black"` parses to `HexproofFrom(Color(Black))`
- `"hexproof from artifacts"` parses to `HexproofFrom(CardType(Artifact))`

---

## Files touched

| File | Change |
|---|---|
| `src/types/ability.rs` | Add `ProtectionQuality`, `source_matches_quality`; rename variant; add `HexproofFromColor` |
| `src/types/effect.rs` | Add 3 fields to `DamageStep` |
| `src/engine/mod.rs` | Add `has_protection_from` shared helper |
| `src/engine/stack.rs` | Expand `inject_source_flags` signature; add protection prevention to `DealDamage` resolution; add protection check to `Attach` and aura ETB attachment |
| `src/engine/targeting.rs` | Expand `is_legal_target`/`legal_targets`; add `HexproofFromColor` check |
| `src/engine/combat.rs` | Guard damage paths; update blocking check |
| `src/engine/state_based_actions.rs` | Update `is_legal_target` call site (pass aura card_types/subtypes); add equipment protection detach check |
| `src/parser/oracle.rs` | Expand protection parsing; add hexproof-from-color parsing |
| `src/serve.rs` | Update `legal_targets` call sites (2) |
| `docs/todo.md` | Remove completed bullets |
