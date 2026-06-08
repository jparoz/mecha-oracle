use super::{
    EngineError,
    mana::{greedy_payment_plan, pay_mana_cost},
    state_based_actions::check_and_apply_sbas,
    triggered::fire_etb_triggers,
};
use crate::types::{GameState, ObjectId, PlayerId, Zone};

/// Move a land from hand to battlefield. One per turn, main phase only (CR 305).
pub fn play_land(
    mut state: GameState,
    player_id: PlayerId,
    object_id: ObjectId,
) -> Result<GameState, EngineError> {
    if state.active_player != player_id {
        return Err(EngineError::CannotCastNow);
    }
    if state.lands_played_this_turn >= 1 {
        return Err(EngineError::LandLimitReached);
    }

    {
        let hand = state
            .hands
            .get(&player_id)
            .ok_or(EngineError::CardNotFound)?;
        if !hand.contains(&object_id) {
            return Err(EngineError::CardNotInHand);
        }
        let obj = state
            .objects
            .get(&object_id)
            .ok_or(EngineError::CardNotFound)?;
        if !obj.is_land() {
            return Err(EngineError::NotALand);
        }
    }

    state.mana_checkpoint = None;
    state
        .hands
        .get_mut(&player_id)
        .unwrap()
        .retain(|&id| id != object_id);
    state.battlefield.push(object_id);
    {
        let obj = state.objects.get_mut(&object_id).unwrap();
        obj.zone = Zone::Battlefield;
        obj.summoning_sick = false; // lands do not have summoning sickness
    }
    state.lands_played_this_turn += 1;

    let state = fire_etb_triggers(state, object_id);
    Ok(check_and_apply_sbas(state))
}

