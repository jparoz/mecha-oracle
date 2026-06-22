# Attachment System Design

**Date:** 2026-06-22  
**Status:** Approved  
**Scope:** Full playable flow — auras and equipment, including cast-time targeting, equip action, continuous-effect grants, and SBAs.

---

## Overview

This design adds an attachment system covering Auras (CR 303.4) and Equipment (CR 301.5). An attachment is a permanent that is "attached to" another permanent; it grants a continuous effect to its host while attached. The two attachment types share a core state model but have different flows for entering the attached state: auras target at cast time and enter attached, equipment enter detached and are attached/re-attached via the Equip activated ability.

---

## 1. Data Model

### `PermanentState` — new field

```rust
pub attached_to: Option<ObjectId>,  // CR 303.4b / 301.5b: what this permanent is currently attached to
```

Initialised to `None`. Set when an aura enters the battlefield or equip resolves. Cleared (set to `None`) when equipment is forcibly detached by an SBA. Auras that fail the SBA check go to the graveyard rather than being detached.

### `PermanentFilter` — new field

```rust
pub object_ids: Vec<ObjectId>,  // if non-empty, only match permanents with these IDs
```

Initialised to empty (no ID constraint). The filter-matching logic in `continuous_pt_bonus` (and anywhere else `PermanentFilter` is evaluated) checks this list first when non-empty: if the target's ID is not in the list, the filter does not match. This is a general mechanism for "match exactly these objects" — used in the attachment evaluation path but available for future effects.

Card definitions always store `object_ids: vec![]` since a card cannot know its runtime host at definition time. The engine injects the host ID dynamically at evaluation time from `PermanentState.attached_to`.

---

## 2. New Rule Variants

Two new variants are added to `Rule` in `src/types/ability.rs`:

```rust
// CR 303.4: an Aura enchants the object matching `enchant`.
// `enchant` is used as the target requirement at cast time (CR 601.2c) and for SBA legality checks.
// `grants` is the continuous effect applied to the attached permanent.
Rule::Aura {
    enchant: TargetFilter,
    grants: ContinuousEffect,
}

// CR 301.5: an Equipment permanent with an Equip activated ability.
// `cost` is the equip cost paid at sorcery speed to attach/re-attach.
// `grants` is the continuous effect applied to the equipped creature.
Rule::Equip {
    cost: Cost,
    grants: ContinuousEffect,
}
```

Both variants bundle the attachment restriction/cost with the continuous effect that applies to the host. This keeps the attachment-related data together and makes it easy for the engine to identify attachment sources when scanning the battlefield.

`KeywordAbility::display_name()` does not need changes — equip and aura are not keyword abilities; they are separate rule variants.

---

## 3. New EffectStep

```rust
// CR 301.5d: resolves the equip action — attaches the ability's source to the first target.
// `source_id` is the equipment's ObjectId, captured at activation time.
EffectStep::Attach { source_id: ObjectId },
```

When `execute_effect_steps` processes `EffectStep::Attach { source_id }`:

1. Get `target_id` from `targets[0]` (must be `EffectTarget::Object`).
2. If the source is still on the battlefield (zone check) and the target is still on the battlefield:
   - Set `state.battlefield[source_id].attached_to = Some(target_id)`.
3. If source or target has left the battlefield since the ability was activated, the step is a no-op (LKI rule — CR 608.2b).

---

## 4. Continuous Effect Evaluation

`engine/mod.rs::continuous_pt_bonus` is extended to also scan for attached sources. After the existing `Rule::Continuous` loop, add a second loop:

```
for each (src_id, src_perm) on the battlefield:
    if src_perm.attached_to == Some(target_id):
        for each Rule::Aura { grants } or Rule::Equip { grants } in src_perm.definition.rules_text:
            apply grants.pt_modification to the bonus
```

This uses `PermanentState.attached_to` as the primary check, bypassing `PermanentFilter` entirely for attachment sources. The `PermanentFilter.object_ids` field is available for other future uses (e.g., "target permanent gets +N/+N until end of turn" stored as a `Rule::Continuous` effect with a specific target ID), but attachment effects do not go through the filter at all.

---

## 5. Engine Flows

### 5a. Aura Casting (CR 601.2, CR 303.4c)

**Cast-time targeting (`engine/casting.rs` / `serve.rs`):**

1. `compute_hand_actions` checks for `Rule::Aura { enchant, .. }` in a card's rules text.
2. For each such rule, enumerate legal targets using `targeting::legal_targets(state, enchant, player, colors)`.
3. One `cast_spell` action is emitted per legal target, with the target in `targets[0]`.

**Resolution (`engine/stack.rs::resolve_top`):**

When resolving a `StackPayload::Spell { card_id }` whose card has `Rule::Aura`:

1. `resolve_top` processes the `StackPayload::Spell` as it does for any permanent spell: `execute_effect_steps` runs the card's `SpellAbility` steps, which include `MoveZone { from: Stack, to: Battlefield }`. This creates the `PermanentState` with `attached_to: None`.
2. After the zone change, if the stack object has a target (`targets[0]` = `EffectTarget::Object { id: host_id }`), and `host_id` is still on the battlefield: set `state.battlefield[card_id].attached_to = Some(host_id)`.
3. If the target is no longer legal (left the battlefield since cast), the aura enters the battlefield unattached; the next SBA check immediately moves it to the graveyard (CR 704.5m).

