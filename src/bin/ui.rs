use mecha_oracle::cards::CardDatabase;
use mecha_oracle::engine::turn::{apply_step_start, draw_card};
use mecha_oracle::types::{CardObject, GameState, Player, PlayerId, Step, Zone};
use serde::Serialize;
use std::path::Path;

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

// ── Game init ────────────────────────────────────────────────────────────────

fn init_game(path: &str, shuffle: bool) -> Result<GameState, String> {
    let db = CardDatabase::open().map_err(|e| format!("Card database error: {e}"))?;
    let config = load_config(path)?;
    build_game_state(config, &db, shuffle)
}

fn main() {
    println!("todo");
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
}