/// Cast a creature from hand. Sorcery speed: active player's main phase, empty stack (CR 307).
/// Phase 1: spell resolves immediately (no stack).
pub fn cast_creature(
    mut state: GameState,
    player_id: PlayerId,
    object_id: ObjectId,
) -> Result<GameState, EngineError> {
    if state.active_player != player_id {
        return Err(EngineError::CannotCastNow);
    }
    if !state.stack.is_empty() {
        return Err(EngineError::CannotCastNow);
    }

    let cost = {
        let hand = state
            .hands
            .get(&player_id)
            .ok_or(EngineError::CardNotFound)?;
        if !hand.contains(&object_id) {
            return Err(EngineError::CardNotInHand);
        }
        let obj = state
            .objects
            .get(&object_id)
            .ok_or(EngineError::CardNotFound)?;
        if !obj.is_creature() {
            return Err(EngineError::NotACreature);
        }
        obj.definition
            .mana_cost
            .clone()
            .ok_or(EngineError::CannotCastNow)?
    };

    let plan = {
        let player = state
            .get_player(player_id)
            .ok_or(EngineError::CardNotFound)?;
        greedy_payment_plan(&cost, &player.mana_pool, player.life)
            .ok_or(EngineError::InsufficientMana)?
    };
    state = pay_mana_cost(state, player_id, &cost, &plan)?;
    state.mana_checkpoint = None;
    state
        .hands
        .get_mut(&player_id)
        .unwrap()
        .retain(|&id| id != object_id);
    state.battlefield.push(object_id);
    {
        let obj = state.objects.get_mut(&object_id).unwrap();
        obj.zone = Zone::Battlefield;
        obj.summoning_sick = true; // creatures always enter with summoning sickness
    }

    let state = fire_etb_triggers(state, object_id);
    Ok(check_and_apply_sbas(state))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cards::test_helpers::test_db;
    use crate::types::{CardObject, OracleSpan, Player, Step};

    fn make_state() -> GameState {
        let mut gs = GameState::new(vec![
            Player::new(PlayerId(0), "Alice"),
            Player::new(PlayerId(1), "Bob"),
        ]);
        gs.step = Step::PreCombatMain;
        gs
    }

    fn put_in_hand(
        state: &mut GameState,
        owner: PlayerId,
        def: crate::types::CardDefinition,
    ) -> ObjectId {
        let id = state.alloc_id();
        let obj = CardObject::new(id, def, owner, Zone::Hand);
        state.hands.get_mut(&owner).unwrap().push(id);
        state.add_object(obj);
        id
    }

    #[test]
    fn play_land_moves_from_hand_to_battlefield() {
        let db = test_db();
        let mut gs = make_state();
        let forest_id = put_in_hand(&mut gs, PlayerId(0), db.get("Forest").unwrap().clone());

        let gs = play_land(gs, PlayerId(0), forest_id).unwrap();

        assert!(!gs.hands[&PlayerId(0)].contains(&forest_id));
        assert!(gs.battlefield.contains(&forest_id));
        assert_eq!(gs.objects[&forest_id].zone, Zone::Battlefield);
        assert_eq!(gs.lands_played_this_turn, 1);
    }

    #[test]
    fn cannot_play_second_land_in_same_turn() {
        let db = test_db();
        let mut gs = make_state();
        gs.lands_played_this_turn = 1;
        let forest_id = put_in_hand(&mut gs, PlayerId(0), db.get("Forest").unwrap().clone());

        assert!(matches!(
            play_land(gs, PlayerId(0), forest_id),
            Err(EngineError::LandLimitReached)
        ));
    }

    #[test]
    fn cast_creature_spends_mana_and_enters_battlefield() {
        let db = test_db();
        let mut gs = make_state();
        gs.get_player_mut(PlayerId(0)).unwrap().mana_pool.green += 2;
        let bear_id = put_in_hand(
            &mut gs,
            PlayerId(0),
            db.get("Grizzly Bears").unwrap().clone(),
        );

        let gs = cast_creature(gs, PlayerId(0), bear_id).unwrap();

        assert!(!gs.hands[&PlayerId(0)].contains(&bear_id));
        assert!(gs.battlefield.contains(&bear_id));
        assert!(gs.objects[&bear_id].summoning_sick);
        assert!(gs.get_player(PlayerId(0)).unwrap().mana_pool.is_empty());
    }

    #[test]
    fn cannot_cast_creature_without_mana() {
        let db = test_db();
        let mut gs = make_state();
        let bear_id = put_in_hand(
            &mut gs,
            PlayerId(0),
            db.get("Grizzly Bears").unwrap().clone(),
        );

        assert!(matches!(
            cast_creature(gs, PlayerId(0), bear_id),
            Err(EngineError::InsufficientMana)
        ));
    }

    #[test]
    fn cast_creature_clears_mana_checkpoint() {
        let db = test_db();
        let mut gs = make_state();
        // Give player enough mana and a checkpoint.
        gs.get_player_mut(PlayerId(0)).unwrap().mana_pool.green += 2;
        // Create a minimal checkpoint manually to simulate a prior tap.
        gs.mana_checkpoint = Some(crate::types::ManaCheckpoint {
            pools: std::collections::HashMap::new(),
            tapped_lands: vec![],
        });
        let bear_id = put_in_hand(
            &mut gs,
            PlayerId(0),
            db.get("Grizzly Bears").unwrap().clone(),
        );

        let gs = cast_creature(gs, PlayerId(0), bear_id).unwrap();

        assert!(gs.mana_checkpoint.is_none());
    }

    #[test]
    fn play_land_clears_mana_checkpoint() {
        let db = test_db();
        let mut gs = make_state();
        gs.mana_checkpoint = Some(crate::types::ManaCheckpoint {
            pools: std::collections::HashMap::new(),
            tapped_lands: vec![],
        });
        let forest_id = put_in_hand(&mut gs, PlayerId(0), db.get("Forest").unwrap().clone());

        let gs = play_land(gs, PlayerId(0), forest_id).unwrap();

        assert!(gs.mana_checkpoint.is_none());
    }

    #[test]
    fn cast_creature_fires_etb_draw_trigger() {
        use crate::types::ability::{AbilityAST, TriggerEvent, TriggeredAbility};
        use crate::types::card::{CardType, TypeLine};
        use crate::types::effect::EffectStep;
        use crate::types::mana::{ManaCost, ManaPip};

        let mut gs = make_state();
        gs.get_player_mut(PlayerId(0)).unwrap().mana_pool.green = 1;

        // Put a card in library so the draw trigger has something to draw.
        let library_card = {
            let def = crate::types::CardDefinition {
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
            let id = gs.alloc_id();
            let obj = CardObject::new(id, def, PlayerId(0), Zone::Library);
            gs.libraries.get_mut(&PlayerId(0)).unwrap().push(id);
            gs.add_object(obj);
            id
        };

        let def = crate::types::CardDefinition {
            name: "Elvish Visionary".into(),
            mana_cost: Some(ManaCost {
                pips: vec![ManaPip::Green],
            }),
            type_line: TypeLine {
                supertypes: vec![],
                card_types: vec![CardType::Creature],
                subtypes: vec!["Elf".into(), "Scout".into()],
            },
            oracle_text: "When this enters, draw a card.".into(),
            abilities: vec![OracleSpan::Parsed(AbilityAST::Triggered(
                TriggeredAbility {
                    trigger: TriggerEvent::EntersTheBattlefield {
                        subject_is_self: true,
                    },
                    effect: vec![EffectStep::DrawCard(1)],
                },
            ))],
            power: Some(1),
            toughness: Some(1),
        };
        let creature_id = put_in_hand(&mut gs, PlayerId(0), def);

        let gs = cast_creature(gs, PlayerId(0), creature_id).unwrap();

        // Creature on battlefield.
        assert!(gs.battlefield.contains(&creature_id));
        // Draw trigger fired: library card now in hand.
        assert!(gs.hands[&PlayerId(0)].contains(&library_card));
        assert!(gs.libraries[&PlayerId(0)].is_empty());
    }

    #[test]
    fn cast_creature_fires_etb_gain_life_trigger() {
        use crate::types::ability::{AbilityAST, TriggerEvent, TriggeredAbility};
        use crate::types::card::{CardType, TypeLine};
        use crate::types::effect::EffectStep;
        use crate::types::mana::{ManaCost, ManaPip};

        let mut gs = make_state();
        let before_life = gs.get_player(PlayerId(0)).unwrap().life;

        let def = crate::types::CardDefinition {
            name: "Gain Life Creature".into(),
            mana_cost: Some(ManaCost {
                pips: vec![ManaPip::Generic(1)],
            }),
            type_line: TypeLine {
                supertypes: vec![],
                card_types: vec![CardType::Creature],
                subtypes: vec![],
            },
            oracle_text: "When this enters, you gain 5 life.".into(),
            abilities: vec![OracleSpan::Parsed(AbilityAST::Triggered(
                TriggeredAbility {
                    trigger: TriggerEvent::EntersTheBattlefield {
                        subject_is_self: true,
                    },
                    effect: vec![EffectStep::GainLife(5)],
                },
            ))],
            power: Some(1),
            toughness: Some(1),
        };
        let creature_id = put_in_hand(&mut gs, PlayerId(0), def);

        // Give enough mana for {1}.
        gs.get_player_mut(PlayerId(0)).unwrap().mana_pool.colorless = 1;

        let gs = cast_creature(gs, PlayerId(0), creature_id).unwrap();

        assert_eq!(gs.get_player(PlayerId(0)).unwrap().life, before_life + 5);
    }
}
