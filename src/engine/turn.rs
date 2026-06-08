use super::combat::deal_combat_damage;
use super::state_based_actions::move_to_graveyard;
use crate::types::ability::StaticAbility;
use crate::types::{CombatState, GameState, ObjectId, PlayerId, Step, Zone};

/// Apply the automatic rules for the start of the current step/phase.
pub fn apply_step_start(state: GameState) -> GameState {
    match state.step {
        Step::Untap => untap_step(state),
        Step::Draw => draw_step(state),
        Step::Cleanup => cleanup_step(state),
        Step::CombatDamage => deal_combat_damage(state),
        Step::EndOfCombat => end_of_combat_step(state),
        _ => state,
    }
}

/// CR 702.147a: at the end of combat, sacrifice any attacking creatures with Decayed.
fn end_of_combat_step(mut state: GameState) -> GameState {
    let to_sacrifice: Vec<ObjectId> = state
        .combat
        .attackers
        .iter()
        .filter(|&&id| {
            state
                .objects
                .get(&id)
                .map(|o| o.has_keyword(StaticAbility::Decayed) && o.zone == Zone::Battlefield)
                .unwrap_or(false)
        })
        .copied()
        .collect();
    for id in to_sacrifice {
        state = move_to_graveyard(state, id);
    }
    state
}

/// Advance to the next step/phase. Checks `extra_steps` queue first (for dynamically
/// inserted steps such as the second combat damage round per CR 510.4).
pub fn advance_step(mut state: GameState) -> GameState {
    // CR 106.4: mana pools empty at end of each step and phase.
    for player in state.players.iter_mut() {
        player.mana_pool = Default::default();
    }
    // Passing priority commits mana choices.
    state.mana_checkpoint = None;
    state.priority_player = state.active_player;
    if let Some(next) = state.extra_steps.pop_front() {
        state.step = next;
        return state;
    }
    match state.step {
        Step::Untap => set(state, Step::Upkeep),
        Step::Upkeep => set(state, Step::Draw),
        Step::Draw => set(state, Step::PreCombatMain),
        Step::PreCombatMain => set(state, Step::BeginningOfCombat),
        Step::BeginningOfCombat => set(state, Step::DeclareAttackers),
        Step::DeclareAttackers => set(state, Step::DeclareBlockers),
        Step::DeclareBlockers => set(state, Step::CombatDamage),
        Step::CombatDamage => set(state, Step::EndOfCombat),
        Step::EndOfCombat => {
            let mut s = set(state, Step::PostCombatMain);
            s.combat = CombatState::empty();
            s
        }
        Step::PostCombatMain => set(state, Step::End),
        Step::End => set(state, Step::Cleanup),
        Step::Cleanup => start_next_turn(state),
    }
}

fn set(mut state: GameState, step: Step) -> GameState {
    state.step = step;
    state
}

fn untap_step(mut state: GameState) -> GameState {
    let active = state.active_player;
    // CR 502: untap all permanents the active player controls; clear summoning sickness.
    let to_untap: Vec<ObjectId> = state
        .battlefield
        .iter()
        .filter(|&&id| {
            state
                .objects
                .get(&id)
                .map(|o| o.controller == active)
                .unwrap_or(false)
        })
        .copied()
        .collect();
    for id in to_untap {
        if let Some(obj) = state.objects.get_mut(&id) {
            obj.tapped = false;
            obj.summoning_sick = false;
        }
    }
    state.lands_played_this_turn = 0;
    state.combat = CombatState::empty();
    state
}

fn draw_step(state: GameState) -> GameState {
    let active = state.active_player;
    draw_card(state, active)
}

