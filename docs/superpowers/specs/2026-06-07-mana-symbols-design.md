# Mana Symbols: Full CR 107.4 Support

**Date:** 2026-06-07
**Status:** Approved

## Problem

1,149 cards are skipped on load because `scryfall::parse_mana_cost` returns `Err` on any mana
symbol it doesn't recognise (e.g. `{X}`, `{B/G}`, `{W/U/P}`). The same gap exists in
`oracle::try_parse_mana_cost`, which silently returns `None` for activation costs containing
these symbols. The current `ManaCost` struct has no fields for variable, hybrid, Phyrexian, or
snow mana.

## Goals

- All cards in the Scryfall oracle dump load without being skipped.
- `ManaCost` accurately represents every symbol in CR 107.4.
- The engine correctly validates and executes payment of any cost, including choices required by
  hybrid, Phyrexian, and snow mana.
- X-cost spells and abilities accept a player-declared value for X.

## Out of scope

- {Y}, {Z} (acorn/silver-border only — CR 107.4b variable symbols, not used in black-border).
- {HW} half-mana (not present in current CR 107.4).
- Full snow mana sourcing beyond the Snow supertype check on permanents (e.g. snow instants).

---

## Section 1 — Type model

### `ManaPip`

Replace the flat `ManaCost` struct with a pip-based model. Each `{…}` symbol in oracle text
maps to one `ManaPip`:

```rust
pub enum ManaPip {
    // CR 107.4a — five colored
    White, Blue, Black, Red, Green,
    // CR 107.4c — colorless
    Colorless,
    // CR 107.4b — numerical generic
    Generic(u32),
    // CR 107.4b — variable (black-border only)
    X,
    // CR 107.4e — two-color hybrid: {W/U}, {W/B}, {U/B}, {U/R}, {B/R},
    //             {B/G}, {R/G}, {R/W}, {G/W}, {G/U}
    Hybrid(ManaColor, ManaColor),
    // CR 107.4e — generic hybrid: {2/W}…{2/G} (pay 2 generic or 1 color)
    GenericHybrid(u32, ManaColor),
    // CR 107.4e — colorless hybrid: {C/W}…{C/G} (pay 1 colorless or 1 color)
    ColorlessHybrid(ManaColor),
    // CR 107.4f — Phyrexian: {W/P}…{G/P} (pay color or 2 life)
    Phyrexian(ManaColor),
    // CR 107.4f — hybrid Phyrexian: {W/U/P}…{G/U/P} (pay either color or 2 life)
    HybridPhyrexian(ManaColor, ManaColor),
    // CR 107.4h — snow mana
    Snow,
}
```

### `ManaCost`

```rust
pub struct ManaCost {
    pub pips: Vec<ManaPip>,
}

impl ManaCost {
    /// CR 202.3: mana value. X=0, GenericHybrid uses the numeric component (largest side).
    pub fn mana_value(&self) -> u32;
    pub fn has_x(&self) -> bool;
}
```

`mana_value()` contribution per pip (CR 202.3b–d):

| Pip | Contribution |
|-----|-------------|
| `White`…`Colorless` | 1 |
| `Generic(n)` | n |
| `X` | 0 |
| `Hybrid(_, _)` | 1 |
| `GenericHybrid(n, _)` | n |
| `ColorlessHybrid(_)` | 1 |
| `Phyrexian(_)` | 1 |
| `HybridPhyrexian(_, _)` | 1 |
| `Snow` | 1 |

The old `converted_mana_cost()` and `total_colored()` methods are removed.

### `ManaPool`

Snow mana is tracked per-color as a shadow subset. Invariant: `snow_X <= X` always.

```rust
pub struct ManaPool {
    // total mana (includes snow-tagged)
    pub white: u32,
    pub blue: u32,
    pub black: u32,
    pub red: u32,
    pub green: u32,
    pub colorless: u32,
    // snow-tagged subset
    pub snow_white: u32,
    pub snow_blue: u32,
    pub snow_black: u32,
    pub snow_red: u32,
    pub snow_green: u32,
    pub snow_colorless: u32,
}

impl ManaPool {
    pub fn add(&mut self, color: ManaColor, amount: u32);
    /// Adds to both the color field and its snow_ shadow.
    pub fn add_snow(&mut self, color: ManaColor, amount: u32);
    pub fn total(&self) -> u32;
    pub fn total_snow(&self) -> u32;
}
```

### `PaymentPlan`

The player supplies a `PaymentPlan` alongside any action that involves paying a mana cost.
It specifies exactly how much of each (color, snow) to deduct from the pool, the declared value
of X, and how many "blood" units to pay for Phyrexian pips.

```rust
pub struct PaymentPlan {
    /// Declared X value. Must be Some(n) iff the cost contains {X}; None otherwise.
    pub x_value: Option<u32>,
    // mana to deduct from pool
    pub white: u32,
    pub blue: u32,
    pub black: u32,
    pub red: u32,
    pub green: u32,
    pub colorless: u32,
    // snow-tagged mana to deduct (must be <= corresponding color field above)
    pub snow_white: u32,
    pub snow_blue: u32,
    pub snow_black: u32,
    pub snow_red: u32,
    pub snow_green: u32,
    pub snow_colorless: u32,
    /// Phyrexian life payments. 1 blood = 2 life deducted.
    /// Two Phyrexian pip payments → blood: 2 → 4 life lost.
    pub blood: u32,
}
```

`PaymentPlan` is designed to be expanded in future to cover non-mana costs that can appear in
activation costs and additional costs (CR 118.8), such as sacrificing permanents or discarding
cards. For now, those costs remain in `CostComponent::Unimplemented`.

---

## Section 2 — Parser

### `src/cards/scryfall.rs` — `parse_mana_cost`

