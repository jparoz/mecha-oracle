use mecha_oracle::cards::CardDatabase;
use mecha_oracle::engine::EngineError;
use mecha_oracle::engine::casting::cast_spell;
use mecha_oracle::engine::stack::resolve_top;
use mecha_oracle::engine::turn::skip_to_first_main;
use mecha_oracle::types::effect::EffectTarget;
use mecha_oracle::types::{
    CardObject, GameState, ObjectId, PermanentState, Player, PlayerId, Zone,
};
use std::path::Path;

fn card_db() -> CardDatabase {
    CardDatabase::from_path(Path::new("tests/fixtures/oracle_cards_test.json")).unwrap()
}

/// Build a minimal two-player GameState at the first main phase (PreCombatMain).
/// Uses skip_to_first_main (CR 103.8a: starting player skips first draw).
fn make_state() -> GameState {
    let gs = GameState::new(vec![
        Player::new(PlayerId(0), "Alice"),
        Player::new(PlayerId(1), "Bob"),
    ]);
    skip_to_first_main(gs)
}

/// Put a card definition into a player's hand and return the ObjectId.
fn put_in_hand(
    state: &mut GameState,
    owner: PlayerId,
    def: mecha_oracle::types::CardDefinition,
) -> ObjectId {
    let id = state.alloc_id();
    let obj = CardObject::new(id, def, owner, Zone::Hand);
    state.hands.get_mut(&owner).unwrap().push(id);
    state.add_object(obj);
    id
}

/// Place a creature on the battlefield and return the ObjectId.
fn place_creature(
    state: &mut GameState,
    owner: PlayerId,
    def: mecha_oracle::types::CardDefinition,
) -> ObjectId {
    let id = state.alloc_id();
    let obj = CardObject::new(id, def, owner, Zone::Battlefield);
    let mut perm = PermanentState::new(&obj.definition);
    perm.controller_since_turn = 0;
    state.battlefield.insert(id, perm);
    state.add_object(obj);
    id
}

/// Tap a Mountain (or Forest) for mana using the tap_land_for_mana engine function.
fn tap_land_for_mana(state: GameState, land_id: ObjectId) -> GameState {
    mecha_oracle::engine::mana::tap_land_for_mana(state, land_id).unwrap()
}

// CR 601.2a, 608.2b: Giant Growth targets a creature; on resolution the
// creature gets +3/+3 until end of turn via BoostPermanentPT.
#[test]
fn giant_growth_cast_and_resolve_boosts_creature() {
    let db = card_db();
    let mut gs = make_state();

    // Player 0 has a Forest and Giant Growth in hand.
    let forest_id = put_in_hand(&mut gs, PlayerId(0), db.get("Forest").unwrap().clone());
    let gg_id = put_in_hand(
        &mut gs,
        PlayerId(0),
        db.get("Giant Growth").unwrap().clone(),
    );

    // Player 1 has a 2/2 creature on the battlefield (Grizzly Bears).
    let bears_id = place_creature(
        &mut gs,
        PlayerId(1),
        db.get("Grizzly Bears").unwrap().clone(),
    );

    // Put the Forest onto the battlefield first, then tap it for {G}.
    gs = mecha_oracle::engine::casting::play_land(gs, PlayerId(0), forest_id).unwrap();
    gs = tap_land_for_mana(gs, forest_id);

    // Cast Giant Growth targeting player 1's creature. Giant Growth is an instant
    // so it can be cast at any time we have priority.
    gs = cast_spell(
        gs,
        PlayerId(0),
        gg_id,
        vec![EffectTarget::Object { id: bears_id }],
        None,
    )
    .unwrap();

    // Resolve (both players pass in succession internally via resolve_top).
    gs = resolve_top(gs);

    // CR 613.1b: +3/+3 until end of turn — creature is now 5/5.
    assert_eq!(
        gs.battlefield[&bears_id].effective_power(0),
        Some(5),
        "Grizzly Bears should be 5/5 after Giant Growth"
    );
    assert_eq!(
        gs.battlefield[&bears_id].effective_toughness(0),
        Some(5),
        "Grizzly Bears should be 5/5 after Giant Growth"
    );
    // Giant Growth should be in player 0's graveyard.
    assert!(gs.graveyards[&PlayerId(0)].contains(&gg_id));
}

// CR 704.5h: a creature with damage equal to or greater than its toughness is
// destroyed by state-based actions. Lightning Bolt deals 3 damage to a 2/2 →
// SBAs run automatically inside resolve_top, killing the creature.
#[test]
fn lightning_bolt_kills_creature() {
    let db = card_db();
    let mut gs = make_state();

    // Player 0 has a Mountain and Lightning Bolt in hand.
    let mountain_id = put_in_hand(&mut gs, PlayerId(0), db.get("Mountain").unwrap().clone());
    let bolt_id = put_in_hand(
        &mut gs,
        PlayerId(0),
        db.get("Lightning Bolt").unwrap().clone(),
    );

    // Player 1 has a 2/2 creature on the battlefield.
    let bears_id = place_creature(
        &mut gs,
        PlayerId(1),
        db.get("Grizzly Bears").unwrap().clone(),
    );

    // Play the Mountain, then tap it for {R}.
    gs = mecha_oracle::engine::casting::play_land(gs, PlayerId(0), mountain_id).unwrap();
    gs = tap_land_for_mana(gs, mountain_id);

    // Cast Lightning Bolt targeting player 1's creature.
    gs = cast_spell(
        gs,
        PlayerId(0),
        bolt_id,
        vec![EffectTarget::Object { id: bears_id }],
        None,
    )
    .unwrap();

    // Resolve — check_and_apply_sbas runs inside resolve_top.
    gs = resolve_top(gs);

    // 2/2 dealt 3 damage → lethal → destroyed by SBA.
    assert!(
        !gs.battlefield.contains_key(&bears_id),
        "Grizzly Bears should be dead after Lightning Bolt"
    );
    // Creature should be in player 1's graveyard (owner is Bob).
    assert!(gs.graveyards[&PlayerId(1)].contains(&bears_id));
}

