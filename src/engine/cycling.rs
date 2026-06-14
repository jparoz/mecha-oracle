use super::EngineError;
use crate::engine::costs::pay_cost_components;
use crate::types::ability::Ability;
use crate::types::effect::EffectStep;
use crate::types::stack::{StackObject, StackPayload};
use crate::types::{GameState, ObjectId, OracleSpan, PlayerId, Zone};

/// CR 702.29: Cycling — pay the cycling cost and discard this card (cost), then draw a card (effect).
/// The draw effect is placed on the stack. The card is discarded immediately as part of the cost.
pub fn cycle_card(
    mut state: GameState,
    card_id: ObjectId,
    player_id: PlayerId,
) -> Result<GameState, EngineError> {
    if state.priority_player != player_id {
        return Err(EngineError::NotYourPriority);
    }

    // Validate card is in player's hand.
    {
        let hand = state
            .hands
            .get(&player_id)
            .ok_or(EngineError::CardNotFound)?;
        if !hand.contains(&card_id) {
            return Err(EngineError::CardNotInHand);
        }
    }

    // Find the cycling cost.
    let cycling_cost = state
        .objects
        .get(&card_id)
        .and_then(|obj| {
            obj.definition.abilities.iter().find_map(|span| {
                if let OracleSpan::Parsed(Ability::Cycling(cost)) = span {
                    Some(cost.clone())
                } else {
                    None
                }
            })
        })
        .ok_or(EngineError::AbilityIndexOutOfRange)?;

    use crate::types::ability::CostComponent;
    state = pay_cost_components(
        state,
        player_id,
        &[CostComponent::Mana(cycling_cost.clone())],
    )?;

    // Pay the discard cost: move the card from hand to graveyard.
    state
        .hands
        .get_mut(&player_id)
        .unwrap()
        .retain(|&id| id != card_id);
    if let Some(obj) = state.objects.get_mut(&card_id) {
        obj.zone = Zone::Graveyard;
    }
    state.graveyards.get_mut(&player_id).unwrap().push(card_id);

    // Put the draw effect on the stack.
    let stack_id = state.alloc_stack_id();
    let stack_obj = StackObject {
        id: stack_id,
        payload: StackPayload::ActivatedAbility {
            source_id: card_id,
            effect: vec![EffectStep::DrawCard(1)],
            label: "Cycling".into(),
        },
        controller: player_id,
        targets: vec![],
    };
    state.stack.push(stack_id);
    state.stack_objects.insert(stack_id, stack_obj);

    state.consecutive_passes = 0;
    state.priority_player = player_id;
    Ok(state)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::ability::{Ability, OracleSpan};
    use crate::types::card::{CardDefinition, CardType, TypeLine};
    use crate::types::mana::{ManaCost, ManaPip};
    use crate::types::{CardObject, Player, Step};

    fn two_player_state() -> GameState {
        let mut gs = GameState::new(vec![
            Player::new(PlayerId(0), "Alice"),
            Player::new(PlayerId(1), "Bob"),
        ]);
        gs.step = Step::PreCombatMain;
        gs
    }

    fn put_in_hand(state: &mut GameState, owner: PlayerId, def: CardDefinition) -> ObjectId {
        let id = state.alloc_id();
        let obj = CardObject::new(id, def, owner, Zone::Hand);
        state.hands.get_mut(&owner).unwrap().push(id);
        state.add_object(obj);
        id
    }

    fn put_in_library(state: &mut GameState, owner: PlayerId) -> ObjectId {
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
            text_annotations: vec![],
            power: Some(1),
            toughness: Some(1),
            colors: vec![],
        };
        let id = state.alloc_id();
        let obj = CardObject::new(id, def, owner, Zone::Library);
        state.libraries.get_mut(&owner).unwrap().push(id);
        state.add_object(obj);
        id
    }

    fn cycling_card_def(cost: ManaCost) -> CardDefinition {
        CardDefinition {
            name: "Desert".into(),
            mana_cost: None,
            type_line: TypeLine {
                supertypes: vec![],
                card_types: vec![CardType::Land],
                subtypes: vec![],
            },
            oracle_text: "Cycling".into(),
            abilities: vec![OracleSpan::Parsed(Ability::Cycling(cost))],
            text_annotations: vec![],
            power: None,
            toughness: None,
            colors: vec![],
        }
    }

    #[test]
    fn cycle_card_discards_card_and_puts_draw_on_stack() {
        let mut gs = two_player_state();
        put_in_library(&mut gs, PlayerId(0));
        gs.get_player_mut(PlayerId(0)).unwrap().mana_pool.colorless = 2;
        let cost = ManaCost {
            pips: vec![ManaPip::Generic(2)],
        };
        let card_id = put_in_hand(&mut gs, PlayerId(0), cycling_card_def(cost));

        let gs = cycle_card(gs, card_id, PlayerId(0)).unwrap();

        // Card moved to graveyard (cost).
        assert!(!gs.hands[&PlayerId(0)].contains(&card_id));
        assert!(gs.graveyards[&PlayerId(0)].contains(&card_id));
        assert_eq!(gs.objects[&card_id].zone, Zone::Graveyard);
        // DrawCard effect on stack.
        assert_eq!(gs.stack.len(), 1);
        // Mana was spent.
        assert!(gs.get_player(PlayerId(0)).unwrap().mana_pool.is_empty());
    }

    #[test]
    fn cycle_card_draw_resolves_after_stack_resolves() {
        use crate::engine::stack::resolve_top;
        let mut gs = two_player_state();
        put_in_library(&mut gs, PlayerId(0));
        gs.get_player_mut(PlayerId(0)).unwrap().mana_pool.colorless = 2;
        let cost = ManaCost {
            pips: vec![ManaPip::Generic(2)],
        };
        let card_id = put_in_hand(&mut gs, PlayerId(0), cycling_card_def(cost));

        let gs = cycle_card(gs, card_id, PlayerId(0)).unwrap();
        let gs = resolve_top(gs);

        assert_eq!(gs.hands[&PlayerId(0)].len(), 1); // drew a card
        assert!(gs.stack.is_empty());
    }

    #[test]
    fn cycle_card_insufficient_mana_returns_error() {
        let mut gs = two_player_state();
        // No mana in pool.
        let cost = ManaCost {
            pips: vec![ManaPip::Generic(2)],
        };
        let card_id = put_in_hand(&mut gs, PlayerId(0), cycling_card_def(cost));
        assert!(matches!(
            cycle_card(gs, card_id, PlayerId(0)),
            Err(EngineError::InsufficientMana)
        ));
    }

    #[test]
    fn cycle_card_not_in_hand_returns_error() {
        let mut gs = two_player_state();
        let cost = ManaCost {
            pips: vec![ManaPip::Generic(2)],
        };
        let def = cycling_card_def(cost);
        // Put the card in the library, not the hand.
        let id = gs.alloc_id();
        let obj = CardObject::new(id, def, PlayerId(0), Zone::Library);
        gs.libraries.get_mut(&PlayerId(0)).unwrap().push(id);
        gs.add_object(obj);
        assert!(matches!(
            cycle_card(gs, id, PlayerId(0)),
            Err(EngineError::CardNotInHand)
        ));
    }

    #[test]
    fn cycle_card_no_cycling_ability_returns_error() {
        let mut gs = two_player_state();
        let def = CardDefinition {
            name: "Plain Card".into(),
            mana_cost: None,
            type_line: TypeLine {
                supertypes: vec![],
                card_types: vec![CardType::Land],
                subtypes: vec![],
            },
            oracle_text: String::new(),
            abilities: vec![],
            text_annotations: vec![],
            power: None,
            toughness: None,
            colors: vec![],
        };
        let card_id = put_in_hand(&mut gs, PlayerId(0), def);
        assert!(matches!(
            cycle_card(gs, card_id, PlayerId(0)),
            Err(EngineError::AbilityIndexOutOfRange)
        ));
    }

    #[test]
    fn cycle_card_not_your_priority_returns_error() {
        let mut gs = two_player_state();
        gs.priority_player = PlayerId(1);
        let cost = ManaCost { pips: vec![] };
        let card_id = put_in_hand(&mut gs, PlayerId(0), cycling_card_def(cost));
        assert!(matches!(
            cycle_card(gs, card_id, PlayerId(0)),
            Err(EngineError::NotYourPriority)
        ));
    }
}
