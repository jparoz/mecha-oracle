use mecha_oracle::engine::turn::{advance_step, apply_step_start};
use mecha_oracle::types::{CardDefinition, CardObject, GameState, Player, PlayerId, Step, Zone};

fn main() {
    println!("=== mecha-oracle: MTG Rules Engine — Phase 1 Demo ===\n");

    let mut gs = build_game();
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

fn build_game() -> GameState {
    let mut gs = GameState::new(vec![
        Player::new(PlayerId(0), "Alice"),
        Player::new(PlayerId(1), "Bob"),
    ]);

    for &owner in &[PlayerId(0), PlayerId(1)] {
        for _ in 0..5 {
            let id = gs.alloc_id();
            let obj = CardObject::new(id, CardDefinition::forest(), owner, Zone::Library);
            gs.libraries.get_mut(&owner).unwrap().push(id);
            gs.add_object(obj);
        }
        for _ in 0..2 {
            let id = gs.alloc_id();
            let obj = CardObject::new(id, CardDefinition::grizzly_bears(), owner, Zone::Library);
            gs.libraries.get_mut(&owner).unwrap().push(id);
            gs.add_object(obj);
        }
    }

    gs
}