Rewritten to build `ManaCost { pips: Vec<ManaPip> }`. Every CR 107.4 symbol maps to a pip.
Unknown symbols (e.g. `{E}` energy, `{PW}` planeswalker loyalty) emit a `tracing::debug!` and
are skipped rather than returning `Err` — no card is dropped due to an unrecognised symbol.

### `src/parser/oracle.rs` — `try_parse_mana_cost`

Same extension: returns `None` only if the string isn't structured as `{…}{…}…` at all (i.e.
plain text such as `"Sacrifice a creature"`). All CR 107.4 symbols produce the corresponding
`ManaPip`.

### `src/parser/oracle.rs` — `try_parse_mana_pool`

Mana pool additions (e.g. `Add {G}{G}.`) only ever contain simple colored or colorless symbols.
Unknown symbols (hybrid, Phyrexian, etc.) in an add-mana context still return `None`, causing
the ability to fall back to `ParsedUnimplemented`. This is the existing behaviour.

### Symbol parsing table

| Oracle text | ManaPip |
|-------------|---------|
| `{W}` `{U}` `{B}` `{R}` `{G}` | `White`…`Green` |
| `{C}` | `Colorless` |
| `{0}`–`{20}` | `Generic(n)` |
| `{X}` | `X` |
| `{W/U}` `{W/B}` `{U/B}` `{U/R}` `{B/R}` `{B/G}` `{R/G}` `{R/W}` `{G/W}` `{G/U}` | `Hybrid(c1, c2)` |
| `{2/W}` `{2/U}` `{2/B}` `{2/R}` `{2/G}` | `GenericHybrid(2, c)` |
| `{C/W}` `{C/U}` `{C/B}` `{C/R}` `{C/G}` | `ColorlessHybrid(c)` |
| `{W/P}` `{U/P}` `{B/P}` `{R/P}` `{G/P}` | `Phyrexian(c)` |
| `{W/U/P}` `{W/B/P}` `{U/B/P}` `{U/R/P}` `{B/R/P}` `{B/G/P}` `{R/G/P}` `{R/W/P}` `{G/W/P}` `{G/U/P}` | `HybridPhyrexian(c1, c2)` |
| `{S}` | `Snow` |

---

## Section 3 — Engine

### `pay_mana_cost`

```rust
pub fn pay_mana_cost(
    state: GameState,
    player_id: PlayerId,
    cost: &ManaCost,
    plan: &PaymentPlan,
) -> Result<GameState, EngineError>
```

Validation runs a pip-matching pass in priority order:

1. **Simple colored pips** (`White`…`Colorless`): plan must include ≥ 1 of that color; decrement.
2. **Snow pips**: sum of plan's `snow_*` fields must be ≥ 1 per pip; decrement the chosen snow-tagged color field (and its non-snow counterpart).
3. **Phyrexian pips** (`Phyrexian(c)`): plan covers via `blood` (decrement 1 blood) or the color
   field (decrement 1 of color c). Error if neither available.
4. **Hybrid Phyrexian pips**: same as Phyrexian but accepts either component color.
5. **Two-color hybrid pips**: plan must have ≥ 1 of either component color; decrement that color.
6. **Colorless hybrid pips** (`ColorlessHybrid(c)`): plan must have ≥ 1 colorless or ≥ 1 of
   color c; decrement the chosen field.
7. **Generic hybrid pips** (`GenericHybrid(n, c)`): plan must have ≥ 1 of color c (use color
   option, contributes 1) or ≥ n unallocated mana (use generic option).
8. **Generic pips**: deduct from remaining unallocated mana in pool order (W, U, B, R, G, C).
9. **X pips**: deduct `x_value * x_count` from remaining unallocated mana.

After validation, the engine applies all deductions atomically: pool fields decremented per plan,
`plan.blood * 2` deducted from player life total.

`EngineError` gains `InvalidPaymentPlan`.

### `can_pay_cost`

Constructs a greedy candidate plan from the current pool without committing to it; returns `bool`.
Greedy strategy:

- Cover simple colored pips first.
- For Phyrexian pips: prefer blood (life payment) unless the player has fewer than 2 life; fall
  back to color in that case.
- For hybrid: pick the side with more available mana in the pool.
- For generic: use any remaining mana.
- X treated as 0 for this check.

Returns `true` if a valid plan exists under this strategy.

### `tap_land_for_mana`

Checks `obj.definition.type_line.supertypes.contains(&Supertype::Snow)`. Snow permanents call
`pool.add_snow(color, 1)` (increments both the color field and the `snow_X` shadow field);
non-snow permanents call `pool.add(color, 1)` as before.

### `EffectStep::AddMana` execution

When the engine executes an `AddMana` effect, it checks whether the source `CardObject` has the
Snow supertype. If so, it uses `add_snow` rather than `add`, so e.g. a Snow version of Llanowar
Elves correctly produces snow-tagged green mana.

### Action changes

`ActivateAbility` gains:

```rust
pub struct ActivateAbility {
    pub object_id: ObjectId,
    pub ability_index: usize,
    pub x_value: Option<u32>,          // Some(n) iff cost contains {X}
    pub payment_plan: Option<PaymentPlan>, // None iff cost is purely {T} (no mana)
}
```

`CastSpell` will gain the same fields when spell-casting is implemented.

---

## Migration notes

- All call sites of `converted_mana_cost()` → `mana_value()`.
- All call sites of `ManaCost { generic, white, … }` struct literals → `ManaCost { pips: vec![…] }`.
- `ManaPool::add` signature unchanged; `add_snow` is additive.
- Existing tests for simple `{W}`, `{G}`, `{2}{G}` costs continue to pass against the new pip
  representation; test fixtures don't need to change.
