pub mod activated;
pub mod casting;
pub mod combat;
pub mod costs;
pub mod cycling;
pub mod mana;
pub mod stack;
pub mod state_based_actions;
pub mod targeting;
pub mod triggered;
pub mod turn;

use crate::types::{ControllerFilter, GameState, ObjectId, PTDelta, Rule, RulesText};

#[derive(Debug, Clone, PartialEq)]
pub enum EngineError {
    CardNotFound,
    CardNotInHand,
    CardNotOnBattlefield,
    AlreadyTapped,
    InsufficientMana,
    CannotCastNow,
    LandLimitReached,
    NotALand,
    NotACreature,
    NotYourCard,
    SummoningSick,
    CreatureTapped,
    InvalidBlocker,            // blocker can't legally block this attacker
    MenaceRequiresTwoBlockers, // menace attacker has exactly one blocker
    NoManaCheckpoint,
    AbilityIndexOutOfRange,
    InvalidPaymentPlan,
    NotYourPriority,
    WrongNumberOfTargets, // CR 601.2c: wrong number of targets declared
    IllegalTarget,        // CR 601.2c: declared target is not a legal target
    InsufficientLife,     // CR 116.5: player cannot pay a life cost component
}

// CR 611.3b: continuous effects from static abilities apply at all times the source permanent
// is on the battlefield. This function sums all P/T modifications from Rule::Continuous entries
// across every battlefield permanent whose subject_filter matches `target_id`.
pub fn continuous_pt_bonus(state: &GameState, target_id: ObjectId) -> PTDelta {
    let target_obj = match state.objects.get(&target_id) {
        Some(o) => o,
        None => return PTDelta::default(),
    };
    if !state.battlefield.contains_key(&target_id) {
        return PTDelta::default();
    }

    let target_controller = target_obj.controller;
    let target_types = &target_obj.definition.type_line.card_types;
    let target_subtypes = &target_obj.definition.type_line.subtypes;
    let target_colors = &target_obj.definition.colors;

    let mut bonus = PTDelta::default();

    for (&src_id, src_perm) in &state.battlefield {
        // src_perm holds the card definition (rules text); src_obj holds the controller.
        // PermanentState has no controller field — ownership lives in CardObject.
        let src_obj = match state.objects.get(&src_id) {
            Some(o) => o,
            None => continue,
        };
        for span in &src_perm.definition.rules_text {
            if let RulesText::Active(Rule::Continuous(effect)) = span
                && let Some(delta) = effect.pt_modification
            {
                let filter = &effect.subject_filter;

                let controller_ok = match filter.controller {
                    ControllerFilter::Any => true,
                    ControllerFilter::You => src_obj.controller == target_controller,
                    ControllerFilter::Opponent => src_obj.controller != target_controller,
                };
                if !controller_ok {
                    continue;
                }

                let type_ok = filter.card_types.is_empty()
                    || filter.card_types.iter().any(|t| target_types.contains(t));
                if !type_ok {
                    continue;
                }

                let subtype_ok = filter.subtypes.is_empty()
                    || filter.subtypes.iter().all(|s| target_subtypes.contains(s));
                if !subtype_ok {
                    continue;
                }

                let color_ok = filter.colors.is_empty()
                    || filter.colors.iter().any(|c| target_colors.contains(c));
                if !color_ok {
                    continue;
                }

                bonus.power += delta.power;
                bonus.toughness += delta.toughness;
            }
        }
    }

    bonus
}

#[cfg(test)]
mod tests {
    use super::continuous_pt_bonus;
    use crate::types::{
        CardDefinition, CardObject, CardType, ContinuousEffect, ControllerFilter, GameState,
        ObjectId, PTDelta, PermanentFilter, PermanentState, Player, PlayerId, Rule, RulesText,
        Zone, card::TypeLine, mana::ManaColor,
    };

    fn two_player_state() -> GameState {
        GameState::new(vec![
            Player::new(PlayerId(0), "Alice"),
            Player::new(PlayerId(1), "Bob"),
        ])
    }

    fn add_permanent(
        state: &mut GameState,
        owner: PlayerId,
        def: CardDefinition,
        zone: Zone,
    ) -> ObjectId {
        let id = state.alloc_id();
        let obj = CardObject::new(id, def, owner, zone);
        if zone == Zone::Battlefield {
            state
                .battlefield
                .insert(id, PermanentState::new(&obj.definition));
        }
        state.add_object(obj);
        id
    }

