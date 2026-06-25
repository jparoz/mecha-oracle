use super::combat::deal_combat_damage;
use super::state_based_actions::move_to_graveyard;
use crate::types::ability::KeywordAbility;
use crate::types::{CombatState, GameState, ObjectId, PTDelta, PlayerId, Step, Zone};

/// Apply the automatic rules for the start of the current step/phase.
///
/// CR 603.2b: At the beginning of each step, fire a PhaseStep event so that
/// "at the beginning of `step`" triggered abilities are collected onto the stack.
pub fn apply_step_start(mut state: GameState) -> GameState {
    // CR 603.2b: collect PhaseStep triggers before step-specific logic runs.
    // Untap and Cleanup have no priority window (CR 502.4, CR 514.3), so triggers
    // accumulated there are technically held, but we fire the event anyway so the
    // dispatch system has a place to hook them in future.
    // TODO(CR 603.3b): Triggers generated during Untap and Cleanup have no priority window;
    // they should be held and placed on the stack at the next opportunity (start of Upkeep).
    // Currently these triggers are pushed directly — a trigger-holding mechanism is needed
    // before Untap/Cleanup-step triggered abilities are implemented.
    // Capture fields before the &mut borrow.
    let current_step = state.step;
    let current_active = state.active_player;
    let step_triggers = crate::engine::triggered::collect_triggers_for_event(
        &mut state,
        &crate::types::GameEvent::PhaseStep {
            step: current_step,
            active_player: current_active,
        },
    );
    for t in step_triggers {
        let id = t.id;
        state.stack.push(id);
        state.stack_objects.insert(id, t);
    }

    // Drain and fire one-shot delayed triggers matching the current step.
    // (e.g. Dash's return-to-hand — CR 702.109a.)
    let (to_fire, to_keep): (Vec<_>, Vec<_>) = state
        .delayed_triggers
        .drain(..)
        .partition(|t| t.fires_on_step == current_step);
    state.delayed_triggers = to_keep;
    for trigger in to_fire {
        let stack_id = state.alloc_stack_id();
        let stack_obj = crate::types::stack::StackObject {
            id: stack_id,
            payload: crate::types::stack::StackPayload::TriggeredAbility {
                source_id: crate::types::ids::ObjectId(0),
                effect: trigger.effect,
                label: "Delayed trigger".into(),
            },
            controller: trigger.controller,
            targets: trigger.targets,
            x_value: None,
            cast_mode: crate::types::ability::CastMode::Standard,
        };
        state.stack.push(stack_id);
        state.stack_objects.insert(stack_id, stack_obj);
    }

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
                .battlefield
                .get(&id)
                .map(|p| p.has_keyword(KeywordAbility::Decayed))
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
        Step::DeclareAttackers => {
            if state.combat.attackers.is_empty() {
                // CR 506.1: DB and CD are skipped when no creatures declared as attackers.
                set(state, Step::EndOfCombat)
            } else {
                set(state, Step::DeclareBlockers)
            }
        }
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

/// CR 502: untap all permanents the active player controls and reset `lands_played_this_turn`.
fn untap_step(mut state: GameState) -> GameState {
    let active = state.active_player;
    // CR 502: untap all permanents the active player controls; clear summoning sickness.
    let to_untap: Vec<ObjectId> = state
        .battlefield
        .keys()
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
        if let Some(perm) = state.battlefield.get_mut(&id) {
            perm.tapped = false;
        }
    }
    state.lands_played_this_turn = 0;
    state.combat = CombatState::empty();
    state
}

/// CR 504.1: the active player draws one card at the start of their draw step.
fn draw_step(state: GameState) -> GameState {
    let active = state.active_player;
    draw_card(state, active, true)
}

