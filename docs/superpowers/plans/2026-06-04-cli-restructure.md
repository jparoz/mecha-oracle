# CLI Restructure: Unified Binary with clap, tracing, and Subcommands

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Merge the `mecha-oracle` and `ui` binaries into a single binary with `demo`, `serve`, and `update-cards` subcommands; add clap for CLI parsing and tracing for logging; suppress per-card parse noise by default.

**Architecture:** The unified binary lives in `src/main.rs` with a `mod serve;` declaration pulling in `src/serve.rs`. CLI parsing uses clap's derive API. `tracing-subscriber` is initialized in `main()` at the level controlled by the global `--verbose`/`-v` flag. `src/cards/mod.rs` replaces `eprintln!` with `tracing::debug!` for individual card failures and `tracing::info!` for the end-of-load summary.

**Tech Stack:** Rust, clap 4 (derive feature), tracing 0.1, tracing-subscriber 0.3, axum 0.7, tokio 1

---

## File Map

| File | Action | Purpose |
|------|--------|---------|
| `Cargo.toml` | Modify | Add clap, tracing, tracing-subscriber deps |
| `src/cards/mod.rs` | Modify | Replace `eprintln!` with tracing; add loaded/skipped counters; emit info summary |
| `src/serve.rs` | Create | Serve subcommand logic (moved from `src/bin/ui.rs`) |
| `src/serve.html` | Move | HTML asset (from `src/bin/ui.html`) |
| `src/main.rs` | Rewrite | Clap CLI entry point; inline demo subcommand; delegate serve to `src/serve.rs` |
| `src/bin/ui.rs` | Delete | Content moved to `src/serve.rs` |
| `src/bin/ui.html` | Delete | Moved to `src/serve.html` |

---

### Task 1: Add dependencies to Cargo.toml

**Files:**
- Modify: `Cargo.toml`

- [ ] **Step 1: Add clap, tracing, and tracing-subscriber**

In `Cargo.toml`, replace the `[dependencies]` section with:

```toml
[dependencies]
serde_json = "1"
ureq = { version = "3", features = ["json"] }
directories = "6"
axum = "0.7"
tokio = { version = "1", features = ["full"] }
serde = { version = "1", features = ["derive"] }
clap = { version = "4", features = ["derive"] }
tracing = "0.1"
tracing-subscriber = "0.3"
```

- [ ] **Step 2: Verify the dependency tree resolves**

```bash
cargo fetch
```

Expected: Downloads new crates with no errors.

- [ ] **Step 3: Commit**

```bash
git add Cargo.toml Cargo.lock
git commit -m "chore: add clap, tracing, tracing-subscriber dependencies"
```

---

### Task 2: Add tracing to cards/mod.rs

**Files:**
- Modify: `src/cards/mod.rs`

- [ ] **Step 1: Verify tests pass before making changes**

```bash
cargo test
```

Expected: All tests pass.

- [ ] **Step 2: Replace the load loop in `from_path`**

In `src/cards/mod.rs`, the `from_path` method currently contains:

```rust
let mut inner = HashMap::new();
for v in &cards {
    match scryfall::parse_card(v) {
        Ok(def) => {
            inner.insert(def.name.to_lowercase(), def);
        }
        Err(e) => {
            let name = v["name"].as_str().unwrap_or("<unknown>");
            eprintln!("Warning: skipping card {name:?}: {e}");
        }
    }
}

Ok(Self { inner })
```

Replace it with:

```rust
let mut inner = HashMap::new();
let mut loaded = 0usize;
let mut skipped = 0usize;
for v in &cards {
    match scryfall::parse_card(v) {
        Ok(def) => {
            inner.insert(def.name.to_lowercase(), def);
            loaded += 1;
        }
        Err(e) => {
            let name = v["name"].as_str().unwrap_or("<unknown>");
            tracing::debug!(card = name, error = %e, "skipped card");
            skipped += 1;
        }
    }
}
tracing::info!(loaded, skipped, "card database loaded");

Ok(Self { inner })
```

- [ ] **Step 3: Run tests to confirm nothing broke**

```bash
cargo test
```

