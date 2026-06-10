use crate::types::ability::{Ability, TriggerEvent};
use crate::types::stack::{StackObject, StackPayload};
use crate::types::{GameState, ObjectId, OracleSpan, PlayerId};

// CR 603.2: collect ETB triggers from `entering_id` into stack objects.
// Returns Vec<StackObject> to be pushed onto the stack by the caller.
pub fn collect_etb_triggers(state: &mut GameState, entering_id: ObjectId) -> Vec<StackObject> {
    let entries: Vec<(PlayerId, Vec<crate::types::EffectStep>, String)> = {
        let obj = match state.objects.get(&entering_id) {
            Some(o) => o,
            None => return vec![],
        };
        obj.definition
            .abilities
            .iter()
            .filter_map(|span| match span {
                OracleSpan::Parsed(Ability::Triggered(t))
                    if matches!(
                        t.trigger,
                        TriggerEvent::EntersTheBattlefield {
                            subject_is_self: true
                        }
                    ) =>
                {
                    let label = format!("{}: ETB trigger", obj.definition.name);
                    Some((obj.controller, t.effect.clone(), label))
                }
                _ => None,
            })
            .collect()
    };

    entries
        .into_iter()
        .map(|(controller, effect, label)| {
            let id = state.alloc_stack_id();
            StackObject {
                id,
                payload: StackPayload::TriggeredAbility {
                    source_id: entering_id,
                    effect,
                    label,
                },
                controller,
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::ability::{Ability, TriggerEvent, TriggeredAbility};
    use crate::types::card::{CardDefinition, CardType, TypeLine};
    use crate::types::effect::EffectStep;
    use crate::types::mana::ManaCost;
    use crate::types::{
        CardObject, GameState, ObjectId, OracleSpan, PermanentState, Player, PlayerId, Zone,
    };

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
        state
            .battlefield
            .insert(id, PermanentState::new(&obj.definition));
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
            abilities: vec![OracleSpan::Parsed(Ability::Triggered(TriggeredAbility {
                trigger: TriggerEvent::EntersTheBattlefield {
                    subject_is_self: true,
                },
                effect: vec![EffectStep::DrawCard(1)],
            }))],
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
            abilities: vec![OracleSpan::Parsed(Ability::Triggered(TriggeredAbility {
                trigger: TriggerEvent::EntersTheBattlefield {
                    subject_is_self: true,
                },
                effect: vec![EffectStep::GainLife(7)],
            }))],
            power: Some(7),
            toughness: Some(7),
        }
    }

    #[test]
    fn collect_etb_draw_trigger_returns_stack_object() {
        use crate::types::stack::StackPayload;
        let mut gs = two_player_state();
        put_in_library(&mut gs, PlayerId(0));
        let creature_id = place_on_battlefield(&mut gs, etb_draw_def(), PlayerId(0));

        let triggers = collect_etb_triggers(&mut gs, creature_id);

        assert_eq!(triggers.len(), 1);
        assert_eq!(triggers[0].controller, PlayerId(0));
        let StackPayload::TriggeredAbility {
            source_id, effect, ..
        } = &triggers[0].payload
        else {
            panic!("expected TriggeredAbility");
        };
        assert_eq!(source_id, &creature_id);
        assert_eq!(*effect, vec![EffectStep::DrawCard(1)]);
    }

    #[test]
    fn collect_etb_gain_life_trigger_returns_stack_object() {
        use crate::types::stack::StackPayload;
        let mut gs = two_player_state();
        let creature_id = place_on_battlefield(&mut gs, etb_gain_life_def(), PlayerId(0));

        let triggers = collect_etb_triggers(&mut gs, creature_id);

        assert_eq!(triggers.len(), 1);
        let StackPayload::TriggeredAbility {
            source_id, effect, ..
        } = &triggers[0].payload
        else {
            panic!("expected TriggeredAbility");
        };
        assert_eq!(source_id, &creature_id);
        assert_eq!(*effect, vec![EffectStep::GainLife(7)]);
    }

    #[test]
    fn collect_etb_no_triggers_returns_empty() {
        let mut gs = two_player_state();
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
        let creature_id = place_on_battlefield(&mut gs, def, PlayerId(0));

        let triggers = collect_etb_triggers(&mut gs, creature_id);

        assert!(triggers.is_empty());
    }

    #[test]
    fn collect_etb_assigns_unique_stack_ids() {
        let mut gs = two_player_state();
        put_in_library(&mut gs, PlayerId(0));
        // creature with two ETB triggers
        let def = CardDefinition {
            name: "Multi Trigger".into(),
            mana_cost: None,
            type_line: TypeLine {
                supertypes: vec![],
                card_types: vec![CardType::Creature],
                subtypes: vec![],
            },
            oracle_text: String::new(),
            abilities: vec![
                OracleSpan::Parsed(Ability::Triggered(TriggeredAbility {
                    trigger: TriggerEvent::EntersTheBattlefield {
                        subject_is_self: true,
                    },
                    effect: vec![EffectStep::DrawCard(1)],
                })),
                OracleSpan::Parsed(Ability::Triggered(TriggeredAbility {
                    trigger: TriggerEvent::EntersTheBattlefield {
                        subject_is_self: true,
                    },
                    effect: vec![EffectStep::GainLife(2)],
                })),
            ],
            power: Some(1),
            toughness: Some(1),
        };
        let creature_id = place_on_battlefield(&mut gs, def, PlayerId(0));

        let triggers = collect_etb_triggers(&mut gs, creature_id);

        assert_eq!(triggers.len(), 2);
        assert_ne!(triggers[0].id, triggers[1].id);
    }
}