/// Draw the top card of a player's library. If the library is empty, that player loses (CR 704.5b).
/// CR 603.2: fires DrawsCard event after a successful draw so that "whenever you draw a card"
/// triggered abilities (e.g. Rhystic Study, future Cumulative Upkeep draw effects) are collected,
/// unless fire_events is false (e.g. during opening hand setup per CR 103.5, which is not gameplay).
pub fn draw_card(mut state: GameState, player_id: PlayerId, fire_events: bool) -> GameState {
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
            // CR 603.2: fire DrawsCard event now that the card has moved to hand, but only
            // during gameplay (not setup per CR 103.5).
            if fire_events {
                let draw_triggers = crate::engine::triggered::collect_triggers_for_event(
                    &mut state,
                    &crate::types::GameEvent::DrawsCard { player: player_id },
                );
                for t in draw_triggers {
                    let id = t.id;
                    state.stack.push(id);
                    state.stack_objects.insert(id, t);
                }
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

/// CR 514.2: remove all damage from permanents and clear the deathtouch/EOT-boost flags.
/// CR 514.1: hand-size discard is noted but not enforced in Phase 1.
fn cleanup_step(mut state: GameState) -> GameState {
    // CR 514.2: remove damage from all permanents and clear deathtouch flag.
    for perm in state.battlefield.values_mut() {
        perm.damage_marked = 0;
        perm.damaged_by_deathtouch = false;
        perm.pt_boost_until_eot = PTDelta::default();
    }
    // CR 514.1: discard to hand size — not enforced in Phase 1 (scripted game stays under 7).
    state
}

/// Transitions to the next player's turn: cleanup, swap active player, increment `turn_number`,
/// reset land count and combat state, and advance to the Untap step.
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
    use crate::types::{CardObject, PermanentState, Phase, Player, Zone};

    fn make_state() -> GameState {
        GameState::new(vec![
            Player::new(PlayerId(0), "Alice"),
            Player::new(PlayerId(1), "Bob"),
        ])
    }

    fn add_land_to_battlefield(state: &mut GameState, owner: PlayerId) -> ObjectId {
        let db = test_db();
        let id = state.alloc_id();
        let obj = CardObject::new(
            id,
            db.get("Forest").unwrap().clone(),
            owner,
            Zone::Battlefield,
        );
        let mut perm = PermanentState::new(&obj.definition);
        perm.tapped = true;
        perm.controller_since_turn = 0; // treat as not sick (entered before turn 1)
        state.battlefield.insert(id, perm);
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
        assert!(gs.battlefield[&forest_id].tapped);

        let gs = apply_step_start(gs);

        assert!(!gs.battlefield[&forest_id].tapped);
    }

    #[test]
    fn untap_step_does_not_untap_opponents_permanents() {
        let mut gs = make_state();
        let forest_id = add_land_to_battlefield(&mut gs, PlayerId(1)); // opponent's land
        assert!(gs.battlefield[&forest_id].tapped);

        let gs = apply_step_start(gs); // active player is PlayerId(0)

        assert!(gs.battlefield[&forest_id].tapped); // stays tapped
    }

    #[test]
    fn untap_step_creature_is_no_longer_summoning_sick() {
        let db = test_db();
        let mut gs = make_state();
        let id = gs.alloc_id();
        let obj = CardObject::new(
            id,
            db.get("Grizzly Bears").unwrap().clone(),
            PlayerId(0),
            Zone::Battlefield,
        );
        // Creature entered last turn (turn 0) under P0's control.
        let mut perm = PermanentState::new(&obj.definition);
        perm.controller_since_turn = 0;
        gs.battlefield.insert(id, perm);
        gs.add_object(obj);

        // gs.turn_number = 1, active_player = P0, so controllers_most_recent_turn(P0) = 1.
        // controller_since_turn (0) < 1 → not sick.
        let gs = apply_step_start(gs);

        assert!(!gs.battlefield[&id].summoning_sick(gs.controllers_most_recent_turn(PlayerId(0))));
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
        let obj = CardObject::new(
            id,
            db.get("Grizzly Bears").unwrap().clone(),
            PlayerId(0),
            Zone::Battlefield,
        );
        let mut perm = PermanentState::new(&obj.definition);
        perm.damage_marked = 1;
        perm.controller_since_turn = 0;
        gs.battlefield.insert(id, perm);
        gs.add_object(obj);

        let gs = apply_step_start(gs);

        assert_eq!(gs.battlefield[&id].damage_marked, 0);
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

        let gs = draw_card(gs, PlayerId(0), true);

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
        gs.battlefield.get_mut(&forest_id).unwrap().tapped = false;

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
        let obj = CardObject::new(
            id,
            db.get("Grizzly Bears").unwrap().clone(),
            PlayerId(0),
            Zone::Battlefield,
        );
        let mut perm = PermanentState::new(&obj.definition);
        perm.controller_since_turn = 0;
        gs.battlefield.insert(id, perm);
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
        use crate::types::{CardDefinition, Rule, RulesText, ability::KeywordAbility};
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
            rules_text: vec![RulesText::Active(Rule::Static(KeywordAbility::Decayed))],
            text_annotations: vec![],
            power: Some(2),
            toughness: Some(2),
            colors: vec![],
        };
        let obj = CardObject::new(id, def, PlayerId(0), Zone::Battlefield);
        let mut perm = PermanentState::new(&obj.definition);
        perm.controller_since_turn = 0;
        gs.battlefield.insert(id, perm);
        gs.add_object(obj);
        gs.combat.attackers = vec![id];

        let gs = apply_step_start(gs);

        assert!(!gs.battlefield.contains_key(&id));
        assert!(gs.graveyards[&PlayerId(0)].contains(&id));
    }

    // CR 506.1: DB and CD are skipped when no attackers declared.
    #[test]
    fn advance_step_from_da_with_no_attackers_goes_to_eoc() {
        let mut gs = make_state();
        gs.step = Step::DeclareAttackers;
        // combat.attackers is empty by default
        let gs = advance_step(gs);
        assert_eq!(gs.step, Step::EndOfCombat);
    }

    #[test]
    fn advance_step_from_da_with_attackers_goes_to_db() {
        let db = test_db();
        let mut gs = make_state();
        gs.step = Step::DeclareAttackers;
        let id = gs.alloc_id();
        let obj = CardObject::new(
            id,
            db.get("Grizzly Bears").unwrap().clone(),
            PlayerId(0),
            Zone::Battlefield,
        );
        gs.battlefield
            .insert(id, PermanentState::new(&obj.definition));
        gs.add_object(obj);
        gs.combat.attackers = vec![id];
        let gs = advance_step(gs);
        assert_eq!(gs.step, Step::DeclareBlockers);
    }

    #[test]
    fn non_decayed_attacker_not_sacrificed_at_end_of_combat() {
        let db = test_db();
        let mut gs = make_state();
        gs.step = Step::EndOfCombat;
        let id = gs.alloc_id();
        let obj = CardObject::new(
            id,
            db.get("Grizzly Bears").unwrap().clone(),
            PlayerId(0),
            Zone::Battlefield,
        );
        let mut perm = PermanentState::new(&obj.definition);
        perm.controller_since_turn = 0;
        gs.battlefield.insert(id, perm);
        gs.add_object(obj);
        gs.combat.attackers = vec![id];

        let gs = apply_step_start(gs);

        assert!(gs.battlefield.contains_key(&id));
    }

    #[test]
    fn cleanup_step_clears_pt_boost_until_eot() {
        use crate::types::{CardObject, PTDelta, PermanentState, Zone};
        let db = test_db();
        let mut gs = make_state();
        gs.step = Step::Cleanup;
        let id = gs.alloc_id();
        let obj = CardObject::new(
            id,
            db.get("Grizzly Bears").unwrap().clone(),
            PlayerId(0),
            Zone::Battlefield,
        );
        let mut perm = PermanentState::new(&obj.definition);
        perm.pt_boost_until_eot = PTDelta {
            power: 3,
            toughness: 3,
        };
        gs.battlefield.insert(id, perm);
        gs.add_object(obj);

        let gs = apply_step_start(gs);

        assert_eq!(gs.battlefield[&id].pt_boost_until_eot, PTDelta::default());
        assert_eq!(gs.battlefield[&id].effective_power(0), Some(2)); // back to 2/2 base
    }

    // --- Task 8 tests: PhaseStep and DrawsCard event emission ---

    /// CR 603.2b: PhaseStep event should be fired at the start of each step and pushed to
    /// the stack when a matching triggered ability exists.
    #[test]
    fn phase_step_event_collects_upkeep_trigger() {
        use crate::types::ability::{
            Rule, TriggerEvent, TriggerTargetMode, TriggeredAbility, TurnOwner,
        };
        use crate::types::card::{CardDefinition, CardType, TypeLine};
        use crate::types::effect::EffectStep;
        use crate::types::{PermanentState, RulesText};

        let mut gs = make_state();
        gs.step = Step::Upkeep;

        // A permanent with "at the beginning of your upkeep" trigger owned by active player (P0).
        let upkeep_trigger = TriggeredAbility {
            trigger: TriggerEvent::PhaseStep {
                step: Step::Upkeep,
                whose_turn: TurnOwner::You,
            },
            condition: None,
            target_mode: TriggerTargetMode::None,
            effect: vec![EffectStep::GainLife(1)],
        };
        let def = CardDefinition {
            name: "Upkeep Trigger Permanent".into(),
            mana_cost: None,
            type_line: TypeLine {
                supertypes: vec![],
                card_types: vec![CardType::Enchantment],
                subtypes: vec![],
            },
            oracle_text: String::new(),
            rules_text: vec![RulesText::Active(Rule::Triggered(upkeep_trigger))],
            text_annotations: vec![],
            power: None,
            toughness: None,
            colors: vec![],
        };
        let id = gs.alloc_id();
        let obj = crate::types::CardObject::new(id, def, PlayerId(0), Zone::Battlefield);
        gs.battlefield
            .insert(id, PermanentState::new(&obj.definition));
        gs.add_object(obj);

        let stack_before = gs.stack.len();
        let gs = apply_step_start(gs);

        assert_eq!(
            gs.stack.len(),
            stack_before + 1,
            "Upkeep trigger should be pushed to stack during Upkeep step start"
        );
    }

    /// CR 603.2b: PhaseStep event for a step whose_turn=You does not fire for the non-active player.
    #[test]
    fn phase_step_event_does_not_fire_for_non_active_player() {
        use crate::types::ability::{
            Rule, TriggerEvent, TriggerTargetMode, TriggeredAbility, TurnOwner,
        };
        use crate::types::card::{CardDefinition, CardType, TypeLine};
        use crate::types::effect::EffectStep;
        use crate::types::{PermanentState, RulesText};

        let mut gs = make_state();
        gs.step = Step::Upkeep;
        // Active player is P0; this permanent is controlled by P1.
        let upkeep_trigger = TriggeredAbility {
            trigger: TriggerEvent::PhaseStep {
                step: Step::Upkeep,
                whose_turn: TurnOwner::You, // "your upkeep" — P1's perspective; active turn is P0's
            },
            condition: None,
            target_mode: TriggerTargetMode::None,
            effect: vec![EffectStep::GainLife(1)],
        };
        let def = CardDefinition {
            name: "Opponent Upkeep Permanent".into(),
            mana_cost: None,
            type_line: TypeLine {
                supertypes: vec![],
                card_types: vec![CardType::Enchantment],
                subtypes: vec![],
            },
            oracle_text: String::new(),
            rules_text: vec![RulesText::Active(Rule::Triggered(upkeep_trigger))],
            text_annotations: vec![],
            power: None,
            toughness: None,
            colors: vec![],
        };
        let id = gs.alloc_id();
        let obj = crate::types::CardObject::new(id, def, PlayerId(1), Zone::Battlefield);
        gs.battlefield
            .insert(id, PermanentState::new(&obj.definition));
        gs.add_object(obj);

        let stack_before = gs.stack.len();
        let gs = apply_step_start(gs);

        assert_eq!(
            gs.stack.len(),
            stack_before,
            "Upkeep trigger should NOT fire when it's not the controller's upkeep"
        );
    }

    /// CR 603.2: DrawsCard event fires once per card drawn; a matching trigger should be collected.
    #[test]
    fn draw_card_fires_draws_card_event_and_collects_trigger() {
        use crate::types::ability::{
            Rule, TriggerEvent, TriggerTargetMode, TriggeredAbility, TurnOwner,
        };
        use crate::types::card::{CardDefinition, CardType, TypeLine};
        use crate::types::effect::EffectStep;
        use crate::types::{PermanentState, RulesText};

        let db = crate::cards::test_helpers::test_db();
        let mut gs = make_state();
        // Put a card in P0's library so the draw succeeds.
        let card_id = {
            let id = gs.alloc_id();
            let obj = crate::types::CardObject::new(
                id,
                db.get("Grizzly Bears").unwrap().clone(),
                PlayerId(0),
                Zone::Library,
            );
            gs.libraries.get_mut(&PlayerId(0)).unwrap().push(id);
            gs.add_object(obj);
            id
        };
        let _ = card_id;

        // A permanent with "whenever you draw a card" trigger owned by P0.
        let draw_trigger = TriggeredAbility {
            trigger: TriggerEvent::DrawsCard {
                who: TurnOwner::You,
            },
            condition: None,
            target_mode: TriggerTargetMode::None,
            effect: vec![EffectStep::GainLife(1)],
        };
        let def = CardDefinition {
            name: "Rhystic Study".into(),
            mana_cost: None,
            type_line: TypeLine {
                supertypes: vec![],
                card_types: vec![CardType::Enchantment],
                subtypes: vec![],
            },
            oracle_text: String::new(),
            rules_text: vec![RulesText::Active(Rule::Triggered(draw_trigger))],
            text_annotations: vec![],
            power: None,
            toughness: None,
            colors: vec![],
        };
        let perm_id = gs.alloc_id();
        let obj = crate::types::CardObject::new(perm_id, def, PlayerId(0), Zone::Battlefield);
        gs.battlefield
            .insert(perm_id, PermanentState::new(&obj.definition));
        gs.add_object(obj);

        let stack_before = gs.stack.len();
        let gs = draw_card(gs, PlayerId(0), true);

        assert_eq!(
            gs.stack.len(),
            stack_before + 1,
            "DrawsCard trigger should be pushed to stack when player draws a card"
        );
    }

    /// Smoke test: drawing a card when no DrawsCard triggers exist does not crash.
    #[test]
    fn draw_event_does_not_fire_non_draw_triggers() {
        use crate::types::ability::{
            Rule, TriggerEvent, TriggerSubjectFilter, TriggerTargetMode, TriggeredAbility,
        };
        use crate::types::card::{CardDefinition, CardType, TypeLine};
        use crate::types::effect::EffectStep;
        use crate::types::{PermanentState, RulesText};

        let db = crate::cards::test_helpers::test_db();
        let mut gs = make_state();
        // Library needs a card to draw from.
        let id = gs.alloc_id();
        let obj = crate::types::CardObject::new(
            id,
            db.get("Grizzly Bears").unwrap().clone(),
            PlayerId(0),
            Zone::Library,
        );
        gs.libraries.get_mut(&PlayerId(0)).unwrap().push(id);
        gs.add_object(obj);

        // A permanent with an ETB trigger (not a DrawsCard trigger).
        let etb_trigger = TriggeredAbility {
            trigger: TriggerEvent::EntersTheBattlefield {
                subject: TriggerSubjectFilter {
                    is_self: Some(true),
                    ..Default::default()
                },
            },
            condition: None,
            target_mode: TriggerTargetMode::None,
            effect: vec![EffectStep::GainLife(2)],
        };
        let def = CardDefinition {
            name: "ETB Permanent".into(),
            mana_cost: None,
            type_line: TypeLine {
                supertypes: vec![],
                card_types: vec![CardType::Creature],
                subtypes: vec![],
            },
            oracle_text: String::new(),
            rules_text: vec![RulesText::Active(Rule::Triggered(etb_trigger))],
            text_annotations: vec![],
            power: Some(1),
            toughness: Some(1),
            colors: vec![],
        };
        let perm_id = gs.alloc_id();
        let obj = crate::types::CardObject::new(perm_id, def, PlayerId(0), Zone::Battlefield);
        gs.battlefield
            .insert(perm_id, PermanentState::new(&obj.definition));
        gs.add_object(obj);

        let stack_before = gs.stack.len();
        let gs = draw_card(gs, PlayerId(0), true);

        // ETB trigger must not fire on DrawsCard event.
        assert_eq!(
            gs.stack.len(),
            stack_before,
            "ETB trigger should not fire when a card is drawn"
        );
    }
}