/// Draw the top card of a player's library. If the library is empty, that player loses (CR 704.5b).
pub fn draw_card(mut state: GameState, player_id: PlayerId) -> GameState {
    let top = state.libraries.get_mut(&player_id).and_then(|lib| {
        if lib.is_empty() {
            None
        } else {
            Some(lib.remove(0))
        }
    });

    match top {
        None => {
            if let Some(p) = state.get_player_mut(player_id) {
                p.has_lost = true;
            }
            state.game_over = true;
        }
        Some(card_id) => {
            state.hands.get_mut(&player_id).unwrap().push(card_id);
            if let Some(obj) = state.objects.get_mut(&card_id) {
                obj.zone = Zone::Hand;
            }
        }
    }
    state
}

/// Advance the initial game state to the first main phase of the starting player's
/// first turn. Skips Untap (nothing to untap at game start), Upkeep (no Phase 1
/// triggers), and Draw (CR 103.8a: the starting player draws no cards).
pub fn skip_to_first_main(mut state: GameState) -> GameState {
    state.step = Step::PreCombatMain;
    state
}

fn cleanup_step(mut state: GameState) -> GameState {
    // CR 514.2: remove damage from all permanents and clear deathtouch flag.
    for obj in state.objects.values_mut() {
        obj.damage_marked = 0;
        obj.damaged_by_deathtouch = false;
    }
    // CR 514.1: discard to hand size — not enforced in Phase 1 (scripted game stays under 7).
    state
}

