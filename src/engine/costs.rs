use super::EngineError;
use crate::engine::mana::{greedy_payment_plan, pay_mana_cost};
use crate::types::ability::CostComponent;
use crate::types::stack::{StackId, StackPayload};
use crate::types::{GameState, ObjectId, PlayerId};

// CR 116.1, 601.2h — unified cost payment for all cost-bearing game actions.
// Mana: greedy allocation. Life: immediate deduction. Tap: caller's responsibility.
pub fn pay_cost_components(
    mut state: GameState,
    player_id: PlayerId,
    components: &[CostComponent],
) -> Result<GameState, EngineError> {
    for component in components {
        match component {
            CostComponent::Mana(cost) => {
                let plan = {
                    let player = state
                        .get_player(player_id)
                        .ok_or(EngineError::CardNotFound)?;
                    greedy_payment_plan(cost, &player.mana_pool, player.life)
                        .ok_or(EngineError::InsufficientMana)?
                };
                state = pay_mana_cost(state, player_id, cost, &plan)?;
            }
            CostComponent::PayLife(n) => {
                let n = *n;
                let player = state
                    .get_player_mut(player_id)
                    .ok_or(EngineError::CardNotFound)?;
                if player.life < n as i32 {
                    return Err(EngineError::InsufficientLife);
                }
                player.life -= n as i32;
            }
            // Tap is handled by the caller before invoking this function.
            CostComponent::Tap
            | CostComponent::Sacrifice(_, _)
            | CostComponent::Discard(_, _)
            | CostComponent::Unimplemented(_) => {}
        }
    }
    Ok(state)
}

// CR 602.2: structural feasibility check before mutating state.
// Tap: checks not already tapped and not summoning sick (with Haste exception).
// Mana/life: always structurally feasible — affordability deferred to payment context.
pub fn can_pay_cost_components(
    state: &GameState,
    player_id: PlayerId,
    object_id: Option<ObjectId>,
    components: &[CostComponent],
) -> bool {
    use crate::types::ability::StaticAbility;
    for component in components {
        if let CostComponent::Tap = component {
            let Some(id) = object_id else { return false };
            let Some(perm) = state.battlefield.get(&id) else {
                return false;
            };
            if perm.tapped {
                return false;
            }
            let cmt = state.controllers_most_recent_turn(player_id);
            if perm.summoning_sick(cmt) && !perm.has_keyword(StaticAbility::Haste) {
                return false;
            }
        }
    }
    true
}

// CR 702.21a: Pay the ward cost for a WardTrigger on top of the stack.
// Immediately resolves the trigger: the targeted spell survives unchanged.
// Removes the WardTrigger from the stack and restores priority to the active player.
pub fn pay_stack_cost(
    mut state: GameState,
    player_id: PlayerId,
    stack_id: StackId,
) -> Result<GameState, EngineError> {
    if state.stack.last() != Some(&stack_id) {
        return Err(EngineError::NotYourPriority);
    }
    let cost: Vec<CostComponent> = {
        let obj = state
            .stack_objects
            .get(&stack_id)
            .ok_or(EngineError::CardNotFound)?;
        match &obj.payload {
            StackPayload::WardTrigger { cost, .. } => cost.clone(),
            _ => return Err(EngineError::NotYourPriority),
        }
    };
    state = pay_cost_components(state, player_id, &cost)?;
    state.stack_objects.remove(&stack_id);
    state.stack.retain(|&id| id != stack_id);
    state.consecutive_passes = 0;
    state.priority_player = state.active_player;
    Ok(state)
}

