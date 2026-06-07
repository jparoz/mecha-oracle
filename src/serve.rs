use axum::{
    Json, Router,
    extract::State,
    response::{Html, IntoResponse},
    routing::{get, post},
};
use mecha_oracle::cards::CardDatabase;
use mecha_oracle::engine::activated::{activate_ability, can_pay_cost};
use mecha_oracle::engine::casting::{cast_creature, play_land};
use mecha_oracle::engine::combat::{declare_attackers, declare_blockers};
use mecha_oracle::engine::mana::{reset_mana, tap_land_for_mana};
use mecha_oracle::engine::turn::{advance_step, apply_step_start, draw_card, skip_to_first_main};
use mecha_oracle::types::ability::{
    AbilityAST, ActivatedAbility, CostComponent, EffectStep, OracleSpan,
};
use mecha_oracle::types::{CardObject, GameState, ObjectId, Player, PlayerId, Step, Zone};
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

    gs = skip_to_first_main(gs);

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
#[serde(rename_all = "snake_case")]
enum SpanKind {
    Parsed,
    Ignored,
    Unparsed,
    ParsedUnimplemented,
}

#[derive(Serialize)]
struct OracleSpanView {
    kind: SpanKind,
    text: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    ignored_kind: Option<mecha_oracle::types::IgnoredKind>,
}

#[derive(Serialize)]
struct ActivatedAbilityView {
    index: usize,
    label: String,
    can_activate: bool,
}

#[derive(Serialize)]
struct CardView {
    id: ObjectId,
    name: String,
    type_line: String,
    oracle_text: Vec<OracleSpanView>,
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
    activated_abilities: Vec<ActivatedAbilityView>,
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
    active_player: PlayerId,
    priority_player: PlayerId,
    lands_played_this_turn: u32,
    game_over: bool,
    winner: Option<PlayerId>,
    p1: PlayerView,
    p2: PlayerView,
    can_reset_mana: bool,
    attackers_declared: bool,
    blockers_declared: bool,
}

