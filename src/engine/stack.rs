use super::{
    EngineError,
    state_based_actions::check_and_apply_sbas,
    triggered::collect_etb_triggers,
    turn::{advance_step, draw_card},
};
use crate::types::effect::EffectStep;
use crate::types::stack::StackPayload;
use crate::types::{GameState, PlayerId, Zone};

// CR 405.5: when all players pass in succession, top of stack resolves;
// if stack is empty, current step/phase ends.
pub fn pass_priority(mut state: GameState, player_id: PlayerId) -> Result<GameState, EngineError> {
    if state.priority_player != player_id {
        return Err(EngineError::NotYourPriority);
    }
    state.consecutive_passes += 1;
    if state.consecutive_passes as usize >= state.players.len() {
        if state.stack.is_empty() {
            state.consecutive_passes = 0; // reset before step advance
            Ok(advance_step(state))
        } else {
            Ok(resolve_top(state))
        }
    } else {
        state.priority_player = state.opponent_of(player_id);
        Ok(state)
    }
}

pub fn resolve_top(mut state: GameState) -> GameState {
    let stack_id = match state.stack.last().copied() {
        Some(id) => id,
        None => return state,
    };
    state.stack.pop();
    let stack_obj = match state.stack_objects.remove(&stack_id) {
        Some(obj) => obj,
        None => {
            unreachable!("stack id {stack_id:?} missing from stack_objects; invariant violated")
        }
    };

    match stack_obj.payload {
        StackPayload::Spell { card_id } => {
            // Move card Stack → Battlefield
            if let Some(obj) = state.objects.get_mut(&card_id) {
                obj.zone = Zone::Battlefield;
                obj.summoning_sick = true;
            }
            state.battlefield.push(card_id);

            // CR 603.3: collect ETB triggers and push onto stack (CR 405.3 APNAP order —
            // for a single entering permanent, all triggers share the same controller
            // so order is trivial; multi-permanent APNAP ordering is a future concern).
            let triggers = collect_etb_triggers(&mut state, card_id);
            for trigger in triggers {
                let id = trigger.id;
                state.stack.push(id);
                state.stack_objects.insert(id, trigger);
            }

            // CR 117.3b: after triggered abilities are put on the stack, active player
            // receives priority (distinct from CR 117.3c where the caster retains priority
            // after casting a spell or activating an ability).
            state.consecutive_passes = 0;
            state.priority_player = state.active_player;
            check_and_apply_sbas(state)
        }
        StackPayload::TriggeredAbility { effect, .. }
        | StackPayload::ActivatedAbility { effect, .. } => {
            let controller = stack_obj.controller;
            for step in &effect {
                match step {
                    EffectStep::DrawCard(n) => {
                        for _ in 0..*n {
                            state = draw_card(state, controller);
                        }
                    }
                    EffectStep::GainLife(n) => {
                        if let Some(player) = state.get_player_mut(controller) {
                            player.life += *n as i32;
                        }
                    }
                    EffectStep::Mill(n) => {
                        let to_mill = (*n as usize)
                            .min(state.libraries.get(&controller).map_or(0, |l| l.len()));
                        for _ in 0..to_mill {
                            if let Some(card_id) = state
                                .libraries
                                .get_mut(&controller)
                                .filter(|l| !l.is_empty())
                                .map(|l| l.remove(0))
                            {
                                if let Some(gy) = state.graveyards.get_mut(&controller) {
                                    gy.push(card_id);
                                }
                                if let Some(obj) = state.objects.get_mut(&card_id) {
                                    obj.zone = Zone::Graveyard;
                                }
                            }
                        }
                    }
                    EffectStep::AddMana(_) => {
                        // Mana abilities never reach the stack (CR 405.6c).
                        unreachable!("AddMana in stack object");
                    }
                }
            }
            state.consecutive_passes = 0;
            state.priority_player = state.active_player;
            check_and_apply_sbas(state)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cards::test_helpers::test_db;
    use crate::types::effect::EffectStep;
    use crate::types::stack::{StackObject, StackPayload};
    use crate::types::{CardObject, ObjectId, Player, Step};

    fn make_state() -> GameState {
        let mut gs = GameState::new(vec![
            Player::new(PlayerId(0), "Alice"),
            Player::new(PlayerId(1), "Bob"),
        ]);
        gs.step = Step::PreCombatMain;
        gs
    }

    fn push_spell(state: &mut GameState, card_id: ObjectId) {
        let stack_id = state.alloc_stack_id();
        let controller = state.active_player;
        let obj = StackObject {
            id: stack_id,
            payload: StackPayload::Spell { card_id },
            controller,
        };
        state.stack.push(stack_id);
        state.stack_objects.insert(stack_id, obj);
    }

    fn push_draw_trigger(state: &mut GameState, source_id: ObjectId) {
        let stack_id = state.alloc_stack_id();
        let controller = state.active_player;
        let obj = StackObject {
            id: stack_id,
            payload: StackPayload::TriggeredAbility {
                source_id,
                effect: vec![EffectStep::DrawCard(1)],
                label: "draw trigger".into(),
            },
            controller,
        };
        state.stack.push(stack_id);
        state.stack_objects.insert(stack_id, obj);
    }

    fn put_in_library(state: &mut GameState, owner: PlayerId) -> ObjectId {
        use crate::types::card::{CardDefinition, CardType, TypeLine};
        let def = CardDefinition {
            name: "Dummy".into(),
            mana_cost: None,
            type_line: TypeLine {
                supertypes: vec![],
                card_types: vec![CardType::Creature],
                subtypes: vec![],
            },
            oracle_text: String::new(),
            abilities: vec![],
            power: Some(1),
            toughness: Some(1),
        };
        let id = state.alloc_id();
        let obj = CardObject::new(id, def, owner, Zone::Library);
        state.libraries.get_mut(&owner).unwrap().push(id);
        state.add_object(obj);
        id
    }

    #[test]
    fn pass_priority_wrong_player_returns_error() {
        let mut gs = make_state();
        gs.priority_player = PlayerId(0);
        assert!(matches!(
            pass_priority(gs, PlayerId(1)),
            Err(EngineError::NotYourPriority)
        ));
    }

    #[test]
    fn pass_priority_once_shifts_priority_to_opponent() {
        let mut gs = make_state();
        gs.priority_player = PlayerId(0);
        let gs = pass_priority(gs, PlayerId(0)).unwrap();
        assert_eq!(gs.priority_player, PlayerId(1));
        assert_eq!(gs.consecutive_passes, 1);
    }

    #[test]
    fn pass_priority_twice_empty_stack_advances_step() {
        let mut gs = make_state();
        gs.step = Step::PreCombatMain;
        gs.priority_player = PlayerId(0);
        let gs = pass_priority(gs, PlayerId(0)).unwrap();
        let gs = pass_priority(gs, PlayerId(1)).unwrap();
        assert_eq!(gs.step(), Step::BeginningOfCombat);
    }

    #[test]
    fn pass_priority_twice_resolves_top_spell() {
        let db = test_db();
        let mut gs = make_state();
        let id = gs.alloc_id();
        let obj = CardObject::new(
            id,
            db.get("Grizzly Bears").unwrap().clone(),
            PlayerId(0),
            Zone::Stack,
        );
        gs.add_object(obj);
        push_spell(&mut gs, id);
        gs.priority_player = PlayerId(0);

        let gs = pass_priority(gs, PlayerId(0)).unwrap();
        let gs = pass_priority(gs, PlayerId(1)).unwrap();

        assert!(gs.battlefield.contains(&id));
        assert_eq!(gs.objects[&id].zone, Zone::Battlefield);
        assert!(gs.stack.is_empty());
    }

    #[test]
    fn pass_priority_resets_consecutive_passes_after_resolution() {
        let db = test_db();
        let mut gs = make_state();
        let id = gs.alloc_id();
        let obj = CardObject::new(
            id,
            db.get("Grizzly Bears").unwrap().clone(),
            PlayerId(0),
            Zone::Stack,
        );
        gs.add_object(obj);
        push_spell(&mut gs, id);
        gs.priority_player = PlayerId(0);

        let gs = pass_priority(gs, PlayerId(0)).unwrap();
        let gs = pass_priority(gs, PlayerId(1)).unwrap();

        assert_eq!(gs.consecutive_passes, 0);
        assert_eq!(gs.priority_player, PlayerId(0));
    }

    #[test]
    fn resolve_top_triggered_ability_draws_card() {
        let mut gs = make_state();
        put_in_library(&mut gs, PlayerId(0));
        push_draw_trigger(&mut gs, ObjectId(99));

        let gs = resolve_top(gs);

        assert_eq!(gs.hands[&PlayerId(0)].len(), 1);
        assert!(gs.libraries[&PlayerId(0)].is_empty());
        assert!(gs.stack.is_empty());
    }

    #[test]
    fn resolve_top_triggered_ability_gains_life() {
        let mut gs = make_state();
        let before_life = gs.get_player(PlayerId(0)).unwrap().life;
        let stack_id = gs.alloc_stack_id();
        let obj = StackObject {
            id: stack_id,
            payload: StackPayload::TriggeredAbility {
                source_id: ObjectId(99),
                effect: vec![EffectStep::GainLife(5)],
                label: "gain life trigger".into(),
            },
            controller: PlayerId(0),
        };
        gs.stack.push(stack_id);
        gs.stack_objects.insert(stack_id, obj);

        let gs = resolve_top(gs);

        assert_eq!(gs.get_player(PlayerId(0)).unwrap().life, before_life + 5);
        assert!(gs.stack.is_empty());
    }

    #[test]
    fn resolve_top_triggered_ability_mills() {
        let mut gs = make_state();
        let card1 = put_in_library(&mut gs, PlayerId(0));
        let card2 = put_in_library(&mut gs, PlayerId(0));
        let stack_id = gs.alloc_stack_id();
        let obj = StackObject {
            id: stack_id,
            payload: StackPayload::TriggeredAbility {
                source_id: ObjectId(99),
                effect: vec![EffectStep::Mill(2)],
                label: "mill trigger".into(),
            },
            controller: PlayerId(0),
        };
        gs.stack.push(stack_id);
        gs.stack_objects.insert(stack_id, obj);

        let gs = resolve_top(gs);

        assert!(gs.libraries[&PlayerId(0)].is_empty());
        assert!(gs.graveyards[&PlayerId(0)].contains(&card1));
        assert!(gs.graveyards[&PlayerId(0)].contains(&card2));
        assert!(gs.stack.is_empty());
    }

    #[test]
    fn resolve_top_spell_moves_card_to_battlefield() {
        let db = test_db();
        let mut gs = make_state();
        let id = gs.alloc_id();
        let obj = CardObject::new(
            id,
            db.get("Grizzly Bears").unwrap().clone(),
            PlayerId(0),
            Zone::Stack,
        );
        gs.add_object(obj);
        push_spell(&mut gs, id);

        let gs = resolve_top(gs);

        assert!(gs.battlefield.contains(&id));
        assert_eq!(gs.objects[&id].zone, Zone::Battlefield);
        assert!(gs.objects[&id].summoning_sick);
        assert!(gs.stack.is_empty());
    }

    #[test]
    fn resolve_top_empty_stack_is_noop() {
        let gs = make_state();
        let gs2 = resolve_top(gs);
        assert!(gs2.stack.is_empty());
        assert_eq!(gs2.consecutive_passes, 0);
    }

    #[test]
    fn resolve_top_spell_collects_etb_triggers_onto_stack() {
        use crate::types::OracleSpan;
        use crate::types::ability::{Ability, TriggerEvent, TriggeredAbility};
        use crate::types::card::{CardDefinition, CardType, TypeLine};
        use crate::types::mana::{ManaCost, ManaPip};

        let mut gs = make_state();
        put_in_library(&mut gs, PlayerId(0));

        let def = CardDefinition {
            name: "Elvish Visionary".into(),
            mana_cost: Some(ManaCost {
                pips: vec![ManaPip::Green],
            }),
            type_line: TypeLine {
                supertypes: vec![],
                card_types: vec![CardType::Creature],
                subtypes: vec!["Elf".into()],
            },
            oracle_text: "When this enters, draw a card.".into(),
            abilities: vec![OracleSpan::Parsed(Ability::Triggered(TriggeredAbility {
                trigger: TriggerEvent::EntersTheBattlefield {
                    subject_is_self: true,
                },
                effect: vec![EffectStep::DrawCard(1)],
            }))],
            power: Some(1),
            toughness: Some(1),
        };
        let id = gs.alloc_id();
        let obj = CardObject::new(id, def, PlayerId(0), Zone::Stack);
        gs.add_object(obj);
        push_spell(&mut gs, id);

        let gs = resolve_top(gs);

        // Creature on battlefield, ETB trigger waiting on stack
        assert!(gs.battlefield.contains(&id));
        assert_eq!(gs.stack.len(), 1);
        // Card not yet drawn — trigger hasn't resolved
        assert!(gs.hands[&PlayerId(0)].is_empty());

        let gs = resolve_top(gs);

        // Trigger resolved
        assert_eq!(gs.hands[&PlayerId(0)].len(), 1);
        assert!(gs.stack.is_empty());
    }
}