Expected: All tests pass. Tracing calls are no-ops without a subscriber, so tests are unaffected.

- [ ] **Step 4: Commit**

```bash
git add src/cards/mod.rs
git commit -m "feat: replace per-card eprintln! with tracing debug/info"
```

---

### Task 3: Create src/serve.rs and move the HTML asset

**Files:**
- Create: `src/serve.rs`
- Create: `src/serve.html` (from `src/bin/ui.html`)

- [ ] **Step 1: Copy the HTML asset to its new location**

```bash
cp src/bin/ui.html src/serve.html
```

- [ ] **Step 2: Create src/serve.rs**

Create `src/serve.rs` with the following complete content. It is `src/bin/ui.rs` with three changes: `include_str!("ui.html")` → `include_str!("serve.html")`; `parse_args()` and the old `#[tokio::main] async fn main()` are removed; a new `pub async fn run(shuffle: bool, deck_path: &str)` replaces them.

```rust
use axum::{
    Json, Router,
    extract::State,
    response::{Html, IntoResponse},
    routing::{get, post},
};
use mecha_oracle::cards::CardDatabase;
use mecha_oracle::engine::casting::{cast_creature, play_land};
use mecha_oracle::engine::combat::{deal_combat_damage, declare_attackers, declare_blockers};
use mecha_oracle::engine::mana::tap_land_for_mana;
use mecha_oracle::engine::turn::{advance_step, apply_step_start, draw_card};
use mecha_oracle::types::{CardObject, GameState, ObjectId, Player, PlayerId, Zone};
use serde::{Deserialize, Serialize};
use std::sync::{Arc, Mutex};

// ── Config ──────────────────────────────────────────────────────────────────

type DeckConfig = Vec<Vec<String>>;

fn load_config(path: &str) -> Result<DeckConfig, String> {
    let text = std::fs::read_to_string(path).map_err(|e| format!("Cannot read {path}: {e}"))?;
    serde_json::from_str(&text).map_err(|e| format!("Invalid JSON in {path}: {e}"))
}

fn build_game_state(
    config: DeckConfig,
    db: &CardDatabase,
    shuffle: bool,
) -> Result<GameState, String> {
    if config.len() != 2 {
        return Err(format!(
            "Config must have exactly 2 decklists, got {}",
            config.len()
        ));
    }

    let players = vec![
        Player::new(PlayerId(0), "Player 1"),
        Player::new(PlayerId(1), "Player 2"),
    ];
    let mut gs = GameState::new(players);

    let base_seed = if shuffle {
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .subsec_nanos() as u64
    } else {
        0
    };

    for (player_idx, names) in config.iter().enumerate() {
        let pid = PlayerId(player_idx as u8);

        for name in names {
            let def = db
                .get(name)
                .ok_or_else(|| format!("Unknown card: {name:?}"))?
                .clone();
            let id = gs.alloc_id();
            let obj = CardObject::new(id, def, pid, Zone::Library);
            gs.add_object(obj);
            gs.libraries.get_mut(&pid).unwrap().push(id);
        }

        if shuffle {
            let lib = gs.libraries.get_mut(&pid).unwrap();
            let n = lib.len();
            let mut rng = base_seed.wrapping_add(player_idx as u64 * 6364136223846793005);
            for i in (1..n).rev() {
                rng = rng
                    .wrapping_mul(6364136223846793005)
                    .wrapping_add(1442695040888963407);
                let j = (rng >> 33) as usize % (i + 1);
                lib.swap(i, j);
            }
        }
    }

    for _ in 0..7 {
        for pid in [PlayerId(0), PlayerId(1)] {
            if !gs.libraries[&pid].is_empty() {
                gs = draw_card(gs, pid);
            }
        }
    }

    gs = apply_step_start(gs);

    Ok(gs)
}

// ── View model ──────────────────────────────────────────────────────────────

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

fn format_mana_cost(cost: &mecha_oracle::types::mana::ManaCost) -> String {
    let mut s = String::new();
    if cost.generic > 0 {
        s.push_str(&cost.generic.to_string());
    }
    for _ in 0..cost.white {
        s.push('W');
    }
    for _ in 0..cost.blue {
        s.push('U');
    }
    for _ in 0..cost.black {
        s.push('B');
    }
    for _ in 0..cost.red {
        s.push('R');
    }
    for _ in 0..cost.green {
        s.push('G');
    }
    for _ in 0..cost.colorless {
        s.push('C');
    }
    s
}

fn format_type_line(tl: &mecha_oracle::types::card::TypeLine) -> String {
    use mecha_oracle::types::card::{CardType, Supertype};
    let mut parts: Vec<&str> = Vec::new();
    for st in &tl.supertypes {
        parts.push(match st {
            Supertype::Basic => "Basic",
            Supertype::Legendary => "Legendary",
            Supertype::Snow => "Snow",
            Supertype::World => "World",
        });
    }
    for ct in &tl.card_types {
        parts.push(match ct {
            CardType::Creature => "Creature",
            CardType::Land => "Land",
            CardType::Instant => "Instant",
            CardType::Sorcery => "Sorcery",
            CardType::Artifact => "Artifact",
            CardType::Enchantment => "Enchantment",
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
    use mecha_oracle::types::ObjectId;
    use std::collections::HashSet;
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
            .filter(|obj| obj.is_land() && !obj.is_creature())
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
        active_player: if state.active_player == PlayerId(0) {
            1
        } else {
            2
        },
        lands_played_this_turn: state.lands_played_this_turn,
        game_over: state.is_game_over(),
        winner: state
            .winner()
            .map(|pid| if pid == PlayerId(0) { 1 } else { 2 }),
        p1: build_player_view(state, PlayerId(0)),
        p2: build_player_view(state, PlayerId(1)),
    }
}

// ── Actions ──────────────────────────────────────────────────────────────────

#[derive(Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
enum ActionRequest {
    TapLand { object_id: u64 },
    PlayLand { object_id: u64 },
    CastCreature { object_id: u64 },
    DeclareAttackers { attacker_ids: Vec<u64> },
    DeclareBlockers { blocks: Vec<[u64; 2]> },
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
            tap_land_for_mana(state, ObjectId(object_id)).map_err(|e| format!("{e:?}"))
        }
        ActionRequest::PlayLand { object_id } => {
            let active = state.active_player;
            play_land(state, active, ObjectId(object_id)).map_err(|e| format!("{e:?}"))
        }
        ActionRequest::CastCreature { object_id } => {
            let active = state.active_player;
            cast_creature(state, active, ObjectId(object_id)).map_err(|e| format!("{e:?}"))
        }
        ActionRequest::DeclareAttackers { attacker_ids } => {
            let ids: Vec<ObjectId> = attacker_ids.iter().map(|&id| ObjectId(id)).collect();
            let active = state.active_player;
            declare_attackers(state, active, &ids).map_err(|e| format!("{e:?}"))
        }
        ActionRequest::DeclareBlockers { blocks } => {
            let pairs: Vec<(ObjectId, ObjectId)> = blocks
                .iter()
                .map(|[b, a]| (ObjectId(*b), ObjectId(*a)))
                .collect();
            let defender = state.opponent_of(state.active_player);
            declare_blockers(state, defender, &pairs).map_err(|e| format!("{e:?}"))
        }
        ActionRequest::DealCombatDamage => Ok(deal_combat_damage(state)),
        ActionRequest::AdvanceStep => {
            let s = advance_step(state);
            Ok(apply_step_start(s))
        }
    }
}

// ── Game init ────────────────────────────────────────────────────────────────

fn init_game(path: &str, shuffle: bool) -> Result<GameState, String> {
    let db = CardDatabase::open().map_err(|e| format!("Card database error: {e}"))?;
    let config = load_config(path)?;
    build_game_state(config, &db, shuffle)
}

const INDEX_HTML: &str = include_str!("serve.html");

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

// ── Entry point ───────────────────────────────────────────────────────────────

pub async fn run(shuffle: bool, deck_path: &str) {
    let gs = init_game(deck_path, shuffle).unwrap_or_else(|e| {
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

// ── Tests ────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use mecha_oracle::types::Step;
    use std::path::Path;

    fn test_db() -> CardDatabase {
        CardDatabase::from_path(Path::new("tests/fixtures/oracle_cards_test.json")).unwrap()
    }

    #[test]
    fn build_game_state_deals_7_cards_each() {
        let config = vec![
            vec![
                "Forest".into(),
                "Forest".into(),
                "Forest".into(),
                "Forest".into(),
                "Grizzly Bears".into(),
                "Grizzly Bears".into(),
                "Grizzly Bears".into(),
                "Grizzly Bears".into(),
                "Forest".into(),
                "Forest".into(),
            ],
            vec![
                "Forest".into(),
                "Forest".into(),
                "Forest".into(),
                "Forest".into(),
                "Grizzly Bears".into(),
                "Grizzly Bears".into(),
                "Grizzly Bears".into(),
                "Grizzly Bears".into(),
                "Forest".into(),
                "Forest".into(),
            ],
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
        let config = vec![vec!["NoSuchCard".into()], vec!["Forest".into()]];
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
        let cost = ManaCost {
            green: 2,
            ..Default::default()
        };
        assert_eq!(format_mana_cost(&cost), "GG");
    }

    #[test]
    fn format_mana_cost_generic_and_color() {
        use mecha_oracle::types::mana::ManaCost;
        let cost = ManaCost {
            generic: 3,
            green: 1,
            ..Default::default()
        };
        assert_eq!(format_mana_cost(&cost), "3G");
    }

    #[test]
    fn format_type_line_with_subtype() {
        let db = test_db();
        let forest = db.get("Forest").unwrap();
        let result = format_type_line(&forest.type_line);
        assert_eq!(result, "Basic Land — Forest");
    }

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
        use mecha_oracle::engine::casting::play_land;
        let config = vec![
            vec![
                "Forest".into(),
                "Forest".into(),
                "Forest".into(),
                "Forest".into(),
                "Grizzly Bears".into(),
                "Grizzly Bears".into(),
                "Grizzly Bears".into(),
                "Grizzly Bears".into(),
                "Forest".into(),
                "Forest".into(),
            ],
            (0..10).map(|_| "Forest".to_string()).collect(),
        ];
        let db = test_db();
        let mut gs = build_game_state(config, &db, false).unwrap();

        gs = dispatch_action(gs, ActionRequest::AdvanceStep).unwrap();
        gs = dispatch_action(gs, ActionRequest::AdvanceStep).unwrap();
        gs = dispatch_action(gs, ActionRequest::AdvanceStep).unwrap();

        let land_id = gs.hands[&PlayerId(0)]
            .iter()
            .find(|id| gs.objects[*id].is_land())
            .copied()
            .unwrap();
        gs = play_land(gs, PlayerId(0), land_id).unwrap();

        let tap_result = dispatch_action(
            gs,
            ActionRequest::TapLand {
                object_id: land_id.0,
            },
        );
        assert!(tap_result.is_ok());
        let gs2 = tap_result.unwrap();
        assert_eq!(gs2.get_player(PlayerId(0)).unwrap().mana_pool.green, 1);
    }
}
```

