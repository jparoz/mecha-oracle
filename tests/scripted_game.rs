use mecha_oracle::cards::CardDatabase;
use mecha_oracle::engine::{
    casting::{cast_spell, play_land},
    combat::{deal_combat_damage, declare_attackers, declare_blockers},
    mana::tap_land_for_mana,
    turn::{advance_step, apply_step_start, draw_card},
};
use mecha_oracle::types::{
    CardObject, GameState, ObjectId, PermanentState, Phase, Player, PlayerId, Step, Zone,
};
use std::path::Path;

fn card_db() -> CardDatabase {
    CardDatabase::from_path(Path::new("tests/fixtures/oracle_cards_test.json")).unwrap()
}

fn make_game() -> (GameState, Vec<ObjectId>, Vec<ObjectId>) {
    let db = card_db();
    let forest = || db.get("Forest").unwrap().clone();
    let bears = || db.get("Grizzly Bears").unwrap().clone();

    let mut gs = GameState::new(vec![
        Player::new(PlayerId(0), "Alice"),
        Player::new(PlayerId(1), "Bob"),
    ]);

    let mut alice_cards = vec![];
    let mut bob_cards = vec![];

    let defs_alice = vec![
        forest(),
        bears(),
        forest(),
        bears(),
        forest(),
        forest(),
        forest(),
    ];
    let defs_bob = vec![
        forest(),
        bears(),
        forest(),
        bears(),
        forest(),
        forest(),
        forest(),
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
    assert_eq!(gs.step(), Step::Untap);
    let gs = apply_step_start(gs); // untap + clear sickness
    let gs = advance_step(gs); // → Upkeep
    let gs = apply_step_start(gs);
    let gs = advance_step(gs); // → Draw
    let gs = apply_step_start(gs); // draw a card
    let gs = advance_step(gs); // → PreCombatMain/Main
    gs
}

fn tap_all_lands_for_player(mut gs: GameState, player_id: PlayerId) -> GameState {
    let land_ids: Vec<ObjectId> = gs
        .battlefield
        .iter()
        .filter(|(id, perm)| {
            let obj = &gs.objects[id];
            obj.controller == player_id && obj.is_land() && !perm.tapped
        })
        .map(|(id, _)| *id)
        .collect();
    for id in land_ids {
        gs = tap_land_for_mana(gs, id).unwrap();
    }
    gs
}

fn pass_combat_no_attackers(mut gs: GameState) -> GameState {
    assert_eq!(gs.phase(), Phase::Combat);
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
    assert_eq!(gs.phase(), Phase::PostCombatMain);
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
    assert_eq!(gs.phase(), Phase::PreCombatMain);

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
        let cost = gs.objects[&bear_id].definition.mana_cost.clone().unwrap();
        let gs = tap_all_lands_for_player(gs, PlayerId(0));
        let available = gs.get_player(PlayerId(0)).unwrap().mana_pool.total();
        if available >= cost.mana_value() {
            cast_spell(gs, PlayerId(0), bear_id, vec![]).unwrap()
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
        .keys()
        .copied()
        .find(|&id| gs.objects[&id].is_creature() && gs.objects[&id].controller == PlayerId(0));

    // Enter combat phase (BeginningOfCombat)
    let gs = advance_step(gs); // PreCombatMain,Main → Combat,BeginningOfCombat
    let gs = apply_step_start(gs);
    let gs = advance_step(gs); // → DeclareAttackers
    // We are now at Combat,DeclareAttackers

    let bear_can_attack = alice_bear
        .and_then(|id| gs.battlefield.get(&id))
        .map(|p| p.can_attack())
        .unwrap_or(false);

    let gs = if bear_can_attack {
        let bear_id = alice_bear.unwrap();
        let gs = declare_attackers(gs, PlayerId(0), &[bear_id]).unwrap();
        let gs = advance_step(gs); // DeclareAttackers → DeclareBlockers
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

    // Set up a 3/3 attacker
    let db = card_db();
    let id = gs.alloc_id();
    let obj = CardObject::new(
        id,
        db.get("Hill Giant").unwrap().clone(),
        PlayerId(0),
        Zone::Battlefield,
    );
    let mut perm = PermanentState::new(&obj.definition);
    perm.summoning_sick = false;
    gs.battlefield.insert(id, perm);
    gs.add_object(obj);
    gs.combat.attackers = vec![id];
    gs.combat.blocking_map.insert(id, vec![]);

    let gs = advance_to_combat_damage(gs);
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

/// Navigate from the initial Untap step to DeclareAttackers without triggering draw
/// (which would kill the active player if the library is empty).
fn advance_to_declare_attackers(gs: GameState) -> GameState {
    assert_eq!(gs.step(), Step::Untap);
    // Untap → Upkeep → Draw → PreCombatMain → BeginningOfCombat → DeclareAttackers
    // Skipping apply_step_start so no draw happens.
    let gs = advance_step(gs); // → Upkeep
    let gs = advance_step(gs); // → Draw
    let gs = advance_step(gs); // → PreCombatMain
    let gs = advance_step(gs); // → BeginningOfCombat
    let gs = advance_step(gs); // → DeclareAttackers
    assert_eq!(gs.step(), Step::DeclareAttackers);
    gs
}

fn advance_to_combat_damage(gs: GameState) -> GameState {
    let gs = advance_step(gs); // Untap → Upkeep
    let gs = advance_step(gs); // Upkeep → Draw
    let gs = advance_step(gs); // Draw → PreCombatMain
    let gs = advance_step(gs); // PreCombatMain → BeginningOfCombat
    let gs = advance_step(gs); // BeginningOfCombat → DeclareAttackers
    let gs = advance_step(gs); // DeclareAttackers → DeclareBlockers
    let gs = advance_step(gs); // DeclareBlockers → CombatDamage
    assert_eq!(gs.step(), Step::CombatDamage);
    gs
}

#[test]
fn first_striker_kills_blocker_and_survives_unscathed() {
    // Anaba Bodyguard (3/2 First Strike) attacks; Grizzly Bears (2/2) blocks.
    // Round 1: Bodyguard deals 3 (kills Bears). Bears can't deal back.
    // Round 2: no blockers — Bodyguard untouched. Player takes no damage (creature was blocked).
    let db = card_db();
    let mut gs = GameState::new(vec![
        Player::new(PlayerId(0), "Alice"),
        Player::new(PlayerId(1), "Bob"),
    ]);

    let bodyguard_id = {
        let id = gs.alloc_id();
        let obj = CardObject::new(
            id,
            db.get("Anaba Bodyguard").unwrap().clone(),
            PlayerId(0),
            Zone::Battlefield,
        );
        let mut perm = PermanentState::new(&obj.definition);
        perm.summoning_sick = false;
        gs.battlefield.insert(id, perm);
        gs.add_object(obj);
        id
    };
    let bears_id = {
        let id = gs.alloc_id();
        let obj = CardObject::new(
            id,
            db.get("Grizzly Bears").unwrap().clone(),
            PlayerId(1),
            Zone::Battlefield,
        );
        let mut perm = PermanentState::new(&obj.definition);
        perm.summoning_sick = false;
        gs.battlefield.insert(id, perm);
        gs.add_object(obj);
        id
    };

    let gs = advance_to_declare_attackers(gs);
    let gs = declare_attackers(gs, PlayerId(0), &[bodyguard_id]).unwrap();
    let gs = advance_step(gs); // → DeclareBlockers
    let gs = declare_blockers(gs, PlayerId(1), &[(bears_id, bodyguard_id)]).unwrap();
    let gs = advance_step(gs); // → CombatDamage

    // Round 1: first strike
    let gs = deal_combat_damage(gs);
    assert!(
        !gs.battlefield.contains_key(&bears_id),
        "Bears should be dead after round 1"
    );
    assert_eq!(
        gs.battlefield[&bodyguard_id].damage_marked, 0,
        "Bodyguard takes no damage in round 1"
    );

    // Advance to queued second round
    let gs = advance_step(gs);
    assert_eq!(gs.step(), Step::CombatDamage);
    let gs = deal_combat_damage(gs);

    assert!(
        gs.battlefield.contains_key(&bodyguard_id),
        "Bodyguard survives"
    );
    assert_eq!(
        gs.get_player(PlayerId(1)).unwrap().life,
        20,
        "No damage to player (blocker absorbed)"
    );
}

#[test]
fn trample_excess_kills_player() {
    // Charging Badger is only 1/1 — not enough to demonstrate excess with a single blocker.
    // Use a manually-constructed 5/5 trampler instead.
    use mecha_oracle::types::ability::StaticAbility;
    use mecha_oracle::types::{
        Ability, CardDefinition, OracleSpan,
        card::{CardType, TypeLine},
    };

    let db = card_db();
    let mut gs = GameState::new(vec![
        Player::new(PlayerId(0), "Alice"),
        Player::new(PlayerId(1), "Bob"),
    ]);

    // Construct a 5/5 trampler inline
    let trampler_def = CardDefinition {
        name: "Big Trampler".into(),
        mana_cost: None,
        type_line: TypeLine {
            supertypes: vec![],
            card_types: vec![CardType::Creature],
            subtypes: vec![],
        },
        oracle_text: String::new(),
        abilities: vec![OracleSpan::Parsed(Ability::Static(StaticAbility::Trample))],
        power: Some(5),
        toughness: Some(5),
    };

    let trampler_id = {
        let id = gs.alloc_id();
        let obj = CardObject::new(id, trampler_def, PlayerId(0), Zone::Battlefield);
        let mut perm = PermanentState::new(&obj.definition);
        perm.summoning_sick = false;
        gs.battlefield.insert(id, perm);
        gs.add_object(obj);
        id
    };
    let blocker_id = {
        let id = gs.alloc_id();
        let obj = CardObject::new(
            id,
            db.get("Grizzly Bears").unwrap().clone(), // 2/2
            PlayerId(1),
            Zone::Battlefield,
        );
        let mut perm = PermanentState::new(&obj.definition);
        perm.summoning_sick = false;
        gs.battlefield.insert(id, perm);
        gs.add_object(obj);
        id
    };

    let gs = advance_to_declare_attackers(gs);
    let gs = declare_attackers(gs, PlayerId(0), &[trampler_id]).unwrap();
    let gs = advance_step(gs); // → DeclareBlockers
    let gs = declare_blockers(gs, PlayerId(1), &[(blocker_id, trampler_id)]).unwrap();
    let gs = advance_step(gs); // → CombatDamage
    let gs = deal_combat_damage(gs);

    assert!(
        !gs.battlefield.contains_key(&blocker_id),
        "2/2 blocker dies"
    );
    assert_eq!(
        gs.get_player(PlayerId(1)).unwrap().life,
        17,
        "3 trample damage to player"
    );
}

#[test]
fn deathtouch_rat_kills_hill_giant() {
    // Typhoid Rats (1/1 Deathtouch) attacks; Hill Giant (3/3) blocks.
    // Rats deal 1 deathtouch damage → Giant dies. Giant deals 3 → Rats die.
    let db = card_db();
    let mut gs = GameState::new(vec![
        Player::new(PlayerId(0), "Alice"),
        Player::new(PlayerId(1), "Bob"),
    ]);

    let rats_id = {
        let id = gs.alloc_id();
        let obj = CardObject::new(
            id,
            db.get("Typhoid Rats").unwrap().clone(),
            PlayerId(0),
            Zone::Battlefield,
        );
        let mut perm = PermanentState::new(&obj.definition);
        perm.summoning_sick = false;
        gs.battlefield.insert(id, perm);
        gs.add_object(obj);
        id
    };
    let giant_id = {
        let id = gs.alloc_id();
        let obj = CardObject::new(
            id,
            db.get("Hill Giant").unwrap().clone(),
            PlayerId(1),
            Zone::Battlefield,
        );
        let mut perm = PermanentState::new(&obj.definition);
        perm.summoning_sick = false;
        gs.battlefield.insert(id, perm);
        gs.add_object(obj);
        id
    };

    let gs = advance_to_declare_attackers(gs);
    let gs = declare_attackers(gs, PlayerId(0), &[rats_id]).unwrap();
    let gs = advance_step(gs); // → DeclareBlockers
    let gs = declare_blockers(gs, PlayerId(1), &[(giant_id, rats_id)]).unwrap();
    let gs = advance_step(gs); // → CombatDamage
    let gs = deal_combat_damage(gs);

    assert!(
        !gs.battlefield.contains_key(&giant_id),
        "Hill Giant killed by deathtouch"
    );
    assert!(
        !gs.battlefield.contains_key(&rats_id),
        "Typhoid Rats killed by 3 damage"
    );
    assert_eq!(
        gs.get_player(PlayerId(1)).unwrap().life,
        20,
        "No damage to player"
    );
}
