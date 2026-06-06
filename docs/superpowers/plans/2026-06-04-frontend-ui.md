# Frontend UI Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Build a local web frontend that drives the rules engine interactively, showing both players' boards and a step-aware action panel so you can verify the engine enforces rules correctly.

**Architecture:** An `axum` HTTP server binary (`src/bin/ui.rs`) holds a `Mutex<GameState>`, exposes `GET /state` (view-model JSON) and `POST /action` (action dispatch), and serves a single `src/bin/ui.html` compiled in via `include_str!`. The frontend polls state after each action and renders cards, zones, and available actions from the current step.

**Tech Stack:** Rust, axum 0.7, tokio 1, serde (derive), serde_json (already present); vanilla JS + HTML/CSS (no build step).

---

## File Map

| File | Status | Responsibility |
|------|--------|----------------|
| `Cargo.toml` | Modify | Add axum, tokio, serde dependencies |
| `docs/test-decks/basic.json` | Create | Example deck config for testing |
| `src/bin/ui.rs` | Create | Server, view model, action dispatch, helpers |
| `src/bin/ui.html` | Create | Single-file frontend (HTML + CSS + JS) |

---

## Task 1: Add dependencies to Cargo.toml

**Files:**
- Modify: `Cargo.toml`

- [ ] **Step 1: Add dependencies**

Replace the `[dependencies]` block in `Cargo.toml` with:

```toml
[dependencies]
serde_json = "1"
ureq = { version = "3", features = ["json"] }
directories = "6"
axum = "0.7"
tokio = { version = "1", features = ["full"] }
serde = { version = "1", features = ["derive"] }
```

- [ ] **Step 2: Verify it compiles**

```bash
cargo build
```

Expected: compiles successfully (new deps downloaded and compiled).

- [ ] **Step 3: Commit**

```bash
git add Cargo.toml Cargo.lock
git commit -m "chore: add axum, tokio, serde for UI binary"
```

---

## Task 2: Create example deck file

**Files:**
- Create: `docs/test-decks/basic.json`

- [ ] **Step 1: Create the test deck**

```json
[
  ["Forest", "Forest", "Forest", "Forest", "Grizzly Bears", "Grizzly Bears", "Grizzly Bears", "Grizzly Bears", "Forest", "Forest"],
  ["Forest", "Forest", "Forest", "Forest", "Grizzly Bears", "Grizzly Bears", "Grizzly Bears", "Grizzly Bears", "Forest", "Forest"]
]
```

Save to `docs/test-decks/basic.json`.

- [ ] **Step 2: Commit**

```bash
git add docs/test-decks/basic.json
git commit -m "docs: add basic test deck config"
```

---

## Task 3: Config parsing and game initialisation

**Files:**
- Create: `src/bin/ui.rs` (scaffold with config/init logic and tests)

- [ ] **Step 1: Write the failing tests**

Create `src/bin/ui.rs` with only the test module to start:

```rust
use mecha_oracle::cards::CardDatabase;
use mecha_oracle::engine::turn::{advance_step, apply_step_start, draw_card};
use mecha_oracle::types::{CardObject, GameState, ObjectId, Player, PlayerId, Step, Zone};
use std::path::Path;

// ── Config ──────────────────────────────────────────────────────────────────

type DeckConfig = Vec<Vec<String>>;

fn load_config(path: &str) -> Result<DeckConfig, String> {
    let text = std::fs::read_to_string(path)
        .map_err(|e| format!("Cannot read {path}: {e}"))?;
    serde_json::from_str(&text)
        .map_err(|e| format!("Invalid JSON in {path}: {e}"))
}

fn build_game_state(
    config: DeckConfig,
    db: &CardDatabase,
    shuffle: bool,
) -> Result<GameState, String> {
    if config.len() != 2 {
        return Err(format!("Config must have exactly 2 decklists, got {}", config.len()));
    }

    let players = vec![
        Player::new(PlayerId(0), "Player 1"),
        Player::new(PlayerId(1), "Player 2"),
    ];
    let mut gs = GameState::new(players);

    for (player_idx, names) in config.iter().enumerate() {
        let pid = PlayerId(player_idx as u8);
        let mut deck_ids: Vec<ObjectId> = Vec::new();

        for name in names {
            let def = db
                .get(name)
                .ok_or_else(|| format!("Unknown card: {name:?}"))?
                .clone();
            let id = gs.alloc_id();
            let obj = CardObject::new(id, def, pid, Zone::Library);
            gs.add_object(obj);
            gs.libraries.get_mut(&pid).unwrap().push(id);
            deck_ids.push(id);
        }

        if shuffle {
            use std::collections::HashMap;
            // Fisher-Yates using a simple LCG seeded from the system time
            let seed = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .subsec_nanos() as u64;
            let lib = gs.libraries.get_mut(&pid).unwrap();
            let n = lib.len();
            let mut rng = seed.wrapping_add(player_idx as u64 * 6364136223846793005);
            for i in (1..n).rev() {
                rng = rng.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
                let j = (rng >> 33) as usize % (i + 1);
                lib.swap(i, j);
            }
        }
    }

    // Deal 7 cards to each player
    for _ in 0..7 {
        for pid in [PlayerId(0), PlayerId(1)] {
            if !gs.libraries[&pid].is_empty() {
                gs = draw_card(gs, pid);
            }
        }
    }

    // Apply initial step start (untap — no-op on turn 1 with empty battlefield)
    gs = apply_step_start(gs);

    Ok(gs)
}

fn init_game(path: &str, shuffle: bool) -> Result<GameState, String> {
    let db = CardDatabase::open().map_err(|e| format!("Card database error: {e}"))?;
    let config = load_config(path)?;
    build_game_state(config, &db, shuffle)
}

// ── Tests ────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    fn test_db() -> CardDatabase {
        CardDatabase::from_path(Path::new("tests/fixtures/oracle_cards_test.json")).unwrap()
    }

    #[test]
    fn build_game_state_deals_7_cards_each() {
        let config = vec![
            vec!["Forest".into(), "Forest".into(), "Forest".into(),
                 "Forest".into(), "Grizzly Bears".into(), "Grizzly Bears".into(),
                 "Grizzly Bears".into(), "Grizzly Bears".into(), "Forest".into(), "Forest".into()],
            vec!["Forest".into(), "Forest".into(), "Forest".into(),
                 "Forest".into(), "Grizzly Bears".into(), "Grizzly Bears".into(),
                 "Grizzly Bears".into(), "Grizzly Bears".into(), "Forest".into(), "Forest".into()],
        ];
        let db = test_db();
        let gs = build_game_state(config, &db, false).unwrap();
        assert_eq!(gs.hands[&PlayerId(0)].len(), 7);
        assert_eq!(gs.hands[&PlayerId(1)].len(), 7);
        assert_eq!(gs.libraries[&PlayerId(0)].len(), 3);
        assert_eq!(gs.libraries[&PlayerId(1)].len(), 3);
    }

    #[test]
    fn build_game_state_unknown_card_returns_error() {
        let config = vec![
            vec!["NoSuchCard".into()],
            vec!["Forest".into()],
        ];
        let db = test_db();
        let result = build_game_state(config, &db, false);
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("Unknown card"));
    }

    #[test]
    fn build_game_state_requires_two_decklists() {
        let config = vec![vec!["Forest".into()]];
        let db = test_db();
        let result = build_game_state(config, &db, false);
        assert!(result.is_err());
    }

    #[test]
    fn build_game_state_starts_at_untap() {
        let config = vec![
            (0..10).map(|_| "Forest".to_string()).collect(),
            (0..10).map(|_| "Forest".to_string()).collect(),
        ];
        let db = test_db();
        let gs = build_game_state(config, &db, false).unwrap();
        assert_eq!(gs.step(), Step::Untap);
    }
}
```

