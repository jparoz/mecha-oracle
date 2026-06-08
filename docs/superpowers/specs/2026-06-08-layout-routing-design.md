# Layout Routing Design

**Date:** 2026-06-08

## Problem

The Scryfall `oracle_cards` bulk data includes both normal cards and tokens. Tokens may share a name with normal cards (e.g., "Llanowar Elves"). `CardDatabase` uses a single `HashMap<String, CardDefinition>` keyed by lowercase name, so the last entry wins — the Llanowar Elves token silently overwrites the creature.

## Goal

Parse the Scryfall `layout` field to route cards and tokens into separate maps, preventing name collisions. Both remain accessible. The `CardDefinition` type is unchanged — `layout` is a routing key only, not stored.

## Design

### `scryfall` module

Introduce a public enum in `src/cards/scryfall.rs`:

```rust
pub enum ParsedEntry {
    Card(CardDefinition),
    Token(CardDefinition),
}
```

Rename `parse_card` to `parse_entry`, returning `Result<ParsedEntry, String>`. Routing logic:
- `layout == "token"` → `ParsedEntry::Token`
- anything else (including absent field) → `ParsedEntry::Card`

`CardDefinition` and all internal parsing helpers are unchanged.

### `CardDatabase`

Add a second map `tokens: HashMap<String, CardDefinition>` alongside the existing `inner`.

Loading loop in `from_path` matches on `ParsedEntry`:
- `Card(def)` → insert into `inner`
- `Token(def)` → insert into `tokens`

The tracing log gains a `tokens` count.

Add `get_token(name: &str) -> Option<&CardDefinition>` for token lookup (parallel to `get()`).

`get()` is unchanged.

### Test fixture

Add `"layout": "normal"` to existing fixture cards and add a second Llanowar Elves entry with `"layout": "token"` (no mana cost, type line `"Token Creature — Elf Druid"`). Update the existing `llanowar_elves_loads_with_activated_ability` test to assert it retrieves the creature, not the token. Add a `get_token` test.

## Out of scope

- Storing `layout` on `CardDefinition`
- Handling other layouts (split, transform, adventure, etc.) differently from normal — they go into `inner`
- Deck legality validation (future work)