// CR 702.21a: Decline an optional stack cost; immediately counter the targeted spell.
// Removes both the WardTrigger and the countered spell from the stack.
pub fn resolve_stack_cost_decline(
    mut state: GameState,
    stack_id: StackId,
) -> Result<GameState, EngineError> {
    if state.stack.last() != Some(&stack_id) {
        return Err(EngineError::NotYourPriority);
    }
    let counters_if_unpaid = {
        let obj = state
            .stack_objects
            .get(&stack_id)
            .ok_or(EngineError::CardNotFound)?;
        match &obj.payload {
            StackPayload::WardTrigger {
                counters_if_unpaid, ..
            } => *counters_if_unpaid,
            _ => return Err(EngineError::NotYourPriority),
        }
    };
    state.stack_objects.remove(&stack_id);
    state.stack.retain(|&id| id != stack_id);
    super::stack::counter_spell_on_stack(&mut state, counters_if_unpaid);
    state.consecutive_passes = 0;
    state.priority_player = state.active_player;
    Ok(state)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::ability::CostComponent;
    use crate::types::mana::{ManaCost, ManaPip};
    use crate::types::stack::{StackObject, StackPayload};
    use crate::types::{GameState, Player, PlayerId, StackId};

    fn two_player_state() -> GameState {
        GameState::new(vec![
            Player::new(PlayerId(0), "Alice"),
            Player::new(PlayerId(1), "Bob"),
        ])
    }

    fn push_ward_trigger(
        state: &mut GameState,
        cost: Vec<CostComponent>,
        counters: StackId,
    ) -> StackId {
        let sid = state.alloc_stack_id();
        state.stack_objects.insert(
            sid,
            StackObject {
                id: sid,
                payload: StackPayload::WardTrigger {
                    counters_if_unpaid: counters,
                    cost,
                    settled: false,
                },
                controller: PlayerId(1),
                targets: vec![],
            },
        );
        state.stack.push(sid);
        sid
    }

    fn push_spell(state: &mut GameState) -> StackId {
        use crate::types::card::{CardDefinition, CardType, TypeLine};
        use crate::types::mana::ManaCost;
        use crate::types::{CardObject, Zone};
        let spell_id = state.alloc_id();
        let def = CardDefinition {
            name: "Lightning Bolt".into(),
            mana_cost: Some(ManaCost { pips: vec![] }),
            type_line: TypeLine {
                supertypes: vec![],
                card_types: vec![CardType::Instant],
                subtypes: vec![],
            },
            oracle_text: String::new(),
            abilities: vec![],
            text_annotations: vec![],
            power: None,
            toughness: None,
            colors: vec![],
        };
        let obj = CardObject::new(spell_id, def, PlayerId(0), Zone::Stack);
        state.add_object(obj);
        let sid = state.alloc_stack_id();
        state.stack_objects.insert(
            sid,
            StackObject {
                id: sid,
                payload: StackPayload::Spell { card_id: spell_id },
                controller: PlayerId(0),
                targets: vec![],
            },
        );
        state.stack.push(sid);
        sid
    }

    // ── pay_cost_components ──────────────────────────────────────────────

    #[test]
    fn pay_mana_component_deducts_from_pool() {
        let mut gs = two_player_state();
        gs.get_player_mut(PlayerId(0)).unwrap().mana_pool.colorless = 2;
        let components = vec![CostComponent::Mana(ManaCost {
            pips: vec![ManaPip::Generic(2)],
        })];
        let gs = pay_cost_components(gs, PlayerId(0), &components).unwrap();
        assert_eq!(gs.get_player(PlayerId(0)).unwrap().mana_pool.colorless, 0);
    }

    #[test]
    fn pay_life_component_deducts_life() {
        let gs = two_player_state();
        let components = vec![CostComponent::PayLife(3)];
        let gs = pay_cost_components(gs, PlayerId(0), &components).unwrap();
        assert_eq!(gs.get_player(PlayerId(0)).unwrap().life, 17);
    }

    #[test]
    fn pay_mana_insufficient_returns_error() {
        let gs = two_player_state();
        let components = vec![CostComponent::Mana(ManaCost {
            pips: vec![ManaPip::Generic(2)],
        })];
        let result = pay_cost_components(gs, PlayerId(0), &components);
        assert!(matches!(result, Err(EngineError::InsufficientMana)));
    }

    #[test]
    fn pay_life_insufficient_returns_error() {
        let mut gs = two_player_state();
        gs.get_player_mut(PlayerId(0)).unwrap().life = 1;
        let components = vec![CostComponent::PayLife(3)];
        let result = pay_cost_components(gs, PlayerId(0), &components);
        assert!(matches!(result, Err(EngineError::InsufficientLife)));
    }

    #[test]
    fn tap_component_is_skipped_by_pay_cost_components() {
        let gs = two_player_state();
        let components = vec![CostComponent::Tap];
        let result = pay_cost_components(gs, PlayerId(0), &components);
        assert!(result.is_ok());
    }

    // ── can_pay_cost_components ──────────────────────────────────────────

    #[test]
    fn can_pay_tap_returns_false_when_already_tapped() {
        use crate::types::card::{CardDefinition, CardType, TypeLine};
        use crate::types::{CardObject, PermanentState, Zone};
        let mut gs = two_player_state();
        let id = gs.alloc_id();
        let def = CardDefinition {
            name: "Forest".into(),
            mana_cost: None,
            type_line: TypeLine {
                supertypes: vec![],
                card_types: vec![CardType::Land],
                subtypes: vec!["Forest".into()],
            },
            oracle_text: String::new(),
            abilities: vec![],
            text_annotations: vec![],
            power: None,
            toughness: None,
            colors: vec![],
        };
        let obj = CardObject::new(id, def, PlayerId(0), Zone::Battlefield);
        gs.battlefield
            .insert(id, PermanentState::new(&obj.definition));
        gs.battlefield.get_mut(&id).unwrap().tapped = true;
        gs.add_object(obj);
        let components = vec![CostComponent::Tap];
        assert!(!can_pay_cost_components(
            &gs,
            PlayerId(0),
            Some(id),
            &components
        ));
    }

    #[test]
    fn can_pay_mana_always_returns_true_structurally() {
        let gs = two_player_state();
        let components = vec![CostComponent::Mana(ManaCost {
            pips: vec![ManaPip::Generic(5)],
        })];
        assert!(can_pay_cost_components(&gs, PlayerId(0), None, &components));
    }

    // ── pay_stack_cost ───────────────────────────────────────────────────

    #[test]
    fn pay_stack_cost_mana_removes_trigger_and_deducts_mana() {
        let mut gs = two_player_state();
        gs.get_player_mut(PlayerId(0)).unwrap().mana_pool.colorless = 2;
        let spell_sid = push_spell(&mut gs);
        let trigger_sid = push_ward_trigger(
            &mut gs,
            vec![CostComponent::Mana(ManaCost {
                pips: vec![ManaPip::Generic(2)],
            })],
            spell_sid,
        );

        let gs = pay_stack_cost(gs, PlayerId(0), trigger_sid).unwrap();

        assert!(!gs.stack.contains(&trigger_sid));
        assert!(!gs.stack_objects.contains_key(&trigger_sid));
        assert!(gs.stack.contains(&spell_sid));
        assert_eq!(gs.get_player(PlayerId(0)).unwrap().mana_pool.colorless, 0);
    }

    #[test]
    fn pay_stack_cost_life_removes_trigger_and_deducts_life() {
        let mut gs = two_player_state();
        let spell_sid = push_spell(&mut gs);
        let trigger_sid = push_ward_trigger(&mut gs, vec![CostComponent::PayLife(2)], spell_sid);

        let gs = pay_stack_cost(gs, PlayerId(0), trigger_sid).unwrap();

        assert!(!gs.stack.contains(&trigger_sid));
        assert!(gs.stack.contains(&spell_sid));
        assert_eq!(gs.get_player(PlayerId(0)).unwrap().life, 18);
    }

    #[test]
    fn pay_stack_cost_not_on_top_returns_error() {
        let mut gs = two_player_state();
        let spell_sid = push_spell(&mut gs);
        let trigger_sid = push_ward_trigger(&mut gs, vec![CostComponent::PayLife(1)], spell_sid);
        let extra = gs.alloc_stack_id();
        gs.stack.push(extra);

        let result = pay_stack_cost(gs, PlayerId(0), trigger_sid);
        assert!(matches!(result, Err(EngineError::NotYourPriority)));
    }

    // ── resolve_stack_cost_decline ───────────────────────────────────────

    #[test]
    fn decline_removes_trigger_and_counters_spell() {
        let mut gs = two_player_state();
        let spell_sid = push_spell(&mut gs);
        let trigger_sid = push_ward_trigger(&mut gs, vec![CostComponent::PayLife(2)], spell_sid);

        let gs = resolve_stack_cost_decline(gs, trigger_sid).unwrap();

        assert!(!gs.stack.contains(&trigger_sid));
        assert!(!gs.stack.contains(&spell_sid));
        let gy = gs.graveyards.get(&PlayerId(0)).unwrap();
        assert!(!gy.is_empty(), "countered spell should be in graveyard");
    }

    #[test]
    fn decline_not_on_top_returns_error() {
        let mut gs = two_player_state();
        let spell_sid = push_spell(&mut gs);
        let trigger_sid = push_ward_trigger(&mut gs, vec![CostComponent::PayLife(1)], spell_sid);
        let extra = gs.alloc_stack_id();
        gs.stack.push(extra);

        let result = resolve_stack_cost_decline(gs, trigger_sid);
        assert!(matches!(result, Err(EngineError::NotYourPriority)));
    }
}
