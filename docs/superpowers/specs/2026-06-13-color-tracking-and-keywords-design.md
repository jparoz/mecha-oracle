# Color Tracking + 6 Keyword Abilities — Design Spec

**Date:** 2026-06-13  
**Scope:** Add authoritative color field; implement Ward, Landwalk, Battle Cry, Fear,
Intimidate, Protection from color.

---

## 1. Data Model Changes

### 1.1 `CardDefinition` — new `colors` field

```rust
pub struct CardDefinition {
    // ... existing fields ...
    pub colors: Vec<ManaColor>,  // W/U/B/R/G only; empty = colorless
}
```

Populated from Scryfall's `colors` JSON array (e.g. `["W","U"]`). This is
authoritative: it covers color indicators (Asmoranomardicadaistinaculdacar),
oracle-text-granted colors (Transguild Courier), and normal mana-cost colors.
`ManaColor::Colorless` is never stored here; an empty `colors` vec means colorless.

All inline test `CardDefinition` literals get `colors: vec![]`.

### 1.2 `StaticAbility` — new variants

```rust
pub enum StaticAbility {
    // ... existing ...
    WardMana(ManaCost),           // CR 702.21 — Ward {N}
    WardLife(u32),                // CR 702.21 — Ward—Pay N life
    Landwalk(LandwalkKind),       // CR 702.14
    BattleCry,                    // CR 702.91
    Fear,                         // CR 702.36
    Intimidate,                   // CR 702.13
    ProtectionFromColor(ManaColor), // CR 702.16 (partial — see §7)
}
```

### 1.3 `LandwalkKind` — new enum (in `ability.rs`)

```rust
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LandwalkKind {
    LandType(String),  // e.g. "Island", "Swamp", "Forest", "Mountain", "Plains"
    Nonbasic,
}
```

### 1.4 `WardCost` — new enum (in `ability.rs`)

```rust
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WardCost {
    Mana(ManaCost),
    Life(u32),
}
```

### 1.5 `StackPayload` — new variant

```rust
pub enum StackPayload {
    // ... existing ...
    WardTrigger {
        /// Stack ID of the spell/ability this trigger will counter if not paid.
        counters_if_unpaid: StackId,
        cost: WardCost,
        paid: bool,
    },
}
```

---

## 2. Scryfall Parser (`cards/scryfall.rs`)

In `parse_entry`, read `v["colors"]`:

```rust
let colors: Vec<ManaColor> = v["colors"]
    .as_array()
    .map(|arr| {
        arr.iter()
            .filter_map(|c| c.as_str().and_then(color_from_str_no_colorless))
            .collect()
    })
    .unwrap_or_default();
```

`color_from_str_no_colorless` maps `"W"/"U"/"B"/"R"/"G"` → `Some(ManaColor::…)`,
everything else → `None` (Scryfall never puts "C" in the `colors` array).

---

## 3. Oracle Parser (`parser/oracle.rs`)

Remove each of the following from the `ParsedUnimplemented` matchers and add
recognized cases to `match_keyword`:

| Input pattern | New result |
|---|---|
| `"fear"` | `StaticAbility::Fear` |
| `"intimidate"` | `StaticAbility::Intimidate` |
| `"battle cry"` | `StaticAbility::BattleCry` |
| `"ward {…}"` (parseable mana cost) | `StaticAbility::WardMana(cost)` |
| `"ward N"` (life cost, numeric N) | `StaticAbility::WardLife(n)` |
| `"ward—pay N life"` / `"ward—…"` | `StaticAbility::WardLife(n)` or `ParsedUnimplemented` |
| `"…walk"` suffix (landwalk) | `StaticAbility::Landwalk(LandwalkKind::LandType(t))` |
| `"nonbasic landwalk"` | `StaticAbility::Landwalk(LandwalkKind::Nonbasic)` |
| `"protection from white/blue/black/red/green"` | `StaticAbility::ProtectionFromColor(c)` |
| Other `"protection from …"` | `ParsedUnimplemented` (unchanged) |

