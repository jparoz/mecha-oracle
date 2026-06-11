# Targeting System Design

**Date:** 2026-06-11  
**Test cases:** Giant Growth ("Target creature gets +3/+3 until end of turn."), Lightning Bolt ("Lightning Bolt deals 3 damage to any target.")  
**CR references:** 115 (targeting), 601.2c (targets declared when spell is cast), 608.2b (fizzle rule), 702.11 (Hexproof), 702.18 (Shroud), 702.21 (Ward)

---

## Scope

Three layers delivered together:

1. **Engine** — target declaration at cast/activation time, legality validation, CR 608.2b fizzle check at resolution.
2. **Enforcement stubs** — Shroud and Hexproof fully implemented (they were waiting on this predicate); Ward left as `ParsedUnimplemented`.
3. **UI round-trip** — game view embeds `valid_targets` per hand card; `CastSpell` and `ActivateAbility` actions carry declared targets.

Multi-target spells (`valid_targets` per slot) are out of scope; noted as a future pass.

---

## Section 1: Types and data model

### `EffectStep::BoostPermanentPT`

Changed from `BoostPermanentPT { target_id: ObjectId, delta: PTDelta }` to a tuple struct:

```rust
BoostPermanentPT(PTDelta)
```

No target embedded. The target always comes from the stack object's `targets` list at execution time. Existing call sites that hardcode a target (Prowess) move the target id into `stack_obj.targets` instead.

### `StackObject.targets`

```rust
pub struct StackObject {
    pub id: StackId,
    pub payload: StackPayload,
    pub controller: PlayerId,
    pub targets: Vec<EffectTarget>,   // new — declared targets (CR 115.1)
}
```

Empty for untargeted spells and abilities.

### `EffectTarget`

Already defined in `types/effect.rs`. Changed to struct variants for clean Serde support:

```rust
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum EffectTarget {
    Player { id: PlayerId },
    Object { id: ObjectId },
}
```

`ObjectId` and `PlayerId` gain `Deserialize` and `#[serde(transparent)]` so they round-trip as bare numbers.

### `TargetFilter`

New enum in `types/` (used by both parser and engine):

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TargetFilter {
    Creature,
    Player,
    Any,   // creature, player, planeswalker, battle — CR 115.4
}
```

### `SpellAbility`

New struct in `types/ability.rs`:

```rust
pub struct SpellAbility {
    pub target_requirements: Vec<TargetFilter>,  // empty for untargeted spells
    pub steps: Vec<EffectStep>,
}
```

`Ability::SpellEffect` changes from `SpellEffect(Effect)` to `SpellEffect(SpellAbility)`.

### `ActivatedAbility`

Gains `target_requirements`:

```rust
pub struct ActivatedAbility {
    pub cost: ActivationCost,
    pub target_requirements: Vec<TargetFilter>,  // new — empty for all current abilities
    pub effect: Effect,
}
```

### New `EffectStep` variants

```rust
EffectStep::DealDamage(u32)   // deals damage to targets[0], Object or Player
```

---

## Section 2: Targeting predicate (`engine/targeting.rs`)

New module with two public functions:

```rust
pub fn is_legal_target(
    state: &GameState,
    target: &EffectTarget,
    filter: TargetFilter,
    caster: PlayerId,
) -> bool

pub fn legal_targets(
    state: &GameState,
    filter: TargetFilter,
    caster: PlayerId,
) -> Vec<EffectTarget>