- [ ] **Step 3: Verify src/serve.rs compiles (src/bin/ui.rs still exists at this point)**

```bash
cargo check
```

Expected: Compiles successfully. Cargo currently sees both `src/bin/ui.rs` and `src/serve.rs` (the latter as an unused file until Task 4 adds `mod serve;`). No errors expected.

- [ ] **Step 4: Commit**

```bash
git add src/serve.rs src/serve.html
git commit -m "feat: add src/serve.rs and src/serve.html (serve subcommand logic)"
```

---

### Task 4: Rewrite src/main.rs with clap

**Files:**
- Modify: `src/main.rs`

- [ ] **Step 1: Write failing CLI tests**

Replace the entire contents of `src/main.rs` with just the struct definitions and tests (no `main()` yet). This lets us verify the clap shape before wiring everything together.

```rust
mod serve;

use clap::{Parser, Subcommand};

#[derive(Parser)]
#[command(name = "mecha-oracle", about = "MTG Rules Engine")]
struct Cli {
    #[arg(short, long, global = true, help = "Show per-card parse warnings")]
    verbose: bool,
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    Demo,
    Serve {
        #[arg(long)]
        shuffle: bool,
        deck: String,
    },
    UpdateCards,
}

fn main() {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cli_serve_requires_deck_argument() {
        assert!(Cli::try_parse_from(["mecha-oracle", "serve"]).is_err());
    }

    #[test]
    fn cli_serve_parses_deck_and_defaults() {
        let cli = Cli::try_parse_from(["mecha-oracle", "serve", "deck.json"]).unwrap();
        assert!(!cli.verbose);
        match cli.command {
            Command::Serve { shuffle, deck } => {
                assert!(!shuffle);
                assert_eq!(deck, "deck.json");
            }
            _ => panic!("expected Serve"),
        }
    }

    #[test]
    fn cli_serve_shuffle_flag() {
        let cli =
            Cli::try_parse_from(["mecha-oracle", "serve", "--shuffle", "deck.json"]).unwrap();
        match cli.command {
            Command::Serve { shuffle, .. } => assert!(shuffle),
            _ => panic!("expected Serve"),
        }
    }

    #[test]
    fn cli_verbose_is_global() {
        let cli = Cli::try_parse_from(["mecha-oracle", "-v", "demo"]).unwrap();
        assert!(cli.verbose);
    }

    #[test]
    fn cli_update_cards_subcommand() {
        let cli = Cli::try_parse_from(["mecha-oracle", "update-cards"]).unwrap();
        assert!(matches!(cli.command, Command::UpdateCards));
    }
}
```

