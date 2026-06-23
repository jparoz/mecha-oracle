use super::EngineError;
use crate::engine::costs::{can_pay_cost_components, pay_cost_components};
use crate::types::ability::{ActivatedAbility, CostComponent, Rule, RulesText};
use crate::types::effect::EffectStep;
use crate::types::{GameState, ManaCheckpoint, ObjectId, PlayerId, Zone};

pub fn activate_ability(
    mut state: GameState,
    object_id: ObjectId,
    ability_index: usize,
    activating_player: PlayerId,
    x_value: Option<u32>,
    declared_targets: Vec<crate::types::effect::EffectTarget>,
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
        .rules_text
        .iter()
        .filter_map(|span| match span {
            RulesText::Active(Rule::Activated(a)) => Some(a.clone()),
            _ => None,
        })
        .nth(ability_index)
        .ok_or(EngineError::AbilityIndexOutOfRange)?;

    // Target validation for non-mana activated abilities
    let produces_mana = ability
        .effect
        .iter()
        .any(|e| matches!(e, EffectStep::AddMana(_)));
    if !produces_mana {
        use crate::engine::targeting::is_legal_target;
        if ability.target_requirements.len() != declared_targets.len() {
            return Err(EngineError::WrongNumberOfTargets);
        }
        let source_colors: Vec<crate::types::mana::ManaColor> = state
            .objects
            .get(&object_id)
            .map(|o| o.definition.colors.clone())
            .unwrap_or_default();
        for (filter, target) in ability
            .target_requirements
            .iter()
            .zip(declared_targets.iter())
        {
            if !is_legal_target(&state, target, filter, activating_player, &source_colors) {
                return Err(EngineError::IllegalTarget);
            }
        }
    }

    // CR 602.2: verify structural feasibility before mutating state.
    if !can_pay_cost_components(&state, activating_player, Some(object_id), &ability.cost) {
        for component in &ability.cost {
            if let CostComponent::Tap = component {
                let perm = state.battlefield.get(&object_id).unwrap();
                if perm.tapped {
                    return Err(EngineError::AlreadyTapped);
                }
                return Err(EngineError::SummoningSick);
            }
        }
        return Err(EngineError::NotYourPriority);
    }

    // If this is a mana ability, create checkpoint before paying anything
    // (produces_mana is already computed above for target validation)
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

    // Pay costs.
    for component in &ability.cost {
        if let CostComponent::Tap = component {
            if produces_mana {
                state
                    .mana_checkpoint
                    .as_mut()
                    .unwrap()
                    .tapped_lands
                    .push(object_id);
            }
            state.battlefield.get_mut(&object_id).unwrap().tapped = true;
        }
    }
    let non_tap: Vec<_> = ability
        .cost
        .iter()
        .filter(|c| !matches!(c, CostComponent::Tap))
        .cloned()
        .collect();
    if !non_tap.is_empty() {
        state = pay_cost_components(state, activating_player, &non_tap, x_value)?;
    }

    if produces_mana {
        // CR 405.6c: mana abilities resolve immediately without using the stack.
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
                _ => unreachable!("non-AddMana step in mana ability effect: {:?}", step),
            }
        }
        // CR 405.6c: the player who had priority before activating retains it afterward.
        // priority_player is already set to that player; no change needed.
        Ok(state)
    } else {
        // CR 405: non-mana activated abilities use the stack.
        // CR 117.1b: a player may activate an activated ability only when they have priority.
        if state.priority_player != activating_player {
            return Err(EngineError::NotYourPriority);
        }
        let label = state
            .objects
            .get(&object_id)
            .map(|o| format!("{}: activated ability", o.definition.name))
            .unwrap_or_else(|| "activated ability".into());
        // Guard: only inject keyword flags from permanents currently on the battlefield.
        // The outer battlefield.get ensures we do not inject from any lingering object ID.
        let (source_rules_text, source_colors, source_card_types, source_subtypes) = state
            .battlefield
            .get(&object_id)
            .and_then(|_| state.objects.get(&object_id))
            .map(|o| {
                (
                    o.definition.rules_text.clone(),
                    o.definition.colors.clone(),
                    o.definition.type_line.card_types.clone(),
                    o.definition.type_line.subtypes.clone(),
                )
            })
            .unwrap_or_default();
        let stack_id = state.alloc_stack_id();
        let stack_obj = crate::types::StackObject {
            id: stack_id,
            payload: crate::types::StackPayload::ActivatedAbility {
                source_id: object_id,
                effect: crate::engine::stack::inject_source_flags(
                    ability.effect.clone(),
                    &source_rules_text,
                    &source_colors,
                    &source_card_types,
                    &source_subtypes,
                ),
                label,
            },
            controller: activating_player,
            targets: declared_targets,
            x_value,
        };
        state.stack.push(stack_id);
        state.stack_objects.insert(stack_id, stack_obj);
        // CR 117.3c: activator retains priority after activating a non-mana ability.
        state.consecutive_passes = 0;
        state.priority_player = activating_player;

        // CR 702.21a: if any declared target is an opponent-controlled battlefield permanent,
        // fire TargetedBy event to collect Ward triggers (Ward is now a TriggeredAbility).
        let ability_targets = state.stack_objects[&stack_id].targets.clone();
        let mut ward_triggers = Vec::new();
        for target in &ability_targets {
            if let crate::types::effect::EffectTarget::Object { id: target_id } = target
                && state
                    .objects
                    .get(target_id)
                    .map(|o| o.controller != activating_player)
                    .unwrap_or(false)
                && state.battlefield.contains_key(target_id)
            {
                let mut t = super::triggered::collect_triggers_for_event(
                    &mut state,
                    &crate::types::GameEvent::TargetedBy {
                        target_id: *target_id,
                        acting_player: activating_player,
                    },
                );
                // Ward triggers must target the triggering ability so CounterSpell resolves correctly.
                for trigger in &mut t {
                    trigger
                        .targets
                        .push(crate::types::effect::EffectTarget::StackObject { id: stack_id });
                }
                ward_triggers.append(&mut t);
            }
        }
        for wt in ward_triggers.into_iter().rev() {
            let id = wt.id;
            state.stack.push(id);
            state.stack_objects.insert(id, wt);
        }

        Ok(state)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::engine::costs::can_pay_cost_components;
    use crate::types::ability::{ActivatedAbility, CostComponent, Rule};
    use crate::types::card::{CardDefinition, CardType, TypeLine};
    use crate::types::effect::EffectStep;
    use crate::types::mana::{ManaCost, ManaPip, ManaPool};
    use crate::types::{CardObject, PermanentState, Player};

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
            rules_text: vec![RulesText::Active(Rule::Activated(ActivatedAbility {
                cost: vec![CostComponent::Tap],
                target_requirements: vec![],
                effect: vec![EffectStep::AddMana(ManaPool {
                    green: 1,
                    ..Default::default()
                })],
            }))],
            text_annotations: vec![],
            power: Some(1),
            toughness: Some(1),
            colors: vec![],
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
            rules_text: vec![RulesText::Active(Rule::Activated(ActivatedAbility {
                cost: vec![CostComponent::Tap],
                target_requirements: vec![],
                effect: vec![EffectStep::Mill(2)],
            }))],
            text_annotations: vec![],
            power: None,
            toughness: None,
            colors: vec![],
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
            rules_text: vec![RulesText::Active(Rule::Activated(ActivatedAbility {
                cost: vec![CostComponent::Mana(ManaCost {
                    pips: vec![ManaPip::Generic(1)],
                })],
                target_requirements: vec![],
                effect: vec![EffectStep::DrawCard(1)],
            }))],
            text_annotations: vec![],
            power: None,
            toughness: None,
            colors: vec![],
        }
    }

    fn place_on_battlefield(
        state: &mut GameState,
        def: CardDefinition,
        owner: PlayerId,
    ) -> ObjectId {
        let id = state.alloc_id();
        let obj = CardObject::new(id, def, owner, Zone::Battlefield);
        let mut perm = PermanentState::new(&obj.definition);
        perm.controller_since_turn = 0;
        state.battlefield.insert(id, perm);
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
            rules_text: vec![],
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

    #[test]
    fn tap_mana_ability_taps_and_adds_mana() {
        let mut gs = two_player_state();
        let id = place_on_battlefield(&mut gs, make_tap_green_def(), PlayerId(0));
        let gs = activate_ability(gs, id, 0, PlayerId(0), None, vec![]).unwrap();
        assert!(gs.battlefield[&id].tapped);
        assert_eq!(gs.get_player(PlayerId(0)).unwrap().mana_pool.green, 1);
    }

    #[test]
    fn tap_mana_ability_creates_checkpoint() {
        let mut gs = two_player_state();
        let id = place_on_battlefield(&mut gs, make_tap_green_def(), PlayerId(0));
        let gs = activate_ability(gs, id, 0, PlayerId(0), None, vec![]).unwrap();
        assert!(gs.mana_checkpoint.is_some());
        assert_eq!(gs.mana_checkpoint.as_ref().unwrap().tapped_lands, vec![id]);
    }

    #[test]
    fn already_tapped_returns_error() {
        let mut gs = two_player_state();
        let id = place_on_battlefield(&mut gs, make_tap_green_def(), PlayerId(0));
        gs.battlefield.get_mut(&id).unwrap().tapped = true;
        assert!(matches!(
            activate_ability(gs, id, 0, PlayerId(0), None, vec![]),
            Err(EngineError::AlreadyTapped)
        ));
    }

    #[test]
    fn summoning_sick_creature_with_tap_cost_returns_error() {
        let mut gs = two_player_state();
        let id = place_on_battlefield(&mut gs, make_tap_green_def(), PlayerId(0));
        gs.battlefield.get_mut(&id).unwrap().controller_since_turn = u32::MAX;
        assert!(matches!(
            activate_ability(gs, id, 0, PlayerId(0), None, vec![]),
            Err(EngineError::SummoningSick)
        ));
    }

    #[test]
    fn insufficient_mana_returns_error() {
        let mut gs = two_player_state();
        let id = place_on_battlefield(&mut gs, make_draw_def(), PlayerId(0));
        assert!(matches!(
            activate_ability(gs, id, 0, PlayerId(0), None, vec![]),
            Err(EngineError::InsufficientMana)
        ));
    }

    #[test]
    fn non_mana_activate_puts_on_stack_not_immediate() {
        let mut gs = two_player_state();
        gs.step = crate::types::Step::PreCombatMain;
        let id = place_on_battlefield(&mut gs, make_draw_def(), PlayerId(0));
        gs.get_player_mut(PlayerId(0)).unwrap().mana_pool.colorless = 1;
        put_in_library(&mut gs, PlayerId(0));

        let gs = activate_ability(gs, id, 0, PlayerId(0), None, vec![]).unwrap();

        // Mana was spent but effect not yet applied
        assert!(gs.get_player(PlayerId(0)).unwrap().mana_pool.is_empty());
        assert!(gs.hands[&PlayerId(0)].is_empty());
        assert_eq!(gs.stack.len(), 1);
    }

    #[test]
    fn non_mana_activate_resolves_via_resolve_top() {
        use crate::engine::stack::resolve_top;
        let mut gs = two_player_state();
        gs.step = crate::types::Step::PreCombatMain;
        let id = place_on_battlefield(&mut gs, make_draw_def(), PlayerId(0));
        gs.get_player_mut(PlayerId(0)).unwrap().mana_pool.colorless = 1;
        put_in_library(&mut gs, PlayerId(0));

        let gs = activate_ability(gs, id, 0, PlayerId(0), None, vec![]).unwrap();
        let gs = resolve_top(gs);

        assert_eq!(gs.hands[&PlayerId(0)].len(), 1);
        assert!(gs.stack.is_empty());
    }

    #[test]
    fn non_mana_activate_not_your_priority_returns_error() {
        let mut gs = two_player_state();
        gs.step = crate::types::Step::PreCombatMain;
        let id = place_on_battlefield(&mut gs, make_mill_def(), PlayerId(0));
        put_in_library(&mut gs, PlayerId(0));
        gs.priority_player = PlayerId(1); // opponent has priority

        assert!(matches!(
            activate_ability(gs, id, 0, PlayerId(0), None, vec![]),
            Err(EngineError::NotYourPriority)
        ));
    }

    #[test]
    fn mill_two_puts_on_stack_not_immediate() {
        let mut gs = two_player_state();
        gs.step = crate::types::Step::PreCombatMain;
        let id = place_on_battlefield(&mut gs, make_mill_def(), PlayerId(0));
        put_in_library(&mut gs, PlayerId(0));
        put_in_library(&mut gs, PlayerId(0));

        let gs = activate_ability(gs, id, 0, PlayerId(0), None, vec![]).unwrap();

        assert_eq!(gs.libraries[&PlayerId(0)].len(), 2); // not milled yet
        assert_eq!(gs.stack.len(), 1);
    }

    #[test]
    fn mill_two_resolves_via_resolve_top() {
        use crate::engine::stack::resolve_top;
        let mut gs = two_player_state();
        gs.step = crate::types::Step::PreCombatMain;
        let id = place_on_battlefield(&mut gs, make_mill_def(), PlayerId(0));
        let card1 = put_in_library(&mut gs, PlayerId(0));
        let card2 = put_in_library(&mut gs, PlayerId(0));

        let gs = activate_ability(gs, id, 0, PlayerId(0), None, vec![]).unwrap();
        let gs = resolve_top(gs);

        assert!(gs.libraries[&PlayerId(0)].is_empty());
        assert!(gs.graveyards[&PlayerId(0)].contains(&card1));
        assert!(gs.graveyards[&PlayerId(0)].contains(&card2));
    }

    #[test]
    fn mill_fewer_cards_than_n_resolves_without_error() {
        use crate::engine::stack::resolve_top;
        let mut gs = two_player_state();
        gs.step = crate::types::Step::PreCombatMain;
        let id = place_on_battlefield(&mut gs, make_mill_def(), PlayerId(0));
        let card1 = put_in_library(&mut gs, PlayerId(0)); // only 1 card, mill 2

        let gs = activate_ability(gs, id, 0, PlayerId(0), None, vec![]).unwrap();
        let gs = resolve_top(gs);

        assert!(gs.libraries[&PlayerId(0)].is_empty());
        assert!(gs.graveyards[&PlayerId(0)].contains(&card1));
    }

    #[test]
    fn unimplemented_cost_puts_effect_on_stack() {
        use crate::engine::stack::resolve_top;
        use crate::types::ability::{ActivatedAbility, CostComponent, Rule};
        use crate::types::card::{CardDefinition, CardType, TypeLine};
        use crate::types::effect::EffectStep;

        let def = CardDefinition {
            name: "Free Mill".into(),
            mana_cost: None,
            type_line: TypeLine {
                supertypes: vec![],
                card_types: vec![CardType::Artifact],
                subtypes: vec![],
            },
            oracle_text: "Sacrifice a creature: Mill 2.".into(),
            rules_text: vec![RulesText::Active(Rule::Activated(ActivatedAbility {
                cost: vec![CostComponent::Unimplemented("Sacrifice a creature".into())],
                target_requirements: vec![],
                effect: vec![EffectStep::Mill(2)],
            }))],
            text_annotations: vec![],
            power: None,
            toughness: None,
            colors: vec![],
        };
        let mut gs = two_player_state();
        gs.step = crate::types::Step::PreCombatMain;
        let id = place_on_battlefield(&mut gs, def, PlayerId(0));
        put_in_library(&mut gs, PlayerId(0));
        put_in_library(&mut gs, PlayerId(0));

        let gs = activate_ability(gs, id, 0, PlayerId(0), None, vec![]).unwrap();
        assert_eq!(gs.stack.len(), 1);

        let gs = resolve_top(gs);
        assert!(gs.libraries[&PlayerId(0)].is_empty());
    }

    #[test]
    fn ability_index_out_of_range_returns_error() {
        let mut gs = two_player_state();
        let id = place_on_battlefield(&mut gs, make_tap_green_def(), PlayerId(0));
        assert!(matches!(
            activate_ability(gs, id, 99, PlayerId(0), None, vec![]),
            Err(EngineError::AbilityIndexOutOfRange)
        ));
    }

    #[test]
    fn can_pay_cost_true_when_untapped_and_mana_sufficient() {
        let mut gs = two_player_state();
        let id = place_on_battlefield(&mut gs, make_tap_green_def(), PlayerId(0));
        let cost = vec![CostComponent::Tap];
        assert!(can_pay_cost_components(&gs, PlayerId(0), Some(id), &cost));
    }

    #[test]
    fn can_pay_cost_false_when_tapped() {
        let mut gs = two_player_state();
        let id = place_on_battlefield(&mut gs, make_tap_green_def(), PlayerId(0));
        gs.battlefield.get_mut(&id).unwrap().tapped = true;
        let cost = vec![CostComponent::Tap];
        assert!(!can_pay_cost_components(&gs, PlayerId(0), Some(id), &cost));
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
            rules_text: vec![RulesText::Active(Rule::Activated(ActivatedAbility {
                cost: vec![CostComponent::Tap],
                target_requirements: vec![],
                effect: vec![EffectStep::AddMana(ManaPool {
                    green: 1,
                    ..Default::default()
                })],
            }))],
            text_annotations: vec![],
            power: Some(1),
            toughness: Some(1),
            colors: vec![],
        };
        let mut gs = two_player_state();
        let id = place_on_battlefield(&mut gs, snow_elves_def, PlayerId(0));
        let gs = activate_ability(gs, id, 0, PlayerId(0), None, vec![]).unwrap();
        let pool = &gs.get_player(PlayerId(0)).unwrap().mana_pool;
        assert_eq!(pool.green, 1);
        assert_eq!(pool.snow_green, 1);
    }

    // --- CR 702.21a Ward trigger tests for activated abilities ---

    fn make_targeted_tap_ability_def() -> CardDefinition {
        use crate::types::ability::TargetFilter;
        CardDefinition {
            name: "Prodigal Sorcerer".into(),
            mana_cost: None,
            type_line: TypeLine {
                supertypes: vec![],
                card_types: vec![CardType::Creature],
                subtypes: vec![],
            },
            oracle_text: "{T}: Deal 1 damage to target creature.".into(),
            rules_text: vec![RulesText::Active(Rule::Activated(ActivatedAbility {
                cost: vec![CostComponent::Tap],
                target_requirements: vec![TargetFilter::Creature],
                effect: vec![EffectStep::DealDamage(crate::types::effect::DamageStep {
                    amount: 1,
                    ..Default::default()
                })],
            }))],
            text_annotations: vec![],
            power: Some(1),
            toughness: Some(1),
            colors: vec![],
        }
    }

    /// CR 702.21a: Targeting an opponent's Ward permanent via activated ability pushes
    /// a TriggeredAbility with a Payment effect above the activated ability on the stack.
    #[test]
    fn ward_trigger_pushed_above_activated_ability_when_targeting_opponent_ward_creature() {
        use crate::types::ability::{
            CostComponent, TriggerEvent, TriggerTargetMode, TriggeredAbility, TurnOwner,
        };
        use crate::types::effect::{EffectStep, EffectTarget};
        use crate::types::mana::ManaCost;
        use crate::types::stack::StackPayload;

        let mut gs = two_player_state();
        gs.step = crate::types::Step::PreCombatMain;

        // Opponent (PlayerId(1)) has a creature with Ward({2}).
        let ward_def = CardDefinition {
            name: "Ward Bear".into(),
            mana_cost: None,
            type_line: TypeLine {
                supertypes: vec![],
                card_types: vec![CardType::Creature],
                subtypes: vec![],
            },
            oracle_text: "Ward {2}".into(),
            rules_text: vec![RulesText::Active(Rule::Triggered(TriggeredAbility {
                trigger: TriggerEvent::TargetedBy {
                    controller: TurnOwner::Opponent,
                },
                condition: None,
                target_mode: TriggerTargetMode::None,
                effect: vec![EffectStep::Payment {
                    cost: vec![CostComponent::Mana(ManaCost {
                        pips: vec![ManaPip::Generic(2)],
                    })],
                    on_paid: vec![],
                    on_declined: vec![EffectStep::CounterSpell],
                }],
            }))],
            text_annotations: vec![],
            power: Some(2),
            toughness: Some(2),
            colors: vec![],
        };
        let ward_id = place_on_battlefield(&mut gs, ward_def, PlayerId(1));

        // Caster (PlayerId(0)) has the Prodigal Sorcerer-like creature.
        let pinger_id = place_on_battlefield(&mut gs, make_targeted_tap_ability_def(), PlayerId(0));

        let gs = activate_ability(
            gs,
            pinger_id,
            0,
            PlayerId(0),
            None,
            vec![EffectTarget::Object { id: ward_id }],
        )
        .unwrap();

        // Stack: [bottom] activated ability, [top] TriggeredAbility (Ward).
        assert_eq!(gs.stack.len(), 2);
        let ability_stack_id = gs.stack[0];
        let ward_trigger_stack_id = gs.stack[1];

        assert!(matches!(
            gs.stack_objects[&ability_stack_id].payload,
            StackPayload::ActivatedAbility { .. }
        ));
        match &gs.stack_objects[&ward_trigger_stack_id].payload {
            StackPayload::TriggeredAbility { effect, .. } => {
                assert_eq!(effect.len(), 1);
                match &effect[0] {
                    EffectStep::Payment {
                        cost, on_declined, ..
                    } => {
                        assert_eq!(
                            *cost,
                            vec![CostComponent::Mana(ManaCost {
                                pips: vec![ManaPip::Generic(2)]
                            })]
                        );
                        assert!(matches!(on_declined[0], EffectStep::CounterSpell));
                    }
                    other => panic!("Expected Payment step, got {other:?}"),
                }
            }
            other => panic!("Expected TriggeredAbility, got {other:?}"),
        }
        // Ward trigger targets the triggering ability (ability_stack_id).
        assert_eq!(
            gs.stack_objects[&ward_trigger_stack_id].targets,
            vec![EffectTarget::StackObject {
                id: ability_stack_id
            }]
        );
        // TriggeredAbility is controlled by the Ward permanent's controller (CR 603.3a).
        assert_eq!(
            gs.stack_objects[&ward_trigger_stack_id].controller,
            PlayerId(1)
        );
    }
}
