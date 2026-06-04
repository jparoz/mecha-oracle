use mecha_oracle::cards::CardDatabase;
use mecha_oracle::engine::turn::{advance_step, apply_step_start, draw_card};
use mecha_oracle::types::{CardObject, GameState, ObjectId, Player, PlayerId, Step, Zone};
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
            let seed = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .subsec_nanos() as u64;
            let lib = gs.libraries.get_mut(&pid).unwrap();
            let n = lib.len();
            let mut rng = seed.wrapping_add(player_idx as u64 * 6364136223846793005);
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
}
