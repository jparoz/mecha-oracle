mod serve;

use clap::{Parser, Subcommand};
use directories::ProjectDirs;
use mecha_oracle::cards::{CardDatabase, update_cards};
use mecha_oracle::engine::turn::{advance_step, apply_step_start};
use mecha_oracle::types::{
    CardDefinition, CardObject, CostComponent, EffectStep, GameState, Player, PlayerId, Rule,
    RulesText, Step, Zone,
};

/// Top-level CLI parsed by clap.
#[derive(Parser)]
#[command(name = "mecha-oracle", about = "MTG Rules Engine")]
struct Cli {
    #[arg(short, long, global = true, help = "Show per-card parse warnings")]
    verbose: bool,
    #[command(subcommand)]
    command: Command,
}

/// Available subcommands.
#[derive(Subcommand)]
enum Command {
    /// Run the headless turn-loop demo (Forest + Grizzly Bears vs. itself).
    Demo,
    /// Start the Axum HTTP server with a deck loaded from `deck` (a JSON path).
    Serve {
        /// Shuffle each player's library on game start.
        #[arg(long)]
        shuffle: bool,
        /// Path to the deck config JSON file (array of two string arrays).
        deck: String,
    },
    /// Download/refresh the Scryfall oracle-cards bulk data file.
    UpdateCards,
    /// Parse every card in the database and print a coverage report.
    ParseCoverage,
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
        Command::ParseCoverage => run_parse_coverage(),
    }
}

/// Resolves the platform data directory and delegates to [`update_cards`].
fn run_update_cards() {
    let dirs =
        ProjectDirs::from("", "", "mecha-oracle").expect("Cannot determine user data directory");
    std::fs::create_dir_all(dirs.data_dir()).expect("Cannot create data directory");
    update_cards(dirs.data_dir()).expect("Card update failed");
}

/// Returns true if `card` contains any unimplemented signal: an Unparsed span,
/// a ParsedUnimplemented span, or an Active span whose cost/effect/steps include
/// any Unimplemented components.
fn card_has_unimplemented(card: &CardDefinition) -> bool {
    card.rules_text.iter().any(|span| match span {
        RulesText::Unparsed(_) => true,
        RulesText::ParsedUnimplemented(_) => true,
        RulesText::Active(Rule::Activated(ab)) => {
            ab.cost
                .iter()
                .any(|c| matches!(c, CostComponent::Unimplemented(_)))
                || ab
                    .effect
                    .iter()
                    .any(|s| matches!(s, EffectStep::Unimplemented(_)))
        }
        RulesText::Active(Rule::SpellAbility(sa)) => sa
            .steps
            .iter()
            .any(|s| matches!(s, EffectStep::Unimplemented(_))),
        RulesText::Active(Rule::Triggered(ta)) => ta
            .effect
            .iter()
            .any(|s| matches!(s, EffectStep::Unimplemented(_))),
        _ => false,
    })
}

