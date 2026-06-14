# Spell Targeting Design — Counterspells

**Date:** 2026-06-15  
**Scope:** Unconditional "counter target [type] spell" effects (Counterspell, Negate, Essence Scatter, Dispel). Conditional counters (Mana Leak, Quench) are explicitly out of scope — see `docs/todo.md`.

---

## Background

Counterspells require targeting spells on the stack, which the engine currently cannot express. `EffectTarget` only covers `Object` (battlefield permanents) and `Player`. `TargetFilter` only covers `Creature`, `Player`, and `Any`. The parser marks "Counter target spell." as `Unimplemented`. The existing `counter_spell_on_stack()` function in `stack.rs` already works correctly (used by Ward) — only the targeting and effect plumbing is missing.

---

## Data Model (`types/`)

### `SpellFilter` (new, `types/ability.rs`)

```rust
pub struct SpellFilter {
    pub included_types: Vec<CardType>, // spell must have ≥1 of these; empty = no constraint
    pub excluded_types: Vec<CardType>, // spell must have none of these
}
```

`matches(&self, card_types: &[CardType]) -> bool`:
- `included` check: empty OR any type in `included_types` is present
- `excluded` check: none of the `excluded_types` are present
- Both must hold

Constructor helpers:
- `SpellFilter::any()` — both vecs empty; matches all spells ("counter target spell")
- `SpellFilter::noncreature()` — `excluded = [Creature]`
- `SpellFilter::creature()` — `included = [Creature]`
- `SpellFilter::instant_or_sorcery()` — `included = [Instant, Sorcery]`

Mirrors the existing `CastFilter` pattern but adds `included_types` for OR-type constraints.

### `TargetFilter::Spell(SpellFilter)` (new variant, `types/ability.rs`)

Added alongside `Creature`, `Player`, `Any`. Because `SpellFilter` contains `Vec`, `TargetFilter` loses `Copy` and becomes `Clone`-only. Call sites using `*filter` become `filter.clone()`.

### `EffectTarget::StackObject { id: StackId }` (new variant, `types/effect.rs`)

Added alongside `Object` and `Player`. `StackId` gains `#[derive(Serialize, Deserialize)]`. Serializes as `{"kind": "stack_object", "id": N}` via the existing `#[serde(tag = "kind", rename_all = "snake_case")]` on `EffectTarget`.

### `EffectStep::CounterSpell` (new step, `types/effect.rs`)

No parameters. The stack object to counter is read from `targets.first()` at resolution, expected to be `EffectTarget::StackObject { id }`.

---

## Engine — Targeting (`engine/targeting.rs`)

### `is_legal_target`

New `EffectTarget::StackObject { id }` arm (only reached when `filter` is `TargetFilter::Spell(_)`):
1. `state.stack_objects.get(id)` must return `Some(obj)`
2. `obj.payload` must be `StackPayload::Spell { card_id }`
3. Fetch `card_types` from `state.objects[card_id].definition.type_line.card_types`
4. `spell_filter.matches(card_types)` must be true

No shroud, hexproof, or protection checks — CR 702.11a and 702.18a scope those abilities to permanents, not spells on the stack. No controller restriction — countering your own spells is legal.

### `legal_targets`

New branch for `TargetFilter::Spell(_)`: iterate `state.stack`, build `EffectTarget::StackObject { id }` candidates, filter through `is_legal_target`. Keeps DRY with the existing function rather than duplicating payload checks.

### `targets_still_legal`

New `EffectTarget::StackObject { id }` arm:
```
state.stack_objects.get(id).map(|o| matches!(o.payload, StackPayload::Spell { .. })).unwrap_or(false)
```
A spell that was countered by something else before our spell resolves is no longer present — correct fizzle per CR 608.2b.

---

## Engine — Stack Resolution (`engine/stack.rs`)

In `execute_effect_steps`, new match arm:

```rust
EffectStep::CounterSpell => {
    if let Some(EffectTarget::StackObject { id }) = targets.first() {
        counter_spell_on_stack(&mut state, *id);
    }
}
```

`counter_spell_on_stack` already handles moving the card to the graveyard (for spells) or simply removing the entry (for abilities). No additional logic needed.

---

## Parser (`parser/oracle.rs`)

In `parse_spell_paragraph`, add new full-sentence patterns before the "no targeting found" fallback. All match on the entire lowercased paragraph:

| Oracle text | `SpellFilter` | `EffectStep` |
|---|---|---|
| `"counter target spell."` | `SpellFilter::any()` | `CounterSpell` |
| `"counter target noncreature spell."` | `SpellFilter::noncreature()` | `CounterSpell` |
| `"counter target creature spell."` | `SpellFilter::creature()` | `CounterSpell` |
| `"counter target instant or sorcery spell."` | `SpellFilter::instant_or_sorcery()` | `CounterSpell` |

Each returns `SpellAbility { target_requirements: vec![TargetFilter::Spell(filter)], steps: vec![EffectStep::CounterSpell] }`. Unlike other patterns, no suffix is stripped and `parse_spell_effect` is not called — these paragraphs have no variable content.

The existing `counterspell_fully_unimplemented` test is updated to assert the fully-parsed form.

---

## Serve Layer (`serve.rs`)

### Target name resolution

In `compute_hand_actions`, add `EffectTarget::StackObject { id }` arm to the `target_name` match:
```
stack_objects[id] → Spell { card_id } → objects[card_id].definition.name
```

### Action generation

No structural changes needed. `legal_targets` returns `EffectTarget::StackObject` values; the existing loop serializes them via `serde_json::to_value(&target)` and builds `"Cast Counterspell → [spell name]"` labels automatically.

### Ward trigger check

No change. `collect_ward_triggers` fires on permanent targets only; counterspells target stack objects, so Ward is never triggered.

### `TargetFilter::Copy` removal

Two or three sites in `serve.rs` dereference `*filter: &TargetFilter`. These become `filter.clone()` once `TargetFilter` loses `Copy`.

---

## Testing

### `engine/targeting.rs`

- `spell_on_stack_is_legal_spell_target` — `SpellFilter::any()` matches a spell in `Zone::Stack`
- `creature_spell_matches_creature_filter` — matches `SpellFilter::creature()`, rejected by `SpellFilter::noncreature()`
- `noncreature_spell_matches_noncreature_filter` — instant matches `SpellFilter::noncreature()`, rejected by `SpellFilter::creature()`
- `triggered_ability_is_not_a_legal_spell_target` — `StackPayload::TriggeredAbility` never matched even by `SpellFilter::any()`
- `countered_spell_no_longer_legal_target` — after `counter_spell_on_stack`, `targets_still_legal` returns false

### `engine/stack.rs` (integration)

- `counterspell_counters_target_spell` — creature spell on stack, Counterspell cast targeting it, both pass; targeted spell in graveyard, stack empty
- `negate_counters_noncreature_spell` — same with a sorcery as target
- `negate_cannot_target_creature_spell` — returns `Err(EngineError::IllegalTarget)`
- `counterspell_fizzles_when_target_leaves_stack` — target already countered by the time Counterspell resolves; Counterspell moves to graveyard, no further effect

### `parser/oracle.rs`

- Update `counterspell_fully_unimplemented` → assert fully-parsed `SpellEffect` with `SpellFilter::any()` + `CounterSpell`
- Add parallel tests for noncreature, creature, and instant-or-sorcery patterns
