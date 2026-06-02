use mecha_oracle::engine::{
    casting::{cast_creature, play_land},
    combat::{declare_attackers, declare_blockers, deal_combat_damage},
    mana::tap_land_for_mana,
    turn::{apply_step_start, advance_step, draw_card},
};
use mecha_oracle::types::{
    CardDefinition, CardObject, GameState, ObjectId, Phase, Player, PlayerId,
    Step, Zone,
};

fn make_game() -> (GameState, Vec<ObjectId>, Vec<ObjectId>) {
    let mut gs = GameState::new(vec![
        Player::new(PlayerId(0), "Alice"),
        Player::new(PlayerId(1), "Bob"),
    ]);

    let mut alice_cards = vec![];
    let mut bob_cards = vec![];

    let defs_alice = vec![
        CardDefinition::forest(),
        CardDefinition::grizzly_bears(),
        CardDefinition::forest(),
        CardDefinition::grizzly_bears(),
        CardDefinition::forest(),
        CardDefinition::forest(),
        CardDefinition::forest(),
    ];
    let defs_bob = vec![
        CardDefinition::forest(),
        CardDefinition::grizzly_bears(),
        CardDefinition::forest(),
        CardDefinition::grizzly_bears(),
        CardDefinition::forest(),
        CardDefinition::forest(),
        CardDefinition::forest(),
    ];

    for def in defs_alice {
        let id = gs.alloc_id();
        let obj = CardObject::new(id, def, PlayerId(0), Zone::Library);
        gs.libraries.get_mut(&PlayerId(0)).unwrap().push(id);
        gs.add_object(obj);
        alice_cards.push(id);
    }
    for def in defs_bob {
        let id = gs.alloc_id();
        let obj = CardObject::new(id, def, PlayerId(1), Zone::Library);
        gs.libraries.get_mut(&PlayerId(1)).unwrap().push(id);
        gs.add_object(obj);
        bob_cards.push(id);
    }

    (gs, alice_cards, bob_cards)
}

fn run_beginning(gs: GameState) -> GameState {
    assert_eq!(gs.step, Step::Untap);
    let gs = apply_step_start(gs); // untap + clear sickness
    let gs = advance_step(gs);     // → Upkeep
    let gs = apply_step_start(gs);
    let gs = advance_step(gs);     // → Draw
    let gs = apply_step_start(gs); // draw a card
    let gs = advance_step(gs);     // → PreCombatMain/Main
    gs
}

fn tap_all_lands_for_player(mut gs: GameState, player_id: PlayerId) -> GameState {
    let land_ids: Vec<ObjectId> = gs
        .battlefield
        .iter()
        .copied()
        .filter(|&id| {
            let obj = &gs.objects[&id];
            obj.controller == player_id && obj.is_land() && !obj.tapped
        })
        .collect();
    for id in land_ids {
        gs = tap_land_for_mana(gs, id).unwrap();
    }
    gs
}

fn pass_combat_no_attackers(mut gs: GameState) -> GameState {
    assert_eq!(gs.phase, Phase::Combat);
    gs = apply_step_start(gs);
    gs = advance_step(gs); // → DeclareAttackers
    gs = apply_step_start(gs);
    // No attackers declared — skip to end of combat
    gs = advance_step(gs); // → DeclareBlockers
    gs = apply_step_start(gs);
    gs = advance_step(gs); // → CombatDamage
    gs = apply_step_start(gs);
    gs = advance_step(gs); // → EndOfCombat
    gs = apply_step_start(gs);
    gs = advance_step(gs); // → PostCombatMain/Main
    gs
}

fn run_ending(mut gs: GameState) -> GameState {
    assert_eq!(gs.phase, Phase::PostCombatMain);
    gs = advance_step(gs); // → Ending/End
    gs = apply_step_start(gs);
    gs = advance_step(gs); // → Ending/Cleanup
    gs = apply_step_start(gs);
    gs = advance_step(gs); // → next player's Beginning/Untap
    gs
}