- [ ] **Step 2: Run tests to verify they fail**

```bash
cargo test --bin ui 2>&1 | head -30
```

Expected: compilation error (functions reference types/imports not yet needed, but struct is missing `fn main`).

- [ ] **Step 3: Add a placeholder main() so tests compile**

Append to `src/bin/ui.rs`:

```rust
fn main() {
    println!("todo");
}
```

- [ ] **Step 4: Run tests — config tests pass**

```bash
cargo test --bin ui
```

Expected: all 4 config tests pass.

- [ ] **Step 5: Commit**

```bash
git add src/bin/ui.rs
git commit -m "feat: ui binary scaffold with config parsing and game init"
```

---

## Task 4: View model types and conversion

**Files:**
- Modify: `src/bin/ui.rs`

- [ ] **Step 1: Write the failing view model tests**

Add to the `#[cfg(test)]` block in `src/bin/ui.rs`:

```rust
    #[test]
    fn build_game_view_initial_life_and_step() {
        let config = vec![
            (0..10).map(|_| "Forest".to_string()).collect(),
            (0..10).map(|_| "Forest".to_string()).collect(),
        ];
        let db = test_db();
        let gs = build_game_state(config, &db, false).unwrap();
        let view = build_game_view(&gs);
        assert_eq!(view.p1.life, 20);
        assert_eq!(view.p2.life, 20);
        assert_eq!(view.active_player, 1);
        assert_eq!(view.step, "Untap");
        assert_eq!(view.turn, 1);
    }

    #[test]
    fn build_game_view_hand_counts() {
        let config = vec![
            (0..10).map(|_| "Forest".to_string()).collect(),
            (0..10).map(|_| "Forest".to_string()).collect(),
        ];
        let db = test_db();
        let gs = build_game_state(config, &db, false).unwrap();
        let view = build_game_view(&gs);
        assert_eq!(view.p1.hand.len(), 7);
        assert_eq!(view.p2.hand.len(), 7);
        assert_eq!(view.p1.library_count, 3);
        assert_eq!(view.p2.library_count, 3);
    }

    #[test]
    fn format_mana_cost_green_green() {
        use mecha_oracle::types::mana::ManaCost;
        let cost = ManaCost { green: 2, ..Default::default() };
        assert_eq!(format_mana_cost(&cost), "GG");
    }

    #[test]
    fn format_mana_cost_generic_and_color() {
        use mecha_oracle::types::mana::ManaCost;
        let cost = ManaCost { generic: 3, green: 1, ..Default::default() };
        assert_eq!(format_mana_cost(&cost), "3G");
    }
```

- [ ] **Step 2: Run to confirm they fail**

```bash
cargo test --bin ui build_game_view 2>&1 | tail -5
cargo test --bin ui format_mana_cost 2>&1 | tail -5
```

Expected: compile errors — `build_game_view`, `format_mana_cost` not defined.

- [ ] **Step 3: Add view model types and helpers**

Insert before `fn init_game` in `src/bin/ui.rs`:

```rust
use mecha_oracle::types::card::{CardType, Supertype};
use mecha_oracle::types::mana::ManaCost;
use serde::{Deserialize, Serialize};
use std::collections::HashSet;

// ── View model ───────────────────────────────────────────────────────────────

#[derive(Serialize)]
struct ManaPoolView {
    w: u32,
    u: u32,
    b: u32,
    r: u32,
    g: u32,
    c: u32,
}

#[derive(Serialize)]
struct CardView {
    id: u64,
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
    can_attack: bool,
    can_block: bool,
}

#[derive(Serialize)]
struct PlayerView {
    life: i32,
    mana_pool: ManaPoolView,
    hand: Vec<CardView>,
    lands: Vec<CardView>,
    creatures: Vec<CardView>,
    library_count: usize,
    graveyard: Vec<CardView>,
}

#[derive(Serialize)]
struct GameView {
    turn: u32,
    step: String,
    active_player: u8,
    lands_played_this_turn: u32,
    game_over: bool,
    winner: Option<u8>,
    p1: PlayerView,
    p2: PlayerView,
}

fn format_mana_cost(cost: &ManaCost) -> String {
    let mut s = String::new();
    if cost.generic > 0 {
        s.push_str(&cost.generic.to_string());
    }
    for _ in 0..cost.white { s.push('W'); }
    for _ in 0..cost.blue  { s.push('U'); }
    for _ in 0..cost.black { s.push('B'); }
    for _ in 0..cost.red   { s.push('R'); }
    for _ in 0..cost.green { s.push('G'); }
    for _ in 0..cost.colorless { s.push('C'); }
    s
}

fn format_type_line(tl: &mecha_oracle::types::card::TypeLine) -> String {
    let mut parts: Vec<&str> = Vec::new();
    for st in &tl.supertypes {
        parts.push(match st {
            Supertype::Basic     => "Basic",
            Supertype::Legendary => "Legendary",
            Supertype::Snow      => "Snow",
            Supertype::World     => "World",
        });
    }
    for ct in &tl.card_types {
        parts.push(match ct {
            CardType::Creature     => "Creature",
            CardType::Land         => "Land",
            CardType::Instant      => "Instant",
            CardType::Sorcery      => "Sorcery",
            CardType::Artifact     => "Artifact",
            CardType::Enchantment  => "Enchantment",
            CardType::Planeswalker => "Planeswalker",
        });
    }
    let main = parts.join(" ");
    if tl.subtypes.is_empty() {
        main
    } else {
        format!("{} — {}", main, tl.subtypes.join(" "))
    }
}

fn build_player_view(state: &GameState, pid: PlayerId) -> PlayerView {
    let player = state.get_player(pid).unwrap();
    let all_blockers: HashSet<ObjectId> = state
        .combat
        .blocking_map
        .values()
        .flatten()
        .copied()
        .collect();

    let to_card_view = |obj: &mecha_oracle::types::CardObject| CardView {
        id: obj.id.0,
        name: obj.definition.name.clone(),
        type_line: format_type_line(&obj.definition.type_line),
        oracle_text: obj.definition.oracle_text.clone(),
        mana_cost: obj.definition.mana_cost.as_ref().map(format_mana_cost),
        power: obj.current_power,
        toughness: obj.current_toughness,
        tapped: obj.tapped,
        summoning_sick: obj.summoning_sick,
        damage_marked: obj.damage_marked,
        is_attacking: state.combat.attackers.contains(&obj.id),
        is_blocking: all_blockers.contains(&obj.id),
        can_attack: obj.can_attack(),
        can_block: obj.can_block(),
    };

    let bf_objects: Vec<_> = state
        .battlefield
        .iter()
        .filter_map(|id| state.objects.get(id))
        .filter(|obj| obj.controller == pid)
        .collect();

    PlayerView {
        life: player.life,
        mana_pool: ManaPoolView {
            w: player.mana_pool.white,
            u: player.mana_pool.blue,
            b: player.mana_pool.black,
            r: player.mana_pool.red,
            g: player.mana_pool.green,
            c: player.mana_pool.colorless,
        },
        hand: state.hands[&pid]
            .iter()
            .filter_map(|id| state.objects.get(id))
            .map(to_card_view)
            .collect(),
        lands: bf_objects
            .iter()
            .filter(|obj| obj.is_land())
            .map(|obj| to_card_view(obj))
            .collect(),
        creatures: bf_objects
            .iter()
            .filter(|obj| obj.is_creature())
            .map(|obj| to_card_view(obj))
            .collect(),
        library_count: state.libraries[&pid].len(),
        graveyard: state.graveyards[&pid]
            .iter()
            .filter_map(|id| state.objects.get(id))
            .map(to_card_view)
            .collect(),
    }
}

fn build_game_view(state: &GameState) -> GameView {
    GameView {
        turn: state.turn_number,
        step: format!("{:?}", state.step()),
        active_player: if state.active_player == PlayerId(0) { 1 } else { 2 },
        lands_played_this_turn: state.lands_played_this_turn,
        game_over: state.is_game_over(),
        winner: state.winner().map(|pid| if pid == PlayerId(0) { 1 } else { 2 }),
        p1: build_player_view(state, PlayerId(0)),
        p2: build_player_view(state, PlayerId(1)),
    }
}
```