Landwalk parsing logic: strip "walk" suffix from the keyword part, then:
- `"island"` → `LandType("Island")`, `"swamp"` → `LandType("Swamp")`,
  `"forest"` → `LandType("Forest")`, `"mountain"` → `LandType("Mountain")`,
  `"plains"` → `LandType("Plains")`
- Any other `"X"` → `LandType` with the word title-cased (for future land types)
- `"nonbasic"` prefix → `LandwalkKind::Nonbasic`

`StaticAbility::display_name` updated to handle all new variants.

---

## 4. Targeting (`engine/targeting.rs`)

Add `source_colors: &[ManaColor]` parameter to `is_legal_target` and `legal_targets`.

New Protection check in `is_legal_target` (after Shroud/Hexproof checks):

```
// CR 702.16c: protection prevents targeting by sources with the protected quality.
for ability in target's abilities:
    if ProtectionFromColor(c) && source_colors.contains(c):
        return false
```

Update all call sites in `cast_spell` (pass spell card's `colors`) and
`activate_ability` (pass the activating permanent's `colors`).

`targets_still_legal` does NOT check protection (protection applies at declaration
time, not at re-validation — CR 608.2b).

---

## 5. Combat (`engine/combat.rs`)

### 5.1 `can_block_attacker`

Add checks after existing evasion rules:

```
// CR 702.36b: Fear — can't be blocked except by artifact or black creatures
if attacker has Fear:
    if !blocker_is_artifact && !blocker_colors.contains(Black):
        return false

// CR 702.13b: Intimidate — can't be blocked except by artifact or same-color creature
if attacker has Intimidate:
    if !blocker_is_artifact && attacker_colors ∩ blocker_colors == ∅:
        return false

// CR 702.14b: Landwalk — can't be blocked if defending player controls matching land
if attacker has Landwalk(LandType(t)):
    if defending player controls any land with subtype t:
        return false
if attacker has Landwalk(Nonbasic):
    if defending player controls any nonbasic land:
        return false

// CR 702.16d: Protection — can't be blocked by creatures with protected quality
if attacker has ProtectionFromColor(c):
    if blocker_colors.contains(c):
        return false
```

`blocker_is_artifact`: check `blocker_obj.definition.type_line.card_types.contains(Artifact)`.
`blocker_colors` and `attacker_colors`: from their `CardDefinition::colors` field.

Defending player's lands: iterate `state.battlefield`, find lands under defending player's
control, check `type_line.subtypes`. A land is nonbasic if it lacks the `Basic` supertype.

### 5.2 `collect_attack_triggers` — Battle Cry

For each attacker with `BattleCry`:
- For each *other* attacker (excluding self): generate one `BoostPermanentPT({ power: 1, toughness: 0 })` triggered ability targeting that other attacker.

Pattern is the same as Exalted/Melee already in that function.

---

## 6. Ward — Stack-Based Enforcement (Option B)

### 6.1 Ward trigger generation (`engine/casting.rs`, `engine/activated.rs`)

Ward fires for both spells and abilities (CR 702.21b: "spell or ability").

In `cast_spell`, after the spell object is pushed onto the stack and before
returning, check each declared target:

```
for each declared target that is a battlefield object:
    let obj = state.objects[target_id]
    if obj.controller != caster (i.e., it's an opponent's permanent):
        for each Ward ability on obj:
            let sid = state.alloc_stack_id()
            push StackPayload::WardTrigger {
                counters_if_unpaid: spell_stack_id,
                cost: WardCost::Mana(cost) or WardCost::Life(n),
                paid: false,
            }
```

Ward triggers are pushed last (above the spell and any cast triggers).

The same logic is added in `activate_ability`, after the ability's stack object
is pushed and before returning (for non-mana abilities with targets only).

### 6.2 Paying Ward — new engine function (`engine/ward.rs`)

```rust
pub fn pay_ward(
    mut state: GameState,
    player_id: PlayerId,    // must be the spell's controller (the payer)
    trigger_id: StackId,
) -> Result<GameState, EngineError>
```

- Validate `player_id` is controller of the spell being countered
- Match on the cost:
  - `WardCost::Mana(cost)`: call `pay_mana_cost` (already exists)
  - `WardCost::Life(n)`: deduct `n` from `player.life` (must have `life > n` or `life >= n` — life can be paid to 0 but not below without dying; SBAs handle the 0-life check)
- Mark the trigger's `paid = true`

Errors: `NotYourPriority` (Ward trigger must be on top of stack), `InsufficientMana`,
new `InsufficientLife` variant.

### 6.3 Ward trigger resolution (`engine/stack.rs`, `resolve_top`)

```rust
StackPayload::WardTrigger { counters_if_unpaid, paid, .. } => {
    if !paid {
        // Counter the spell: remove it from the stack and move card to graveyard.
        counter_spell_on_stack(&mut state, counters_if_unpaid);
    }
    // Either way, pop the WardTrigger itself (already done by resolve_top).
}
```

`counter_spell_on_stack` is a small helper: removes the stack object, moves the card
back to the graveyard (or back to hand if the stack item was an ability — Ward only
counters spells; abilities are also countered per CR 702.21c but not moved to graveyard).

---

## 7. `docs/todo.md` — Protection from X Remaining Work

Add a subsection under the unblocked color-tracking block noting:

```markdown
### Protection from X — partial (only ProtectionFromColor blocking/targeting is done)
- **Damage prevention (D in DEBT)**: prevent all damage from sources with protected quality — requires a "protection check" in the combat damage path and the DealDamage effect step.
- **Enchant/Equip prevention (E in DEBT)**: can't be enchanted or equipped by things with protected quality — requires aura attachment rules (future work).
- **Protection from non-color qualities**: protection from artifacts, from instants, from a specific creature type, from a card name (e.g. "protection from Eldrazi") — each needs a richer `ProtectionQuality` enum beyond just `ManaColor`.
- **Protection from everything** (CR 702.16e): shorthand for all qualities — needs `StaticAbility::ProtectionFromAll`.
- **Hexproof from color** (CR 702.11e, e.g. "hexproof from black") — related, but a separate keyword; currently ParsedUnimplemented.
```

---

## 8. Testing Highlights

- Parser round-trips: each new keyword variant parses correctly and emits `Parsed(...)` not `ParsedUnimplemented`.
- `is_legal_target` with source_colors: blue spell can't target `ProtectionFromColor(Blue)` target.
- `can_block_attacker`: Fear/Intimidate/Landwalk/Protection gating with appropriate blocker configurations.
- Ward trigger stack placement: after cast, Ward trigger appears above the spell on the stack.
- Ward trigger resolution (unpaid): spell is countered, card moves to graveyard.
- Ward trigger resolution (paid): spell survives after `pay_ward` call.
- Battle Cry: generates one boost per other attacker, not self.

---

## 9. File Change Summary

| File | Change |
|---|---|
| `types/card.rs` | Add `colors: Vec<ManaColor>` to `CardDefinition` |
| `types/ability.rs` | New `StaticAbility` variants, `LandwalkKind`, `WardCost` |
| `types/stack.rs` | New `StackPayload::WardTrigger` variant |
| `types/mod.rs` | Re-export new types |
| `engine/mod.rs` | Add `InsufficientLife` to `EngineError`; declare `ward` module |
| `engine/activated.rs` | Generate Ward triggers after non-mana ability pushed; pass `source_colors` to targeting |
| `engine/ward.rs` | New file: `pay_ward` function |
| `engine/stack.rs` | Handle `WardTrigger` in `resolve_top`; add `counter_spell_on_stack` helper |
| `engine/casting.rs` | Generate Ward triggers after spell pushed; pass `source_colors` to targeting |
| `engine/combat.rs` | Add Fear/Intimidate/Landwalk/Protection/BattleCry in blocking and triggers |
| `engine/targeting.rs` | Add `source_colors` param, Protection check |
| `engine/activated.rs` | Update `is_legal_target` call with source colors |
| `parser/oracle.rs` | Promote keywords from unimplemented to parsed |
| `cards/scryfall.rs` | Parse `colors` field; populate `CardDefinition::colors` |
| `docs/todo.md` | Add Protection from X remaining work notes |
