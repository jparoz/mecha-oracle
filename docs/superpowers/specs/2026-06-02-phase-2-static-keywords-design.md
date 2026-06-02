# Phase 2: Static Keyword Abilities — Design Spec

**Date:** 2026-06-02
**Status:** Approved

---

## Goal

Extend the Mecha-CR from vanilla-only (Phase 1) to support the eleven evergreen static keyword abilities, with all card behaviour derived from parsed Oracle text. No stack is introduced; keyword abilities operate as static modifiers to existing turn structure and combat resolution.

---

## Scope

### Included keywords (all evergreen static abilities)

| Keyword | CR rule | Engine hook |
|---|---|---|
| Flying | 702.9 | `declare_blockers` restriction |
| Reach | 702.17 | `declare_blockers` restriction |
| Trample | 702.19 | `deal_combat_damage` excess damage |
| First Strike | 702.7 | combat damage step split |
| Double Strike | 702.4 | combat damage step split |
| Vigilance | 702.20 | `declare_attackers` (no tap) |
| Haste | 702.10 | `declare_attackers` / `can_attack` (no sickness check) |
| Lifelink | 702.15 | `deal_combat_damage` life gain |
| Deathtouch | 702.2 / 704.5h | `deal_combat_damage` flag + SBA |
| Menace | 702.111 | `declare_blockers` post-declaration validation |
| Indestructible | 702.12 | SBA exemption (704.5g, 704.5h) |

### Excluded from Phase 2

- Reminder text enforcement (parenthetical oracle text is ignored at parse time — CR 305.6: basic land mana abilities are intrinsic to land types, not derived from oracle text)
- Activated abilities, triggered abilities, stack
- Keywords not in the above table

---

## Section 1: AST and Parser

### `StaticAbility` enum (`src/types/ability.rs`)

`StaticAbility` changes from an empty placeholder struct to a real enum:

```rust
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StaticAbility {
    Flying,
    Reach,
    Trample,
    FirstStrike,
    DoubleStrike,
    Vigilance,
    Haste,
    Lifelink,
    Deathtouch,
    Menace,
    Indestructible,
}
```

No `Unknown` variant — unrecognised keywords are parse errors, not silent skips.

### `ParseError` (`src/parser/mod.rs`)

New public type:

```rust
#[derive(Debug)]
pub enum ParseError {
    UnknownKeyword { keyword: String, card_text: String },
}
```

### `parse_oracle_text` (`src/parser/oracle.rs`)

Signature changes from `fn(text: &str) -> Vec<AbilityAST>` to:

```rust
pub fn parse_oracle_text(text: &str) -> Result<Vec<AbilityAST>, ParseError>
```

Algorithm:
1. **Strip reminder text** — remove all content inside parentheses (including the parens). This is correct per CR 305.6: parenthetical text is reminder text, not rules text; the engine should not use it for rules enforcement.
2. **Tokenise** — split on newlines and commas; trim whitespace; skip empty tokens.
3. **Map to variants** — case-insensitive match against keyword names. Unrecognised tokens return `Err(ParseError::UnknownKeyword { keyword: token, card_text: text.to_string() })`.
4. Wrap each recognised keyword as `AbilityAST::Static(StaticAbility::...)`.

### Propagation through CardDatabase

`scryfall::parse_card` already returns `Result<CardDefinition, String>`. It maps `ParseError` to a `String` error, propagating the unknown keyword info. `CardDatabase::from_path` already logs-and-skips erroring cards, so cards with unknown keywords are skipped with a clear warning message.

`CardDefinition.abilities` type stays `Vec<AbilityAST>` — it holds the successfully-parsed result.

---

## Section 2: `has_keyword` API

`CardObject::has_ability(&str) -> bool` is removed and replaced with:

```rust
pub fn has_keyword(&self, kw: StaticAbility) -> bool {
    self.definition.abilities.iter().any(|a| {
        matches!(a, AbilityAST::Static(k) if *k == kw)
    })
}
```

