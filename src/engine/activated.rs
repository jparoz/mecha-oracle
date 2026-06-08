use super::EngineError;
use crate::engine::mana::{can_pay_mana, greedy_payment_plan, pay_mana_cost};
use crate::engine::turn::draw_card;
use crate::types::ability::StaticAbility;
use crate::types::ability::{AbilityAST, ActivatedAbility, CostComponent, OracleSpan};
use crate::types::effect::EffectStep;
use crate::types::{GameState, ManaCheckpoint, ObjectId, PaymentPlan, PlayerId, Zone};

pub fn activate_ability(
    mut state: GameState,
    object_id: ObjectId,
    ability_index: usize,
    activating_player: PlayerId,
    x_value: Option<u32>,
    payment_plan: Option<PaymentPlan>,
) -> Result<GameState, EngineError> {
    // Validate object
    {
        let obj = state
            .objects
            .get(&object_id)
            .ok_or(EngineError::CardNotFound)?;
        if obj.zone != Zone::Battlefield {
            return Err(EngineError::CardNotOnBattlefield);
        }
        if obj.controller != activating_player {
            return Err(EngineError::NotYourCard);
        }
    }

    // Get the ability at index
    let ability: ActivatedAbility = state
        .objects
        .get(&object_id)
        .unwrap()
        .definition
        .abilities
        .iter()
        .filter_map(|span| match span {
            OracleSpan::Parsed(AbilityAST::Activated(a)) => Some(a.clone()),
            _ => None,
        })
        .nth(ability_index)
        .ok_or(EngineError::AbilityIndexOutOfRange)?;

    // Check costs (read-only)
    for component in &ability.cost {
        match component {
            CostComponent::Tap => {
                let obj = state.objects.get(&object_id).unwrap();
                if obj.tapped {
                    return Err(EngineError::AlreadyTapped);
                }
                if obj.is_creature() && obj.summoning_sick && !obj.has_keyword(StaticAbility::Haste)
                {
                    return Err(EngineError::SummoningSick);
                }
            }
            CostComponent::Mana(cost) => {
                let player = state
                    .get_player(activating_player)
                    .ok_or(EngineError::CardNotFound)?;
                if !can_pay_mana(cost, &player.mana_pool, player.life) {
                    return Err(EngineError::InsufficientMana);
                }
            }
            _ => {} // Unimplemented, PayLife, Sacrifice, Discard — not enforced
        }
    }

    // If this is a mana ability, create checkpoint before paying anything
    let produces_mana = ability
        .effect
        .iter()
        .any(|e| matches!(e, EffectStep::AddMana(_)));
    if produces_mana && state.mana_checkpoint.is_none() {
        let pools = state
            .players
            .iter()
            .map(|p| (p.id, p.mana_pool.clone()))
            .collect();
        state.mana_checkpoint = Some(ManaCheckpoint {
            pools,
            tapped_lands: vec![],
        });
    }

    // Pay costs
    for component in ability.cost.iter() {
        match component {
            CostComponent::Tap => {
                if produces_mana {
                    state
                        .mana_checkpoint
                        .as_mut()
                        .unwrap()
                        .tapped_lands
                        .push(object_id);
                }
                state.objects.get_mut(&object_id).unwrap().tapped = true;
            }
            CostComponent::Mana(cost) => {
                let plan = match &payment_plan {
                    Some(p) => {
                        let mut p = p.clone();
                        if let Some(xv) = x_value {
                            p.x_value = Some(xv);
                        }
                        p
                    }
                    None => {
                        let player = state
                            .get_player(activating_player)
                            .ok_or(EngineError::CardNotFound)?;
                        let mut p = greedy_payment_plan(cost, &player.mana_pool, player.life)
                            .ok_or(EngineError::InsufficientMana)?;
                        if let Some(xv) = x_value {
                            p.x_value = Some(xv);
                        }
                        p
                    }
                };
                state = pay_mana_cost(state, activating_player, cost, &plan)?;
            }
            _ => {}
        }
    }

    // Apply effects
    for step in &ability.effect {
        match step {
            EffectStep::AddMana(pool_add) => {
                // CR 107.4k: mana from a Snow source is snow-tagged.
                let is_snow = state
                    .objects
                    .get(&object_id)
                    .map(|obj| {
                        obj.definition
                            .type_line
                            .supertypes
                            .contains(&crate::types::card::Supertype::Snow)
                    })
                    .unwrap_or(false);
                let player = state.get_player_mut(activating_player).unwrap();
                if is_snow {
                    player
                        .mana_pool
                        .add_snow(crate::types::mana::ManaColor::White, pool_add.white);
                    player
                        .mana_pool
                        .add_snow(crate::types::mana::ManaColor::Blue, pool_add.blue);
                    player
                        .mana_pool
                        .add_snow(crate::types::mana::ManaColor::Black, pool_add.black);
                    player
                        .mana_pool
                        .add_snow(crate::types::mana::ManaColor::Red, pool_add.red);
                    player
                        .mana_pool
                        .add_snow(crate::types::mana::ManaColor::Green, pool_add.green);
                    player
                        .mana_pool
                        .add_snow(crate::types::mana::ManaColor::Colorless, pool_add.colorless);
                } else {
                    player.mana_pool.white += pool_add.white;
                    player.mana_pool.blue += pool_add.blue;
                    player.mana_pool.black += pool_add.black;
                    player.mana_pool.red += pool_add.red;
                    player.mana_pool.green += pool_add.green;
                    player.mana_pool.colorless += pool_add.colorless;
                }
            }
            EffectStep::Mill(n) => {
                let to_mill = (*n as usize).min(
                    state
                        .libraries
                        .get(&activating_player)
                        .map_or(0, |l| l.len()),
                );
                for _ in 0..to_mill {
                    if let Some(card_id) = state
                        .libraries
                        .get_mut(&activating_player)
                        .filter(|l| !l.is_empty())
                        .map(|l| l.remove(0))
                    {
                        state
                            .graveyards
                            .get_mut(&activating_player)
                            .unwrap()
                            .push(card_id);
                        if let Some(obj) = state.objects.get_mut(&card_id) {
                            obj.zone = Zone::Graveyard;
                        }
                    }
                }
            }
            EffectStep::DrawCard(n) => {
                for _ in 0..*n {
                    state = draw_card(state, activating_player);
                }
            }
            EffectStep::GainLife(_) => {
                debug_assert!(false, "GainLife not expected in activated ability effect");
            }
        }
    }

    Ok(state)
}

