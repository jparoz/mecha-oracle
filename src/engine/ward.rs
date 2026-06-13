// CR 702.21a: Ward — when a permanent with Ward becomes the target of a spell or ability
// an opponent controls, that spell/ability is countered unless the opponent pays the Ward cost.

use super::EngineError;
use crate::types::stack::{StackId, StackPayload};
use crate::types::{GameState, PlayerId, StackObject, WardCost};

/// CR 702.21a: Collect WardTrigger stack objects for any declared targets that are
/// opponent-controlled permanents with Ward. Each Ward ability on such a target generates
/// one WardTrigger pushed above the triggering spell/ability on the stack.
/// The trigger is controlled by the Ward permanent's controller (CR 603.3a).
pub fn collect_ward_triggers(
    state: &mut GameState,
    triggering_stack_id: StackId,
    acting_player: PlayerId,
    targets: &[crate::types::effect::EffectTarget],
) -> Vec<StackObject> {
    use crate::types::ability::{Ability, OracleSpan, StaticAbility, WardCost};
    use crate::types::effect::EffectTarget;
    use crate::types::stack::{StackObject, StackPayload};

    let mut triggers = Vec::new();
    for target in targets {
        let target_obj_id = match target {
            EffectTarget::Object { id } => *id,
            EffectTarget::Player { .. } => continue,
        };
        if !state.battlefield.contains_key(&target_obj_id) {
            continue;
        }
        let target_obj = match state.objects.get(&target_obj_id) {
            Some(o) => o,
            None => continue,
        };
        // Ward only fires when an opponent's permanent is targeted
        if target_obj.controller == acting_player {
            continue;
        }
        let ward_permanent_controller = target_obj.controller;
        let ward_costs: Vec<WardCost> = target_obj
            .definition
            .abilities
            .iter()
            .filter_map(|span| match span {
                OracleSpan::Parsed(Ability::Static(StaticAbility::WardMana(cost))) => {
                    Some(WardCost::Mana(cost.clone()))
                }
                OracleSpan::Parsed(Ability::Static(StaticAbility::WardLife(n))) => {
                    Some(WardCost::Life(*n))
                }
                _ => None,
            })
            .collect();
        for cost in ward_costs {
            let sid = state.alloc_stack_id();
            // CR 603.3a: triggered ability is controlled by the controller of its source
            triggers.push(StackObject {
                id: sid,
                payload: StackPayload::WardTrigger {
                    counters_if_unpaid: triggering_stack_id,
                    cost,
                    paid: false,
                },
                controller: ward_permanent_controller,
                targets: vec![],
            });
        }
    }
    triggers
}