- [ ] **Step 4: Run view model tests — they pass**

```bash
cargo test --bin ui
```

Expected: all tests pass (config tests + view model tests).

- [ ] **Step 5: Commit**

```bash
git add src/bin/ui.rs
git commit -m "feat: view model types and GameState → GameView conversion"
```

---

## Task 5: Action request types and dispatch

**Files:**
- Modify: `src/bin/ui.rs`

- [ ] **Step 1: Write the failing action dispatch tests**

Add to the `#[cfg(test)]` block:

```rust
    #[test]
    fn dispatch_advance_step_moves_to_upkeep() {
        let config = vec![
            (0..10).map(|_| "Forest".to_string()).collect(),
            (0..10).map(|_| "Forest".to_string()).collect(),
        ];
        let db = test_db();
        let gs = build_game_state(config, &db, false).unwrap();
        assert_eq!(gs.step(), Step::Untap);
        let gs2 = dispatch_action(gs, ActionRequest::AdvanceStep).unwrap();
        assert_eq!(gs2.step(), Step::Upkeep);
    }

    #[test]
    fn dispatch_tap_land_adds_mana_to_pool() {
        // Build a game, advance to Main 1, play a land, then tap it
        use mecha_oracle::engine::{casting::play_land, turn::advance_step as adv};
        let config = vec![
            vec!["Forest".into(), "Forest".into(), "Forest".into(),
                 "Forest".into(), "Grizzly Bears".into(), "Grizzly Bears".into(),
                 "Grizzly Bears".into(), "Grizzly Bears".into(), "Forest".into(), "Forest".into()],
            (0..10).map(|_| "Forest".to_string()).collect(),
        ];
        let db = test_db();
        let mut gs = build_game_state(config, &db, false).unwrap();

        // Advance through Untap → Upkeep → Draw → Main 1
        gs = dispatch_action(gs, ActionRequest::AdvanceStep).unwrap(); // → Upkeep
        gs = dispatch_action(gs, ActionRequest::AdvanceStep).unwrap(); // → Draw
        gs = dispatch_action(gs, ActionRequest::AdvanceStep).unwrap(); // → PreCombatMain

        // Play a land from P1's hand
        let land_id = gs.hands[&PlayerId(0)]
            .iter()
            .find(|id| gs.objects[id].is_land())
            .copied()
            .unwrap();
        gs = play_land(gs, PlayerId(0), land_id).unwrap();

        // Tap it for mana
        let tap_result = dispatch_action(gs, ActionRequest::TapLand { object_id: land_id.0 });
        assert!(tap_result.is_ok());
        let gs2 = tap_result.unwrap();
        assert_eq!(gs2.get_player(PlayerId(0)).unwrap().mana_pool.green, 1);
    }
```

- [ ] **Step 2: Run to confirm they fail**

```bash
cargo test --bin ui dispatch 2>&1 | tail -5
```

Expected: compile error — `ActionRequest`, `dispatch_action` not defined.

- [ ] **Step 3: Add action types and dispatch function**

Insert before `fn init_game` in `src/bin/ui.rs`:

```rust
use mecha_oracle::engine::{
    casting::{cast_creature, play_land},
    combat::{deal_combat_damage, declare_attackers, declare_blockers},
    mana::tap_land_for_mana,
    turn::{advance_step, apply_step_start},
};

// ── Actions ──────────────────────────────────────────────────────────────────

#[derive(Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
enum ActionRequest {
    TapLand      { object_id: u64 },
    PlayLand     { object_id: u64 },
    CastCreature { object_id: u64 },
    DeclareAttackers { attacker_ids: Vec<u64> },
    DeclareBlockers  { blocks: Vec<[u64; 2]> },
    DealCombatDamage,
    AdvanceStep,
}

#[derive(Serialize)]
struct ActionResponse {
    ok: bool,
    state: GameView,
    #[serde(skip_serializing_if = "Option::is_none")]
    error: Option<String>,
}

fn dispatch_action(state: GameState, action: ActionRequest) -> Result<GameState, String> {
    match action {
        ActionRequest::TapLand { object_id } => {
            tap_land_for_mana(state, ObjectId(object_id))
                .map_err(|e| format!("{e:?}"))
        }
        ActionRequest::PlayLand { object_id } => {
            play_land(state, state.active_player, ObjectId(object_id))
                .map_err(|e| format!("{e:?}"))
        }
        ActionRequest::CastCreature { object_id } => {
            cast_creature(state, state.active_player, ObjectId(object_id))
                .map_err(|e| format!("{e:?}"))
        }
        ActionRequest::DeclareAttackers { attacker_ids } => {
            let ids: Vec<ObjectId> = attacker_ids.iter().map(|&id| ObjectId(id)).collect();
            declare_attackers(state, state.active_player, &ids)
                .map_err(|e| format!("{e:?}"))
        }
        ActionRequest::DeclareBlockers { blocks } => {
            let pairs: Vec<(ObjectId, ObjectId)> = blocks
                .iter()
                .map(|[b, a]| (ObjectId(*b), ObjectId(*a)))
                .collect();
            let defender = state.opponent_of(state.active_player);
            declare_blockers(state, defender, &pairs)
                .map_err(|e| format!("{e:?}"))
        }
        ActionRequest::DealCombatDamage => Ok(deal_combat_damage(state)),
        ActionRequest::AdvanceStep => {
            let s = advance_step(state);
            Ok(apply_step_start(s))
        }
    }
}
```

- [ ] **Step 4: Run all tests — all pass**

```bash
cargo test --bin ui
```

Expected: all tests pass.

- [ ] **Step 5: Commit**

```bash
git add src/bin/ui.rs
git commit -m "feat: action request types and dispatch for UI binary"
```

---

## Task 6: axum server and main()

**Files:**
- Modify: `src/bin/ui.rs` — replace placeholder `main()` with real server

- [ ] **Step 1: Replace `fn main()` with the full server**

Replace the placeholder `fn main() { println!("todo"); }` with:

```rust
use axum::{
    extract::State,
    http::StatusCode,
    response::{Html, IntoResponse},
    routing::{get, post},
    Json, Router,
};
use std::sync::{Arc, Mutex};

const INDEX_HTML: &str = include_str!("ui.html");

// ── App state ────────────────────────────────────────────────────────────────

#[derive(Clone)]
struct AppState {
    game: Arc<Mutex<GameState>>,
}

// ── Handlers ─────────────────────────────────────────────────────────────────

async fn index_handler() -> impl IntoResponse {
    Html(INDEX_HTML)
}

async fn state_handler(State(app): State<AppState>) -> Json<GameView> {
    let gs = app.game.lock().unwrap();
    Json(build_game_view(&gs))
}

async fn action_handler(
    State(app): State<AppState>,
    Json(req): Json<ActionRequest>,
) -> Json<ActionResponse> {
    let mut gs = app.game.lock().unwrap();
    let current = gs.clone();
    match dispatch_action(current, req) {
        Ok(new_state) => {
            *gs = new_state;
            Json(ActionResponse {
                ok: true,
                state: build_game_view(&gs),
                error: None,
            })
        }
        Err(e) => Json(ActionResponse {
            ok: false,
            state: build_game_view(&gs),
            error: Some(e),
        }),
    }
}

// ── CLI args ─────────────────────────────────────────────────────────────────

fn parse_args() -> Result<(bool, String), String> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.is_empty() {
        return Err("Usage: ui [--shuffle] <deck.json>".into());
    }
    let shuffle = args.contains(&"--shuffle".to_string());
    let path = args
        .iter()
        .find(|a| !a.starts_with("--"))
        .cloned()
        .ok_or("Usage: ui [--shuffle] <deck.json>")?;
    Ok((shuffle, path))
}

// ── Entry point ───────────────────────────────────────────────────────────────

#[tokio::main]
async fn main() {
    let (shuffle, path) = parse_args().unwrap_or_else(|e| {
        eprintln!("{e}");
        std::process::exit(1);
    });

    let gs = init_game(&path, shuffle).unwrap_or_else(|e| {
        eprintln!("Error: {e}");
        std::process::exit(1);
    });

    let app_state = AppState {
        game: Arc::new(Mutex::new(gs)),
    };

    let router = Router::new()
        .route("/", get(index_handler))
        .route("/state", get(state_handler))
        .route("/action", post(action_handler))
        .with_state(app_state);

    let listener = tokio::net::TcpListener::bind("0.0.0.0:3000").await.unwrap();
    println!("Mecha-Oracle UI running at http://localhost:3000");
    axum::serve(listener, router).await.unwrap();
}
```

- [ ] **Step 2: Create an empty placeholder ui.html so `include_str!` compiles**

Create `src/bin/ui.html` with:

```html
<!DOCTYPE html><html><body>Loading...</body></html>
```

- [ ] **Step 3: Build the binary**

```bash
cargo build --bin ui
```