fn format_mana_cost(cost: &mecha_oracle::types::mana::ManaCost) -> String {
    use mecha_oracle::types::mana::ManaPip;
    cost.pips
        .iter()
        .map(|pip| match pip {
            ManaPip::White => "W".to_string(),
            ManaPip::Blue => "U".to_string(),
            ManaPip::Black => "B".to_string(),
            ManaPip::Red => "R".to_string(),
            ManaPip::Green => "G".to_string(),
            ManaPip::Colorless => "C".to_string(),
            ManaPip::Generic(n) => n.to_string(),
            ManaPip::X => "X".to_string(),
            ManaPip::Hybrid(c1, c2) => format!("{c1}/{c2}"),
            ManaPip::GenericHybrid(n, c) => format!("{n}/{c}"),
            ManaPip::ColorlessHybrid(c) => format!("C/{c}"),
            ManaPip::Phyrexian(c) => format!("{c}/P"),
            ManaPip::HybridPhyrexian(c1, c2) => format!("{c1}/{c2}/P"),
            ManaPip::Snow => "S".to_string(),
        })
        .collect::<Vec<_>>()
        .join("")
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

fn format_mana_cost_braced(cost: &mecha_oracle::types::mana::ManaCost) -> String {
    use mecha_oracle::types::mana::ManaPip;
    cost.pips
        .iter()
        .map(|pip| match pip {
            ManaPip::White => "{W}".to_string(),
            ManaPip::Blue => "{U}".to_string(),
            ManaPip::Black => "{B}".to_string(),
            ManaPip::Red => "{R}".to_string(),
            ManaPip::Green => "{G}".to_string(),
            ManaPip::Colorless => "{C}".to_string(),
            ManaPip::Generic(n) => format!("{{{n}}}"),
            ManaPip::X => "{X}".to_string(),
            ManaPip::Hybrid(c1, c2) => format!("{{{c1}/{c2}}}"),
            ManaPip::GenericHybrid(n, c) => format!("{{{n}/{c}}}"),
            ManaPip::ColorlessHybrid(c) => format!("{{C/{c}}}"),
            ManaPip::Phyrexian(c) => format!("{{{c}/P}}"),
            ManaPip::HybridPhyrexian(c1, c2) => format!("{{{c1}/{c2}/P}}"),
            ManaPip::Snow => "{S}".to_string(),
        })
        .collect::<String>()
}

fn format_mana_pool(pool: &mecha_oracle::types::mana::ManaPool) -> String {
    let mut s = String::new();
    for _ in 0..pool.white {
        s.push_str("{W}");
    }
    for _ in 0..pool.blue {
        s.push_str("{U}");
    }
    for _ in 0..pool.black {
        s.push_str("{B}");
    }
    for _ in 0..pool.red {
        s.push_str("{R}");
    }
    for _ in 0..pool.green {
        s.push_str("{G}");
    }
    for _ in 0..pool.colorless {
        s.push_str("{C}");
    }
    s
}

fn format_activated_ability(ability: &ActivatedAbility) -> String {
    let cost_parts: Vec<String> = ability
        .cost
        .iter()
        .map(|c| match c {
            CostComponent::Tap => "{T}".to_string(),
            CostComponent::Mana(m) => format_mana_cost_braced(m),
            CostComponent::PayLife(n) => format!("Pay {n} life"),
            CostComponent::Sacrifice(n, _) => format!("Sacrifice {n}"),
            CostComponent::Discard(n, _) => format!("Discard {n}"),
            CostComponent::Unimplemented(s) => s.clone(),
        })
        .collect();
    let effect_parts: Vec<String> = ability
        .effect
        .iter()
        .map(|e| match e {
            EffectStep::AddMana(pool) => format!("Add {}", format_mana_pool(pool)),
            EffectStep::Mill(n) => format!("Mill {n}"),
            EffectStep::DrawCard(n) => {
                if *n == 1 {
                    "Draw a card".to_string()
                } else {
                    format!("Draw {n} cards")
                }
            }
        })
        .collect();
    format!("{}: {}", cost_parts.join(", "), effect_parts.join(". "))
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
        id: obj.id,
        name: obj.definition.name.clone(),
        type_line: format_type_line(&obj.definition.type_line),
        oracle_text: {
            obj.definition
                .abilities
                .iter()
                .map(|span| match span {
                    OracleSpan::Parsed(AbilityAST::Static(kw)) => OracleSpanView {
                        kind: SpanKind::Parsed,
                        text: kw.display_name().to_string(),
                        ignored_kind: None,
                    },
                    OracleSpan::Parsed(AbilityAST::Activated(a)) => OracleSpanView {
                        kind: SpanKind::Parsed,
                        text: format_activated_ability(a),
                        ignored_kind: None,
                    },
                    OracleSpan::Ignored(kind, t) => OracleSpanView {
                        kind: SpanKind::Ignored,
                        text: t.clone(),
                        ignored_kind: Some(kind.clone()),
                    },
                    OracleSpan::Unparsed(t) => OracleSpanView {
                        kind: SpanKind::Unparsed,
                        text: t.clone(),
                        ignored_kind: None,
                    },
                    OracleSpan::ParsedUnimplemented(t) => OracleSpanView {
                        kind: SpanKind::ParsedUnimplemented,
                        text: t.clone(),
                        ignored_kind: None,
                    },
                    _ => OracleSpanView {
                        kind: SpanKind::Unparsed,
                        text: format!("{span:?}"),
                        ignored_kind: None,
                    },
                })
                .collect()
        },
        mana_cost: obj.definition.mana_cost.as_ref().map(format_mana_cost),
        power: obj.current_power,
        toughness: obj.current_toughness,
        tapped: obj.tapped,
        summoning_sick: obj.summoning_sick,
        damage_marked: obj.damage_marked,
        is_attacking: state.combat.attackers.contains(&obj.id),
        is_blocking: all_blockers.contains(&obj.id),
        can_attack: state.step() == Step::DeclareAttackers
            && pid == state.active_player
            && obj.can_attack(),
        can_block: state.step() == Step::DeclareBlockers
            && pid != state.active_player
            && obj.can_block(),
        activated_abilities: obj
            .definition
            .abilities
            .iter()
            .filter_map(|span| match span {
                OracleSpan::Parsed(AbilityAST::Activated(a)) => Some(a),
                _ => None,
            })
            .enumerate()
            .map(|(i, ability)| ActivatedAbilityView {
                index: i,
                label: format_activated_ability(ability),
                can_activate: can_pay_cost(state, obj.id, ability, pid),
            })
            .collect(),
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
        active_player: state.active_player,
        priority_player: state.priority_player,
        lands_played_this_turn: state.lands_played_this_turn,
        game_over: state.is_game_over(),
        winner: state.winner(),
        p1: build_player_view(state, PlayerId(0)),
        p2: build_player_view(state, PlayerId(1)),
        can_reset_mana: state.mana_checkpoint.is_some(),
        attackers_declared: state.combat.attackers_declared,
        blockers_declared: state.combat.blockers_declared,
    }
}