pub fn targets_still_legal(state: &GameState, targets: &[EffectTarget]) -> bool
```

### `is_legal_target` logic

For `EffectTarget::Object { id }`:
1. Object exists in `state.objects` with `zone == Battlefield`
2. Passes filter: `Creature` → `obj.is_creature()`; `Any` → `obj.is_creature()` (planeswalkers/battles future)
3. CR 702.18 Shroud: `obj.has_keyword(Shroud)` → illegal for everyone
4. CR 702.11 Hexproof: `obj.has_keyword(Hexproof)` and `obj.controller != caster` → illegal for opponents

For `EffectTarget::Player { id }`:
- Player exists in game and `!player.has_lost`
- Shroud/Hexproof do not apply to players

### `legal_targets` logic

- `Creature` → battlefield creatures passing the predicate, wrapped as `Object { id }`
- `Player` → active players, wrapped as `Player { id }`
- `Any` → both combined

### `targets_still_legal`

Used at resolution (CR 608.2b). Simpler check — just zone/existence:
- `Object { id }` → object exists with `zone == Battlefield`
- `Player { id }` → player `!has_lost`

### Shroud and Hexproof

Both are fully implemented here (they were `ParsedUnimplemented` because enforcement depended on this predicate). Ward (CR 702.21) is a triggered ability, not a static one — it remains `ParsedUnimplemented`.

---

## Section 3: Cast and activation flow

### `cast_spell` signature

```rust
pub fn cast_spell(
    state: GameState,
    player_id: PlayerId,
    object_id: ObjectId,
    declared_targets: Vec<EffectTarget>,
) -> Result<GameState, EngineError>
```

Validation order (targets before mana, matching CR 601.2c):
1. Priority check
2. Timing check
3. Target count: `spell_ability.target_requirements.len() == declared_targets.len()` → `WrongNumberOfTargets`
4. Target legality: each `(filter, target)` pair passes `is_legal_target` → `IllegalTarget`
5. Mana check and payment
6. `StackObject { ..., targets: declared_targets }`

Non-targeted spells pass `declared_targets: vec![]`; step 3 passes trivially.

### `activate_ability` signature

Gains `declared_targets: Vec<EffectTarget>` parameter (added after `ability_index`).

For non-mana abilities: same target count and legality checks (steps 3–4 above), then `StackObject { ..., targets: declared_targets }`.

Mana abilities resolve immediately and cannot be targeted — `declared_targets` is ignored for them.

### New `EngineError` variants

- `WrongNumberOfTargets`
- `IllegalTarget`

---

## Section 4: Resolution

### CR 608.2b fizzle check

Added at the top of `resolve_top` for spells and non-mana activated abilities, before executing any steps:

- All targets legal (`targets_still_legal`) → proceed
- All targets illegal → spell countered by rules; instant/sorcery still moves to graveyard, effects not applied
- Partial legality (future multi-target) → resolves, only affects legal targets

### `execute_effect_steps` signature

```rust
fn execute_effect_steps(
    state: GameState,
    controller: PlayerId,
    steps: &[EffectStep],
    targets: &[EffectTarget],
) -> GameState
```

All call sites pass `&stack_obj.targets`.

### `BoostPermanentPT(delta)` arm

Reads `targets[0]`, expects `EffectTarget::Object`:

```rust
EffectStep::BoostPermanentPT(delta) => {
    if let Some(EffectTarget::Object { id }) = targets.first() {
        if let Some(perm) = state.battlefield.get_mut(id) {
            perm.pt_boost_until_eot.power += delta.power;
            perm.pt_boost_until_eot.toughness += delta.toughness;
        }
    }
}
```

### `DealDamage(n)` arm

Reads `targets[0]`, handles both variants:

```rust
EffectStep::DealDamage(n) => {
    match targets.first() {
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
        None => {}
    }
}
```

Creature death from lethal damage is handled by the existing SBA pass after resolution.

---

## Section 5: Parser changes

### `parse_instant_or_sorcery`

Gains `card_name: &str` parameter (like `parse_permanent` already has). Call site in `scryfall.rs` threads the name through.

Return type changes: each paragraph now produces `SpellAbility` instead of `Effect`, wrapped in `Ability::SpellEffect(SpellAbility { ... })`.

### `parse_spell_paragraph(paragraph, card_name) -> SpellAbility`

New private function. Detects two target patterns before delegating to the step parser:

**Pattern A — target at front:**  
`"Target creature gets +3/+3 until end of turn"` → strip `"target creature "` → `filter = Creature`, remainder = `"gets +3/+3 until end of turn"`

Prefixes recognised:
- `"target creature "` → `TargetFilter::Creature`
- `"target player "` → `TargetFilter::Player`
- `"any target"` (front or suffix) → `TargetFilter::Any`

**Pattern B — card name + damage + target at end:**  
`"Lightning Bolt deals 3 damage to any target"` → strip card name → `"deals 3 damage to any target"` → strip `" to any target"` → `filter = Any`, remainder = `"deals 3 damage"`

If no pattern matches, `target_requirements` stays empty (untargeted).

### New `try_parse_effect_step` patterns

- `"gets +N/+M until end of turn"` → `BoostPermanentPT(PTDelta { power: N, toughness: M })`
- `"deals N damage"` → `DealDamage(N)`

### Shroud and Hexproof

Added to `StaticAbility`:

```rust
Shroud,    // CR 702.18
Hexproof,  // CR 702.11
```

Added to `match_keyword` before the `is_cr702_keyword` fallback:

```rust
"shroud" => return parsed!(Shroud),
"hexproof" => return parsed!(Hexproof),
```

The existing parser test asserting `"Hexproof"` → `ParsedUnimplemented` changes to `Parsed(Static(Hexproof))`.

### `ActivatedAbility` construction in `parse_permanent`

The colon path gains `target_requirements: vec![]` for all currently parsed activated abilities.

---

## Section 6: Game view and API

### `TargetView`

```rust
#[derive(Serialize)]
struct TargetView {
    kind: String,   // "permanent" | "player"
    id: u64,
    name: String,
}
```

### `CardView` additions

```rust
valid_targets: Vec<TargetView>,
```

Populated only when `can_cast == true` and the spell has non-empty `target_requirements`. Empty otherwise. For multi-target spells (future), this exposes the union of legal targets across all slots — a per-slot structure is deferred.

### `ActivatedAbilityView` additions

```rust
valid_targets: Vec<TargetView>,
```

Empty for all current abilities; wired up when targeted activated abilities are added.

### `ActionRequest` changes

```rust
CastSpell {
    object_id: u64,
    #[serde(default)]
    targets: Vec<EffectTarget>,
},
ActivateAbility {
    object_id: u64,
    ability_index: usize,
    #[serde(default)]
    targets: Vec<EffectTarget>,
    #[serde(default)]
    x_value: Option<u32>,
    #[serde(default)]
    payment_plan: Option<PaymentPlan>,
},
```

`#[serde(default)]` keeps existing non-targeted requests working without changes.

### `format_spell_effect` in `serve.rs`

Currently takes `&[EffectStep]`. With `SpellAbility` it receives `&spell_ability.steps` — minor update, logic unchanged.

---

## Known limitations / future work

- **Multi-target spells:** `valid_targets` exposes a flat union; a per-slot structure is needed for spells like "deal 2 damage to one target and 1 damage to another."
- **Ward (CR 702.21):** Remains `ParsedUnimplemented`; it's a triggered ability that taxes the caster, not a static targeting restriction.
- **Player targets in `TargetFilter::Any`:** Currently includes creatures and players. Planeswalkers and battles extend this when those card types are added.
- **Targeted activated abilities:** The framework is in place (`target_requirements` on `ActivatedAbility`, `targets` on `StackObject`); no current abilities exercise it.
- **"Change the target" effects (Redirect, etc.):** Targets are stored on `StackObject.targets` for this future use.