`StaticAbility` derives `PartialEq + Eq`. The engine never reads oracle text directly — all keyword queries go through `has_keyword`. All existing call sites that used `has_ability("flying")` etc. in tests are updated to use the typed variant.

---

## Section 3: Dynamic Step Insertion

### Motivation

CR 510.4 states that if any first/double strikers are present at the start of the combat damage step, a **second** combat damage step is added to the phase. The word "added" reflects insertion semantics — this is not a new named step type, it is the same `CombatDamage` step appearing twice. The same mechanism will later support extra combat phases.

### `GameState` change

```rust
pub struct GameState {
    // ... existing fields ...
    pub extra_steps: VecDeque<Step>,
}
```

Initialised as empty. Any code that inserts extra steps pushes onto this deque.

### `CombatState` change

```rust
pub struct CombatState {
    // ... existing fields ...
    pub first_strike_done: bool,
}
```

Tracks whether the first-strike combat damage round has already executed this combat. Reset in `untap_step` (alongside the rest of `CombatState`).

### `advance_step` change

Checks `extra_steps` before the static sequence:

```rust
pub fn advance_step(state: GameState) -> GameState {
    let mut state = state;
    if let Some(next) = state.extra_steps.pop_front() {
        state.step = next;
        return state;
    }
    // ... existing static match ...
}
```

### First strike / double strike round logic in `deal_combat_damage`

At the start of `deal_combat_damage`:

- **Determine if a first-strike round is needed**: any participating creature (attacker or blocker) has `FirstStrike` or `DoubleStrike`.
- **If needed and `first_strike_done == false`** (first-strike round):
  - Only creatures with `FirstStrike` or `DoubleStrike` assign damage.
  - Set `combat.first_strike_done = true`.
  - Push `Step::CombatDamage` onto `extra_steps` (the second round).
- **If `first_strike_done == true`** (regular round):
  - Only creatures **without** `FirstStrike` assign damage. This correctly includes double strikers (who have no `FirstStrike` variant) per CR 702.4b.
- **If no first/double strikers present**: all creatures assign damage as normal (existing behaviour), `first_strike_done` stays `false`.

---

## Section 4: Engine Keyword Integration

### `declare_attackers` (`src/engine/combat.rs`)

**Vigilance (CR 702.20b):** After validation, skip tapping for attackers with `Vigilance`. Currently all attackers are unconditionally tapped.

**Haste (CR 702.10b):** Skip the summoning sickness check for creatures with `Haste`. Change:
```rust
// before
if obj.summoning_sick { return Err(EngineError::SummoningSick); }
// after
if obj.summoning_sick && !obj.has_keyword(StaticAbility::Haste) {
    return Err(EngineError::SummoningSick);
}
```

### `CardObject::can_attack` (`src/types/card_object.rs`)

Same haste exception:
```rust
pub fn can_attack(&self) -> bool {
    self.is_creature()
        && self.zone == Zone::Battlefield
        && !self.tapped
        && (!self.summoning_sick || self.has_keyword(StaticAbility::Haste))
}
```

### `declare_blockers` (`src/engine/combat.rs`)

**Flying / Reach (CR 702.9b, 702.17b):** For each `(blocker_id, attacker_id)` pair, if the attacker has `Flying`, the blocker must have `Flying` or `Reach`. New error variant: `EngineError::InvalidBlocker`.

**Menace (CR 702.111b):** After all pairs are recorded into `blocking_map`, a post-declaration pass checks each attacker. If an attacker has `Menace`, it must have exactly 0 or ≥2 blockers in the map. Exactly 1 blocker is illegal. New error variant: `EngineError::MenaceRequiresTwoBlockers`.

### `deal_combat_damage` (`src/engine/combat.rs`)