- [ ] **Step 2: Run the CLI tests to verify they pass**

```bash
cargo test --bin mecha-oracle
```

Expected: 5 CLI tests pass.

- [ ] **Step 3: Expand main.rs to the full implementation**

Replace the entire contents of `src/main.rs` with the complete implementation:

```rust
mod serve;

use clap::{Parser, Subcommand};
use directories::ProjectDirs;
use mecha_oracle::cards::{CardDatabase, update_cards};
use mecha_oracle::engine::turn::{advance_step, apply_step_start};
use mecha_oracle::types::{CardObject, GameState, Player, PlayerId, Step, Zone};

#[derive(Parser)]
#[command(name = "mecha-oracle", about = "MTG Rules Engine")]
struct Cli {
    #[arg(short, long, global = true, help = "Show per-card parse warnings")]
    verbose: bool,
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    Demo,
    Serve {
        #[arg(long)]
        shuffle: bool,
        deck: String,
    },
    UpdateCards,
}

#[tokio::main]
async fn main() {
    let cli = Cli::parse();

    let level = if cli.verbose {
        tracing::Level::DEBUG
    } else {
        tracing::Level::INFO
    };
    tracing_subscriber::fmt()
        .with_max_level(level)
        .without_time()
        .with_target(false)
        .init();

    match cli.command {
        Command::Demo => run_demo(),
        Command::Serve { shuffle, deck } => serve::run(shuffle, &deck).await,
        Command::UpdateCards => run_update_cards(),
    }
}

fn run_update_cards() {
    let dirs = ProjectDirs::from("", "", "mecha-oracle")
        .expect("Cannot determine user data directory");
    std::fs::create_dir_all(dirs.data_dir()).expect("Cannot create data directory");
    update_cards(dirs.data_dir()).expect("Card update failed");
}

fn run_demo() {
    let db = CardDatabase::open()
        .expect("Card database not found — run `mecha-oracle update-cards` first");

    println!("=== mecha-oracle: MTG Rules Engine — Phase 1 Demo ===\n");

    let mut gs = build_game(&db);
    let mut step_count = 0;

    while !gs.is_game_over() && step_count < 200 {
        let step = gs.step();
        let active = gs.active_player;
        let turn = gs.turn_number;

        if step == Step::Untap {
            println!("--- Turn {turn} (Active: {active:?}) ---");
            let life0 = gs.get_player(PlayerId(0)).unwrap().life;
            let life1 = gs.get_player(PlayerId(1)).unwrap().life;
            println!("  Life: Alice={life0}, Bob={life1}");
        }

        gs = apply_step_start(gs);
        gs = advance_step(gs);
        step_count += 1;
    }

    match gs.winner() {
        Some(pid) => println!("\nGame over! Winner: {pid:?}"),
        None => println!("\nGame ended (draw or step limit reached)."),
    }
}

fn build_game(db: &CardDatabase) -> GameState {
    let forest = || db.get("Forest").expect("Forest not in database").clone();
    let bears = || {
        db.get("Grizzly Bears")
            .expect("Grizzly Bears not in database")
            .clone()
    };

    let mut gs = GameState::new(vec![
        Player::new(PlayerId(0), "Alice"),
        Player::new(PlayerId(1), "Bob"),
    ]);

    for &owner in &[PlayerId(0), PlayerId(1)] {
        for _ in 0..5 {
            let id = gs.alloc_id();
            let obj = CardObject::new(id, forest(), owner, Zone::Library);
            gs.libraries.get_mut(&owner).unwrap().push(id);
            gs.add_object(obj);
        }
        for _ in 0..2 {
            let id = gs.alloc_id();
            let obj = CardObject::new(id, bears(), owner, Zone::Library);
            gs.libraries.get_mut(&owner).unwrap().push(id);
            gs.add_object(obj);
        }
    }

    gs
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cli_serve_requires_deck_argument() {
        assert!(Cli::try_parse_from(["mecha-oracle", "serve"]).is_err());
    }

    #[test]
    fn cli_serve_parses_deck_and_defaults() {
        let cli = Cli::try_parse_from(["mecha-oracle", "serve", "deck.json"]).unwrap();
        assert!(!cli.verbose);
        match cli.command {
            Command::Serve { shuffle, deck } => {
                assert!(!shuffle);
                assert_eq!(deck, "deck.json");
            }
            _ => panic!("expected Serve"),
        }
    }

    #[test]
    fn cli_serve_shuffle_flag() {
        let cli =
            Cli::try_parse_from(["mecha-oracle", "serve", "--shuffle", "deck.json"]).unwrap();
        match cli.command {
            Command::Serve { shuffle, .. } => assert!(shuffle),
            _ => panic!("expected Serve"),
        }
    }

    #[test]
    fn cli_verbose_is_global() {
        let cli = Cli::try_parse_from(["mecha-oracle", "-v", "demo"]).unwrap();
        assert!(cli.verbose);
    }

    #[test]
    fn cli_update_cards_subcommand() {
        let cli = Cli::try_parse_from(["mecha-oracle", "update-cards"]).unwrap();
        assert!(matches!(cli.command, Command::UpdateCards));
    }
}
```