/// CR 702.21a: Pay the Ward cost for a WardTrigger on top of the stack.
/// The player paying is the spell's controller (who cast the targeting spell).
/// After payment, marks the trigger as paid so it resolves without countering.
pub fn pay_ward(
    mut state: GameState,
    player_id: PlayerId,
    trigger_id: StackId,
) -> Result<GameState, EngineError> {
    // Step 1: trigger_id must be on top of the stack (last element).
    if state.stack.last() != Some(&trigger_id) {
        return Err(EngineError::NotYourPriority);
    }

    // Step 2: get the WardTrigger payload; check if already paid.
    let (cost, already_paid) = {
        let obj = state
            .stack_objects
            .get(&trigger_id)
            .ok_or(EngineError::CardNotFound)?;
        match &obj.payload {
            StackPayload::WardTrigger { cost, paid, .. } => (cost.clone(), *paid),
            _ => return Err(EngineError::NotYourPriority),
        }
    };

    if already_paid {
        return Ok(state);
    }

    // Step 3: pay the cost.
    match &cost {
        WardCost::Mana(mana_cost) => {
            let plan = {
                let player = state
                    .get_player(player_id)
                    .ok_or(EngineError::CardNotFound)?;
                super::mana::greedy_payment_plan(mana_cost, &player.mana_pool, player.life)
                    .ok_or(EngineError::InsufficientMana)?
            };
            state = super::mana::pay_mana_cost(state, player_id, mana_cost, &plan)?;
        }
        WardCost::Life(n) => {
            let n = *n;
            let player = state
                .get_player_mut(player_id)
                .ok_or(EngineError::CardNotFound)?;
            if player.life < n as i32 {
                return Err(EngineError::InsufficientLife);
            }
            player.life -= n as i32;
        }
    }

    // Step 4: mark the trigger as paid.
    let obj = state.stack_objects.get_mut(&trigger_id).unwrap();
    if let StackPayload::WardTrigger { paid, .. } = &mut obj.payload {
        *paid = true;
    }

    Ok(state)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::mana::{ManaCost, ManaPip};
    use crate::types::stack::{StackObject, StackPayload};
    use crate::types::{GameState, Player, PlayerId};

    fn make_two_player_state() -> GameState {
        GameState::new(vec![
            Player::new(PlayerId(0), "Alice"),
            Player::new(PlayerId(1), "Bob"),
        ])
    }

    fn push_ward_trigger(state: &mut GameState, cost: WardCost) -> StackId {
        let sid = state.alloc_stack_id();
        let obj = StackObject {
            id: sid,
            payload: StackPayload::WardTrigger {
                counters_if_unpaid: StackId(0),
                cost,
                paid: false,
            },
            controller: PlayerId(1),
            targets: vec![],
        };
        state.stack_objects.insert(sid, obj);
        state.stack.push(sid);
        sid
    }

    #[test]
    fn pay_ward_mana_marks_paid_and_deducts_mana() {
        let mut gs = make_two_player_state();
        // Give caster (PlayerId(0)) 2 colorless mana
        gs.get_player_mut(PlayerId(0)).unwrap().mana_pool.colorless = 2;
        let cost = WardCost::Mana(ManaCost {
            pips: vec![ManaPip::Generic(2)],
        });
        let trigger_id = push_ward_trigger(&mut gs, cost);

        let gs = pay_ward(gs, PlayerId(0), trigger_id).unwrap();

        // Trigger should be marked paid
        let obj = gs.stack_objects.get(&trigger_id).unwrap();
        match &obj.payload {
            StackPayload::WardTrigger { paid, .. } => assert!(*paid, "trigger should be paid"),
            _ => panic!("expected WardTrigger"),
        }
        // Mana should be spent
        assert_eq!(
            gs.get_player(PlayerId(0)).unwrap().mana_pool.colorless,
            0,
            "mana should be deducted"
        );
    }

    #[test]
    fn pay_ward_life_marks_paid_and_deducts_life() {
        let mut gs = make_two_player_state();
        // PlayerId(0) starts with 20 life
        let cost = WardCost::Life(2);
        let trigger_id = push_ward_trigger(&mut gs, cost);

        let gs = pay_ward(gs, PlayerId(0), trigger_id).unwrap();

        // Trigger should be marked paid
        let obj = gs.stack_objects.get(&trigger_id).unwrap();
        match &obj.payload {
            StackPayload::WardTrigger { paid, .. } => assert!(*paid, "trigger should be paid"),
            _ => panic!("expected WardTrigger"),
        }
        // Life should be reduced by 2
        assert_eq!(
            gs.get_player(PlayerId(0)).unwrap().life,
            18,
            "life should be reduced by 2"
        );
    }

    #[test]
    fn pay_ward_fails_insufficient_mana() {
        let mut gs = make_two_player_state();
        // Player has 0 mana, ward costs {2}
        let cost = WardCost::Mana(ManaCost {
            pips: vec![ManaPip::Generic(2)],
        });
        let trigger_id = push_ward_trigger(&mut gs, cost);

        let result = pay_ward(gs, PlayerId(0), trigger_id);
        assert!(
            matches!(result, Err(EngineError::InsufficientMana)),
            "expected InsufficientMana, got {result:?}"
        );
    }

    #[test]
    fn pay_ward_fails_insufficient_life() {
        let mut gs = make_two_player_state();
        // Give player only 3 life; ward costs 5 life
        gs.get_player_mut(PlayerId(0)).unwrap().life = 3;
        let cost = WardCost::Life(5);
        let trigger_id = push_ward_trigger(&mut gs, cost);

        let result = pay_ward(gs, PlayerId(0), trigger_id);
        assert!(
            matches!(result, Err(EngineError::InsufficientLife)),
            "expected InsufficientLife, got {result:?}"
        );
    }

    #[test]
    fn pay_ward_fails_when_trigger_not_on_top() {
        let mut gs = make_two_player_state();
        let cost = WardCost::Life(1);
        let trigger_id = push_ward_trigger(&mut gs, cost);
        // Push another object on top, making trigger_id no longer top
        let other_sid = StackId(999);
        gs.stack.push(other_sid);

        let result = pay_ward(gs, PlayerId(0), trigger_id);
        assert!(
            matches!(result, Err(EngineError::NotYourPriority)),
            "expected NotYourPriority"
        );
    }

    #[test]
    fn pay_ward_already_paid_is_noop() {
        let mut gs = make_two_player_state();
        gs.get_player_mut(PlayerId(0)).unwrap().life = 20;
        let sid = gs.alloc_stack_id();
        let obj = StackObject {
            id: sid,
            payload: StackPayload::WardTrigger {
                counters_if_unpaid: StackId(0),
                cost: WardCost::Life(2),
                paid: true, // already paid
            },
            controller: PlayerId(1),
            targets: vec![],
        };
        gs.stack_objects.insert(sid, obj);
        gs.stack.push(sid);

        let gs = pay_ward(gs, PlayerId(0), sid).unwrap();
        // Life should NOT have changed
        assert_eq!(gs.get_player(PlayerId(0)).unwrap().life, 20);
    }
}