**Trample (CR 702.19b):** For an attacker with `Trample` and ≥1 blocker, the damage assignment loop assigns the minimum lethal amount to each blocker; any remainder after all blockers have received lethal goes to the defending player rather than the last blocker. "Lethal" is `max(0, blocker.effective_toughness() - blocker.damage_marked)`, **except** if the attacker also has `Deathtouch`, in which case 1 damage is considered lethal per blocker (CR 702.2c).

**Lifelink (CR 702.15b):** When a lifelink creature deals damage (to a player or to a creature), its controller gains life equal to the damage dealt. Tracked per-creature during the damage calculation pass, applied alongside the damage.

**Deathtouch (CR 702.2b):** After assigning damage, for each creature that received any damage from a deathtouch attacker or blocker, set `damaged_by_deathtouch = true` on the target `CardObject`.

### New `CardObject` field (`src/types/card_object.rs`)

```rust
pub damaged_by_deathtouch: bool,
```

Initialised `false`. Cleared:
- In `move_to_graveyard` (creature leaves battlefield)
- In `cleanup_step` (alongside `damage_marked`)

### SBAs (`src/engine/state_based_actions.rs`)

**Indestructible (CR 702.12b):** The `find_sbas` function skips both the lethal damage SBA (CR 704.5g) and the deathtouch damage SBA (CR 704.5h) for creatures with `Indestructible`. The check is in `find_sbas`, not `apply_sbas` — this ensures the loop terminates cleanly even when an indestructible creature has `damaged_by_deathtouch = true` (the SBA is never generated, so no infinite loop).

**Deathtouch (CR 704.5h):** New SBA clause alongside the existing lethal damage check:
```
If a creature has toughness > 0, damaged_by_deathtouch == true, and !has_keyword(Indestructible)
→ MoveToGraveyard
```

### New `EngineError` variants

```rust
InvalidBlocker,          // blocker can't block attacker (e.g. non-flier blocking flier)
MenaceRequiresTwoBlockers, // menace attacker blocked by exactly one creature
```

---

## Test strategy

Each keyword gets at least one unit test in the relevant module, following the TDD pattern established in Phase 1:

- **Flying/Reach**: non-flier can't block flier; flier can block flier; reach can block flier
- **Menace**: blocked by 2 ok; blocked by 1 errors; unblocked ok
- **Vigilance**: attacker doesn't tap
- **Haste**: summoning-sick creature with haste can attack
- **Trample**: excess after lethal goes to player; trample + deathtouch: 1 damage lethal per blocker
- **First Strike**: first strikers deal damage in round 1 only; non-first-strikers deal in round 2 only; double strikers deal in both
- **Double Strike**: deals damage in both rounds
- **Lifelink**: controller gains life equal to damage dealt
- **Deathtouch**: target with 1 damage from deathtouch source dies; indestructible target survives deathtouch
- **Indestructible**: survives lethal damage; survives deathtouch
- Integration test in `tests/scripted_game.rs`: scenario using ≥3 keywords together to verify interactions (e.g. trample + first strike, deathtouch + indestructible blocker)

---

## Files changed

| File | Change |
|---|---|
| `src/types/ability.rs` | `StaticAbility` → real enum |
| `src/types/card_object.rs` | `has_keyword`, `can_attack` update, `damaged_by_deathtouch` field |
| `src/types/game_state.rs` | `extra_steps: VecDeque<Step>`, `CombatState::first_strike_done` |
| `src/parser/mod.rs` | `ParseError` type |
| `src/parser/oracle.rs` | Full `parse_oracle_text` implementation |
| `src/engine/combat.rs` | All declare/damage changes |
| `src/engine/state_based_actions.rs` | Indestructible + deathtouch SBAs |
| `src/engine/turn.rs` | Clear `damaged_by_deathtouch` in cleanup; `advance_step` queue check |
| `src/engine/mod.rs` | New `EngineError` variants |
| `tests/fixtures/oracle_cards_test.json` | Add keyword creatures (e.g. a flier, a trampler) |
| `tests/scripted_game.rs` | Integration scenario with keywords |
