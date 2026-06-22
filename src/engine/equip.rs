use super::EngineError;
use crate::engine::costs::{can_pay_cost_components, pay_cost_components};
use crate::types::ability::{Cost, Rule, RulesText};
use crate::types::effect::{EffectStep, EffectTarget};
use crate::types::stack::{StackObject, StackPayload};
use crate::types::{GameState, ObjectId, PlayerId, Step, Zone};

/// CR 301.5d: Equip — pay the equip cost as a sorcery, targeting a creature you control;
/// on resolution the equipment attaches to that creature.
pub fn activate_equip(
    mut state: GameState,
    equipment_id: ObjectId,
    target_creature_id: ObjectId,
    player_id: PlayerId,
) -> Result<GameState, EngineError> {
    if state.priority_player != player_id {
        return Err(EngineError::NotYourPriority);
    }

    // CR 301.5d: equip only as a sorcery (active player, main phase, empty stack).
    if state.active_player != player_id {
        return Err(EngineError::CannotCastNow);
    }
    if !matches!(state.step(), Step::PreCombatMain | Step::PostCombatMain) {
        return Err(EngineError::CannotCastNow);
    }
    if !state.stack.is_empty() {
        return Err(EngineError::CannotCastNow);
    }

    // Validate the equipment is on the battlefield and is ours.
    {
        let obj = state
            .objects
            .get(&equipment_id)
            .ok_or(EngineError::CardNotFound)?;
        if obj.zone != Zone::Battlefield {
            return Err(EngineError::CardNotOnBattlefield);
        }
        if obj.controller != player_id {
            return Err(EngineError::NotYourCard);
        }
    }

    // Find the equip cost from the equipment's rules text.
    let cost: Cost = state
        .objects
        .get(&equipment_id)
        .and_then(|obj| {
            obj.definition.rules_text.iter().find_map(|span| {
                if let RulesText::Active(Rule::Equip { cost, .. }) = span {
                    Some(cost.clone())
                } else {
                    None
                }
            })
        })
        .ok_or(EngineError::AbilityIndexOutOfRange)?;

    // Validate the target is a creature on the battlefield controlled by us.
    {
        let target_obj = state
            .objects
            .get(&target_creature_id)
            .ok_or(EngineError::CardNotFound)?;
        if target_obj.zone != Zone::Battlefield {
            return Err(EngineError::CardNotOnBattlefield);
        }
        if target_obj.controller != player_id {
            return Err(EngineError::NotYourCard);
        }
        if !target_obj.definition.type_line.is_creature() {
            return Err(EngineError::NotACreature);
        }
    }

    // Check and pay the cost.
    if !can_pay_cost_components(&state, player_id, Some(equipment_id), &cost) {
        return Err(EngineError::InsufficientMana);
    }
    state = pay_cost_components(state, player_id, &cost, None)?;

    // Build the stack entry with the Attach effect step.
    let stack_id = state.alloc_stack_id();
    let label = state
        .objects
        .get(&equipment_id)
        .map(|o| format!("Equip \u{2014} {}", o.definition.name))
        .unwrap_or_else(|| "Equip".into());
    let stack_obj = StackObject {
        id: stack_id,
        payload: StackPayload::ActivatedAbility {
            source_id: equipment_id,
            effect: vec![EffectStep::Attach {
                source_id: equipment_id,
            }],
            label,
        },
        controller: player_id,
        targets: vec![EffectTarget::Object {
            id: target_creature_id,
        }],
        x_value: None,
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
    use crate::cards::test_helpers::test_db;
    use crate::types::ability::{CostComponent, TargetFilter};
    use crate::types::mana::{ManaCost, ManaPip};
    use crate::types::permanent::PTDelta;
    use crate::types::{
        CardDefinition, CardObject, CardType, ContinuousEffect, PermanentFilter, PermanentState,
        Player, Rule, TypeLine, Zone,
    };

    fn two_player_state() -> GameState {
        GameState::new(vec![
            Player::new(PlayerId(0), "Alice"),
            Player::new(PlayerId(1), "Bob"),
        ])
    }

    fn make_bonesplitter() -> CardDefinition {
        CardDefinition {
            name: "Bonesplitter".into(),
            mana_cost: Some(ManaCost {
                pips: vec![ManaPip::Generic(1)],
            }),
            type_line: TypeLine {
                supertypes: vec![],
                card_types: vec![CardType::Artifact],
                subtypes: vec!["Equipment".into()],
            },
            oracle_text: "Equipped creature gets +2/+0. Equip {1}".into(),
            rules_text: vec![crate::types::RulesText::Active(Rule::Equip {
                cost: vec![CostComponent::Mana(ManaCost {
                    pips: vec![ManaPip::Generic(1)],
                })],
                grants: ContinuousEffect {
                    subject_filter: PermanentFilter::default(),
                    pt_modification: Some(PTDelta {
                        power: 2,
                        toughness: 0,
                    }),
                },
            })],
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

    fn setup_equip_state() -> (GameState, ObjectId, ObjectId) {
        let mut gs = two_player_state();
        gs.step = Step::PreCombatMain;
        let db = test_db();
        let bear_id = place_on_battlefield(
            &mut gs,
            db.get("Grizzly Bears").unwrap().clone(),
            PlayerId(0),
        );
        let equip_id = place_on_battlefield(&mut gs, make_bonesplitter(), PlayerId(0));
        gs.get_player_mut(PlayerId(0)).unwrap().mana_pool.colorless = 1;
        (gs, equip_id, bear_id)
    }

    #[test]
    fn equip_puts_ability_on_stack() {
        let (gs, equip_id, bear_id) = setup_equip_state();
        let gs = activate_equip(gs, equip_id, bear_id, PlayerId(0)).unwrap();
        assert_eq!(gs.stack.len(), 1);
    }

    #[test]
    fn equip_resolution_sets_attached_to() {
        use crate::engine::stack::resolve_top;
        let (gs, equip_id, bear_id) = setup_equip_state();
        let gs = activate_equip(gs, equip_id, bear_id, PlayerId(0)).unwrap();
        let gs = resolve_top(gs);
        assert_eq!(gs.battlefield[&equip_id].attached_to, Some(bear_id));
    }

    #[test]
    fn equip_deducts_mana_cost() {
        let (gs, equip_id, bear_id) = setup_equip_state();
        let gs = activate_equip(gs, equip_id, bear_id, PlayerId(0)).unwrap();
        assert_eq!(gs.get_player(PlayerId(0)).unwrap().mana_pool.colorless, 0);
    }

    #[test]
    fn equip_fails_not_your_priority() {
        let (mut gs, equip_id, bear_id) = setup_equip_state();
        gs.priority_player = PlayerId(1);
        assert!(matches!(
            activate_equip(gs, equip_id, bear_id, PlayerId(0)),
            Err(EngineError::NotYourPriority)
        ));
    }

    #[test]
    fn equip_fails_not_main_phase() {
        let (mut gs, equip_id, bear_id) = setup_equip_state();
        gs.step = Step::BeginningOfCombat;
        assert!(matches!(
            activate_equip(gs, equip_id, bear_id, PlayerId(0)),
            Err(EngineError::CannotCastNow)
        ));
    }

    #[test]
    fn equip_fails_stack_not_empty() {
        use crate::types::stack::{StackObject, StackPayload};
        let (mut gs, equip_id, bear_id) = setup_equip_state();
        // Push a dummy stack object
        let sid = gs.alloc_stack_id();
        gs.stack.push(sid);
        gs.stack_objects.insert(
            sid,
            StackObject {
                id: sid,
                payload: StackPayload::ActivatedAbility {
                    source_id: equip_id,
                    effect: vec![],
                    label: "dummy".into(),
                },
                controller: PlayerId(0),
                targets: vec![],
                x_value: None,
            },
        );
        assert!(matches!(
            activate_equip(gs, equip_id, bear_id, PlayerId(0)),
            Err(EngineError::CannotCastNow)
        ));
    }

    #[test]
    fn equip_fails_target_is_not_creature() {
        let mut gs = two_player_state();
        gs.step = Step::PreCombatMain;
        let db = test_db();
        let land_id = {
            let id = gs.alloc_id();
            let obj = CardObject::new(
                id,
                db.get("Forest").unwrap().clone(),
                PlayerId(0),
                Zone::Battlefield,
            );
            let perm = PermanentState::new(&obj.definition);
            gs.battlefield.insert(id, perm);
            gs.add_object(obj);
            id
        };
        let equip_id = place_on_battlefield(&mut gs, make_bonesplitter(), PlayerId(0));
        gs.get_player_mut(PlayerId(0)).unwrap().mana_pool.colorless = 1;
        assert!(matches!(
            activate_equip(gs, equip_id, land_id, PlayerId(0)),
            Err(EngineError::NotACreature)
        ));
    }

    #[test]
    fn equip_reattach_moves_attachment() {
        // Equip to bear A, then equip to bear B — attached_to should update.
        use crate::engine::stack::resolve_top;
        let mut gs = two_player_state();
        gs.step = Step::PreCombatMain;
        let db = test_db();
        let bear_a = place_on_battlefield(
            &mut gs,
            db.get("Grizzly Bears").unwrap().clone(),
            PlayerId(0),
        );
        let bear_b = place_on_battlefield(
            &mut gs,
            db.get("Grizzly Bears").unwrap().clone(),
            PlayerId(0),
        );
        let equip_id = place_on_battlefield(&mut gs, make_bonesplitter(), PlayerId(0));

        // First equip to bear_a
        gs.battlefield.get_mut(&equip_id).unwrap().attached_to = Some(bear_a);

        // Re-equip to bear_b
        gs.get_player_mut(PlayerId(0)).unwrap().mana_pool.colorless = 1;
        let gs = activate_equip(gs, equip_id, bear_b, PlayerId(0)).unwrap();
        let gs = resolve_top(gs);
        assert_eq!(gs.battlefield[&equip_id].attached_to, Some(bear_b));
    }

    #[test]
    #[allow(unused_variables)]
    fn equip_target_filter_is_unused_but_compiles() {
        // Ensure TargetFilter is importable (it's in the use block); just a compile-time check.
        let _filter = TargetFilter::Creature;
    }
}