pub fn can_pay_cost(
    state: &GameState,
    object_id: ObjectId,
    ability: &ActivatedAbility,
    player: PlayerId,
) -> bool {
    for component in &ability.cost {
        match component {
            CostComponent::Tap => {
                let obj = match state.objects.get(&object_id) {
                    Some(o) if o.zone == Zone::Battlefield => o,
                    _ => return false,
                };
                if obj.tapped {
                    return false;
                }
                if obj.is_creature() && obj.summoning_sick && !obj.has_keyword(StaticAbility::Haste)
                {
                    return false;
                }
            }
            CostComponent::Mana(cost) => {
                let p = match state.get_player(player) {
                    Some(p) => p,
                    None => return false,
                };
                if !can_pay_mana(cost, &p.mana_pool, p.life) {
                    return false;
                }
            }
            _ => {}
        }
    }
    true
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::ability::{AbilityAST, ActivatedAbility, CostComponent};
    use crate::types::card::{CardDefinition, CardType, TypeLine};
    use crate::types::effect::EffectStep;
    use crate::types::mana::{ManaCost, ManaPip, ManaPool};
    use crate::types::{CardObject, Player};

    fn two_player_state() -> GameState {
        GameState::new(vec![
            Player::new(PlayerId(0), "Alice"),
            Player::new(PlayerId(1), "Bob"),
        ])
    }

    fn make_tap_green_def() -> CardDefinition {
        CardDefinition {
            name: "Llanowar Elves".into(),
            mana_cost: Some(ManaCost {
                pips: vec![ManaPip::Green],
            }),
            type_line: TypeLine {
                supertypes: vec![],
                card_types: vec![CardType::Creature],
                subtypes: vec!["Elf".into(), "Druid".into()],
            },
            oracle_text: "{T}: Add {G}.".into(),
            abilities: vec![OracleSpan::Parsed(AbilityAST::Activated(
                ActivatedAbility {
                    cost: vec![CostComponent::Tap],
                    effect: vec![EffectStep::AddMana(ManaPool {
                        green: 1,
                        ..Default::default()
                    })],
                },
            ))],
            power: Some(1),
            toughness: Some(1),
        }
    }

    fn make_mill_def() -> CardDefinition {
        CardDefinition {
            name: "Mill Thingy".into(),
            mana_cost: None,
            type_line: TypeLine {
                supertypes: vec![],
                card_types: vec![CardType::Artifact],
                subtypes: vec![],
            },
            oracle_text: "{T}: Mill 2.".into(),
            abilities: vec![OracleSpan::Parsed(AbilityAST::Activated(
                ActivatedAbility {
                    cost: vec![CostComponent::Tap],
                    effect: vec![EffectStep::Mill(2)],
                },
            ))],
            power: None,
            toughness: None,
        }
    }

    fn make_draw_def() -> CardDefinition {
        CardDefinition {
            name: "Staff".into(),
            mana_cost: None,
            type_line: TypeLine {
                supertypes: vec![],
                card_types: vec![CardType::Artifact],
                subtypes: vec![],
            },
            oracle_text: "{1}: Draw a card.".into(),
            abilities: vec![OracleSpan::Parsed(AbilityAST::Activated(
                ActivatedAbility {
                    cost: vec![CostComponent::Mana(ManaCost {
                        pips: vec![ManaPip::Generic(1)],
                    })],
                    effect: vec![EffectStep::DrawCard(1)],
                },
            ))],
            power: None,
            toughness: None,
        }
    }

    fn place_on_battlefield(
        state: &mut GameState,
        def: CardDefinition,
        owner: PlayerId,
    ) -> ObjectId {
        let id = state.alloc_id();
        let mut obj = CardObject::new(id, def, owner, Zone::Battlefield);
        obj.summoning_sick = false;
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

    #[test]
    fn tap_mana_ability_taps_and_adds_mana() {
        let mut gs = two_player_state();
        let id = place_on_battlefield(&mut gs, make_tap_green_def(), PlayerId(0));
        let gs = activate_ability(gs, id, 0, PlayerId(0), None, None).unwrap();
        assert!(gs.objects[&id].tapped);
        assert_eq!(gs.get_player(PlayerId(0)).unwrap().mana_pool.green, 1);
    }

    #[test]
    fn tap_mana_ability_creates_checkpoint() {
        let mut gs = two_player_state();
        let id = place_on_battlefield(&mut gs, make_tap_green_def(), PlayerId(0));
        let gs = activate_ability(gs, id, 0, PlayerId(0), None, None).unwrap();
        assert!(gs.mana_checkpoint.is_some());
        assert_eq!(gs.mana_checkpoint.as_ref().unwrap().tapped_lands, vec![id]);
    }

    #[test]
    fn already_tapped_returns_error() {
        let mut gs = two_player_state();
        let id = place_on_battlefield(&mut gs, make_tap_green_def(), PlayerId(0));
        gs.objects.get_mut(&id).unwrap().tapped = true;
        assert!(matches!(
            activate_ability(gs, id, 0, PlayerId(0), None, None),
            Err(EngineError::AlreadyTapped)
        ));
    }

    #[test]
    fn summoning_sick_creature_with_tap_cost_returns_error() {
        let mut gs = two_player_state();
        let id = place_on_battlefield(&mut gs, make_tap_green_def(), PlayerId(0));
        gs.objects.get_mut(&id).unwrap().summoning_sick = true;
        assert!(matches!(
            activate_ability(gs, id, 0, PlayerId(0), None, None),
            Err(EngineError::SummoningSick)
        ));
    }

    #[test]
    fn insufficient_mana_returns_error() {
        let mut gs = two_player_state();
        let id = place_on_battlefield(&mut gs, make_draw_def(), PlayerId(0));
        assert!(matches!(
            activate_ability(gs, id, 0, PlayerId(0), None, None),
            Err(EngineError::InsufficientMana)
        ));
    }

    #[test]
    fn mana_cost_ability_deducts_mana() {
        let mut gs = two_player_state();
        let id = place_on_battlefield(&mut gs, make_draw_def(), PlayerId(0));
        gs.get_player_mut(PlayerId(0)).unwrap().mana_pool.colorless = 1;
        put_in_library(&mut gs, PlayerId(0));
        let gs = activate_ability(gs, id, 0, PlayerId(0), None, None).unwrap();
        assert!(gs.get_player(PlayerId(0)).unwrap().mana_pool.is_empty());
        assert_eq!(gs.hands[&PlayerId(0)].len(), 1);
    }

    #[test]
    fn mill_two_moves_top_two_to_graveyard() {
        let mut gs = two_player_state();
        let id = place_on_battlefield(&mut gs, make_mill_def(), PlayerId(0));
        let card1 = put_in_library(&mut gs, PlayerId(0));
        let card2 = put_in_library(&mut gs, PlayerId(0));
        let gs = activate_ability(gs, id, 0, PlayerId(0), None, None).unwrap();
        assert!(gs.libraries[&PlayerId(0)].is_empty());
        assert!(gs.graveyards[&PlayerId(0)].contains(&card1));
        assert!(gs.graveyards[&PlayerId(0)].contains(&card2));
    }

    #[test]
    fn mill_with_fewer_cards_than_n_mills_all_without_error() {
        let mut gs = two_player_state();
        let id = place_on_battlefield(&mut gs, make_mill_def(), PlayerId(0));
        let card1 = put_in_library(&mut gs, PlayerId(0));
        let gs = activate_ability(gs, id, 0, PlayerId(0), None, None).unwrap();
        assert!(gs.libraries[&PlayerId(0)].is_empty());
        assert!(gs.graveyards[&PlayerId(0)].contains(&card1));
    }

    #[test]
    fn ability_index_out_of_range_returns_error() {
        let mut gs = two_player_state();
        let id = place_on_battlefield(&mut gs, make_tap_green_def(), PlayerId(0));
        assert!(matches!(
            activate_ability(gs, id, 99, PlayerId(0), None, None),
            Err(EngineError::AbilityIndexOutOfRange)
        ));
    }

    #[test]
    fn unimplemented_cost_component_is_skipped() {
        let def = CardDefinition {
            name: "Free Mill".into(),
            mana_cost: None,
            type_line: TypeLine {
                supertypes: vec![],
                card_types: vec![CardType::Artifact],
                subtypes: vec![],
            },
            oracle_text: "Sacrifice a creature: Mill 2.".into(),
            abilities: vec![OracleSpan::Parsed(AbilityAST::Activated(
                ActivatedAbility {
                    cost: vec![CostComponent::Unimplemented("Sacrifice a creature".into())],
                    effect: vec![EffectStep::Mill(2)],
                },
            ))],
            power: None,
            toughness: None,
        };
        let mut gs = two_player_state();
        let id = place_on_battlefield(&mut gs, def, PlayerId(0));
        put_in_library(&mut gs, PlayerId(0));
        put_in_library(&mut gs, PlayerId(0));
        let gs = activate_ability(gs, id, 0, PlayerId(0), None, None).unwrap();
        assert!(gs.libraries[&PlayerId(0)].is_empty());
    }

    #[test]
    fn can_pay_cost_true_when_untapped_and_mana_sufficient() {
        let mut gs = two_player_state();
        let id = place_on_battlefield(&mut gs, make_tap_green_def(), PlayerId(0));
        let ability = ActivatedAbility {
            cost: vec![CostComponent::Tap],
            effect: vec![EffectStep::AddMana(ManaPool::default())],
        };
        assert!(can_pay_cost(&gs, id, &ability, PlayerId(0)));
    }

    #[test]
    fn can_pay_cost_false_when_tapped() {
        let mut gs = two_player_state();
        let id = place_on_battlefield(&mut gs, make_tap_green_def(), PlayerId(0));
        gs.objects.get_mut(&id).unwrap().tapped = true;
        let ability = ActivatedAbility {
            cost: vec![CostComponent::Tap],
            effect: vec![],
        };
        assert!(!can_pay_cost(&gs, id, &ability, PlayerId(0)));
    }

    #[test]
    fn snow_mana_source_adds_snow_tagged_mana() {
        use crate::types::card::{CardDefinition, CardType, Supertype, TypeLine};
        let snow_elves_def = CardDefinition {
            name: "Snow Elves".into(),
            mana_cost: Some(ManaCost {
                pips: vec![ManaPip::Green],
            }),
            type_line: TypeLine {
                supertypes: vec![Supertype::Snow],
                card_types: vec![CardType::Creature],
                subtypes: vec!["Elf".into()],
            },
            oracle_text: "{T}: Add {G}.".into(),
            abilities: vec![OracleSpan::Parsed(AbilityAST::Activated(
                ActivatedAbility {
                    cost: vec![CostComponent::Tap],
                    effect: vec![EffectStep::AddMana(ManaPool {
                        green: 1,
                        ..Default::default()
                    })],
                },
            ))],
            power: Some(1),
            toughness: Some(1),
        };
        let mut gs = two_player_state();
        let id = place_on_battlefield(&mut gs, snow_elves_def, PlayerId(0));
        let gs = activate_ability(gs, id, 0, PlayerId(0), None, None).unwrap();
        let pool = &gs.get_player(PlayerId(0)).unwrap().mana_pool;
        assert_eq!(pool.green, 1);
        assert_eq!(pool.snow_green, 1);
    }
}
