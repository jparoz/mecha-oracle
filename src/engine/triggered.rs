use crate::engine::turn::draw_card;
use crate::types::ability::{AbilityAST, TriggerEvent};
use crate::types::effect::EffectStep;
use crate::types::{GameState, ObjectId, OracleSpan};

// CR 603.2: triggered abilities trigger when their trigger event occurs.
// Phase C fires ETB triggers immediately (fire-and-forget, no stack).
// Stack project: replace this body with "collect onto stack"; signature stays.
pub fn fire_etb_triggers(mut state: GameState, entering_id: ObjectId) -> GameState {
    let (controller, effects): (_, Vec<_>) = {
        let obj = match state.objects.get(&entering_id) {
            Some(o) => o,
            None => return state,
        };
        let triggered = obj
            .definition
            .abilities
            .iter()
            .filter_map(|span| match span {
                OracleSpan::Parsed(AbilityAST::Triggered(t))
                    if matches!(
                        t.trigger,
                        TriggerEvent::EntersTheBattlefield {
                            subject_is_self: true
                        }
                    ) =>
                {
                    Some(t.effect.clone())
                }
                _ => None,
            })
            .collect();
        (obj.controller, triggered)
    };

    for effect in effects {
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
                _ => {
                    debug_assert!(false, "unexpected EffectStep in ETB trigger: {step:?}");
                }
            }
        }
    }

    state
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::ability::{AbilityAST, TriggerEvent, TriggeredAbility};
    use crate::types::card::{CardDefinition, CardType, TypeLine};
    use crate::types::effect::EffectStep;
    use crate::types::mana::ManaCost;
    use crate::types::{CardObject, GameState, ObjectId, OracleSpan, Player, PlayerId, Zone};

    fn two_player_state() -> GameState {
        GameState::new(vec![
            Player::new(PlayerId(0), "Alice"),
            Player::new(PlayerId(1), "Bob"),
        ])
    }

    fn place_on_battlefield(
        state: &mut GameState,
        def: CardDefinition,
        owner: PlayerId,
    ) -> ObjectId {
        let id = state.alloc_id();
        let obj = CardObject::new(id, def, owner, Zone::Battlefield);
        state.battlefield.push(id);
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
            power: Some(1),
            toughness: Some(1),
        };
        let id = state.alloc_id();
        let obj = CardObject::new(id, def, owner, Zone::Library);
        state.libraries.get_mut(&owner).unwrap().push(id);
        state.add_object(obj);
        id
    }

    fn etb_draw_def() -> CardDefinition {
        CardDefinition {
            name: "Elvish Visionary".into(),
            mana_cost: Some(ManaCost { pips: vec![] }),
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
        }
    }

    fn etb_gain_life_def() -> CardDefinition {
        CardDefinition {
            name: "Pelakka Wurm".into(),
            mana_cost: Some(ManaCost { pips: vec![] }),
            type_line: TypeLine {
                supertypes: vec![],
                card_types: vec![CardType::Creature],
                subtypes: vec!["Wurm".into()],
            },
            oracle_text: "When this enters, you gain 7 life.".into(),
            abilities: vec![OracleSpan::Parsed(AbilityAST::Triggered(
                TriggeredAbility {
                    trigger: TriggerEvent::EntersTheBattlefield {
                        subject_is_self: true,
                    },
                    effect: vec![EffectStep::GainLife(7)],
                },
            ))],
            power: Some(7),
            toughness: Some(7),
        }
    }

    #[test]
    fn etb_draw_trigger_draws_card_for_controller() {
        let mut gs = two_player_state();
        let library_card = put_in_library(&mut gs, PlayerId(0));
        let creature_id = place_on_battlefield(&mut gs, etb_draw_def(), PlayerId(0));

        let gs = fire_etb_triggers(gs, creature_id);

        assert!(gs.hands[&PlayerId(0)].contains(&library_card));
        assert!(gs.libraries[&PlayerId(0)].is_empty());
        // Opponent's hand unchanged
        assert!(gs.hands[&PlayerId(1)].is_empty());
    }

    #[test]
    fn etb_gain_life_trigger_increases_controller_life() {
        let mut gs = two_player_state();
        let creature_id = place_on_battlefield(&mut gs, etb_gain_life_def(), PlayerId(0));
        let before = gs.get_player(PlayerId(0)).unwrap().life;

        let gs = fire_etb_triggers(gs, creature_id);

        assert_eq!(gs.get_player(PlayerId(0)).unwrap().life, before + 7);
        // Opponent's life unchanged
        assert_eq!(gs.get_player(PlayerId(1)).unwrap().life, before);
    }

    #[test]
    fn etb_multistep_effect_applies_all_steps() {
        let def = CardDefinition {
            name: "Multi".into(),
            mana_cost: None,
            type_line: TypeLine {
                supertypes: vec![],
                card_types: vec![CardType::Creature],
                subtypes: vec![],
            },
            oracle_text: "When this enters, draw a card. You gain 2 life.".into(),
            abilities: vec![OracleSpan::Parsed(AbilityAST::Triggered(
                TriggeredAbility {
                    trigger: TriggerEvent::EntersTheBattlefield {
                        subject_is_self: true,
                    },
                    effect: vec![EffectStep::DrawCard(1), EffectStep::GainLife(2)],
                },
            ))],
            power: Some(1),
            toughness: Some(1),
        };
        let mut gs = two_player_state();
        put_in_library(&mut gs, PlayerId(0));
        let creature_id = place_on_battlefield(&mut gs, def, PlayerId(0));
        let before_life = gs.get_player(PlayerId(0)).unwrap().life;

        let gs = fire_etb_triggers(gs, creature_id);

        assert_eq!(gs.hands[&PlayerId(0)].len(), 1);
        assert_eq!(gs.get_player(PlayerId(0)).unwrap().life, before_life + 2);
    }

    #[test]
    fn no_triggered_abilities_returns_state_unchanged() {
        let def = CardDefinition {
            name: "Vanilla".into(),
            mana_cost: None,
            type_line: TypeLine {
                supertypes: vec![],
                card_types: vec![CardType::Creature],
                subtypes: vec![],
            },
            oracle_text: String::new(),
            abilities: vec![],
            power: Some(2),
            toughness: Some(2),
        };
        let mut gs = two_player_state();
        let creature_id = place_on_battlefield(&mut gs, def, PlayerId(0));
        let before_life = gs.get_player(PlayerId(0)).unwrap().life;

        let gs = fire_etb_triggers(gs, creature_id);

        assert_eq!(gs.get_player(PlayerId(0)).unwrap().life, before_life);
        assert!(gs.hands[&PlayerId(0)].is_empty());
    }
}
