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
    let dirs =
        ProjectDirs::from("", "", "mecha-oracle").expect("Cannot determine user data directory");
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
        let cli = Cli::try_parse_from(["mecha-oracle", "serve", "--shuffle", "deck.json"]).unwrap();
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
