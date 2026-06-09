# Instants & Sorceries Design

**Date:** 2026-06-09  
**Goal:** Make the `blue_abilities.json` test deck playable — instants and sorceries go on the stack, resolve with their effects, and go to the graveyard. Draw spells draw cards. Counterspells can be cast but resolve with no effect until targeting is implemented.

---

## Scope

Implement the card-type infrastructure for instants and sorceries. Targeting is explicitly deferred; cards whose effects require a target (Counterspell, Mana Leak, Negate) will be castable and will go on the stack, but will resolve with no rules effect.

Cards in `blue_abilities.json` that will work end-to-end after this change:

| Card | Type | Effect after this change |
|------|------|--------------------------|
| Brainstorm | Instant | Draws 3; "put 2 back" skipped |
| Opt | Instant | Draws 1; scry 1 skipped |
| Ponder | Sorcery | Draws 1; look-at-top-3 and shuffle skipped |
| Serum Visions | Sorcery | Draws 1; scry 2 skipped |
| Counterspell | Instant | Casts and resolves; no effect (targeting deferred) |
| Mana Leak | Instant | Same |
| Negate | Instant | Same |
| Cryptic Command | Instant | Same (modal, very complex) |
| Island | Land | Already works |
| Creature cards | Creature | Already works; `cast_spell` unifies casting |

---

## 1. Type system additions

### `effect.rs` — new `EffectStep` variant

```rust
pub enum EffectStep {
    AddMana(ManaPool),
    Mill(u32),
    DrawCard(u32),
    GainLife(u32),
    Unimplemented(String),  // parsed but not yet executable; skipped silently at resolution
}
```

`Unimplemented` steps are produced only by the lenient spell-body parser. They are never produced for activated or triggered ability effects, which remain strict (`None` if any step is unknown). All existing match arms add `EffectStep::Unimplemented(_) => {}` (skip) in the executor.

### `ability.rs` — new `Ability` variant

```rust
pub enum Ability {
    Static(StaticAbility),
    Triggered(TriggeredAbility),
    Activated(ActivatedAbility),
    SpellEffect(Effect),    // the on-resolution body of an instant or sorcery (CR 608)
}
```

`SpellEffect` is stored in `CardDefinition.abilities` exactly like other oracle spans. It is displayed in the UI as a formatted effect description.

> **Note for later:** The name `Ability` is a bit odd for `Ability::SpellEffect`. Consider renaming to `Rule` or `RulesText` in a future pass (tracked in `docs/todo.md`).

---

## 2. Oracle parser — two public functions

The current `parse_oracle_text` is split into two distinct public functions. `scryfall.rs` dispatches based on the already-parsed `TypeLine`.

### `parse_permanent(text: &str, card_name: &str) -> Vec<OracleSpan>`

Identical to the current `parse_oracle_text` behaviour:
- Em-dash → ability/flavour word split
- Colon → activated ability
- `When`/`Whenever … enters` → ETB triggered ability
- Fallback: comma-split keyword tokens

### `parse_instant_or_sorcery(text: &str) -> Vec<OracleSpan>`

Each non-empty paragraph of the oracle text is parsed as a `SpellEffect`:
1. Strip trailing `.`
2. Split on `". "` to get sentences
3. For each sentence, strip a leading `"Then "` (handles `. Then X` inter-sentence linking)
4. Split each sentence on `", then "` to get sub-steps (handles intra-sentence `X, then Y` linking)
5. Each sub-step: call `try_parse_effect_step`. If `None`, emit `EffectStep::Unimplemented(text)`
6. Collect all sub-steps into one `Effect`; emit `OracleSpan::Parsed(Ability::SpellEffect(effect))`

This correctly handles the key patterns:

| Oracle text | Steps produced |
|-------------|---------------|
| `"Draw three cards, then put two cards from your hand on top of your library in any order."` | `[DrawCard(3), Unimpl("put two cards…")]` |
| `"Scry 1. Draw a card."` | `[Unimpl("Scry 1"), DrawCard(1)]` |
| `"Look at the top three cards of your library, then put them back in any order. You may shuffle. Draw a card."` | `[Unimpl("Look at…"), Unimpl("put them back…"), Unimpl("You may shuffle"), DrawCard(1)]` |
| `"Draw a card, then scry 2."` | `[DrawCard(1), Unimpl("scry 2")]` |
| `"Counter target spell."` | `[Unimpl("Counter target spell")]` |

