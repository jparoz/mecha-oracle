// CR 702.21a: Ward — when a permanent with Ward becomes the target of a spell or ability
// an opponent controls, that spell/ability is countered unless the opponent pays the Ward cost.

use super::EngineError;
use crate::types::stack::StackId;
use crate::types::{GameState, PlayerId, StackObject};

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

pub fn pay_ward(
    _state: GameState,
    _player_id: PlayerId,
    _trigger_id: StackId,
) -> Result<GameState, EngineError> {
    unimplemented!("pay_ward: implemented in Task 8")
}