Expected: compiles successfully. (Don't run yet — ui.html is a placeholder.)

- [ ] **Step 4: Run tests to confirm nothing broke**

```bash
cargo test --bin ui
```

Expected: all tests pass.

- [ ] **Step 5: Commit**

```bash
git add src/bin/ui.rs src/bin/ui.html
git commit -m "feat: axum server, route handlers, and main() for UI binary"
```

---

## Task 7: Frontend HTML

**Files:**
- Replace: `src/bin/ui.html`

- [ ] **Step 1: Write the full frontend**

Replace `src/bin/ui.html` with the complete implementation:

```html
<!DOCTYPE html>
<html lang="en">
<head>
<meta charset="UTF-8">
<title>Mecha-Oracle</title>
<style>
* { box-sizing: border-box; margin: 0; padding: 0; }
body { background: #0d1117; color: #e0e0e0; font-family: 'Segoe UI', system-ui, sans-serif; font-size: 13px; height: 100vh; overflow: hidden; }
#root { display: flex; height: 100vh; }
#board { flex: 1; display: flex; flex-direction: column; overflow: hidden; min-width: 0; }
#sidebar { width: 240px; background: #161b22; border-left: 1px solid #30363d; display: flex; flex-direction: column; flex-shrink: 0; }

/* Hand rows */
.hand-row { display: flex; justify-content: center; align-items: center; gap: 6px; padding: 8px 12px; flex-shrink: 0; flex-wrap: wrap; }
.hand-row.p2 { background: #120909; border-bottom: 1px solid #2a1a1a; }
.hand-row.p1 { background: #091209; border-top: 1px solid #1a2a1a; }

/* Player sections */
.player-section { flex: 1; display: flex; flex-direction: column; padding: 6px 10px; gap: 5px; min-height: 0; overflow: hidden; }
.player-section.p2 { background: #160d0d; border-bottom: 2px solid #30363d; justify-content: flex-end; }
.player-section.p1 { background: #0d1610; border-top: 2px solid #30363d; }
.player-header { display: flex; align-items: center; gap: 10px; padding: 2px 0; flex-shrink: 0; }
.player-name { font-weight: bold; font-size: 13px; }
.p2 .player-name { color: #ff7b7b; }
.p1 .player-name { color: #7bff9a; }
.life { font-size: 18px; font-weight: bold; }
.p2 .life { color: #ff6b6b; }
.p1 .life { color: #51cf66; }
.mana-pool { display: flex; gap: 3px; align-items: center; }
.pip { width: 14px; height: 14px; border-radius: 50%; border: 1px solid #555; display: inline-flex; align-items: center; justify-content: center; font-size: 8px; font-weight: bold; }
.pip-W { background: #4d4a1a; border-color: #9a932d; color: #e8e06f; }
.pip-U { background: #1a2a4d; border-color: #2d4a9a; color: #6f8ee8; }
.pip-B { background: #2a1a2a; border-color: #6a3a6a; color: #c06fc0; }
.pip-R { background: #4d1a1a; border-color: #9a2d2d; color: #e86f6f; }
.pip-G { background: #1a4d1a; border-color: #2d7a2d; color: #6fd86f; }
.pip-C { background: #2a2a2a; border-color: #666; color: #ccc; }
.zone-info { margin-left: auto; display: flex; gap: 8px; align-items: center; font-size: 11px; color: #666; }

/* Zone rows */
.zone-row { display: flex; align-items: center; gap: 6px; min-height: 72px; flex-shrink: 0; }
.zone-label { font-size: 10px; color: #555; text-transform: uppercase; letter-spacing: 0.5px; width: 18px; writing-mode: vertical-rl; transform: rotate(180deg); flex-shrink: 0; align-self: center; }

/* Cards */
.card-wrap { display: inline-flex; align-items: center; justify-content: center; flex-shrink: 0; position: relative; }
.card-wrap.tapped-wrap { width: 86px; height: 62px; }
.card {
  width: 56px; height: 78px; border-radius: 4px; border: 1px solid #444;
  background: #1c2a1c; display: flex; flex-direction: column; padding: 4px;
  cursor: pointer; position: relative;
  transition: transform 0.15s ease, box-shadow 0.15s;
  transform-origin: center center;
  user-select: none;
}
.card:hover { box-shadow: 0 0 10px rgba(100,200,100,0.5); border-color: #6a9a6a; transform: none !important; }
.card.land { background: #2a1e10; border-color: #6b4c1e; }
.card.land:hover { box-shadow: 0 0 10px rgba(180,140,60,0.5) !important; border-color: #c49a3a !important; }
.card.tapped { transform: rotate(90deg); }
.card.attacking { border-color: #ff6b6b; box-shadow: 0 0 8px rgba(255,100,100,0.5); }
.card.blocking { border-color: #88bbff; box-shadow: 0 0 8px rgba(100,150,255,0.4); }
.card.selected { border-color: #ffdd57 !important; box-shadow: 0 0 10px rgba(255,220,80,0.7) !important; }
.card-name { font-size: 7.5px; font-weight: bold; color: #ddd; line-height: 1.2; flex: 1; }
.card-cost { font-size: 8px; color: #aaa; text-align: right; }
.card-type { font-size: 7px; color: #888; margin-top: 2px; }
.card-pt { font-size: 9px; font-weight: bold; color: #ccc; text-align: right; margin-top: auto; padding-top: 2px; border-top: 1px solid #333; }
.card-pt.damaged { color: #ff9966; }

/* Tooltip */
.tooltip {
  display: none; position: absolute; left: 64px; top: 0;
  width: 200px; background: #1e2430; border: 1px solid #4a6a9a;
  border-radius: 6px; padding: 10px; z-index: 100;
  font-size: 11px; color: #ccc; pointer-events: none;
  box-shadow: 0 4px 16px rgba(0,0,0,0.6); line-height: 1.5;
  transform: none !important;
}
.card:hover .tooltip { display: block; }
.tooltip-name { font-size: 13px; font-weight: bold; color: #fff; margin-bottom: 2px; }
.tooltip-cost { font-size: 11px; color: #aaa; margin-bottom: 4px; }
.tooltip-type { font-size: 10px; color: #888; border-bottom: 1px solid #2a3a4a; padding-bottom: 4px; margin-bottom: 6px; }
.tooltip-text { color: #bbb; font-style: italic; margin-bottom: 6px; }
.tooltip-pt { font-weight: bold; color: #ddd; margin-bottom: 6px; }
.tooltip-tags { display: flex; flex-wrap: wrap; gap: 3px; }
.tag { display: inline-block; border-radius: 3px; padding: 1px 6px; font-size: 10px; font-style: normal; }
.tag-sick     { background: #3a2010; border: 1px solid #7a4010; color: #e08040; }
.tag-tapped   { background: #1a2a3a; border: 1px solid #3a5a7a; color: #6aaada; }
.tag-damage   { background: #3a1a1a; border: 1px solid #7a3030; color: #ee7070; }
.tag-attack   { background: #3a1010; border: 1px solid #8a2020; color: #ee6060; }
.tag-block    { background: #1a1a3a; border: 1px solid #3a3a8a; color: #8080ee; }

/* Library / Graveyard */
.library-pile { width: 52px; height: 72px; background: #1a1a2e; border: 1px dashed #3a3a5a; border-radius: 4px; display: flex; align-items: center; justify-content: center; flex-direction: column; gap: 2px; flex-shrink: 0; }
.library-pile .pile-count { font-size: 16px; color: #444; font-weight: bold; }
.library-pile .pile-label { font-size: 9px; color: #555; }
.gy-wrap { position: relative; width: 56px; height: 72px; flex-shrink: 0; cursor: pointer; }
.gy-card { position: absolute; width: 50px; height: 68px; border-radius: 4px; border: 1px solid #5a2a2a; background: #1a1010; }
.gy-card:nth-child(1) { left: 6px; top: 4px; }
.gy-card:nth-child(2) { left: 3px; top: 2px; background: #1e1212; border-color: #6a3030; }
.gy-card:nth-child(3) { left: 0; top: 0; background: #221414; border-color: #7a3535; display: flex; flex-direction: column; padding: 4px; transition: transform 0.15s; }
.gy-wrap:hover .gy-card:nth-child(3) { transform: translateY(-4px); }
.gy-card-name { font-size: 7.5px; font-weight: bold; color: #aa6666; line-height: 1.2; flex: 1; }
.gy-card-type { font-size: 7px; color: #664444; }
.gy-card-pt   { font-size: 9px; color: #885555; text-align: right; margin-top: auto; padding-top: 2px; border-top: 1px solid #442222; }
.gy-label { position: absolute; bottom: -14px; left: 0; right: 0; text-align: center; font-size: 9px; color: #555; text-transform: uppercase; }

/* Turn tracker */
#turn-tracker { background: #161b22; border-top: 1px solid #30363d; border-bottom: 1px solid #30363d; padding: 5px 12px; display: flex; align-items: center; gap: 4px; font-size: 11px; flex-shrink: 0; flex-wrap: wrap; }
.step-chip { padding: 2px 6px; border-radius: 3px; color: #555; }
.step-chip.done { color: #555; }
.step-chip.active { color: #ffd700; background: #2a2500; border: 1px solid #5a4a00; font-weight: bold; }
.step-chip.upcoming { color: #444; }
.step-sep { color: #333; }
.active-label { margin-left: auto; font-size: 11px; color: #6a9a6a; }

/* Sidebar */
#sidebar-header { padding: 10px 12px; border-bottom: 1px solid #30363d; }
#sidebar-header h3 { font-size: 12px; text-transform: uppercase; letter-spacing: 1px; color: #888; }
#actions { padding: 8px; flex: 1; overflow-y: auto; }
.action-group { margin-bottom: 10px; }
.action-group-label { font-size: 10px; color: #555; text-transform: uppercase; letter-spacing: 0.5px; margin-bottom: 4px; padding-left: 2px; }
.action-btn { display: block; width: 100%; background: #1c2a3a; border: 1px solid #2a4a6a; border-radius: 4px; padding: 6px 10px; color: #7ab8e8; font-size: 12px; cursor: pointer; text-align: left; margin-bottom: 3px; transition: background 0.1s; }
.action-btn:hover { background: #243650; border-color: #4a7aaa; }
.action-btn.pass { background: #1a2a1a; border-color: #2a5a2a; color: #7aaa7a; }
.action-btn.pass:hover { background: #243524; }
.action-btn.selected { border-color: #ffdd57; color: #ffdd57; background: #2a2500; }
.action-btn .cost { float: right; font-size: 10px; color: #888; }
#game-over-banner { display: none; background: #2a1a00; border: 1px solid #8a5a00; border-radius: 4px; padding: 8px 10px; margin-bottom: 8px; font-size: 12px; color: #ffcc44; text-align: center; }

/* Game log */
#log { border-top: 1px solid #30363d; padding: 8px; max-height: 200px; overflow-y: auto; }
#log h4 { font-size: 10px; text-transform: uppercase; letter-spacing: 1px; color: #555; margin-bottom: 6px; }
.log-entry { font-size: 11px; color: #666; padding: 2px 0; border-bottom: 1px solid #1e2530; line-height: 1.4; }
.log-p1 .who { color: #51cf66; font-weight: bold; }
.log-p2 .who { color: #ff7b7b; font-weight: bold; }
.log-engine { color: #8888aa; font-style: italic; }
.log-error  { color: #ff6b6b; }

/* GY modal */
#gy-modal { display: none; position: fixed; inset: 0; background: rgba(0,0,0,0.7); z-index: 200; align-items: center; justify-content: center; }
#gy-modal.open { display: flex; }
.gy-modal-box { background: #1e2430; border: 1px solid #4a6a9a; border-radius: 8px; padding: 16px; min-width: 300px; max-width: 480px; }
.gy-modal-box h3 { font-size: 13px; color: #aaa; margin-bottom: 12px; text-transform: uppercase; letter-spacing: 1px; }
.gy-modal-card { display: flex; align-items: center; gap: 8px; padding: 5px 0; border-bottom: 1px solid #2a3a4a; font-size: 12px; color: #ccc; }
.gy-modal-card:last-child { border-bottom: none; }
.gy-modal-card .gy-name { flex: 1; }
.gy-modal-card .gy-type { font-size: 10px; color: #666; }
.gy-modal-card .gy-pt { font-size: 11px; color: #888; }
.gy-modal-close { margin-top: 12px; display: block; width: 100%; background: #2a3a4a; border: 1px solid #4a6a8a; border-radius: 4px; padding: 6px; color: #aac; font-size: 12px; cursor: pointer; text-align: center; }
.gy-empty { font-size: 12px; color: #555; text-align: center; padding: 12px 0; }
</style>
</head>
<body>
<div id="root">
  <div id="board">
    <div class="hand-row p2" id="p2-hand"></div>
    <div class="player-section p2" id="p2-section">
      <div class="zone-row"><span class="zone-label">Creatures</span><div id="p2-creatures" style="display:flex;gap:6px;flex-wrap:wrap"></div></div>
      <div class="zone-row"><span class="zone-label">Lands</span><div id="p2-lands" style="display:flex;gap:6px;flex-wrap:wrap"></div></div>
      <div class="player-header">
        <span class="player-name p2">Player 2</span>
        <span class="life p2" id="p2-life">♥ 20</span>
        <div class="mana-pool" id="p2-mana"></div>
        <div class="zone-info">
          <div class="library-pile" id="p2-lib"><span class="pile-count">0</span><span class="pile-label">Library</span></div>
          <div class="gy-wrap" id="p2-gy-wrap" onclick="openGY(2)">
            <div class="gy-card"></div><div class="gy-card"></div>
            <div class="gy-card" id="p2-gy-top"></div>
            <span class="gy-label" id="p2-gy-label">GY (0)</span>
          </div>
        </div>
      </div>
    </div>
    <div id="turn-tracker"></div>
    <div class="player-section p1" id="p1-section">
      <div class="player-header">
        <span class="player-name p1">Player 1</span>
        <span class="life p1" id="p1-life">♥ 20</span>
        <div class="mana-pool" id="p1-mana"></div>
        <div class="zone-info">
          <div class="library-pile" id="p1-lib"><span class="pile-count">0</span><span class="pile-label">Library</span></div>
          <div class="gy-wrap" id="p1-gy-wrap" onclick="openGY(1)">
            <div class="gy-card"></div><div class="gy-card"></div>
            <div class="gy-card" id="p1-gy-top"></div>
            <span class="gy-label" id="p1-gy-label">GY (0)</span>
          </div>
        </div>
      </div>
      <div class="zone-row"><span class="zone-label">Lands</span><div id="p1-lands" style="display:flex;gap:6px;flex-wrap:wrap"></div></div>
      <div class="zone-row"><span class="zone-label">Creatures</span><div id="p1-creatures" style="display:flex;gap:6px;flex-wrap:wrap"></div></div>
    </div>
    <div class="hand-row p1" id="p1-hand"></div>
  </div>
  <div id="sidebar">
    <div id="sidebar-header"><h3>Actions</h3></div>
    <div id="actions">
      <div id="game-over-banner"></div>
    </div>
    <div id="log"><h4>Game Log</h4></div>
  </div>
</div>

<!-- GY Modal -->
<div id="gy-modal">
  <div class="gy-modal-box">
    <h3 id="gy-modal-title">Graveyard</h3>
    <div id="gy-modal-cards"></div>
    <button class="gy-modal-close" onclick="closeGY()">Close</button>
  </div>
</div>

<script>
// ── State ────────────────────────────────────────────────────────────────────

let currentState = null;
let attackersSelected = [];
let blockersAssignment = {}; // blocker_id (number) -> attacker_id (number)
let gyData = { 1: [], 2: [] };

// ── Fetch / send ─────────────────────────────────────────────────────────────

async function fetchState() {
  const res = await fetch('/state');
  currentState = await res.json();
  render(currentState);
}

async function sendAction(action) {
  const res = await fetch('/action', {
    method: 'POST',
    headers: { 'Content-Type': 'application/json' },
    body: JSON.stringify(action),
  });
  const data = await res.json();
  currentState = data.state;
  if (data.ok) {
    appendLog(describeAction(action), actionLogClass(action, currentState));
    // Reset selection state on step change
    attackersSelected = [];
    blockersAssignment = {};
  } else {
    appendLog('Engine: ' + data.error, 'log-error');
  }
  render(currentState);
}

function describeAction(action) {
  switch (action.type) {
    case 'tap_land':       return `<span class="who">P${currentState.active_player}</span> tapped a land for mana`;
    case 'play_land':      return `<span class="who">P${currentState.active_player}</span> played a land`;
    case 'cast_creature':  return `<span class="who">P${currentState.active_player}</span> cast a creature`;
    case 'declare_attackers': return `<span class="who">P${currentState.active_player}</span> declared attackers`;
    case 'declare_blockers':  return `<span class="who">P${currentState.active_player}</span> declared blockers`;
    case 'deal_combat_damage': return '<span class="log-engine">— Combat damage resolved —</span>';
    case 'advance_step':   return `<span class="log-engine">— Advanced to ${currentState.step} —</span>`;
    default: return JSON.stringify(action);
  }
}

function actionLogClass(action, state) {
  if (action.type === 'advance_step' || action.type === 'deal_combat_damage') return 'log-engine';
  return state.active_player === 1 ? 'log-p1' : 'log-p2';
}

// ── Rendering ─────────────────────────────────────────────────────────────────

const STEP_ORDER = [
  'Untap','Upkeep','Draw','PreCombatMain',
  'BeginningOfCombat','DeclareAttackers','DeclareBlockers','CombatDamage','EndOfCombat',
  'PostCombatMain','End','Cleanup'
];
const STEP_LABELS = {
  Untap:'Untap', Upkeep:'Upkeep', Draw:'Draw', PreCombatMain:'Main 1',
  BeginningOfCombat:'Begin Combat', DeclareAttackers:'Attackers',
  DeclareBlockers:'Blockers', CombatDamage:'Damage', EndOfCombat:'End Combat',
  PostCombatMain:'Main 2', End:'End', Cleanup:'Cleanup'
};

function render(s) {
  gyData = { 1: s.p1.graveyard, 2: s.p2.graveyard };

  // Life
  document.getElementById('p1-life').textContent = '♥ ' + s.p1.life;
  document.getElementById('p2-life').textContent = '♥ ' + s.p2.life;

  // Mana pools
  renderMana('p1-mana', s.p1.mana_pool);
  renderMana('p2-mana', s.p2.mana_pool);

  // Libraries
  document.querySelector('#p1-lib .pile-count').textContent = s.p1.library_count;
  document.querySelector('#p2-lib .pile-count').textContent = s.p2.library_count;

  // Graveyards
  renderGYPile('p1', s.p1.graveyard);
  renderGYPile('p2', s.p2.graveyard);

  // Zones
  document.getElementById('p1-hand').innerHTML     = s.p1.hand.map(c => cardHTML(c, 'hand')).join('');
  document.getElementById('p2-hand').innerHTML     = s.p2.hand.map(c => cardHTML(c, 'hand')).join('');
  document.getElementById('p1-lands').innerHTML    = s.p1.lands.map(c => cardHTML(c, 'bf')).join('');
  document.getElementById('p2-lands').innerHTML    = s.p2.lands.map(c => cardHTML(c, 'bf')).join('');
  document.getElementById('p1-creatures').innerHTML = s.p1.creatures.map(c => cardHTML(c, 'bf')).join('');
  document.getElementById('p2-creatures').innerHTML = s.p2.creatures.map(c => cardHTML(c, 'bf')).join('');

  // Turn tracker
  renderTurnTracker(s);

  // Actions
  renderActions(s);
}

function renderMana(elId, pool) {
  const colors = [
    ['W', pool.w], ['U', pool.u], ['B', pool.b],
    ['R', pool.r], ['G', pool.g], ['C', pool.c],
  ];
  document.getElementById(elId).innerHTML = colors
    .filter(([, n]) => n > 0)
    .flatMap(([c, n]) => Array(n).fill(`<span class="pip pip-${c}">${c}</span>`))
    .join('');
}

function renderGYPile(prefix, graveyard) {
  const top = graveyard[graveyard.length - 1];
  const label = document.getElementById(prefix + '-gy-label');
  const topEl = document.getElementById(prefix + '-gy-top');
  label.textContent = `GY (${graveyard.length})`;
  if (top) {
    topEl.innerHTML = `<span class="gy-card-name">${top.name}</span><span class="gy-card-type">${top.type_line}</span>` +
      (top.power != null ? `<span class="gy-card-pt">${top.power}/${top.toughness}</span>` : '');
  } else {
    topEl.innerHTML = '<span style="font-size:10px;color:#442222;text-align:center;width:100%">empty</span>';
  }
  document.getElementById(prefix + '-gy-wrap').style.cursor = graveyard.length > 0 ? 'pointer' : 'default';
}

function cardHTML(card, zone) {
  const isLand = card.type_line.includes('Land');
  const isSelected = attackersSelected.includes(card.id) ||
    Object.keys(blockersAssignment).map(Number).includes(card.id);

  let classes = 'card';
  if (isLand) classes += ' land';
  if (card.tapped) classes += ' tapped';
  if (card.is_attacking) classes += ' attacking';
  if (card.is_blocking) classes += ' blocking';
  if (isSelected) classes += ' selected';

  const wrap = card.tapped ? 'card-wrap tapped-wrap' : 'card-wrap';

  const tags = [];
  if (card.tapped)        tags.push('<span class="tag tag-tapped">Tapped</span>');
  if (card.summoning_sick) tags.push('<span class="tag tag-sick">Summoning sickness</span>');
  if (card.damage_marked > 0) tags.push(`<span class="tag tag-damage">${card.damage_marked} damage</span>`);
  if (card.is_attacking)  tags.push('<span class="tag tag-attack">Attacking</span>');
  if (card.is_blocking)   tags.push('<span class="tag tag-block">Blocking</span>');

  const tooltip = `
    <div class="tooltip">
      <div class="tooltip-name">${card.name}</div>
      ${card.mana_cost ? `<div class="tooltip-cost">${card.mana_cost}</div>` : ''}
      <div class="tooltip-type">${card.type_line}</div>
      ${card.oracle_text ? `<div class="tooltip-text">${card.oracle_text}</div>` : ''}
      ${card.power != null ? `<div class="tooltip-pt">${card.power} / ${card.toughness}</div>` : ''}
      ${tags.length ? `<div class="tooltip-tags">${tags.join('')}</div>` : ''}
    </div>`;

  const pt = card.power != null
    ? `<span class="card-pt${card.damage_marked > 0 ? ' damaged' : ''}">${card.power}/${card.toughness}</span>`
    : '';

  return `<div class="${wrap}"><div class="${classes}" data-id="${card.id}">
    <span class="card-name">${card.name}</span>
    ${card.mana_cost ? `<span class="card-cost">${card.mana_cost}</span>` : ''}
    <span class="card-type">${card.type_line}</span>
    ${pt}
    ${tooltip}
  </div></div>`;
}

function renderTurnTracker(s) {
  const cur = STEP_ORDER.indexOf(s.step);
  const chips = STEP_ORDER.map((step, i) => {
    const cls = i < cur ? 'done' : i === cur ? 'active' : 'upcoming';
    return `<span class="step-chip ${cls}">${STEP_LABELS[step]}</span>`;
  }).join('<span class="step-sep">·</span>');
  const ap = s.active_player === 1 ? 'Player 1' : 'Player 2';
  document.getElementById('turn-tracker').innerHTML =
    `<span style="color:#888;margin-right:4px">Turn ${s.turn}</span>${chips}<span class="active-label">Active: ${ap}</span>`;
}

// ── Actions panel ─────────────────────────────────────────────────────────────

function renderActions(s) {
  const el = document.getElementById('actions');
  const banner = document.getElementById('game-over-banner');

  if (s.game_over) {
    const winner = s.winner ? `Player ${s.winner} wins!` : 'Draw!';
    banner.textContent = `Game Over — ${winner}`;
    banner.style.display = 'block';
    el.innerHTML = '<div id="game-over-banner" style="display:block">' + banner.textContent + '</div>';
    return;
  }

  const ap   = s.active_player === 1 ? s.p1 : s.p2;
  const def  = s.active_player === 1 ? s.p2 : s.p1;
  let html = '';

  if (s.step === 'PreCombatMain' || s.step === 'PostCombatMain') {
    // Tap lands
    const untapped = ap.lands.filter(c => !c.tapped);
    if (untapped.length > 0) {
      html += group('Tap for mana', untapped.map(c =>
        btn(c.name, `sendAction({type:'tap_land',object_id:${c.id}})`)));
    }
    // Play land
    if (s.lands_played_this_turn === 0) {
      const lands = ap.hand.filter(c => c.type_line.includes('Land'));
      if (lands.length > 0) {
        html += group('Play land', lands.map(c =>
          btn('Play ' + c.name, `sendAction({type:'play_land',object_id:${c.id}})`)));
      }
    }
    // Cast creatures
    const castable = ap.hand.filter(c => c.type_line.includes('Creature'));
    if (castable.length > 0) {
      html += group('Cast creature', castable.map(c =>
        btn(c.name, `sendAction({type:'cast_creature',object_id:${c.id}})`,
            c.mana_cost || '')));
    }

  } else if (s.step === 'DeclareAttackers') {
    const eligible = ap.creatures.filter(c => c.can_attack);
    if (eligible.length > 0) {
      const btns = eligible.map(c => {
        const sel = attackersSelected.includes(c.id);
        return `<button class="action-btn${sel ? ' selected' : ''}" onclick="toggleAttacker(${c.id})">${c.name}${sel ? ' ✓' : ''}</button>`;
      });
      btns.push(btn('Confirm Attackers', 'confirmAttackers()'));
      html += group('Select attackers', btns);
    } else {
      html += '<p style="font-size:11px;color:#666;padding:4px">No eligible attackers.</p>';
    }

  } else if (s.step === 'DeclareBlockers') {
    const attackers = ap.creatures.filter(c => c.is_attacking);
    const blockers  = def.creatures.filter(c => c.can_block);
    if (attackers.length > 0 && blockers.length > 0) {
      let inner = '';
      for (const attacker of attackers) {
        inner += `<div style="margin-bottom:6px"><div class="action-group-label">Block ${attacker.name}</div>`;
        for (const blocker of blockers) {
          const assigned = blockersAssignment[blocker.id] === attacker.id;
          inner += `<button class="action-btn${assigned ? ' selected' : ''}" onclick="toggleBlocker(${blocker.id},${attacker.id})">${blocker.name}${assigned ? ' ✓' : ''}</button>`;
        }
        inner += '</div>';
      }
      inner += btn('Confirm Blockers', 'confirmBlockers()');
      html += group('Assign blockers', [inner]);
    } else if (attackers.length === 0) {
      html += '<p style="font-size:11px;color:#666;padding:4px">No attackers.</p>';
    }

  } else if (s.step === 'CombatDamage') {
    html += group('Combat', [btn('Resolve Damage', "sendAction({type:'deal_combat_damage'})")]);
  }

  // Always: advance step
  html += group('Priority', [`<button class="action-btn pass" onclick="sendAction({type:'advance_step'})">Pass priority →</button>`]);

  el.innerHTML = html;
}

function group(label, btns) {
  return `<div class="action-group"><div class="action-group-label">${label}</div>${btns.join('')}</div>`;
}
function btn(label, onclick, cost) {
  const costHtml = cost ? `<span class="cost">${cost}</span>` : '';
  return `<button class="action-btn" onclick="${onclick}">${label}${costHtml}</button>`;
}

// ── Attacker / Blocker selection ──────────────────────────────────────────────

function toggleAttacker(id) {
  const idx = attackersSelected.indexOf(id);
  if (idx >= 0) attackersSelected.splice(idx, 1);
  else attackersSelected.push(id);
  render(currentState);
}

function toggleBlocker(blockerId, attackerId) {
  if (blockersAssignment[blockerId] === attackerId) {
    delete blockersAssignment[blockerId];
  } else {
    blockersAssignment[blockerId] = attackerId;
  }
  render(currentState);
}

function confirmAttackers() {
  sendAction({ type: 'declare_attackers', attacker_ids: attackersSelected });
  attackersSelected = [];
}

function confirmBlockers() {
  const blocks = Object.entries(blockersAssignment)
    .map(([b, a]) => [parseInt(b), parseInt(a)]);
  sendAction({ type: 'declare_blockers', blocks });
  blockersAssignment = {};
}

// ── Graveyard modal ───────────────────────────────────────────────────────────

function openGY(player) {
  const cards = gyData[player];
  if (cards.length === 0) return;
  document.getElementById('gy-modal-title').textContent = `Player ${player} — Graveyard`;
  const html = cards.length === 0
    ? '<div class="gy-empty">Empty</div>'
    : cards.map(c =>
        `<div class="gy-modal-card">
          <span class="gy-name">${c.name}</span>
          <span class="gy-type">${c.type_line}</span>
          ${c.power != null ? `<span class="gy-pt">${c.power}/${c.toughness}</span>` : ''}
        </div>`
      ).join('');
  document.getElementById('gy-modal-cards').innerHTML = html;
  document.getElementById('gy-modal').classList.add('open');
}
function closeGY() { document.getElementById('gy-modal').classList.remove('open'); }
document.getElementById('gy-modal').addEventListener('click', e => { if (e.target === e.currentTarget) closeGY(); });

// ── Log ───────────────────────────────────────────────────────────────────────

function appendLog(html, cls) {
  const log = document.getElementById('log');
  const entry = document.createElement('div');
  entry.className = 'log-entry ' + (cls || '');
  entry.innerHTML = html;
  log.appendChild(entry);
  log.scrollTop = log.scrollHeight;
}

// ── Boot ──────────────────────────────────────────────────────────────────────

fetchState();
</script>
</body>
</html>
```

- [ ] **Step 2: Build and run**

```bash
cargo build --bin ui && cargo run --bin ui -- docs/test-decks/basic.json
```

Expected: "Mecha-Oracle UI running at http://localhost:3000" printed. Open browser.

- [ ] **Step 3: Smoke-test the UI manually**

Walk through this sequence in the browser and verify each step:

1. Both hands show 7 cards each at top and bottom of screen
2. Life totals show 20/20
3. Step tracker shows Untap (active/gold)
4. Click "Pass priority" → step advances to Upkeep, then Draw (with draw applied automatically), then Main 1
5. In Main 1: tap a Forest → G pip appears in mana pool; "Tap Forest" button disappears for that land
6. Play a land from hand → land moves to battlefield row; "Play land" group disappears
7. Cast a creature (after tapping 2 Forests for GG) → creature appears in creatures row with tooltip showing summoning sickness
8. Advance to DeclareAttackers → attacker selection appears in panel
9. Select a creature → it highlights gold; Confirm Attackers → creature shows red attacking border
10. Advance to DeclareBlockers → blocker assignment appears in panel
11. Assign a blocker, Confirm → creature shows blue blocking border
12. Resolve Damage → creatures with lethal damage move to graveyard; life totals update
13. GY stack appears for player with dead creatures; click it → modal shows card contents
14. Hover any tapped card → it rotates back to upright and tooltip appears

- [ ] **Step 4: Run full test suite to confirm nothing regressed**

```bash
cargo test
```

Expected: all tests pass.

- [ ] **Step 5: Commit**

```bash
git add src/bin/ui.html
git commit -m "feat: complete frontend HTML for interactive rules engine testing"
```

---

## Self-Review Checklist

**Spec coverage:**
- ✅ Embedded web server (axum) — Task 6
- ✅ `include_str!` for HTML — Task 6
- ✅ `GET /`, `GET /state`, `POST /action` endpoints — Task 6
- ✅ JSON deck config format — Task 2 + Task 3
- ✅ `--shuffle` CLI flag — Task 3
- ✅ Unknown card name → startup error — Task 3 (`build_game_state`)
- ✅ 7-card opening hands — Task 3
- ✅ View model with all fields from spec — Task 4
- ✅ All 7 action types — Task 5
- ✅ `advance_step` calls `apply_step_start` — Task 5
- ✅ Board layout: hands top/bottom centred, lands/creatures separate rows, turn tracker — Task 7
- ✅ Tapped cards rotate 90°, hover un-rotates — Task 7 (CSS: `transform: none !important` on hover)
- ✅ Tooltip with oracle text and status tags — Task 7
- ✅ Graveyard clickable stack → modal — Task 7
- ✅ Action panel step-aware — Task 7 (`renderActions`)
- ✅ Engine errors shown in log — Task 7 (`sendAction`)
- ✅ `active_player` drives whose actions show — Task 7
- ✅ `can_attack` / `can_block` computed in view model — Task 4

**Type consistency:** `CardView.id: u64` used as `object_id` and `attacker_ids` throughout. `ActionRequest` variants match `sendAction` calls in the HTML exactly. `blocks: Vec<[u64; 2]>` in Rust matches `[[blocker_id, attacker_id], ...]` in JS `confirmBlockers()`.

**No placeholders found.**