fn start_next_turn(mut state: GameState) -> GameState {
    state = cleanup_step(state);
    let next = state.opponent_of(state.active_player);
    state.active_player = next;
    state.priority_player = next;
    state.turn_number += 1;
    state.lands_played_this_turn = 0;
    state.combat = CombatState::empty();
    state.step = Step::Untap;
    state
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cards::test_helpers::test_db;
    use crate::types::{CardObject, Phase, Player, Zone};

    fn make_state() -> GameState {
        GameState::new(vec![
            Player::new(PlayerId(0), "Alice"),
            Player::new(PlayerId(1), "Bob"),
        ])
    }

    fn add_land_to_battlefield(state: &mut GameState, owner: PlayerId) -> ObjectId {
        let db = test_db();
        let id = state.alloc_id();
        let mut obj = CardObject::new(
            id,
            db.get("Forest").unwrap().clone(),
            owner,
            Zone::Battlefield,
        );
        obj.tapped = true;
        obj.summoning_sick = false;
        state.battlefield.push(id);
        state.add_object(obj);
        id
    }

    fn put_in_library(
        state: &mut GameState,
        owner: PlayerId,
        def: crate::types::CardDefinition,
    ) -> ObjectId {
        let id = state.alloc_id();
        let obj = CardObject::new(id, def, owner, Zone::Library);
        state.libraries.get_mut(&owner).unwrap().push(id);
        state.add_object(obj);
        id
    }

    #[test]
    fn untap_step_untaps_active_player_permanents() {
        let mut gs = make_state();
        let forest_id = add_land_to_battlefield(&mut gs, PlayerId(0));
        assert!(gs.objects[&forest_id].tapped);

        let gs = apply_step_start(gs);

        assert!(!gs.objects[&forest_id].tapped);
    }

    #[test]
    fn untap_step_does_not_untap_opponents_permanents() {
        let mut gs = make_state();
        let forest_id = add_land_to_battlefield(&mut gs, PlayerId(1)); // opponent's land
        assert!(gs.objects[&forest_id].tapped);

        let gs = apply_step_start(gs); // active player is PlayerId(0)

        assert!(gs.objects[&forest_id].tapped); // stays tapped
    }

    #[test]
    fn untap_step_clears_summoning_sickness() {
        let db = test_db();
        let mut gs = make_state();
        let id = gs.alloc_id();
        let mut obj = CardObject::new(
            id,
            db.get("Grizzly Bears").unwrap().clone(),
            PlayerId(0),
            Zone::Battlefield,
        );
        obj.summoning_sick = true;
        gs.battlefield.push(id);
        gs.add_object(obj);

        let gs = apply_step_start(gs);

        assert!(!gs.objects[&id].summoning_sick);
    }

    #[test]
    fn draw_step_moves_top_card_to_hand() {
        let db = test_db();
        let mut gs = make_state();
        gs.step = Step::Draw;
        let card_id = put_in_library(
            &mut gs,
            PlayerId(0),
            db.get("Grizzly Bears").unwrap().clone(),
        );

        let gs = apply_step_start(gs);

        assert!(gs.hands[&PlayerId(0)].contains(&card_id));
        assert!(gs.libraries[&PlayerId(0)].is_empty());
        assert_eq!(gs.objects[&card_id].zone, Zone::Hand);
    }

    #[test]
    fn drawing_from_empty_library_causes_loss() {
        let mut gs = make_state();
        gs.step = Step::Draw;
        // library is empty by default

        let gs = apply_step_start(gs);

        assert!(gs.is_game_over());
        assert_eq!(gs.winner(), Some(PlayerId(1)));
    }

    #[test]
    fn cleanup_step_removes_damage_from_creatures() {
        let db = test_db();
        let mut gs = make_state();
        gs.step = Step::Cleanup;
        let id = gs.alloc_id();
        let mut obj = CardObject::new(
            id,
            db.get("Grizzly Bears").unwrap().clone(),
            PlayerId(0),
            Zone::Battlefield,
        );
        obj.damage_marked = 1;
        obj.summoning_sick = false;
        gs.battlefield.push(id);
        gs.add_object(obj);

        let gs = apply_step_start(gs);

        assert_eq!(gs.objects[&id].damage_marked, 0);
    }

    #[test]
    fn advance_step_sequences_correctly() {
        let gs = make_state(); // Untap (Beginning phase)
        let gs = advance_step(gs);
        assert_eq!(gs.step(), Step::Upkeep);
        assert_eq!(gs.phase(), Phase::Beginning);
        let gs = advance_step(gs);
        assert_eq!(gs.step(), Step::Draw);
        assert_eq!(gs.phase(), Phase::Beginning);
        let gs = advance_step(gs);
        assert_eq!(gs.step(), Step::PreCombatMain);
        assert_eq!(gs.phase(), Phase::PreCombatMain);
    }

    #[test]
    fn end_of_cleanup_rotates_active_player_and_resets_turn() {
        let mut gs = make_state();
        gs.step = Step::Cleanup;
        gs.lands_played_this_turn = 1;

        let gs = advance_step(gs);

        assert_eq!(gs.active_player, PlayerId(1));
        assert_eq!(gs.turn_number, 2);
        assert_eq!(gs.lands_played_this_turn, 0);
        assert_eq!(gs.phase(), Phase::Beginning);
        assert_eq!(gs.step(), Step::Untap);
    }

    #[test]
    fn draw_card_function_works_directly() {
        let db = test_db();
        let mut gs = make_state();
        let card_id = put_in_library(
            &mut gs,
            PlayerId(0),
            db.get("Grizzly Bears").unwrap().clone(),
        );

        let gs = draw_card(gs, PlayerId(0));

        assert!(gs.hands[&PlayerId(0)].contains(&card_id));
        assert!(gs.libraries[&PlayerId(0)].is_empty());
    }

    #[test]
    fn advance_step_consumes_extra_steps_before_static_sequence() {
        let mut gs = make_state();
        gs.step = Step::CombatDamage;
        gs.extra_steps.push_back(Step::CombatDamage); // simulate second combat damage round

        let gs = advance_step(gs);

        // Should have consumed the queued step, not gone to EndOfCombat
        assert_eq!(gs.step(), Step::CombatDamage);
        assert!(gs.extra_steps.is_empty());
    }

    #[test]
    fn advance_step_drains_all_mana_pools() {
        let mut gs = make_state();
        gs.step = Step::PreCombatMain;
        gs.get_player_mut(PlayerId(0)).unwrap().mana_pool.green += 2;
        gs.get_player_mut(PlayerId(1)).unwrap().mana_pool.red += 1;

        let gs = advance_step(gs);

        assert!(gs.get_player(PlayerId(0)).unwrap().mana_pool.is_empty());
        assert!(gs.get_player(PlayerId(1)).unwrap().mana_pool.is_empty());
    }

    #[test]
    fn advance_from_end_of_combat_clears_combat_state() {
        let mut gs = make_state();
        gs.step = Step::EndOfCombat;
        gs.combat.attackers.push(ObjectId(99));

        let gs = advance_step(gs);

        assert_eq!(gs.step(), Step::PostCombatMain);
        assert!(gs.combat.attackers.is_empty());
        assert!(gs.combat.blocking_map.is_empty());
    }

    #[test]
    fn advance_step_clears_mana_checkpoint() {
        use crate::engine::mana::tap_land_for_mana;
        let _db = test_db();
        let mut gs = make_state();
        gs.step = Step::PreCombatMain;
        // add_land_to_battlefield creates a tapped land; untap it.
        let forest_id = add_land_to_battlefield(&mut gs, PlayerId(0));
        gs.objects.get_mut(&forest_id).unwrap().tapped = false;

        let gs = tap_land_for_mana(gs, forest_id).unwrap();
        assert!(gs.mana_checkpoint.is_some());

        let gs = advance_step(gs);

        assert!(gs.mana_checkpoint.is_none());
    }

    #[test]
    fn advance_step_resets_priority_to_active_player() {
        let mut gs = make_state();
        gs.step = Step::PreCombatMain;
        gs.priority_player = PlayerId(1); // manually set to NAP

        let gs = advance_step(gs);

        assert_eq!(gs.priority_player, PlayerId(0)); // reset to AP
        assert_eq!(gs.step(), Step::BeginningOfCombat);
    }

    #[test]
    fn apply_step_start_resolves_combat_damage() {
        let db = test_db();
        let mut gs = make_state();

        // Put an unblocked 2/2 attacker for P0
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
        gs.combat.attackers = vec![id];
        gs.combat.blocking_map.insert(id, vec![]);
        gs.step = Step::CombatDamage;

        let gs = apply_step_start(gs);

        // Unblocked 2/2 deals 2 damage to P1
        assert_eq!(gs.get_player(PlayerId(1)).unwrap().life, 18);
    }

    #[test]
    fn decayed_attacker_sacrificed_at_end_of_combat() {
        use crate::types::card::{CardType, TypeLine};
        use crate::types::{Ability, CardDefinition, OracleSpan, ability::StaticAbility};
        let mut gs = make_state();
        gs.step = Step::EndOfCombat;
        let id = gs.alloc_id();
        let def = CardDefinition {
            name: "Zombie Token".into(),
            mana_cost: None,
            type_line: TypeLine {
                supertypes: vec![],
                card_types: vec![CardType::Creature],
                subtypes: vec![],
            },
            oracle_text: String::new(),
            abilities: vec![OracleSpan::Parsed(Ability::Static(StaticAbility::Decayed))],
            power: Some(2),
            toughness: Some(2),
        };
        let mut obj = CardObject::new(id, def, PlayerId(0), Zone::Battlefield);
        obj.summoning_sick = false;
        gs.battlefield.push(id);
        gs.add_object(obj);
        gs.combat.attackers = vec![id];

        let gs = apply_step_start(gs);

        assert!(!gs.battlefield.contains(&id));
        assert!(gs.graveyards[&PlayerId(0)].contains(&id));
    }

    #[test]
    fn non_decayed_attacker_not_sacrificed_at_end_of_combat() {
        let db = test_db();
        let mut gs = make_state();
        gs.step = Step::EndOfCombat;
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
        gs.combat.attackers = vec![id];

        let gs = apply_step_start(gs);

        assert!(gs.battlefield.contains(&id));
    }
}
