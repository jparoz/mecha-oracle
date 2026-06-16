use super::EngineError;
use crate::engine::mana::{greedy_payment_plan, pay_mana_cost};
use crate::engine::stack::execute_effect_steps;
use crate::types::ability::CostComponent;
use crate::types::{GameState, ObjectId, PlayerId};

// CR 116.1, 601.2h — unified cost payment for all cost-bearing game actions.
// Mana: greedy allocation. Life: immediate deduction. Tap: caller's responsibility.
pub fn pay_cost_components(
    mut state: GameState,
    player_id: PlayerId,
    components: &[CostComponent],
    x_value: Option<u32>,
) -> Result<GameState, EngineError> {
    for component in components {
        match component {
            CostComponent::Mana(cost) => {
                let plan = {
                    let player = state
                        .get_player(player_id)
                        .ok_or(EngineError::CardNotFound)?;
                    greedy_payment_plan(cost, &player.mana_pool, player.life, x_value)
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

// CR 118.12: pay an inline cost obligation and execute on_paid + continuation steps.
pub fn pay_pending_cost(
    mut state: GameState,
    player_id: PlayerId,
) -> Result<GameState, EngineError> {
    let pending = match state.pending_payment.take() {
        Some(p) => p,
        None => return Err(EngineError::NotYourPriority),
    };
    if pending.paying_player != player_id {
        state.pending_payment = Some(pending);
        return Err(EngineError::NotYourPriority);
    }
    state = pay_cost_components(state, player_id, &pending.cost, None)?;
    state = execute_effect_steps(
        state,
        pending.controller,
        &pending.on_paid,
        &pending.targets,
        None,
    );
    state = execute_effect_steps(
        state,
        pending.controller,
        &pending.continuation,
        &pending.targets,
        None,
    );
    state.consecutive_passes = 0;
    state.priority_player = state.active_player;
    Ok(state)
}

// CR 118.12: decline an inline cost obligation; execute on_declined + continuation steps.
pub fn decline_pending_cost(mut state: GameState) -> Result<GameState, EngineError> {
    let pending = state
        .pending_payment
        .take()
        .ok_or(EngineError::NotYourPriority)?;
    state = execute_effect_steps(
        state,
        pending.controller,
        &pending.on_declined,
        &pending.targets,
        None,
    );
    state = execute_effect_steps(
        state,
        pending.controller,
        &pending.continuation,
        &pending.targets,
        None,
    );
    state.consecutive_passes = 0;
    state.priority_player = state.active_player;
    Ok(state)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::ability::CostComponent;
    use crate::types::mana::{ManaCost, ManaPip};
    use crate::types::{GameState, Player, PlayerId};

    fn two_player_state() -> GameState {
        GameState::new(vec![
            Player::new(PlayerId(0), "Alice"),
            Player::new(PlayerId(1), "Bob"),
        ])
    }

    // ── pay_cost_components ──────────────────────────────────────────────

    #[test]
    fn pay_mana_component_deducts_from_pool() {
        let mut gs = two_player_state();
        gs.get_player_mut(PlayerId(0)).unwrap().mana_pool.colorless = 2;
        let components = vec![CostComponent::Mana(ManaCost {
            pips: vec![ManaPip::Generic(2)],
        })];
        let gs = pay_cost_components(gs, PlayerId(0), &components, None).unwrap();
        assert_eq!(gs.get_player(PlayerId(0)).unwrap().mana_pool.colorless, 0);
    }

    #[test]
    fn pay_life_component_deducts_life() {
        let gs = two_player_state();
        let components = vec![CostComponent::PayLife(3)];
        let gs = pay_cost_components(gs, PlayerId(0), &components, None).unwrap();
        assert_eq!(gs.get_player(PlayerId(0)).unwrap().life, 17);
    }

    #[test]
    fn pay_mana_insufficient_returns_error() {
        let gs = two_player_state();
        let components = vec![CostComponent::Mana(ManaCost {
            pips: vec![ManaPip::Generic(2)],
        })];
        let result = pay_cost_components(gs, PlayerId(0), &components, None);
        assert!(matches!(result, Err(EngineError::InsufficientMana)));
    }

    #[test]
    fn pay_life_insufficient_returns_error() {
        let mut gs = two_player_state();
        gs.get_player_mut(PlayerId(0)).unwrap().life = 1;
        let components = vec![CostComponent::PayLife(3)];
        let result = pay_cost_components(gs, PlayerId(0), &components, None);
        assert!(matches!(result, Err(EngineError::InsufficientLife)));
    }

    #[test]
    fn tap_component_is_skipped_by_pay_cost_components() {
        let gs = two_player_state();
        let components = vec![CostComponent::Tap];
        let result = pay_cost_components(gs, PlayerId(0), &components, None);
        assert!(result.is_ok());
    }

    #[test]
    fn pay_mana_component_with_x_deducts_x_mana() {
        let mut gs = two_player_state();
        gs.get_player_mut(PlayerId(0)).unwrap().mana_pool.green = 5;
        let components = vec![CostComponent::Mana(ManaCost {
            pips: vec![ManaPip::X, ManaPip::Green],
        })];
        // x_value = Some(3): pay 3 generic + 1 green = 4 green total
        let gs = pay_cost_components(gs, PlayerId(0), &components, Some(3)).unwrap();
        assert_eq!(gs.get_player(PlayerId(0)).unwrap().mana_pool.green, 1);
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

    // ── pay_pending_cost / decline_pending_cost ─────────────────────────

    #[test]
    fn pay_pending_cost_clears_payment_and_runs_on_paid() {
        use crate::types::ability::CostComponent;
        use crate::types::effect::EffectStep;
        use crate::types::game_state::PendingPayment;
        use crate::types::mana::{ManaCost, ManaPip};

        let mut gs = two_player_state();
        gs.get_player_mut(PlayerId(0)).unwrap().mana_pool.colorless = 3;
        let cost = vec![CostComponent::Mana(ManaCost {
            pips: vec![ManaPip::Generic(3)],
        })];
        gs.pending_payment = Some(PendingPayment {
            paying_player: PlayerId(0),
            cost: cost.clone(),
            on_paid: vec![],
            on_declined: vec![EffectStep::CounterSpell],
            continuation: vec![],
            targets: vec![],
            controller: PlayerId(1),
        });

        let gs = pay_pending_cost(gs, PlayerId(0)).unwrap();

        assert!(gs.pending_payment.is_none());
        assert_eq!(gs.get_player(PlayerId(0)).unwrap().mana_pool.colorless, 0);
    }

    #[test]
    fn pay_pending_cost_wrong_player_returns_error() {
        use crate::types::ability::CostComponent;
        use crate::types::effect::EffectStep;
        use crate::types::game_state::PendingPayment;

        let mut gs = two_player_state();
        gs.pending_payment = Some(PendingPayment {
            paying_player: PlayerId(0),
            cost: vec![CostComponent::PayLife(1)],
            on_paid: vec![],
            on_declined: vec![EffectStep::CounterSpell],
            continuation: vec![],
            targets: vec![],
            controller: PlayerId(1),
        });

        let result = pay_pending_cost(gs, PlayerId(1)); // wrong player
        assert!(matches!(result, Err(EngineError::NotYourPriority)));
    }

    #[test]
    fn decline_pending_cost_executes_on_declined_and_clears() {
        use crate::types::ability::CostComponent;
        use crate::types::card::{CardDefinition, CardType, TypeLine};
        use crate::types::effect::{EffectStep, EffectTarget};
        use crate::types::game_state::PendingPayment;
        use crate::types::mana::ManaCost;
        use crate::types::stack::{StackObject, StackPayload};
        use crate::types::{CardObject, Zone};

        let mut gs = two_player_state();

        // Put a spell on the stack so CounterSpell has something to counter
        let card_id = gs.alloc_id();
        let def = CardDefinition {
            name: "Victim".into(),
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
        let obj = CardObject::new(card_id, def, PlayerId(1), Zone::Stack);
        gs.add_object(obj);
        let sid = gs.alloc_stack_id();
        gs.stack_objects.insert(
            sid,
            StackObject {
                id: sid,
                payload: StackPayload::Spell { card_id },
                controller: PlayerId(1),
                targets: vec![],
                x_value: None,
            },
        );
        gs.stack.push(sid);

        gs.pending_payment = Some(PendingPayment {
            paying_player: PlayerId(1),
            cost: vec![CostComponent::PayLife(3)],
            on_paid: vec![],
            on_declined: vec![EffectStep::CounterSpell],
            continuation: vec![],
            targets: vec![EffectTarget::StackObject { id: sid }],
            controller: PlayerId(0),
        });

        let gs = decline_pending_cost(gs).unwrap();

        assert!(gs.pending_payment.is_none());
        assert!(!gs.stack.contains(&sid));
        assert!(!gs.stack_objects.contains_key(&sid));
        let gy = gs.graveyards.get(&PlayerId(1)).unwrap();
        assert!(gy.contains(&card_id));
    }
}