    fn grizzly_bears_def() -> CardDefinition {
        use crate::cards::test_helpers::test_db;
        test_db().get("grizzly bears").unwrap().clone()
    }

    fn anthem_def() -> CardDefinition {
        CardDefinition {
            name: "Test Anthem".into(),
            mana_cost: None,
            type_line: TypeLine {
                supertypes: vec![],
                card_types: vec![CardType::Enchantment],
                subtypes: vec![],
            },
            oracle_text: "Creatures you control get +1/+1.".into(),
            rules_text: vec![RulesText::Active(Rule::Continuous(ContinuousEffect {
                subject_filter: PermanentFilter {
                    controller: ControllerFilter::You,
                    card_types: vec![CardType::Creature],
                    ..Default::default()
                },
                pt_modification: Some(PTDelta {
                    power: 1,
                    toughness: 1,
                }),
            }))],
            text_annotations: vec![],
            power: None,
            toughness: None,
            colors: vec![ManaColor::White],
        }
    }

    #[test]
    fn no_anthem_means_zero_bonus() {
        let mut gs = two_player_state();
        let bear_id = add_permanent(&mut gs, PlayerId(0), grizzly_bears_def(), Zone::Battlefield);
        let bonus = continuous_pt_bonus(&gs, bear_id);
        assert_eq!(
            bonus,
            PTDelta {
                power: 0,
                toughness: 0
            }
        );
    }

    #[test]
    fn anthem_grants_bonus_to_same_controller_creature() {
        let mut gs = two_player_state();
        let bear_id = add_permanent(&mut gs, PlayerId(0), grizzly_bears_def(), Zone::Battlefield);
        let _anthem_id = add_permanent(&mut gs, PlayerId(0), anthem_def(), Zone::Battlefield);
        let bonus = continuous_pt_bonus(&gs, bear_id);
        assert_eq!(
            bonus,
            PTDelta {
                power: 1,
                toughness: 1
            }
        );
    }

    #[test]
    fn anthem_does_not_apply_to_opponent_creature() {
        let mut gs = two_player_state();
        let opp_bear_id =
            add_permanent(&mut gs, PlayerId(1), grizzly_bears_def(), Zone::Battlefield);
        let _anthem_id = add_permanent(&mut gs, PlayerId(0), anthem_def(), Zone::Battlefield);
        let bonus = continuous_pt_bonus(&gs, opp_bear_id);
        assert_eq!(
            bonus,
            PTDelta {
                power: 0,
                toughness: 0
            }
        );
    }

    #[test]
    fn anthem_does_not_apply_to_non_creature() {
        let mut gs = two_player_state();
        let anthem_id = add_permanent(&mut gs, PlayerId(0), anthem_def(), Zone::Battlefield);
        // The anthem itself is an Enchantment, not a Creature — should not boost itself.
        let bonus = continuous_pt_bonus(&gs, anthem_id);
        assert_eq!(
            bonus,
            PTDelta {
                power: 0,
                toughness: 0
            }
        );
    }

    #[test]
    fn two_anthems_stack() {
        let mut gs = two_player_state();
        let bear_id = add_permanent(&mut gs, PlayerId(0), grizzly_bears_def(), Zone::Battlefield);
        let _a1 = add_permanent(&mut gs, PlayerId(0), anthem_def(), Zone::Battlefield);
        let _a2 = add_permanent(&mut gs, PlayerId(0), anthem_def(), Zone::Battlefield);
        let bonus = continuous_pt_bonus(&gs, bear_id);
        assert_eq!(
            bonus,
            PTDelta {
                power: 2,
                toughness: 2
            }
        );
    }

    #[test]
    fn bonus_drops_to_zero_when_anthem_leaves_battlefield() {
        let mut gs = two_player_state();
        let bear_id = add_permanent(&mut gs, PlayerId(0), grizzly_bears_def(), Zone::Battlefield);
        let anthem_id = add_permanent(&mut gs, PlayerId(0), anthem_def(), Zone::Battlefield);
        assert_eq!(
            continuous_pt_bonus(&gs, bear_id),
            PTDelta {
                power: 1,
                toughness: 1
            }
        );
        gs.battlefield.remove(&anthem_id);
        assert_eq!(
            continuous_pt_bonus(&gs, bear_id),
            PTDelta {
                power: 0,
                toughness: 0
            }
        );
    }
}
