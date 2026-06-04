# Frontend UI Design

**Date:** 2026-06-04  
**Status:** Approved

## Context

The rules engine has no way to observe its behaviour interactively. Tests cover specific scenarios but can't show the full turn-by-turn flow. A simple frontend gives a visual, interactive way to drive a game and verify that the engine enforces rules correctly — legal actions are offered, illegal ones are absent or rejected.

## Approach

An embedded web server (`axum`) serves a single-page HTML frontend. The Rust binary reads a JSON deck config, initialises a `GameState`, then handles two endpoints: `GET /state` returns a view-model JSON, and `POST /action` accepts a named action and returns the updated state or an error. The HTML file is compiled into the binary via `include_str!`.

## File Structure

```
src/
  bin/
    ui.rs        — axum server, game loop, view-model serialisation
    ui.html      — single-file frontend (included at compile time)
docs/
  test-decks/
    basic.json   — example deck config
```

## Config File

A JSON array of exactly two card-name arrays — Player 1's deck first, Player 2's second. Cards are dealt top-to-bottom in array order.

```json
[
  ["Forest", "Forest", "Forest", "Forest", "Grizzly Bears", "Grizzly Bears"],
  ["Forest", "Forest", "Llanowar Elves", "Llanowar Elves", "Giant Spider"]
]
```

**CLI:**
```
cargo run --bin ui -- basic.json          # fixed order
cargo run --bin ui -- --shuffle basic.json  # shuffle both decks before dealing
```

Unknown card names produce a startup error before the game begins. Each player draws 7 cards to start.

## Server Endpoints

| Method | Path | Body | Response |
|--------|------|------|----------|
| `GET` | `/` | — | `ui.html` (text/html) |
| `GET` | `/state` | — | `GameView` JSON |
| `POST` | `/action` | `ActionRequest` JSON | `ActionResponse` JSON |

**ActionRequest** (tagged enum):
```json
{ "type": "tap_land",          "object_id": 3 }
{ "type": "play_land",         "object_id": 3 }
{ "type": "cast_creature",     "object_id": 5 }
{ "type": "declare_attackers", "attacker_ids": [3, 7] }
{ "type": "declare_blockers",  "blocks": [[4, 3]] }
{ "type": "deal_combat_damage" }
{ "type": "advance_step" }
```

**ActionResponse:**
```json
{ "ok": true,  "state": { ...GameView } }
{ "ok": false, "error": "LandLimitReached" }
```

On `advance_step`, the server calls `engine::turn::advance_step` then immediately `engine::turn::apply_step_start` on the result, so automatic step actions (untap, draw, cleanup) are always applied before returning.

## View Model

The engine's internal types (`ObjectId`, `PlayerId`, `HashMap<ObjectId, …>`) are not serialised directly. A thin view-model in `ui.rs` maps `GameState` to a flat JSON structure the frontend can consume without knowing internal IDs:

```rust
struct GameView {
    turn: u32,
    step: String,           // e.g. "DeclareBlockers"
    active_player: u8,      // 1 or 2
    game_over: bool,
    winner: Option<u8>,
    p1: PlayerView,
    p2: PlayerView,
}

struct PlayerView {
    life: i32,
    mana_pool: ManaPoolView,   // { g: u8, w: u8, … }
    hand:        Vec<CardView>,
    lands:       Vec<CardView>,  // battlefield, lands only
    creatures:   Vec<CardView>,  // battlefield, creatures only
    library_count: usize,
    graveyard:   Vec<CardView>,
}

struct CardView {
    id: u64,             // ObjectId inner value — used for action payloads
    name: String,
    type_line: String,
    oracle_text: String,
    mana_cost: Option<String>,
    power: Option<i32>,
    toughness: Option<i32>,
    tapped: bool,
    summoning_sick: bool,
    damage_marked: u32,
    is_attacking: bool,
    is_blocking: bool,
}
```

## Frontend Layout

```
┌──────────────────────────────────────────┬──────────────┐
│  [P2 Hand — centred, cards face up]      │              │
├──────────────────────────────────────────│   Sidebar    │
│  P2 player bar  (life · mana · lib · gy) │              │
│  P2 Lands row                            │  Action list │
│  P2 Creatures row                        │  (step-      │
├──── Turn tracker ────────────────────────│   dependent) │
│  P1 Creatures row                        │              │
│  P1 Lands row                            │  Game log    │
│  P1 player bar  (life · mana · lib · gy) │              │
├──────────────────────────────────────────│              │
│  [P1 Hand — centred, cards face up]      │              │
└──────────────────────────────────────────┴──────────────┘
```

- Lands and creatures are separated into distinct rows on the battlefield
- Turn tracker shows all steps; current step highlighted in gold, past steps dimmed
- Graveyard: fanned card-stack icon in player bar; click opens a modal listing full contents
- Tapped cards rotate 90°; hovering temporarily un-rotates and shows a tooltip with oracle text + status tags (Tapped, Summoning sick, Attacking, Blocking, Damage marked)
- No summoning sickness dot on the card face

## Action Model

The sidebar lists only actions that are meaningful for the current step. The engine enforces legality; if the frontend sends an invalid action, the error string is shown in the game log.

| Step | Actions shown |
|------|---------------|
| Upkeep / End / BeginningOfCombat / EndOfCombat | Advance step |
| Draw | Advance step (draw is auto-applied by apply_step_start) |
| Main 1 / Main 2 | Tap land (each untapped land); Play land (each land in hand, if land-play available); Cast creature (each affordable creature in hand); Advance step |
| DeclareAttackers | Toggle-select each eligible creature on the battlefield; Confirm Attackers (calls `declare_attackers`); or Advance step directly to skip combat with no attackers |
| DeclareBlockers | Toggle-select blockers per attacker; Confirm Blockers (calls `declare_blockers`); or Advance step to confirm no blocks |
| CombatDamage | Resolve Damage (triggers deal_combat_damage) |
| Cleanup | Advance step (auto-handled) |

`active_player` in the view model tells the frontend which player's actions to show. Both boards are always visible.

## Dependencies Added

```toml
axum      = "0.7"
tokio     = { version = "1", features = ["full"] }
serde     = { version = "1", features = ["derive"] }
# serde_json already present
```

`axum` and `tokio` used only by the `ui` binary target, so they don't affect the library build.

## Verification

1. `cargo build --bin ui` compiles cleanly
2. `cargo run --bin ui -- --shuffle docs/test-decks/basic.json` starts server, prints URL
3. Open browser, both hands visible, life totals correct, 7 cards each
4. Untap → Draw → Main 1: "Tap Forest" and "Play Forest" actions appear
5. Play a land: it moves from hand to battlefield, land-play count exhausted, action disappears
6. Cast a creature: mana deducted, creature enters with summoning sick (visible in tooltip)
7. Advance to combat: declare attackers, confirm, declare blockers, confirm, resolve damage — life/graveyard update correctly
8. Engine errors (e.g. try casting a creature with no mana via a direct POST): error appears in log
9. Player at 0 life: game_over flag shown, winner displayed