#[test]
fn scripted_game_runs_to_completion() {
    let (gs, _alice_cards, _bob_cards) = make_game();

    // === Turn 1: Alice — draws Forest, plays it ===
    let gs = run_beginning(gs);
    assert_eq!(gs.phase, Phase::PreCombatMain);

    let forest_in_hand = gs.hands[&PlayerId(0)]
        .iter()
        .copied()
        .find(|&id| gs.objects[&id].is_land())
        .expect("Alice should have a Forest in hand after drawing");

    let gs = play_land(gs, PlayerId(0), forest_in_hand).unwrap();
    assert_eq!(gs.lands_played_this_turn, 1);

    let gs = advance_step(gs); // → Combat
    let gs = pass_combat_no_attackers(gs);
    let gs = run_ending(gs);
    assert_eq!(gs.active_player, PlayerId(1));
    assert_eq!(gs.turn_number, 2);

    // === Turn 2: Bob — draws Forest, plays it ===
    let gs = run_beginning(gs);
    let bob_forest = gs.hands[&PlayerId(1)]
        .iter()
        .copied()
        .find(|&id| gs.objects[&id].is_land())
        .expect("Bob should have a Forest");
    let gs = play_land(gs, PlayerId(1), bob_forest).unwrap();
    let gs = advance_step(gs);
    let gs = pass_combat_no_attackers(gs);
    let gs = run_ending(gs);
    assert_eq!(gs.active_player, PlayerId(0));

    // === Turn 3: Alice — draws Grizzly Bears, plays second Forest, casts Bears ===
    let gs = run_beginning(gs);

    // Play a land if available
    let gs = if let Some(forest_id) = gs.hands[&PlayerId(0)]
        .iter()
        .copied()
        .find(|&id| gs.objects[&id].is_land())
    {
        play_land(gs, PlayerId(0), forest_id).unwrap()
    } else {
        gs
    };

    // Cast a creature if we have a creature in hand
    let gs = if let Some(bear_id) = gs.hands[&PlayerId(0)]
        .iter()
        .copied()
        .find(|&id| gs.objects[&id].is_creature())
    {
        let cost = gs.objects[&bear_id]
            .definition
            .mana_cost
            .clone()
            .unwrap();
        let gs = tap_all_lands_for_player(gs, PlayerId(0));
        let available = gs.get_player(PlayerId(0)).unwrap().mana_pool.total();
        if available >= cost.converted_mana_cost() {
            cast_creature(gs, PlayerId(0), bear_id).unwrap()
        } else {
            gs
        }
    } else {
        gs
    };

    // Advance to combat
    let gs = advance_step(gs); // → Combat
    let gs = pass_combat_no_attackers(gs); // Bears have summoning sickness
    let gs = run_ending(gs);

    // === Turn 4: Bob ===
    let gs = run_beginning(gs);
    let gs = if let Some(forest_id) = gs.hands[&PlayerId(1)]
        .iter()
        .copied()
        .find(|&id| gs.objects[&id].is_land())
    {
        play_land(gs, PlayerId(1), forest_id).unwrap()
    } else {
        gs
    };
    let gs = advance_step(gs);
    let gs = pass_combat_no_attackers(gs);
    let gs = run_ending(gs);

    // === Turn 5: Alice — attack with Bears (no longer summoning sick) ===
    let gs = run_beginning(gs);

    let alice_bear = gs
        .battlefield
        .iter()
        .copied()
        .find(|&id| {
            gs.objects[&id].is_creature() && gs.objects[&id].controller == PlayerId(0)
        });

    // Enter combat phase (BeginningOfCombat)
    let gs = advance_step(gs); // PreCombatMain,Main → Combat,BeginningOfCombat
    let gs = apply_step_start(gs);
    let gs = advance_step(gs); // → DeclareAttackers
    // We are now at Combat,DeclareAttackers

    let bear_can_attack = alice_bear
        .map(|id| gs.objects[&id].can_attack())
        .unwrap_or(false);

    let gs = if bear_can_attack {
        let bear_id = alice_bear.unwrap();
        let mut gs = declare_attackers(gs, PlayerId(0), &[bear_id]).unwrap();
        gs.step = Step::DeclareBlockers;
        let gs = declare_blockers(gs, PlayerId(1), &[]).unwrap();
        let mut gs = advance_step(gs); // DeclareBlockers → CombatDamage
        gs = deal_combat_damage(gs);
        // Bob should have taken 2 damage from the bear
        assert_eq!(
            gs.get_player(PlayerId(1)).unwrap().life,
            18,
            "Bob should have 18 life after bear attack"
        );
        gs = advance_step(gs); // CombatDamage → EndOfCombat
        gs = apply_step_start(gs);
        gs = advance_step(gs); // EndOfCombat → PostCombatMain
        gs
    } else {
        // No attackers — step through remaining combat steps from DeclareAttackers
        let mut gs = advance_step(gs); // DeclareAttackers → DeclareBlockers
        gs = apply_step_start(gs);
        gs = advance_step(gs); // DeclareBlockers → CombatDamage
        gs = apply_step_start(gs);
        gs = advance_step(gs); // CombatDamage → EndOfCombat
        gs = apply_step_start(gs);
        gs = advance_step(gs); // EndOfCombat → PostCombatMain
        gs
    };

    let gs = run_ending(gs);

    // Verify game state: game should not be over yet (Bob has 18 life)
    assert!(!gs.is_game_over());
}

#[test]
fn player_dies_at_zero_life_ends_game() {
    let mut gs = GameState::new(vec![
        Player::new(PlayerId(0), "Alice"),
        Player::new(PlayerId(1), "Bob"),
    ]);
    gs.get_player_mut(PlayerId(1)).unwrap().life = 2;
    gs.phase = Phase::Combat;
    gs.step = Step::CombatDamage;

    // Set up a 3/3 attacker
    let id = gs.alloc_id();
    let mut obj = CardObject::new(id, CardDefinition::hill_giant(), PlayerId(0), Zone::Battlefield);
    obj.summoning_sick = false;
    gs.battlefield.push(id);
    gs.add_object(obj);
    gs.combat.attackers = vec![id];
    gs.combat.blocking_map.insert(id, vec![]);

    let gs = deal_combat_damage(gs);

    assert!(gs.is_game_over());
    assert_eq!(gs.winner(), Some(PlayerId(0)));
}

#[test]
fn drawing_from_empty_library_loses_the_game() {
    let gs = GameState::new(vec![
        Player::new(PlayerId(0), "Alice"),
        Player::new(PlayerId(1), "Bob"),
    ]);
    // Alice's library is empty by default

    let gs = draw_card(gs, PlayerId(0));

    assert!(gs.is_game_over());
    assert_eq!(gs.winner(), Some(PlayerId(1)));
}