- [ ] **Step 4: Run all tests**

```bash
cargo test
```

Expected: All tests pass, including the 5 CLI tests and all serve module tests.

- [ ] **Step 5: Commit**

```bash
git add src/main.rs
git commit -m "feat: unified binary with clap subcommands (demo/serve/update-cards)"
```

---

### Task 5: Delete src/bin/ui.rs and src/bin/ui.html

**Files:**
- Delete: `src/bin/ui.rs`
- Delete: `src/bin/ui.html`

- [ ] **Step 1: Remove the old binary files**

```bash
git rm src/bin/ui.rs src/bin/ui.html
```

- [ ] **Step 2: Run the full test suite**

```bash
cargo test
```

Expected: All tests pass. The `ui` binary target is gone (Cargo auto-discovered it from `src/bin/ui.rs`; removing the file removes the target).

- [ ] **Step 3: Verify the binary builds and help text is correct**

```bash
cargo build && ./target/debug/mecha-oracle --help
```

Expected output (approximate):
```
MTG Rules Engine

Usage: mecha-oracle [OPTIONS] <COMMAND>

Commands:
  demo          
  serve         
  update-cards  
  help          Print this message or the help of the given subcommand(s)

Options:
  -v, --verbose  Show per-card parse warnings
  -h, --help     Print help
```

- [ ] **Step 4: Commit**

```bash
git commit -m "chore: remove src/bin/ui.rs and src/bin/ui.html (merged into unified binary)"
```