/// Parses every card in the database and prints a coverage breakdown.
fn run_parse_coverage() {
    use std::collections::HashMap;

    let db = CardDatabase::open()
        .expect("Card database not found — run `mecha-oracle update-cards` first");

    let mut clean = 0usize;
    let mut partial = 0usize;
    let mut opaque = 0usize;

    let mut effect_step_counts: HashMap<String, usize> = HashMap::new();
    let mut keyword_counts: HashMap<String, usize> = HashMap::new();
    let mut cost_counts: HashMap<String, usize> = HashMap::new();
    let mut unparsed_counts: HashMap<String, usize> = HashMap::new();

    for card in db.iter() {
        let has_active = card
            .rules_text
            .iter()
            .any(|s| matches!(s, RulesText::Active(_)));
        let has_unimpl = card_has_unimplemented(card);

        if !has_unimpl {
            clean += 1;
        } else if !has_active {
            opaque += 1;
        } else {
            partial += 1;
        }

        // Collect breakdown data
        for span in &card.rules_text {
            match span {
                RulesText::Unparsed(s) => {
                    *unparsed_counts.entry(s.trim().to_lowercase()).or_insert(0) += 1;
                }
                RulesText::ParsedUnimplemented(s) => {
                    let key = s.split_whitespace().next().unwrap_or(s).to_lowercase();
                    *keyword_counts.entry(key).or_insert(0) += 1;
                }
                RulesText::Active(Rule::Activated(ab)) => {
                    for c in &ab.cost {
                        if let CostComponent::Unimplemented(s) = c {
                            *cost_counts.entry(s.to_lowercase()).or_insert(0) += 1;
                        }
                    }
                    for s in &ab.effect {
                        if let EffectStep::Unimplemented(text) = s {
                            *effect_step_counts.entry(text.to_lowercase()).or_insert(0) += 1;
                        }
                    }
                }
                RulesText::Active(Rule::SpellAbility(sa)) => {
                    for s in &sa.steps {
                        if let EffectStep::Unimplemented(text) = s {
                            *effect_step_counts.entry(text.to_lowercase()).or_insert(0) += 1;
                        }
                    }
                }
                RulesText::Active(Rule::Triggered(ta)) => {
                    for s in &ta.effect {
                        if let EffectStep::Unimplemented(text) = s {
                            *effect_step_counts.entry(text.to_lowercase()).or_insert(0) += 1;
                        }
                    }
                }
                _ => {}
            }
        }
    }

    let total = clean + partial + opaque;
    let pct = |n: usize| {
        if total > 0 {
            n as f64 * 100.0 / total as f64
        } else {
            0.0
        }
    };

    println!("=== Parse Coverage Report ===");
    println!("Total cards:          {total:>7}");
    println!();
    println!("Clean (fully parsed): {clean:>7}  ({:.1}%)", pct(clean));
    println!("Partial:              {partial:>7}  ({:.1}%)", pct(partial));
    println!("Opaque (no Active):   {opaque:>7}  ({:.1}%)", pct(opaque));

    print_top_15("Top unimplemented effect steps", &effect_step_counts, 15);
    print_top_15("Top unimplemented keywords", &keyword_counts, 15);
    print_top_15("Top unimplemented activation costs", &cost_counts, 15);
    print_top_15("Top unparsed paragraphs", &unparsed_counts, 10);
}

fn print_top_15(header: &str, counts: &std::collections::HashMap<String, usize>, n: usize) {
    if counts.is_empty() {
        return;
    }
    let mut entries: Vec<_> = counts.iter().collect();
    entries.sort_by(|a, b| b.1.cmp(a.1).then(a.0.cmp(b.0)));
    println!();
    println!("=== {header} (top {n}) ===");
    for (i, (text, count)) in entries.iter().take(n).enumerate() {
        println!(
            "  {:>2}. {:.<60}  × {:>6}",
            i + 1,
            format!("\"{text}\""),
            count
        );
    }
}

/// Runs a headless turn-loop demo: two players each with 5 Forests and 2 Grizzly Bears,
/// no decision-making, advancing steps automatically until the game ends or 200 steps pass.
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

/// Builds the demo `GameState`: Alice and Bob each receive 5 Forests and 2 Grizzly Bears
/// placed in their libraries (no shuffle, no opening-hand draw — `run_demo` advances manually).
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

    #[test]
    fn parse_coverage_classification_clean_card() {
        use mecha_oracle::types::RulesText;
        // Use the test fixture database (does not require a production database download).
        // Serra Angel is in tests/fixtures/oracle_cards_test.json and has only implemented keywords.
        let db = CardDatabase::from_path(std::path::Path::new(
            "tests/fixtures/oracle_cards_test.json",
        ))
        .expect("test fixture not found");
        let card = db
            .get("Serra Angel")
            .expect("Serra Angel not found in test fixture");
        let has_active = card
            .rules_text
            .iter()
            .any(|s| matches!(s, RulesText::Active(_)));
        let has_unimpl = card_has_unimplemented(card);
        assert!(has_active, "Serra Angel should have Active spans");
        assert!(
            !has_unimpl,
            "Serra Angel should have no unimplemented signals"
        );
    }

    #[test]
    fn cli_parse_coverage_subcommand() {
        let cli = Cli::try_parse_from(["mecha-oracle", "parse-coverage"]).unwrap();
        assert!(matches!(cli.command, Command::ParseCoverage));
    }
}