The function name `parse_oracle_text` in `parser/mod.rs` is removed; call sites are updated.

---

## 3. Casting — unified `cast_spell`

`casting.rs` gains `cast_spell(state, player_id, object_id) -> Result<GameState, EngineError>` which subsumes and replaces `cast_creature`.

Timing validation (CR 307.1, CR 302.1):
- Card must be in the player's hand and have a mana cost
- Player must have priority
- If the card is **instant**: no further timing restriction
- If the card is **anything else** (creature, sorcery, artifact, enchantment, planeswalker): must also be active player, in a main phase (`PreCombatMain` or `PostCombatMain`), and stack must be empty

Mana payment, zone transition (`Hand → Stack`), and stack-push logic are identical to current `cast_creature`. Caster retains priority (CR 117.3c).

`cast_creature` is removed. All call sites (engine tests, `serve.rs`) are updated to `cast_spell`.

---

## 4. Resolution — non-permanent spells go to graveyard

`resolve_top` in `stack.rs` is updated to branch on card type when resolving `StackPayload::Spell { card_id }`:

```
if card.definition.type_line.is_permanent():
    // existing path: move to battlefield, collect ETB triggers, SBAs
else:
    // new path for instants/sorceries:
    execute_effect_steps(state, controller, spell_effects_from(card))
    move card → Zone::Graveyard
    add to state.graveyards[controller]
    reset consecutive_passes, priority → active_player
    run check_and_apply_sbas
```

`execute_effect_steps` is extracted from the existing `TriggeredAbility`/`ActivatedAbility` arm — the same logic, now shared. It skips `EffectStep::Unimplemented` silently.

`spell_effects_from(card)` collects all `EffectStep`s from `OracleSpan::Parsed(Ability::SpellEffect(steps))` spans in `card.definition.abilities`, flattened in order.

---

## 5. View model and API

### `CardView` in `serve.rs`

Adds `can_cast: bool`. Computed per card in hand:
- Player has priority
- Card has a mana cost the player can afford (using `greedy_payment_plan`)
- If instant: no further restriction
- If anything else: active player, main phase, stack empty

### `ActionRequest` enum

`CastCreature { object_id }` → `CastSpell { object_id }` (same shape, dispatches to `cast_spell`).

### Oracle span display

A new `format_spell_effect(effect: &Effect) -> String` function in `serve.rs` formats the steps, producing readable text for the card's oracle text area. `Unimplemented` steps display their raw text (shown in the UI with the same `parsed_unimplemented` styling as other partially-parsed spans, so the yellow underline makes clear they aren't enforced).

The `build_player_view` oracle span match gains an arm for `Ability::SpellEffect`.

---

## Files changed

| File | Change |
|------|--------|
| `src/types/effect.rs` | Add `EffectStep::Unimplemented` |
| `src/types/ability.rs` | Add `Ability::SpellEffect` |
| `src/parser/oracle.rs` | Rename `parse_oracle_text` → `parse_permanent`; add `parse_instant_or_sorcery`; add internal `parse_spell_effect` helper |
| `src/parser/mod.rs` | Update public exports |
| `src/cards/scryfall.rs` | Dispatch to correct parser based on `TypeLine` |
| `src/engine/casting.rs` | Replace `cast_creature` with `cast_spell`; update timing check |
| `src/engine/stack.rs` | Branch on permanent vs. instant/sorcery in `resolve_top`; extract `execute_effect_steps` |
| `src/serve.rs` | `CastCreature` → `CastSpell`; add `can_cast` to `CardView`; add `SpellEffect` span formatting |
| `src/serve.html` | Update JS to send `cast_spell` action type |
| `docs/todo.md` | Note about renaming `Ability` → `Rule`/`RulesText` (already added) |