// ── Actions ──────────────────────────────────────────────────────────────────

#[derive(Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
enum ActionRequest {
    TapLand {
        object_id: u64,
    },
    PlayLand {
        object_id: u64,
    },
    CastCreature {
        object_id: u64,
    },
    DeclareAttackers {
        attacker_ids: Vec<u64>,
    },
    DeclareBlockers {
        blocks: Vec<[u64; 2]>,
    },
    AdvanceStep,
    ResetMana,
    ActivateAbility {
        object_id: u64,
        ability_index: usize,
        #[serde(default)]
        x_value: Option<u32>,
        #[serde(default)]
        payment_plan: Option<mecha_oracle::types::mana::PaymentPlan>,
    },
}

#[derive(Serialize)]
struct ActionResponse {
    ok: bool,
    state: GameView,
    #[serde(skip_serializing_if = "Option::is_none")]
    error: Option<String>,
}

fn advance_with_auto_steps(mut state: GameState) -> GameState {
    loop {
        state = advance_step(state);
        state = apply_step_start(state);
        if !matches!(state.step(), Step::Untap | Step::Cleanup) || state.is_game_over() {
            break;
        }
    }
    state
}

fn dispatch_action(mut state: GameState, action: ActionRequest) -> Result<GameState, String> {
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
        ActionRequest::AdvanceStep => {
            // Turn-based actions (CR 508/509) must complete before priority opens
            if state.step() == Step::DeclareAttackers && !state.combat.attackers_declared {
                return Err("must declare attackers before passing priority".to_string());
            }
            if state.step() == Step::DeclareBlockers && !state.combat.blockers_declared {
                return Err("must declare blockers before passing priority".to_string());
            }
            let nap = state.opponent_of(state.active_player);
            if state.priority_player == state.active_player {
                state.priority_player = nap;
                Ok(state)
            } else {
                Ok(advance_with_auto_steps(state))
            }
        }
        ActionRequest::ResetMana => reset_mana(state).map_err(|e| format!("{e:?}")),
        ActionRequest::ActivateAbility {
            object_id,
            ability_index,
            x_value,
            payment_plan,
        } => {
            let player = state.priority_player;
            activate_ability(
                state,
                ObjectId(object_id),
                ability_index,
                player,
                x_value,
                payment_plan,
            )
            .map_err(|e| format!("{e:?}"))
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

    let listener = tokio::net::TcpListener::bind("0.0.0.0:3000")
        .await
        .unwrap_or_else(|e| {
            eprintln!("Cannot bind to port 3000: {e}");
            std::process::exit(1);
        });
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
    fn build_game_state_starts_at_pre_combat_main() {
        let config = vec![
            (0..10).map(|_| "Forest".to_string()).collect(),
            (0..10).map(|_| "Forest".to_string()).collect(),
        ];
        let db = test_db();
        let gs = build_game_state(config, &db, false).unwrap();
        assert_eq!(gs.step(), Step::PreCombatMain);
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
        assert_eq!(view.active_player, PlayerId(0));
        assert_eq!(view.priority_player, PlayerId(0));
        assert_eq!(view.step, "PreCombatMain");
        assert_eq!(view.turn, 1);
        assert!(!view.attackers_declared);
        assert!(!view.blockers_declared);
    }

    #[test]
    fn game_view_includes_combat_declared_flags() {
        let config = vec![
            (0..10).map(|_| "Forest".to_string()).collect(),
            (0..10).map(|_| "Forest".to_string()).collect(),
        ];
        let db = test_db();
        let mut gs = build_game_state(config, &db, false).unwrap();
        // Navigate to DeclareAttackers: 4 passes (2 per step × 2 steps)
        for _ in 0..4 {
            gs = dispatch_action(gs, ActionRequest::AdvanceStep).unwrap();
        }
        assert!(!build_game_view(&gs).attackers_declared);

        gs = dispatch_action(
            gs,
            ActionRequest::DeclareAttackers {
                attacker_ids: vec![],
            },
        )
        .unwrap();
        assert!(build_game_view(&gs).attackers_declared);
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
        use mecha_oracle::types::mana::{ManaCost, ManaPip};
        let cost = ManaCost {
            pips: vec![ManaPip::Green, ManaPip::Green],
        };
        assert_eq!(format_mana_cost(&cost), "GG");
    }

    #[test]
    fn format_mana_cost_generic_and_color() {
        use mecha_oracle::types::mana::{ManaCost, ManaPip};
        let cost = ManaCost {
            pips: vec![ManaPip::Generic(3), ManaPip::Green],
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
    fn ap_passing_priority_shifts_to_nap_without_advancing_step() {
        let config = vec![
            (0..10).map(|_| "Forest".to_string()).collect(),
            (0..10).map(|_| "Forest".to_string()).collect(),
        ];
        let db = test_db();
        let gs = build_game_state(config, &db, false).unwrap();
        assert_eq!(gs.step(), Step::PreCombatMain);
        assert_eq!(gs.priority_player, PlayerId(0));

        let gs = dispatch_action(gs, ActionRequest::AdvanceStep).unwrap();

        assert_eq!(gs.step(), Step::PreCombatMain); // step did NOT advance
        assert_eq!(gs.priority_player, PlayerId(1)); // priority shifted to NAP
    }

    #[test]
    fn nap_passing_priority_advances_step() {
        let config = vec![
            (0..10).map(|_| "Forest".to_string()).collect(),
            (0..10).map(|_| "Forest".to_string()).collect(),
        ];
        let db = test_db();
        let gs = build_game_state(config, &db, false).unwrap();

        let gs = dispatch_action(gs, ActionRequest::AdvanceStep).unwrap(); // AP passes
        let gs = dispatch_action(gs, ActionRequest::AdvanceStep).unwrap(); // NAP passes → advances

        assert_eq!(gs.step(), Step::BeginningOfCombat);
        assert_eq!(gs.priority_player, PlayerId(0)); // resets to AP
    }

    #[test]
    fn advance_step_blocked_before_attackers_declared() {
        let config = vec![
            (0..10).map(|_| "Forest".to_string()).collect(),
            (0..10).map(|_| "Forest".to_string()).collect(),
        ];
        let db = test_db();
        let mut gs = build_game_state(config, &db, false).unwrap();
        // Navigate to DeclareAttackers (4 passes: PC×2, BOC×2)
        for _ in 0..4 {
            gs = dispatch_action(gs, ActionRequest::AdvanceStep).unwrap();
        }
        assert_eq!(gs.step(), Step::DeclareAttackers);
        assert!(!gs.combat.attackers_declared);

        // AdvanceStep must be rejected before attackers are declared
        assert!(dispatch_action(gs, ActionRequest::AdvanceStep).is_err());
    }

    #[test]
    fn advance_step_blocked_before_blockers_declared() {
        let config = vec![
            (0..10).map(|_| "Forest".to_string()).collect(),
            (0..10).map(|_| "Forest".to_string()).collect(),
        ];
        let db = test_db();
        let mut gs = build_game_state(config, &db, false).unwrap();
        // Navigate to DeclareAttackers (4 passes), declare, then advance to DeclareBlockers (2 passes)
        for _ in 0..4 {
            gs = dispatch_action(gs, ActionRequest::AdvanceStep).unwrap();
        }
        gs = dispatch_action(
            gs,
            ActionRequest::DeclareAttackers {
                attacker_ids: vec![],
            },
        )
        .unwrap();
        for _ in 0..2 {
            gs = dispatch_action(gs, ActionRequest::AdvanceStep).unwrap();
        }
        assert_eq!(gs.step(), Step::DeclareBlockers);
        assert!(!gs.combat.blockers_declared);

        // AdvanceStep must be rejected before blockers are declared
        assert!(dispatch_action(gs, ActionRequest::AdvanceStep).is_err());
    }

    #[test]
    fn advancing_from_end_step_auto_advances_to_next_upkeep() {
        use mecha_oracle::types::Step;
        let config = vec![
            (0..10).map(|_| "Forest".to_string()).collect(),
            (0..10).map(|_| "Forest".to_string()).collect(),
        ];
        let db = test_db();
        let mut gs = build_game_state(config, &db, false).unwrap();
        // PC (2) + BOC (2) → DeclareAttackers
        for _ in 0..4 {
            gs = dispatch_action(gs, ActionRequest::AdvanceStep).unwrap();
        }
        // Turn-based action: declare (empty) before priority opens
        gs = dispatch_action(
            gs,
            ActionRequest::DeclareAttackers {
                attacker_ids: vec![],
            },
        )
        .unwrap();
        // DA (2) → DeclareBlockers
        for _ in 0..2 {
            gs = dispatch_action(gs, ActionRequest::AdvanceStep).unwrap();
        }
        // Turn-based action: declare (empty) before priority opens
        gs = dispatch_action(gs, ActionRequest::DeclareBlockers { blocks: vec![] }).unwrap();
        // DB (2) → CD (auto-resolves) → CD (2) → EOC (2) → PC2 (2) → End
        for _ in 0..8 {
            gs = dispatch_action(gs, ActionRequest::AdvanceStep).unwrap();
        }
        assert_eq!(gs.step(), Step::End);
        assert_eq!(gs.active_player, PlayerId(0));

        // Two more passes → Cleanup (auto) → Untap (auto) → Upkeep for P1
        let gs = dispatch_action(gs, ActionRequest::AdvanceStep).unwrap();
        assert_eq!(gs.step(), Step::End); // still End after first pass
        let gs = dispatch_action(gs, ActionRequest::AdvanceStep).unwrap();
        assert_eq!(gs.step(), Step::Upkeep);
        assert_eq!(gs.active_player, PlayerId(1));
    }

    #[test]
    fn reset_mana_action_untaps_land_and_restores_pool() {
        use mecha_oracle::engine::casting::play_land;
        let db = test_db();
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
        let mut gs = build_game_state(config, &db, false).unwrap();

        // Play a land from hand so we have one untapped land on the battlefield.
        let land_id = gs.hands[&PlayerId(0)]
            .iter()
            .find(|id| gs.objects[*id].is_land())
            .copied()
            .unwrap();
        gs = play_land(gs, PlayerId(0), land_id).unwrap();

        // Tap it for mana via the action dispatcher.
        gs = dispatch_action(
            gs,
            ActionRequest::TapLand {
                object_id: land_id.0,
            },
        )
        .unwrap();
        assert!(gs.objects[&land_id].tapped);
        assert_eq!(gs.get_player(PlayerId(0)).unwrap().mana_pool.green, 1);
        assert!(gs.mana_checkpoint.is_some());
        let view = build_game_view(&gs);
        assert!(view.can_reset_mana);

        // Reset mana.
        gs = dispatch_action(gs, ActionRequest::ResetMana).unwrap();

        assert!(!gs.objects[&land_id].tapped, "land untapped");
        assert!(
            gs.get_player(PlayerId(0)).unwrap().mana_pool.is_empty(),
            "pool empty"
        );
        assert!(gs.mana_checkpoint.is_none());
        let view = build_game_view(&gs);
        assert!(!view.can_reset_mana);
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

    #[test]
    fn can_attack_true_only_for_active_player_at_declare_attackers() {
        use mecha_oracle::types::{CardObject, Step, Zone};
        let db = test_db();
        let config = vec![
            (0..10).map(|_| "Forest".to_string()).collect(),
            (0..10).map(|_| "Forest".to_string()).collect(),
        ];
        let mut gs = build_game_state(config, &db, false).unwrap();

        // Place one untapped, non-sick creature for each player.
        let p1_id = {
            let id = gs.alloc_id();
            let mut obj = CardObject::new(
                id,
                db.get("Grizzly Bears").unwrap().clone(),
                PlayerId(0),
                Zone::Battlefield,
            );
            obj.summoning_sick = false;
            gs.battlefield.push(id);
            gs.add_object(obj);
            id.0
        };
        let p2_id = {
            let id = gs.alloc_id();
            let mut obj = CardObject::new(
                id,
                db.get("Grizzly Bears").unwrap().clone(),
                PlayerId(1),
                Zone::Battlefield,
            );
            obj.summoning_sick = false;
            gs.battlefield.push(id);
            gs.add_object(obj);
            id.0
        };

        // 4 passes to reach DeclareAttackers (2 per step × 2 steps)
        for _ in 0..4 {
            gs = dispatch_action(gs, ActionRequest::AdvanceStep).unwrap();
        }
        assert_eq!(gs.step(), Step::DeclareAttackers);

        let view = build_game_view(&gs);
        let p1_c = view
            .p1
            .creatures
            .iter()
            .find(|c| c.id == ObjectId(p1_id))
            .unwrap();
        let p2_c = view
            .p2
            .creatures
            .iter()
            .find(|c| c.id == ObjectId(p2_id))
            .unwrap();

        assert!(p1_c.can_attack, "active player's creature shows can_attack");
        assert!(
            !p2_c.can_attack,
            "defending player's creature does not show can_attack"
        );
        assert!(
            !p1_c.can_block,
            "can_block is false outside DeclareBlockers"
        );
        assert!(
            !p2_c.can_block,
            "can_block is false outside DeclareBlockers"
        );
    }

    #[test]
    fn can_block_true_only_for_defending_player_at_declare_blockers() {
        use mecha_oracle::types::{CardObject, Step, Zone};
        let db = test_db();
        let config = vec![
            (0..10).map(|_| "Forest".to_string()).collect(),
            (0..10).map(|_| "Forest".to_string()).collect(),
        ];
        let mut gs = build_game_state(config, &db, false).unwrap();

        let p1_id = {
            let id = gs.alloc_id();
            let mut obj = CardObject::new(
                id,
                db.get("Grizzly Bears").unwrap().clone(),
                PlayerId(0),
                Zone::Battlefield,
            );
            obj.summoning_sick = false;
            gs.battlefield.push(id);
            gs.add_object(obj);
            id.0
        };
        let p2_id = {
            let id = gs.alloc_id();
            let mut obj = CardObject::new(
                id,
                db.get("Grizzly Bears").unwrap().clone(),
                PlayerId(1),
                Zone::Battlefield,
            );
            obj.summoning_sick = false;
            gs.battlefield.push(id);
            gs.add_object(obj);
            id.0
        };

        // 4 passes to reach DeclareAttackers
        for _ in 0..4 {
            gs = dispatch_action(gs, ActionRequest::AdvanceStep).unwrap();
        }
        // Declare P1's bear as attacker
        let gs = dispatch_action(
            gs,
            ActionRequest::DeclareAttackers {
                attacker_ids: vec![p1_id],
            },
        )
        .unwrap();
        // 2 passes to advance DeclareAttackers → DeclareBlockers
        let gs = dispatch_action(gs, ActionRequest::AdvanceStep).unwrap();
        let gs = dispatch_action(gs, ActionRequest::AdvanceStep).unwrap();
        assert_eq!(gs.step(), Step::DeclareBlockers);

        let view = build_game_view(&gs);
        let p1_c = view
            .p1
            .creatures
            .iter()
            .find(|c| c.id == ObjectId(p1_id))
            .unwrap();
        let p2_c = view
            .p2
            .creatures
            .iter()
            .find(|c| c.id == ObjectId(p2_id))
            .unwrap();

        assert!(
            !p1_c.can_block,
            "active player's creature does not show can_block"
        );
        assert!(
            p2_c.can_block,
            "defending player's creature shows can_block"
        );
        assert!(
            !p1_c.can_attack,
            "declared attacker (tapped) does not show can_attack"
        );
    }
}