// CR 120.3a: damage reduces a player's life total by that amount.
// CR 608.2b: "any target" allows players as targets.
#[test]
fn lightning_bolt_damages_player() {
    let db = card_db();
    let mut gs = make_state();

    // Player 0 has a Mountain and Lightning Bolt in hand.
    let mountain_id = put_in_hand(&mut gs, PlayerId(0), db.get("Mountain").unwrap().clone());
    let bolt_id = put_in_hand(
        &mut gs,
        PlayerId(0),
        db.get("Lightning Bolt").unwrap().clone(),
    );

    // Play Mountain, tap for {R}.
    gs = mecha_oracle::engine::casting::play_land(gs, PlayerId(0), mountain_id).unwrap();
    gs = tap_land_for_mana(gs, mountain_id);

    // Cast Lightning Bolt targeting player 1 directly.
    gs = cast_spell(
        gs,
        PlayerId(0),
        bolt_id,
        vec![EffectTarget::Player { id: PlayerId(1) }],
        None,
    )
    .unwrap();

    gs = resolve_top(gs);

    // Player 1 started at 20, took 3 damage → 17.
    assert_eq!(
        gs.get_player(PlayerId(1)).unwrap().life,
        17,
        "Player 1 should have 17 life after Lightning Bolt"
    );
}

// CR 608.2b: if all targets of a spell are illegal at the time it would resolve,
// the spell is countered by the rules and has no effect. The spell still moves
// to the graveyard.
#[test]
fn giant_growth_fizzles_when_target_leaves() {
    let db = card_db();
    let mut gs = make_state();

    // Player 0 has a Forest and Giant Growth in hand.
    let forest_id = put_in_hand(&mut gs, PlayerId(0), db.get("Forest").unwrap().clone());
    let gg_id = put_in_hand(
        &mut gs,
        PlayerId(0),
        db.get("Giant Growth").unwrap().clone(),
    );

    // Player 1 has a creature on the battlefield.
    let bears_id = place_creature(
        &mut gs,
        PlayerId(1),
        db.get("Grizzly Bears").unwrap().clone(),
    );

    // Play Forest and tap it for {G}.
    gs = mecha_oracle::engine::casting::play_land(gs, PlayerId(0), forest_id).unwrap();
    gs = tap_land_for_mana(gs, forest_id);

    // Cast Giant Growth targeting the creature.
    gs = cast_spell(
        gs,
        PlayerId(0),
        gg_id,
        vec![EffectTarget::Object { id: bears_id }],
        None,
    )
    .unwrap();

    // Before resolution: remove the target creature from the battlefield (opponent
    // responded with a removal spell effect; we simulate by directly manipulating state).
    gs.battlefield.remove(&bears_id);
    gs.objects.get_mut(&bears_id).unwrap().zone = Zone::Graveyard;
    gs.graveyards.get_mut(&PlayerId(1)).unwrap().push(bears_id);

    // Resolve — target is now illegal so the spell fizzles (CR 608.2b).
    gs = resolve_top(gs);

    // Giant Growth should be in the graveyard; creature is still gone.
    assert!(
        gs.graveyards[&PlayerId(0)].contains(&gg_id),
        "Fizzled Giant Growth should be in player 0's graveyard"
    );
    assert!(
        !gs.battlefield.contains_key(&bears_id),
        "Target creature should still be off the battlefield"
    );
}

// CR 702.18a: Shroud prevents a permanent from being the target of any spell or ability.
// CR 601.2c: if a declared target is not legal, the spell cannot be cast.
#[test]
fn cant_cast_giant_growth_targeting_shroud_creature() {
    use mecha_oracle::types::ability::StaticAbility;
    use mecha_oracle::types::card::{CardType, TypeLine};
    use mecha_oracle::types::{CardDefinition, Rule, RulesText};

    let db = card_db();
    let mut gs = make_state();

    // Player 0 has a Forest and Giant Growth in hand.
    let forest_id = put_in_hand(&mut gs, PlayerId(0), db.get("Forest").unwrap().clone());
    let gg_id = put_in_hand(
        &mut gs,
        PlayerId(0),
        db.get("Giant Growth").unwrap().clone(),
    );

    // Construct a creature with Shroud for player 1.
    let shroud_creature_def = CardDefinition {
        name: "Invisible Stalker".into(),
        mana_cost: None,
        type_line: TypeLine {
            supertypes: vec![],
            card_types: vec![CardType::Creature],
            subtypes: vec![],
        },
        oracle_text: "Shroud".into(),
        rules_text: vec![RulesText::Active(Rule::Static(StaticAbility::Shroud))],
        text_annotations: vec![],
        power: Some(1),
        toughness: Some(1),
        colors: vec![],
    };
    let shroud_id = place_creature(&mut gs, PlayerId(1), shroud_creature_def);

    // Play Forest and tap it for {G}.
    gs = mecha_oracle::engine::casting::play_land(gs, PlayerId(0), forest_id).unwrap();
    gs = tap_land_for_mana(gs, forest_id);

    // Attempt to cast Giant Growth targeting the Shroud creature — must fail.
    let result = cast_spell(
        gs,
        PlayerId(0),
        gg_id,
        vec![EffectTarget::Object { id: shroud_id }],
        None,
    );

    assert!(
        matches!(result, Err(EngineError::IllegalTarget)),
        "Casting a spell targeting a Shroud creature should return IllegalTarget"
    );
}