### 5b. Equip Action (CR 301.5d)

**New function `engine/equip.rs::activate_equip`:**

```
activate_equip(state, equipment_id, target_creature_id, player_id) -> Result<GameState, EngineError>
```

Validation:
- `player_id` has priority.
- Active player only; main phase; stack empty (equip only as a sorcery — CR 301.5d).
- `equipment_id` is on the battlefield, controlled by `player_id`, has `Rule::Equip`.
- `target_creature_id` is a creature on the battlefield controlled by `player_id` (CR 301.5d: "target creature you control").
- `can_pay_cost_components` for the equip cost.

If valid:
1. Pay the equip cost via `pay_cost_components`.
2. Create a `StackObject` with:
   - `payload: StackPayload::ActivatedAbility { source_id: equipment_id, effect: [EffectStep::Attach { source_id: equipment_id }], label: "Equip" }`
   - `targets: [EffectTarget::Object { id: target_creature_id }]`
3. Push to the stack; reset `consecutive_passes`; player retains priority.

**`ActionRequest` extension (`serve.rs`):**

```rust
ActionRequest::ActivateEquip {
    equipment_id: u64,
    target_id: u64,
}
```

`compute_battlefield_actions` adds `ActivateEquip` actions for equipment that can pay their cost, during the player's main phase with an empty stack.

### 5c. State-Based Actions (CR 704.5m, 704.5n, 704.5r)

Three new SBAs added to `engine/state_based_actions.rs::find_sbas`:

```rust
enum Sba {
    // ... existing ...
    AuraToGraveyard(ObjectId),   // CR 704.5m / 704.5n
    DetachEquipment(ObjectId),   // CR 704.5r
}
```

**CR 704.5m — Aura not attached to anything:**
For each permanent P with `Rule::Aura` where `P.attached_to.is_none()`: push `AuraToGraveyard(P_id)`.

**CR 704.5n — Aura attached to something illegal:**
For each permanent P with `Rule::Aura { enchant, .. }` where `P.attached_to == Some(host_id)`:
- If `host_id` is not on the battlefield, or does not satisfy `enchant` as a target filter (using `targeting::is_legal_target`): push `AuraToGraveyard(P_id)`.

**CR 704.5r — Equipment attached to a non-creature:**
For each permanent P with `Rule::Equip` where `P.attached_to == Some(host_id)`:
- If `host_id` is not on the battlefield or is not a creature: push `DetachEquipment(P_id)`.

**`apply_sbas` handling:**
- `AuraToGraveyard(id)`: call `move_to_graveyard(state, id)` (existing function).
- `DetachEquipment(id)`: set `state.battlefield[id].attached_to = None`. Equipment stays on the battlefield.

---

## 6. API Changes (`serve.rs`)

### `ActionRequest` — new variant:
```rust
ActivateEquip {
    equipment_id: u64,
    target_id: u64,
}
```

### `compute_hand_actions` — aura targeting:
Extends existing targeted-spell logic: after scanning `Rule::SpellAbility` for target requirements, also check for `Rule::Aura { enchant, .. }` and generate `cast_spell` actions using `enchant` as the target filter.

### `compute_battlefield_actions` — equip:
After the existing `Rule::Activated` loop, add a loop over `Rule::Equip`:
- Only during the controller's main phase with an empty stack.
- For each creature the player controls (excluding the equipment itself), emit an `ActivateEquip` action if the equip cost can be paid.

### Game view — attachment display:
The `PermanentView` struct (or equivalent in `serve.rs`) should expose `attached_to: Option<u64>` so the frontend can display which permanent an aura or equipment is attached to.

---

## 7. Test Data

Two cards added to `docs/test-decks/green_abilities.json` (player 2's G/W deck):

**"Bonesplitter"** — Artifact, Equipment  
- `Rule::Equip { cost: [CostComponent::Mana({1})], grants: ContinuousEffect { pt_modification: Some(PTDelta { power: 2, toughness: 0 }) } }`

**"Unholy Strength"** — Enchantment, Aura  
- `Rule::Aura { enchant: TargetFilter::Creature, grants: ContinuousEffect { pt_modification: Some(PTDelta { power: 2, toughness: 1 }) } }`

These are sufficient to write integration tests for both attachment flows, both SBAs, and continuous-effect application.

---

## 8. Out of Scope

- **Aura on players** ("Enchant player") — requires `EffectTarget::Player` attachment logic.
- **Parser integration** — "Equip {N}", "Enchant creature", "Equipped creature gets +N/+N" oracle text parsing. Initial implementation uses JSON test data with manually specified rules.
- **Protection E-in-DEBT** — preventing illegal enchantment/equipment of protected permanents — noted in `docs/todo.md`; depends on this feature but is a follow-up.
- **Fortifications** (CR 301.6) — like equipment but for lands.
- **Bestow** (CR 702.103) — alternative casting as aura vs. creature.
- **Reconfigure** (CR 702.174) — equipment that can become a creature.
- **Multiple attachment** — a single permanent cannot be attached to more than one thing (CR 303.4b).
- **Aura on a card in another zone** — Auras can only enchant permanents on the battlefield in standard play.
