use super::{
    EngineError,
    state_based_actions::check_and_apply_sbas,
    turn::{advance_step, draw_card},
};
#[cfg(test)]
use crate::types::ability::CastMode;
use crate::types::effect::EffectStep;
use crate::types::stack::StackPayload;
use crate::types::{GameState, PermanentState, PlayerId, Zone};

// CR 405.5: when all players pass in succession, top of stack resolves;
// if stack is empty, current step/phase ends.
pub fn pass_priority(mut state: GameState, player_id: PlayerId) -> Result<GameState, EngineError> {
    if state.priority_player != player_id {
        return Err(EngineError::NotYourPriority);
    }
    state.consecutive_passes += 1;
    if state.consecutive_passes as usize >= state.players.len() {
        if state.stack.is_empty() {
            state.consecutive_passes = 0; // reset before step advance
            Ok(advance_step(state))
        } else {
            Ok(resolve_top(state))
        }
    } else {
        state.priority_player = state.opponent_of(player_id);
        Ok(state)
    }
}

// CR 107.4: substitute the resolving spell/ability's own fixed X for any
// ManaPip::X in a payment cost. Used for effects like "unless its controller
// pays {X}" (CR 118.12), where the X referenced is the caster's X choice,
// not a fresh choice made by the player paying.
fn resolve_x_in_cost(
    cost: &crate::types::ability::Cost,
    x_value: Option<u32>,
) -> crate::types::ability::Cost {
    use crate::types::ability::CostComponent;
    use crate::types::mana::ManaPip;
    let x = x_value.unwrap_or(0);
    cost.iter()
        .map(|component| match component {
            CostComponent::Mana(mana_cost) => CostComponent::Mana(crate::types::mana::ManaCost {
                pips: mana_cost
                    .pips
                    .iter()
                    .map(|pip| {
                        if *pip == ManaPip::X {
                            ManaPip::Generic(x)
                        } else {
                            *pip
                        }
                    })
                    .collect(),
            }),
            other => other.clone(),
        })
        .collect()
}

/// Reads keyword flags from `source_rules_text` and injects them into any `DealDamage`
/// steps in `effect`. Called at stack-push time so flags are snapshotted from the
/// source's current state. CR 702.2e, 702.15b, 702.80a, 702.90b each define last-known
/// information rules for their respective keywords; snapshotting at push time captures
/// the source's state when the ability is activated, which is equivalent to LKI if the
/// source later leaves the battlefield.
pub(crate) fn inject_source_flags(
    effect: crate::types::effect::Effect,
    source_rules_text: &[crate::types::RulesText],
    source_colors: &[crate::types::mana::ManaColor],
    source_card_types: &[crate::types::card::CardType],
    source_subtypes: &[String],
) -> crate::types::effect::Effect {
    use crate::types::ability::KeywordAbility;
    use crate::types::effect::{DamageStep, EffectStep};

    effect
        .into_iter()
        .map(|step| match step {
            EffectStep::DealDamage(s) => EffectStep::DealDamage(DamageStep {
                lifelink: has_damage_kw(source_rules_text, &KeywordAbility::Lifelink),
                deathtouch: has_damage_kw(source_rules_text, &KeywordAbility::Deathtouch),
                wither: has_damage_kw(source_rules_text, &KeywordAbility::Wither),
                infect: has_damage_kw(source_rules_text, &KeywordAbility::Infect),
                source_colors: source_colors.to_vec(),
                source_card_types: source_card_types.to_vec(),
                source_subtypes: source_subtypes.to_vec(),
                ..s
            }),
            other => other,
        })
        .collect()
}

fn has_damage_kw(
    rules_text: &[crate::types::RulesText],
    kw: &crate::types::ability::KeywordAbility,
) -> bool {
    use crate::types::RulesText;
    use crate::types::ability::Rule;
    rules_text
        .iter()
        .any(|span| matches!(span, RulesText::Active(Rule::Static(k)) if k == kw))
}

// CR 608.2b: execute each effect step for the given controller.
// Shared by instant/sorcery spell resolution and triggered/activated ability resolution.
pub(crate) fn execute_effect_steps(
    mut state: GameState,
    controller: PlayerId,
    steps: &[EffectStep],
    targets: &[crate::types::effect::EffectTarget],
    x_value: Option<u32>,
) -> GameState {
    use crate::types::effect::EffectTarget;
    for (i, step) in steps.iter().enumerate() {
        match step {
            EffectStep::DrawCard(n) => {
                for _ in 0..*n {
                    state = draw_card(state, controller, true);
                }
            }
            EffectStep::GainLife(n) => {
                if let Some(player) = state.get_player_mut(controller) {
                    player.life += *n as i32;
                }
            }
            EffectStep::Mill(n) => {
                let to_mill =
                    (*n as usize).min(state.libraries.get(&controller).map_or(0, |l| l.len()));
                for _ in 0..to_mill {
                    if let Some(card_id) = state
                        .libraries
                        .get_mut(&controller)
                        .filter(|l| !l.is_empty())
                        .map(|l| l.remove(0))
                    {
                        if let Some(gy) = state.graveyards.get_mut(&controller) {
                            gy.push(card_id);
                        }
                        if let Some(obj) = state.objects.get_mut(&card_id) {
                            obj.zone = Zone::Graveyard;
                        }
                    }
                }
            }
            EffectStep::AddMana(_) => {
                // Mana abilities never reach the stack (CR 405.6c).
                unreachable!("AddMana in stack object");
            }
            EffectStep::BoostPermanentPT(delta) => {
                // Iterate all targets so multi-target effects (e.g. Battle Cry via
                // AllOtherAttackers) boost every creature, not just the first.
                for target in targets {
                    if let EffectTarget::Object { id } = target
                        && let Some(perm) = state.battlefield.get_mut(id)
                    {
                        perm.pt_boost_until_eot.power += delta.power;
                        perm.pt_boost_until_eot.toughness += delta.toughness;
                    }
                }
            }
            EffectStep::AddCounter { kind, count } => {
                // Iterate all targets so multi-target counter effects apply to every target,
                // not just the first.
                for target in targets.iter() {
                    match target {
                        EffectTarget::Object { id } => {
                            if let Some(perm) = state.battlefield.get_mut(id) {
                                perm.add_counters(kind.clone(), *count);
                            }
                        }
                        EffectTarget::Player { id } => {
                            if let Some(player) = state.get_player_mut(*id) {
                                player.add_counters(kind.clone(), *count);
                            }
                        }
                        _ => {}
                    }
                }
            }
            // CR 702.15b, 702.2b, 702.80a/b, 702.90b/c
            EffectStep::DealDamage(s) => {
                let amount = s.amount;
                // CR 702.16e: damage from a source with the stated quality to a permanent
                // with protection from that quality is prevented entirely.
                // Clone the check result first to avoid holding a shared borrow on state
                // while later code needs mutable access.
                let protected = if let Some(EffectTarget::Object { id }) = targets.first() {
                    state.objects.get(id).is_some_and(|obj| {
                        crate::engine::has_protection_from(
                            obj,
                            &s.source_colors,
                            &s.source_card_types,
                            &s.source_subtypes,
                        )
                    })
                } else {
                    false
                };
                if protected {
                    continue;
                }
                match targets.first() {
                    Some(EffectTarget::Object { id }) => {
                        if let Some(perm) = state.battlefield.get_mut(id) {
                            if s.wither || s.infect {
                                // CR 702.80a / 702.90c: damage to a creature from a wither/infect
                                // source becomes -1/-1 counters instead of marked damage.
                                perm.add_counters(
                                    crate::types::CounterKind::PtModifier {
                                        power: -1,
                                        toughness: -1,
                                    },
                                    amount,
                                );
                            } else {
                                perm.damage_marked += amount;
                            }
                            if s.deathtouch && amount > 0 {
                                // CR 702.2b: any nonzero damage from a deathtouch source is lethal.
                                perm.damaged_by_deathtouch = true;
                            }
                        }
                    }
                    Some(EffectTarget::Player { id }) => {
                        if let Some(player) = state.get_player_mut(*id) {
                            if s.infect {
                                // CR 702.90b: infect damage to a player is poison counters, not life loss.
                                player.add_counters(crate::types::CounterKind::Poison, amount);
                            } else {
                                // CR 702.80a: wither only converts damage to -1/-1 counters on creatures;
                                // wither damage to a player is still regular life loss.
                                player.life -= amount as i32;
                            }
                        }
                    }
                    _ => {}
                }
                if s.lifelink && amount > 0 {
                    // CR 702.15b: source's controller gains life equal to damage dealt.
                    if let Some(player) = state.get_player_mut(controller) {
                        player.life += amount as i32;
                    }
                }
            }
            // CR 701.5: move the targeted stack object to the graveyard (if a spell)
            // or simply remove it (if an ability). counter_spell_on_stack handles both.
            EffectStep::CounterSpell => {
                if let Some(EffectTarget::StackObject { id }) = targets.first() {
                    counter_spell_on_stack(&mut state, *id);
                }
            }
            // CR 118.12: pause resolution and raise a cost-payment obligation.
            // The paying player is derived from the first StackObject target's controller
            // (the caster of the targeted spell). Falls back to the resolving controller.
            EffectStep::Payment {
                cost,
                on_paid,
                on_declined,
            } => {
                let paying_player = targets
                    .iter()
                    .find_map(|t| {
                        if let EffectTarget::StackObject { id } = t {
                            state.stack_objects.get(id).map(|o| o.controller)
                        } else {
                            None
                        }
                    })
                    .unwrap_or(controller);
                let continuation = steps[i + 1..].to_vec();
                state.pending_payment = Some(crate::types::game_state::PendingPayment {
                    paying_player,
                    cost: resolve_x_in_cost(cost, x_value),
                    on_paid: on_paid.clone(),
                    on_declined: on_declined.clone(),
                    continuation,
                    targets: targets.to_vec(),
                    controller,
                });
                return state; // early return; remaining steps stored in continuation
            }
            EffectStep::MoveZone {
                from,
                to,
                to_player,
            } => {
                use crate::types::ZoneOwner;
                for target in targets.iter() {
                    if let EffectTarget::Object { id } = target {
                        let id = *id;
                        // Snapshot before mutation to avoid borrow conflicts.
                        let (owner, current_zone, controller_at_move, def) =
                            match state.objects.get(&id) {
                                Some(o) => (o.owner, o.zone, o.controller, o.definition.clone()),
                                None => continue,
                            };
                        // CR 400.7: no-op if the object is not in the expected source zone.
                        if current_zone != *from {
                            continue;
                        }
                        // Guard against same-zone moves which would silently wipe PermanentState.
                        if from == to {
                            continue;
                        }
                        let new_controller = match to_player {
                            ZoneOwner::CardOwner => owner,
                            ZoneOwner::CardController => controller_at_move,
                        };
                        // Remove from source zone.
                        match from {
                            Zone::Graveyard => {
                                if let Some(gy) = state.graveyards.get_mut(&owner) {
                                    gy.retain(|&x| x != id);
                                }
                            }
                            Zone::Battlefield => {
                                state.battlefield.remove(&id);
                            }
                            Zone::Exile => {
                                state.exile.retain(|&x| x != id);
                            }
                            Zone::Hand => {
                                if let Some(hand) = state.hands.get_mut(&owner) {
                                    hand.retain(|&x| x != id);
                                }
                            }
                            Zone::Library => {
                                if let Some(lib) = state.libraries.get_mut(&owner) {
                                    lib.retain(|&x| x != id);
                                }
                            }
                            Zone::Stack | Zone::Command => {}
                        }
                        // Update object zone and controller.
                        if let Some(obj) = state.objects.get_mut(&id) {
                            obj.zone = *to;
                            if *to == Zone::Battlefield {
                                obj.controller = new_controller;
                            }
                        }
                        // Insert into destination zone.
                        match to {
                            Zone::Battlefield => {
                                let mut perm = PermanentState::new(&def);
                                perm.controller_since_turn = state.turn_number;
                                state.battlefield.insert(id, perm);
                                // CR 603.3: place detected ETB triggers onto the stack.
                                let etb_triggers =
                                    crate::engine::triggered::collect_triggers_for_event(
                                        &mut state,
                                        &crate::types::GameEvent::EntersTheBattlefield {
                                            subject_id: id,
                                        },
                                    );
                                for t in etb_triggers {
                                    let tid = t.id;
                                    state.stack.push(tid);
                                    state.stack_objects.insert(tid, t);
                                }
                            }
                            Zone::Graveyard => {
                                if let Some(gy) = state.graveyards.get_mut(&owner) {
                                    gy.push(id);
                                }
                            }
                            Zone::Exile => {
                                state.exile.push(id);
                            }
                            Zone::Hand => {
                                if let Some(hand) = state.hands.get_mut(&new_controller) {
                                    hand.push(id);
                                }
                            }
                            Zone::Library => {
                                if let Some(lib) = state.libraries.get_mut(&owner) {
                                    lib.push(id);
                                }
                            }
                            Zone::Stack | Zone::Command => {}
                        }
                    }
                }
            }
            EffectStep::Unimplemented(_) => {}
            // CR 702.6a: attach the equipment (source_id) to the first target.
            // Both source and target must still be on the battlefield (LKI — CR 608.2b).
            // CR 702.16d: skip if target has protection from the equipment's quality.
            EffectStep::Attach { source_id } => {
                if let Some(EffectTarget::Object { id: target_id }) = targets.first() {
                    let target_id = *target_id;
                    if state.battlefield.contains_key(source_id)
                        && state.battlefield.contains_key(&target_id)
                    {
                        let (equip_colors, equip_types, equip_subtypes) = state
                            .objects
                            .get(source_id)
                            .map(|o| {
                                (
                                    o.definition.colors.clone(),
                                    o.definition.type_line.card_types.clone(),
                                    o.definition.type_line.subtypes.clone(),
                                )
                            })
                            .unwrap_or_default();
                        let protected = state
                            .objects
                            .get(&target_id)
                            .map(|o| {
                                crate::engine::has_protection_from(
                                    o,
                                    &equip_colors,
                                    &equip_types,
                                    &equip_subtypes,
                                )
                            })
                            .unwrap_or(false);
                        if !protected && let Some(perm) = state.battlefield.get_mut(source_id) {
                            perm.attached_to = Some(target_id);
                        }
                    }
                }
            }
        }
    }
    state
}

/// CR 702.21a: Counter a spell or ability on the stack without it resolving.
/// For spells, the card moves to the graveyard (CR 608.2b).
/// For activated abilities, the ability simply ceases to exist (no card to move).
pub(crate) fn counter_spell_on_stack(
    state: &mut GameState,
    stack_id: crate::types::stack::StackId,
) {
    if let Some(obj) = state.stack_objects.remove(&stack_id) {
        state.stack.retain(|&id| id != stack_id);
        if let StackPayload::Spell { card_id } = obj.payload {
            let controller = obj.controller;
            if let Some(card) = state.objects.get_mut(&card_id) {
                card.zone = Zone::Graveyard;
            }
            if let Some(gy) = state.graveyards.get_mut(&controller) {
                gy.push(card_id);
            }
        }
        // ActivatedAbility and TriggeredAbility stack objects have no card to move.
    }
}

pub fn resolve_top(mut state: GameState) -> GameState {
    let stack_id = match state.stack.last().copied() {
        Some(id) => id,
        None => return state,
    };
    state.stack.pop();
    let stack_obj = match state.stack_objects.remove(&stack_id) {
        Some(obj) => obj,
        None => {
            unreachable!("stack id {stack_id:?} missing from stack_objects; invariant violated")
        }
    };

    let is_activated = matches!(stack_obj.payload, StackPayload::ActivatedAbility { .. });
    let targets = stack_obj.targets.clone();
    let x_value = stack_obj.x_value;

    match stack_obj.payload {
        StackPayload::Spell { card_id } => {
            let controller = stack_obj.controller;
            let is_permanent = state
                .objects
                .get(&card_id)
                .map(|o| o.definition.type_line.is_permanent())
                .unwrap_or(false);

            if is_permanent {
                // CR 303.4g: if this is an aura spell and its target is no longer legal
                // (target left the battlefield in response), the aura goes directly to the
                // owner's graveyard instead of entering the battlefield. This prevents ETB
                // triggers on the aura from firing incorrectly.
                let is_aura = state
                    .objects
                    .get(&card_id)
                    .map(|o| {
                        o.definition.rules_text.iter().any(|span| {
                            matches!(
                                span,
                                crate::types::RulesText::Active(crate::types::Rule::Aura { .. })
                            )
                        })
                    })
                    .unwrap_or(false);
                if is_aura && !crate::engine::targeting::targets_still_legal(&state, &targets) {
                    if let Some(obj) = state.objects.get_mut(&card_id) {
                        obj.zone = Zone::Graveyard;
                    }
                    if let Some(gy) = state.graveyards.get_mut(&controller) {
                        gy.push(card_id);
                    }
                    state.consecutive_passes = 0;
                    state.priority_player = state.active_player;
                    return apply_sbas_and_push_triggers(state);
                }

                // CR 608.3: permanent spells resolve by entering the battlefield.
                let def = state.objects.get(&card_id).map(|o| o.definition.clone());
                if let Some(obj) = state.objects.get_mut(&card_id) {
                    obj.zone = Zone::Battlefield;
                }
                if let Some(def) = def {
                    let mut perm = PermanentState::new(&def);
                    perm.controller_since_turn = state.turn_number;
                    state.battlefield.insert(card_id, perm);
                }

                // CR 603.2 / CR 702.100b: collect ETB triggers and Evolve triggers via unified dispatch.
                let etb_triggers = crate::engine::triggered::collect_triggers_for_event(
                    &mut state,
                    &crate::types::GameEvent::EntersTheBattlefield {
                        subject_id: card_id,
                    },
                );
                for trigger in etb_triggers {
                    let id = trigger.id;
                    state.stack.push(id);
                    state.stack_objects.insert(id, trigger);
                }

                // CR 303.4 / 702.16c: attach aura only if host doesn't have protection from it.
                // If protected, the aura stays on battlefield unattached; SBA 704.5m removes it.
                if is_aura
                    && let Some(crate::types::effect::EffectTarget::Object { id: host_id }) =
                        targets.first()
                {
                    let host_id = *host_id;
                    if state.battlefield.contains_key(&host_id) {
                        let (aura_colors, aura_types, aura_subtypes) = state
                            .objects
                            .get(&card_id)
                            .map(|o| {
                                (
                                    o.definition.colors.clone(),
                                    o.definition.type_line.card_types.clone(),
                                    o.definition.type_line.subtypes.clone(),
                                )
                            })
                            .unwrap_or_default();
                        let host_protected = state
                            .objects
                            .get(&host_id)
                            .map(|o| {
                                crate::engine::has_protection_from(
                                    o,
                                    &aura_colors,
                                    &aura_types,
                                    &aura_subtypes,
                                )
                            })
                            .unwrap_or(false);
                        if !host_protected && let Some(perm) = state.battlefield.get_mut(&card_id) {
                            perm.attached_to = Some(host_id);
                        }
                    }
                }
            } else {
                // CR 608.2b: instant/sorcery — execute effects, then move to graveyard.
                let steps: Vec<EffectStep> = state
                    .objects
                    .get(&card_id)
                    .map(|obj| {
                        obj.definition
                            .rules_text
                            .iter()
                            .filter_map(|span| match span {
                                crate::types::RulesText::Active(
                                    crate::types::Rule::SpellAbility(spell_ability),
                                ) => Some(spell_ability.steps.clone()),
                                _ => None,
                            })
                            .flatten()
                            .collect()
                    })
                    .unwrap_or_default();

                let (spell_rules_text, spell_colors, spell_card_types, spell_subtypes) = state
                    .objects
                    .get(&card_id)
                    .map(|o| {
                        (
                            o.definition.rules_text.clone(),
                            o.definition.colors.clone(),
                            o.definition.type_line.card_types.clone(),
                            o.definition.type_line.subtypes.clone(),
                        )
                    })
                    .unwrap_or_default();
                let steps = inject_source_flags(
                    steps,
                    &spell_rules_text,
                    &spell_colors,
                    &spell_card_types,
                    &spell_subtypes,
                );

                // CR 608.2b: if all targets are illegal at resolution, spell is countered
                // by the rules (instant/sorcery still moves to graveyard, effects not applied).
                if !targets.is_empty()
                    && !crate::engine::targeting::targets_still_legal(&state, &targets)
                {
                    if let Some(obj) = state.objects.get_mut(&card_id) {
                        obj.zone = Zone::Graveyard;
                    }
                    if let Some(gy) = state.graveyards.get_mut(&controller) {
                        gy.push(card_id);
                    }
                    state.consecutive_passes = 0;
                    state.priority_player = state.active_player;
                    return apply_sbas_and_push_triggers(state);
                }

                state = execute_effect_steps(state, controller, &steps, &targets, x_value);

                if let Some(obj) = state.objects.get_mut(&card_id) {
                    obj.zone = Zone::Graveyard;
                }
                if let Some(gy) = state.graveyards.get_mut(&controller) {
                    gy.push(card_id);
                }
            }

            // If a Payment step paused resolution, give priority to the paying player.
            if let Some(pp) = &state.pending_payment {
                let paying_player = pp.paying_player;
                state.consecutive_passes = 0;
                state.priority_player = paying_player;
                return apply_sbas_and_push_triggers(state);
            }

            // CR 117.3b: after triggered abilities are put on the stack, active player
            // receives priority (distinct from CR 117.3c where the caster retains priority
            // after casting a spell or activating an ability).
            state.consecutive_passes = 0;
            state.priority_player = state.active_player;
            apply_sbas_and_push_triggers(state)
        }
        StackPayload::TriggeredAbility { effect, .. }
        | StackPayload::ActivatedAbility { effect, .. } => {
            let controller = stack_obj.controller;
            // CR 608.2b: non-mana activated abilities with all-illegal targets fizzle.
            // Triggered abilities don't fizzle — they just silently have no effect.
            if is_activated
                && !targets.is_empty()
                && !crate::engine::targeting::targets_still_legal(&state, &targets)
            {
                state.consecutive_passes = 0;
                state.priority_player = state.active_player;
                return apply_sbas_and_push_triggers(state);
            }
            state = execute_effect_steps(state, controller, &effect, &targets, x_value);

            // If a Payment step paused resolution, give priority to the paying player.
            if let Some(pp) = &state.pending_payment {
                let paying_player = pp.paying_player;
                state.consecutive_passes = 0;
                state.priority_player = paying_player;
                return apply_sbas_and_push_triggers(state);
            }

            state.consecutive_passes = 0;
            state.priority_player = state.active_player;
            apply_sbas_and_push_triggers(state)
        }
    }
}

/// Run SBAs and push any resulting triggers (e.g. Dies) onto the stack.
/// CR 704.3 / CR 603.2.
fn apply_sbas_and_push_triggers(state: GameState) -> GameState {
    let (mut state, sba_triggers) = check_and_apply_sbas(state);
    for t in sba_triggers {
        let id = t.id;
        state.stack.push(id);
        state.stack_objects.insert(id, t);
    }
    state
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cards::test_helpers::test_db;
    use crate::types::card::{CardDefinition, CardType, TypeLine};
    use crate::types::effect::EffectStep;
    use crate::types::mana::{ManaCost, ManaPip};
    use crate::types::stack::{StackObject, StackPayload};
    use crate::types::{CardObject, ObjectId, Player, Rule, RulesText, Step};

    fn make_state() -> GameState {
        let mut gs = GameState::new(vec![
            Player::new(PlayerId(0), "Alice"),
            Player::new(PlayerId(1), "Bob"),
        ]);
        gs.step = Step::PreCombatMain;
        gs
    }

    fn push_spell(state: &mut GameState, card_id: ObjectId) {
        let stack_id = state.alloc_stack_id();
        let controller = state.active_player;
        let obj = StackObject {
            id: stack_id,
            payload: StackPayload::Spell { card_id },
            controller,
            targets: vec![],
            x_value: None,
            cast_mode: CastMode::Standard,
        };
        state.stack.push(stack_id);
        state.stack_objects.insert(stack_id, obj);
    }

    fn push_draw_trigger(state: &mut GameState, source_id: ObjectId) {
        let stack_id = state.alloc_stack_id();
        let controller = state.active_player;
        let obj = StackObject {
            id: stack_id,
            payload: StackPayload::TriggeredAbility {
                source_id,
                effect: vec![EffectStep::DrawCard(1)],
                label: "draw trigger".into(),
            },
            controller,
            targets: vec![],
            x_value: None,
            cast_mode: CastMode::Standard,
        };
        state.stack.push(stack_id);
        state.stack_objects.insert(stack_id, obj);
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
    fn pass_priority_wrong_player_returns_error() {
        let mut gs = make_state();
        gs.priority_player = PlayerId(0);
        assert!(matches!(
            pass_priority(gs, PlayerId(1)),
            Err(EngineError::NotYourPriority)
        ));
    }

    #[test]
    fn pass_priority_once_shifts_priority_to_opponent() {
        let mut gs = make_state();
        gs.priority_player = PlayerId(0);
        let gs = pass_priority(gs, PlayerId(0)).unwrap();
        assert_eq!(gs.priority_player, PlayerId(1));
        assert_eq!(gs.consecutive_passes, 1);
    }

    #[test]
    fn pass_priority_twice_empty_stack_advances_step() {
        let mut gs = make_state();
        gs.step = Step::PreCombatMain;
        gs.priority_player = PlayerId(0);
        let gs = pass_priority(gs, PlayerId(0)).unwrap();
        let gs = pass_priority(gs, PlayerId(1)).unwrap();
        assert_eq!(gs.step(), Step::BeginningOfCombat);
    }

    #[test]
    fn pass_priority_twice_resolves_top_spell() {
        let db = test_db();
        let mut gs = make_state();
        let id = gs.alloc_id();
        let obj = CardObject::new(
            id,
            db.get("Grizzly Bears").unwrap().clone(),
            PlayerId(0),
            Zone::Stack,
        );
        gs.add_object(obj);
        push_spell(&mut gs, id);
        gs.priority_player = PlayerId(0);

        let gs = pass_priority(gs, PlayerId(0)).unwrap();
        let gs = pass_priority(gs, PlayerId(1)).unwrap();

        assert!(gs.battlefield.contains_key(&id));
        assert_eq!(gs.objects[&id].zone, Zone::Battlefield);
        assert!(gs.stack.is_empty());
    }

    #[test]
    fn pass_priority_resets_consecutive_passes_after_resolution() {
        let db = test_db();
        let mut gs = make_state();
        let id = gs.alloc_id();
        let obj = CardObject::new(
            id,
            db.get("Grizzly Bears").unwrap().clone(),
            PlayerId(0),
            Zone::Stack,
        );
        gs.add_object(obj);
        push_spell(&mut gs, id);
        gs.priority_player = PlayerId(0);

        let gs = pass_priority(gs, PlayerId(0)).unwrap();
        let gs = pass_priority(gs, PlayerId(1)).unwrap();

        assert_eq!(gs.consecutive_passes, 0);
        assert_eq!(gs.priority_player, PlayerId(0));
    }

    #[test]
    fn resolve_top_triggered_ability_draws_card() {
        let mut gs = make_state();
        put_in_library(&mut gs, PlayerId(0));
        push_draw_trigger(&mut gs, ObjectId(99));

        let gs = resolve_top(gs);

        assert_eq!(gs.hands[&PlayerId(0)].len(), 1);
        assert!(gs.libraries[&PlayerId(0)].is_empty());
        assert!(gs.stack.is_empty());
    }

    #[test]
    fn resolve_top_triggered_ability_gains_life() {
        let mut gs = make_state();
        let before_life = gs.get_player(PlayerId(0)).unwrap().life;
        let stack_id = gs.alloc_stack_id();
        let obj = StackObject {
            id: stack_id,
            payload: StackPayload::TriggeredAbility {
                source_id: ObjectId(99),
                effect: vec![EffectStep::GainLife(5)],
                label: "gain life trigger".into(),
            },
            controller: PlayerId(0),
            targets: vec![],
            x_value: None,
            cast_mode: CastMode::Standard,
        };
        gs.stack.push(stack_id);
        gs.stack_objects.insert(stack_id, obj);

        let gs = resolve_top(gs);

        assert_eq!(gs.get_player(PlayerId(0)).unwrap().life, before_life + 5);
        assert!(gs.stack.is_empty());
    }

    #[test]
    fn resolve_top_triggered_ability_mills() {
        let mut gs = make_state();
        let card1 = put_in_library(&mut gs, PlayerId(0));
        let card2 = put_in_library(&mut gs, PlayerId(0));
        let stack_id = gs.alloc_stack_id();
        let obj = StackObject {
            id: stack_id,
            payload: StackPayload::TriggeredAbility {
                source_id: ObjectId(99),
                effect: vec![EffectStep::Mill(2)],
                label: "mill trigger".into(),
            },
            controller: PlayerId(0),
            targets: vec![],
            x_value: None,
            cast_mode: CastMode::Standard,
        };
        gs.stack.push(stack_id);
        gs.stack_objects.insert(stack_id, obj);

        let gs = resolve_top(gs);

        assert!(gs.libraries[&PlayerId(0)].is_empty());
        assert!(gs.graveyards[&PlayerId(0)].contains(&card1));
        assert!(gs.graveyards[&PlayerId(0)].contains(&card2));
        assert!(gs.stack.is_empty());
    }

    #[test]
    fn resolve_top_spell_moves_card_to_battlefield() {
        let db = test_db();
        let mut gs = make_state();
        let id = gs.alloc_id();
        let obj = CardObject::new(
            id,
            db.get("Grizzly Bears").unwrap().clone(),
            PlayerId(0),
            Zone::Stack,
        );
        gs.add_object(obj);
        push_spell(&mut gs, id);

        let gs = resolve_top(gs);

        assert!(gs.battlefield.contains_key(&id));
        assert_eq!(gs.objects[&id].zone, Zone::Battlefield);
        let cmt = gs.controllers_most_recent_turn(PlayerId(0));
        assert!(gs.battlefield[&id].summoning_sick(cmt));
        assert!(gs.stack.is_empty());
    }

    #[test]
    fn resolve_top_empty_stack_is_noop() {
        let gs = make_state();
        let gs2 = resolve_top(gs);
        assert!(gs2.stack.is_empty());
        assert_eq!(gs2.consecutive_passes, 0);
    }

    fn make_instant_obj(
        state: &mut GameState,
        owner: PlayerId,
        steps: Vec<EffectStep>,
    ) -> ObjectId {
        use crate::types::ability::SpellAbility;
        let def = CardDefinition {
            name: "Test Instant".into(),
            mana_cost: Some(ManaCost {
                pips: vec![ManaPip::Blue],
            }),
            type_line: TypeLine {
                supertypes: vec![],
                card_types: vec![CardType::Instant],
                subtypes: vec![],
            },
            oracle_text: String::new(),
            rules_text: vec![RulesText::Active(Rule::SpellAbility(SpellAbility {
                target_requirements: vec![],
                steps,
            }))],
            text_annotations: vec![],
            power: None,
            toughness: None,
            colors: vec![],
        };
        let id = state.alloc_id();
        let obj = CardObject::new(id, def, owner, Zone::Stack);
        state.add_object(obj);
        id
    }

    fn make_sorcery_obj(
        state: &mut GameState,
        owner: PlayerId,
        steps: Vec<EffectStep>,
    ) -> ObjectId {
        use crate::types::ability::SpellAbility;
        let def = CardDefinition {
            name: "Test Sorcery".into(),
            mana_cost: Some(ManaCost {
                pips: vec![ManaPip::Blue],
            }),
            type_line: TypeLine {
                supertypes: vec![],
                card_types: vec![CardType::Sorcery],
                subtypes: vec![],
            },
            oracle_text: String::new(),
            rules_text: vec![RulesText::Active(Rule::SpellAbility(SpellAbility {
                target_requirements: vec![],
                steps,
            }))],
            text_annotations: vec![],
            power: None,
            toughness: None,
            colors: vec![],
        };
        let id = state.alloc_id();
        let obj = CardObject::new(id, def, owner, Zone::Stack);
        state.add_object(obj);
        id
    }

    #[test]
    fn instant_spell_resolves_to_graveyard() {
        let mut gs = make_state();
        let id = make_instant_obj(&mut gs, PlayerId(0), vec![]);
        push_spell(&mut gs, id);

        let gs = resolve_top(gs);

        assert_eq!(gs.objects[&id].zone, Zone::Graveyard);
        assert!(gs.graveyards[&PlayerId(0)].contains(&id));
        assert!(!gs.battlefield.contains_key(&id));
        assert!(gs.stack.is_empty());
    }

    #[test]
    fn instant_spell_draw_effect_executes_before_graveyard() {
        let mut gs = make_state();
        put_in_library(&mut gs, PlayerId(0));
        let id = make_instant_obj(&mut gs, PlayerId(0), vec![EffectStep::DrawCard(1)]);
        push_spell(&mut gs, id);

        let gs = resolve_top(gs);

        assert_eq!(gs.objects[&id].zone, Zone::Graveyard);
        assert_eq!(gs.hands[&PlayerId(0)].len(), 1);
    }

    #[test]
    fn instant_draw_three_executes_fully() {
        let mut gs = make_state();
        put_in_library(&mut gs, PlayerId(0));
        put_in_library(&mut gs, PlayerId(0));
        put_in_library(&mut gs, PlayerId(0));
        let id = make_instant_obj(&mut gs, PlayerId(0), vec![EffectStep::DrawCard(3)]);
        push_spell(&mut gs, id);

        let gs = resolve_top(gs);

        assert_eq!(gs.hands[&PlayerId(0)].len(), 3);
        assert_eq!(gs.objects[&id].zone, Zone::Graveyard);
    }

    #[test]
    fn unimplemented_steps_are_skipped_silently() {
        let mut gs = make_state();
        put_in_library(&mut gs, PlayerId(0));
        let id = make_instant_obj(
            &mut gs,
            PlayerId(0),
            vec![
                EffectStep::DrawCard(1),
                EffectStep::Unimplemented("scry 2".into()),
            ],
        );
        push_spell(&mut gs, id);
        let before_life = gs.get_player(PlayerId(0)).unwrap().life;

        let gs = resolve_top(gs);

        assert_eq!(gs.hands[&PlayerId(0)].len(), 1); // drew 1
        assert_eq!(gs.get_player(PlayerId(0)).unwrap().life, before_life); // no life gain
        assert_eq!(gs.objects[&id].zone, Zone::Graveyard);
    }

    #[test]
    fn sorcery_with_no_parseable_effects_just_goes_to_graveyard() {
        let mut gs = make_state();
        let before_hand = gs.hands[&PlayerId(0)].len();
        let id = make_sorcery_obj(
            &mut gs,
            PlayerId(0),
            vec![EffectStep::Unimplemented("Counter target spell".into())],
        );
        push_spell(&mut gs, id);

        let gs = resolve_top(gs);

        assert_eq!(gs.objects[&id].zone, Zone::Graveyard);
        assert_eq!(gs.hands[&PlayerId(0)].len(), before_hand); // nothing happened
    }

    #[test]
    fn creature_spell_still_resolves_to_battlefield() {
        // Regression: permanents must still go to battlefield, not graveyard
        let db = test_db();
        let mut gs = make_state();
        let id = gs.alloc_id();
        let obj = CardObject::new(
            id,
            db.get("Grizzly Bears").unwrap().clone(),
            PlayerId(0),
            Zone::Stack,
        );
        gs.add_object(obj);
        push_spell(&mut gs, id);

        let gs = resolve_top(gs);

        assert!(gs.battlefield.contains_key(&id));
        assert_eq!(gs.objects[&id].zone, Zone::Battlefield);
        assert!(!gs.graveyards[&PlayerId(0)].contains(&id));
    }

    #[test]
    fn resolve_top_spell_collects_etb_triggers_onto_stack() {
        use crate::types::ability::{
            TriggerEvent, TriggerSubjectFilter, TriggerTargetMode, TriggeredAbility,
        };

        let mut gs = make_state();
        put_in_library(&mut gs, PlayerId(0));

        let def = CardDefinition {
            name: "Elvish Visionary".into(),
            mana_cost: Some(ManaCost {
                pips: vec![ManaPip::Green],
            }),
            type_line: TypeLine {
                supertypes: vec![],
                card_types: vec![CardType::Creature],
                subtypes: vec!["Elf".into()],
            },
            oracle_text: "When this enters, draw a card.".into(),
            rules_text: vec![RulesText::Active(Rule::Triggered(TriggeredAbility {
                trigger: TriggerEvent::EntersTheBattlefield {
                    subject: TriggerSubjectFilter {
                        is_self: Some(true),
                        ..Default::default()
                    },
                },
                condition: None,
                target_mode: TriggerTargetMode::None,
                effect: vec![EffectStep::DrawCard(1)],
            }))],
            text_annotations: vec![],
            power: Some(1),
            toughness: Some(1),
            colors: vec![],
        };
        let id = gs.alloc_id();
        let obj = CardObject::new(id, def, PlayerId(0), Zone::Stack);
        gs.add_object(obj);
        push_spell(&mut gs, id);

        let gs = resolve_top(gs);

        // Creature on battlefield, ETB trigger waiting on stack
        assert!(gs.battlefield.contains_key(&id));
        assert_eq!(gs.stack.len(), 1);
        // Card not yet drawn — trigger hasn't resolved
        assert!(gs.hands[&PlayerId(0)].is_empty());

        let gs = resolve_top(gs);

        // Trigger resolved
        assert_eq!(gs.hands[&PlayerId(0)].len(), 1);
        assert!(gs.stack.is_empty());
    }

    #[test]
    fn boost_permanent_pt_effect_applies_delta() {
        use crate::types::PTDelta;

        let mut gs = make_state();
        // Put a 2/2 creature on the battlefield.
        let def = CardDefinition {
            name: "Target".into(),
            mana_cost: None,
            type_line: TypeLine {
                supertypes: vec![],
                card_types: vec![CardType::Creature],
                subtypes: vec![],
            },
            oracle_text: String::new(),
            rules_text: vec![],
            text_annotations: vec![],
            power: Some(2),
            toughness: Some(2),
            colors: vec![],
        };
        let id = gs.alloc_id();
        let obj = CardObject::new(id, def, PlayerId(0), Zone::Battlefield);
        let mut perm = PermanentState::new(&obj.definition);
        perm.controller_since_turn = 0;
        gs.battlefield.insert(id, perm);
        gs.add_object(obj);

        // Push a BoostPermanentPT trigger onto the stack.
        let stack_id = gs.alloc_stack_id();
        use crate::types::effect::EffectTarget;
        let stack_obj = StackObject {
            id: stack_id,
            payload: StackPayload::TriggeredAbility {
                source_id: id,
                effect: vec![EffectStep::BoostPermanentPT(PTDelta {
                    power: 1,
                    toughness: 1,
                })],
                label: "test boost".into(),
            },
            controller: PlayerId(0),
            targets: vec![EffectTarget::Object { id }],
            x_value: None,
            cast_mode: CastMode::Standard,
        };
        gs.stack.push(stack_id);
        gs.stack_objects.insert(stack_id, stack_obj);

        let gs = resolve_top(gs);

        assert_eq!(gs.battlefield[&id].effective_power(0), Some(3));
        assert_eq!(gs.battlefield[&id].effective_toughness(0), Some(3));
        assert!(gs.stack.is_empty());
    }

    #[test]
    fn boost_permanent_pt_noop_if_not_on_battlefield() {
        use crate::types::PTDelta;

        let mut gs = make_state();
        let nonexistent_id = ObjectId(999);
        let stack_id = gs.alloc_stack_id();
        use crate::types::effect::EffectTarget;
        let stack_obj = StackObject {
            id: stack_id,
            payload: StackPayload::TriggeredAbility {
                source_id: nonexistent_id,
                effect: vec![EffectStep::BoostPermanentPT(PTDelta {
                    power: 5,
                    toughness: 5,
                })],
                label: "noop boost".into(),
            },
            controller: PlayerId(0),
            targets: vec![EffectTarget::Object { id: nonexistent_id }],
            x_value: None,
            cast_mode: CastMode::Standard,
        };
        gs.stack.push(stack_id);
        gs.stack_objects.insert(stack_id, stack_obj);

        // Should not panic.
        let gs = resolve_top(gs);
        assert!(gs.stack.is_empty());
    }

    #[test]
    fn deal_damage_to_creature_marks_damage() {
        let mut gs = make_state();
        // Toughness 4 so 3 damage is sub-lethal and the creature survives SBAs.
        let def = CardDefinition {
            name: "Target Creature".into(),
            mana_cost: None,
            type_line: TypeLine {
                supertypes: vec![],
                card_types: vec![CardType::Creature],
                subtypes: vec![],
            },
            oracle_text: String::new(),
            rules_text: vec![],
            text_annotations: vec![],
            power: Some(2),
            toughness: Some(4),
            colors: vec![],
        };
        let id = gs.alloc_id();
        let obj = CardObject::new(id, def, PlayerId(1), Zone::Battlefield);
        gs.battlefield
            .insert(id, PermanentState::new(&obj.definition));
        gs.add_object(obj);

        let stack_id = gs.alloc_stack_id();
        use crate::types::effect::EffectTarget;
        let stack_obj = StackObject {
            id: stack_id,
            payload: StackPayload::TriggeredAbility {
                source_id: ObjectId(99),
                effect: vec![EffectStep::DealDamage(crate::types::effect::DamageStep {
                    amount: 3,
                    ..Default::default()
                })],
                label: "test damage".into(),
            },
            controller: PlayerId(0),
            targets: vec![EffectTarget::Object { id }],
            x_value: None,
            cast_mode: CastMode::Standard,
        };
        gs.stack.push(stack_id);
        gs.stack_objects.insert(stack_id, stack_obj);

        let gs = resolve_top(gs);

        assert_eq!(gs.battlefield[&id].damage_marked, 3);
        assert!(gs.stack.is_empty());
    }

    #[test]
    fn deal_damage_to_player_reduces_life() {
        let mut gs = make_state();
        let before_life = gs.get_player(PlayerId(1)).unwrap().life;

        let stack_id = gs.alloc_stack_id();
        use crate::types::effect::EffectTarget;
        let stack_obj = StackObject {
            id: stack_id,
            payload: StackPayload::TriggeredAbility {
                source_id: ObjectId(99),
                effect: vec![EffectStep::DealDamage(crate::types::effect::DamageStep {
                    amount: 3,
                    ..Default::default()
                })],
                label: "test damage".into(),
            },
            controller: PlayerId(0),
            targets: vec![EffectTarget::Player { id: PlayerId(1) }],
            x_value: None,
            cast_mode: CastMode::Standard,
        };
        gs.stack.push(stack_id);
        gs.stack_objects.insert(stack_id, stack_obj);

        let gs = resolve_top(gs);

        assert_eq!(gs.get_player(PlayerId(1)).unwrap().life, before_life - 3);
        assert!(gs.stack.is_empty());
    }

    #[test]
    fn targeted_spell_fizzles_when_target_leaves_before_resolution() {
        use crate::types::PTDelta;
        use crate::types::ability::{SpellAbility, TargetFilter};
        use crate::types::effect::EffectTarget;

        let mut gs = make_state();

        // Put a creature on battlefield as target
        let def = CardDefinition {
            name: "Target".into(),
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
        let creature_id = gs.alloc_id();
        let creature_obj = CardObject::new(creature_id, def, PlayerId(1), Zone::Battlefield);
        gs.battlefield
            .insert(creature_id, PermanentState::new(&creature_obj.definition));
        gs.add_object(creature_obj);

        // Put a Giant Growth targeting that creature on the stack
        let gg_def = CardDefinition {
            name: "Giant Growth".into(),
            mana_cost: Some(ManaCost {
                pips: vec![ManaPip::Green],
            }),
            type_line: TypeLine {
                supertypes: vec![],
                card_types: vec![CardType::Instant],
                subtypes: vec![],
            },
            oracle_text: "Target creature gets +3/+3 until end of turn.".into(),
            rules_text: vec![RulesText::Active(Rule::SpellAbility(SpellAbility {
                target_requirements: vec![TargetFilter::Creature],
                steps: vec![EffectStep::BoostPermanentPT(PTDelta {
                    power: 3,
                    toughness: 3,
                })],
            }))],
            text_annotations: vec![],
            power: None,
            toughness: None,
            colors: vec![],
        };
        let gg_id = gs.alloc_id();
        let gg_obj = CardObject::new(gg_id, gg_def, PlayerId(0), Zone::Stack);
        gs.add_object(gg_obj);
        push_spell(&mut gs, gg_id);
        // Set the target on the stack object
        let stack_id = *gs.stack.last().unwrap();
        gs.stack_objects.get_mut(&stack_id).unwrap().targets =
            vec![EffectTarget::Object { id: creature_id }];

        // Remove creature from battlefield BEFORE resolution
        gs.battlefield.remove(&creature_id);
        gs.objects.get_mut(&creature_id).unwrap().zone = Zone::Graveyard;

        let gs = resolve_top(gs);

        // Spell fizzled: no boost applied (creature is gone), spell moved to graveyard
        assert!(gs.stack.is_empty());
        assert!(!gs.battlefield.contains_key(&creature_id));
        assert!(gs.graveyards[&PlayerId(0)].contains(&gg_id));
        // No crash; game continues normally
    }

    #[test]
    fn targeted_activated_ability_fizzles_when_player_target_loses() {
        // Discriminating fizzle test: activated DealDamage targeting a player who has_lost=true
        // would apply damage without the fizzle check (player still exists in state via get_player_mut).
        // Triggered abilities don't fizzle; activated abilities do (CR 608.2b).
        use crate::types::effect::EffectTarget;
        let mut gs = make_state();
        let before_life = gs.get_player(PlayerId(1)).unwrap().life;

        let stack_id = gs.alloc_stack_id();
        let stack_obj = StackObject {
            id: stack_id,
            payload: StackPayload::ActivatedAbility {
                source_id: ObjectId(99),
                effect: vec![EffectStep::DealDamage(crate::types::effect::DamageStep {
                    amount: 3,
                    ..Default::default()
                })],
                label: "test damage".into(),
            },
            controller: PlayerId(0),
            targets: vec![EffectTarget::Player { id: PlayerId(1) }],
            x_value: None,
            cast_mode: CastMode::Standard,
        };
        gs.stack.push(stack_id);
        gs.stack_objects.insert(stack_id, stack_obj);

        // Player 1 "loses" before resolution — target becomes illegal
        gs.get_player_mut(PlayerId(1)).unwrap().has_lost = true;

        let gs = resolve_top(gs);

        // Fizzle: damage NOT applied (player is illegal target)
        assert_eq!(gs.get_player(PlayerId(1)).unwrap().life, before_life);
        assert!(gs.stack.is_empty());
    }

    #[test]
    fn payment_step_sets_pending_payment() {
        use crate::types::ability::{Cost, CostComponent};
        use crate::types::card::{CardDefinition, CardType, TypeLine};
        use crate::types::effect::{EffectStep, EffectTarget};
        use crate::types::mana::{ManaCost, ManaPip};
        use crate::types::stack::{StackObject, StackPayload};
        use crate::types::{CardObject, Zone};

        let mut gs = make_state();

        // Put a target spell on the stack (the spell being "paid against")
        let target_card_id = gs.alloc_id();
        let def = CardDefinition {
            name: "Target Spell".into(),
            mana_cost: Some(ManaCost { pips: vec![] }),
            type_line: TypeLine {
                supertypes: vec![],
                card_types: vec![CardType::Instant],
                subtypes: vec![],
            },
            oracle_text: String::new(),
            rules_text: vec![],
            text_annotations: vec![],
            power: None,
            toughness: None,
            colors: vec![],
        };
        let target_obj = CardObject::new(target_card_id, def, PlayerId(1), Zone::Stack);
        gs.add_object(target_obj);
        let target_sid = gs.alloc_stack_id();
        gs.stack_objects.insert(
            target_sid,
            StackObject {
                id: target_sid,
                payload: StackPayload::Spell {
                    card_id: target_card_id,
                },
                controller: PlayerId(1),
                targets: vec![],
                x_value: None,
                cast_mode: CastMode::Standard,
            },
        );
        gs.stack.push(target_sid);

        let cost: Cost = vec![CostComponent::Mana(ManaCost {
            pips: vec![ManaPip::Generic(3)],
        })];
        let steps = vec![EffectStep::Payment {
            cost: cost.clone(),
            on_paid: vec![],
            on_declined: vec![EffectStep::CounterSpell],
        }];
        let targets = vec![EffectTarget::StackObject { id: target_sid }];

        let gs = execute_effect_steps(gs, PlayerId(0), &steps, &targets, None);

        assert!(gs.pending_payment.is_some());
        let pp = gs.pending_payment.as_ref().unwrap();
        assert_eq!(pp.paying_player, PlayerId(1)); // target spell's controller
        assert_eq!(pp.cost, cost);
        assert_eq!(pp.on_declined, vec![EffectStep::CounterSpell]);
        // target spell still on stack (not countered yet)
        assert!(gs.stack.contains(&target_sid));
    }

    /// CR 107.4 / CR 118.12: "unless its controller pays {X}" (e.g. Condescend)
    /// resolves X using the resolving spell's own fixed X, not a fresh choice
    /// by the player paying — so the stored PendingPayment cost must already
    /// contain a concrete Generic pip, not ManaPip::X.
    #[test]
    fn payment_step_substitutes_resolving_spells_x_value() {
        use crate::types::ability::{Cost, CostComponent};
        use crate::types::card::{CardDefinition, CardType, TypeLine};
        use crate::types::effect::{EffectStep, EffectTarget};
        use crate::types::mana::{ManaCost, ManaPip};
        use crate::types::stack::{StackObject, StackPayload};
        use crate::types::{CardObject, Zone};

        let mut gs = make_state();

        let target_card_id = gs.alloc_id();
        let def = CardDefinition {
            name: "Target Spell".into(),
            mana_cost: Some(ManaCost { pips: vec![] }),
            type_line: TypeLine {
                supertypes: vec![],
                card_types: vec![CardType::Instant],
                subtypes: vec![],
            },
            oracle_text: String::new(),
            rules_text: vec![],
            text_annotations: vec![],
            power: None,
            toughness: None,
            colors: vec![],
        };
        let target_obj = CardObject::new(target_card_id, def, PlayerId(1), Zone::Stack);
        gs.add_object(target_obj);
        let target_sid = gs.alloc_stack_id();
        gs.stack_objects.insert(
            target_sid,
            StackObject {
                id: target_sid,
                payload: StackPayload::Spell {
                    card_id: target_card_id,
                },
                controller: PlayerId(1),
                targets: vec![],
                x_value: None,
                cast_mode: CastMode::Standard,
            },
        );
        gs.stack.push(target_sid);

        let cost: Cost = vec![CostComponent::Mana(ManaCost {
            pips: vec![ManaPip::X],
        })];
        let steps = vec![EffectStep::Payment {
            cost,
            on_paid: vec![],
            on_declined: vec![EffectStep::CounterSpell],
        }];
        let targets = vec![EffectTarget::StackObject { id: target_sid }];

        // The resolving spell (e.g. Condescend) was cast with X=4.
        let gs = execute_effect_steps(gs, PlayerId(0), &steps, &targets, Some(4));

        let pp = gs.pending_payment.as_ref().unwrap();
        assert_eq!(
            pp.cost,
            vec![CostComponent::Mana(ManaCost {
                pips: vec![ManaPip::Generic(4)]
            })]
        );
    }

    #[test]
    fn add_counter_to_creature_places_counter_on_permanent() {
        use crate::types::CounterKind;
        use crate::types::effect::EffectTarget;

        let mut gs = make_state();
        let def = CardDefinition {
            name: "Target".into(),
            mana_cost: None,
            type_line: TypeLine {
                supertypes: vec![],
                card_types: vec![CardType::Creature],
                subtypes: vec![],
            },
            oracle_text: String::new(),
            rules_text: vec![],
            text_annotations: vec![],
            power: Some(2),
            toughness: Some(2),
            colors: vec![],
        };
        let id = gs.alloc_id();
        let obj = CardObject::new(id, def, PlayerId(0), Zone::Battlefield);
        gs.battlefield
            .insert(id, PermanentState::new(&obj.definition));
        gs.add_object(obj);

        let stack_id = gs.alloc_stack_id();
        let stack_obj = StackObject {
            id: stack_id,
            payload: StackPayload::TriggeredAbility {
                source_id: id,
                effect: vec![EffectStep::AddCounter {
                    kind: CounterKind::PtModifier {
                        power: 1,
                        toughness: 1,
                    },
                    count: 2,
                }],
                label: "add counter test".into(),
            },
            controller: PlayerId(0),
            targets: vec![EffectTarget::Object { id }],
            x_value: None,
            cast_mode: CastMode::Standard,
        };
        gs.stack.push(stack_id);
        gs.stack_objects.insert(stack_id, stack_obj);

        let gs = resolve_top(gs);

        assert_eq!(
            gs.battlefield[&id].counter_count(&CounterKind::PtModifier {
                power: 1,
                toughness: 1
            }),
            2
        );
        assert_eq!(gs.battlefield[&id].effective_power(0), Some(4)); // 2 base + 2 counters
        assert_eq!(gs.battlefield[&id].effective_toughness(0), Some(4));
        assert!(gs.stack.is_empty());
    }

    #[test]
    fn add_counter_to_player_places_counter_on_player() {
        use crate::types::CounterKind;
        use crate::types::effect::EffectTarget;

        let mut gs = make_state();

        let stack_id = gs.alloc_stack_id();
        let stack_obj = StackObject {
            id: stack_id,
            payload: StackPayload::TriggeredAbility {
                source_id: ObjectId(99),
                effect: vec![EffectStep::AddCounter {
                    kind: CounterKind::Poison,
                    count: 3,
                }],
                label: "poison test".into(),
            },
            controller: PlayerId(0),
            targets: vec![EffectTarget::Player { id: PlayerId(1) }],
            x_value: None,
            cast_mode: CastMode::Standard,
        };
        gs.stack.push(stack_id);
        gs.stack_objects.insert(stack_id, stack_obj);

        let gs = resolve_top(gs);

        assert_eq!(
            gs.get_player(PlayerId(1))
                .unwrap()
                .counter_count(&CounterKind::Poison),
            3
        );
        assert!(gs.stack.is_empty());
    }

    // ── DamageStep keyword resolution tests ─────────────────────────────────────

    fn make_creature_on_battlefield(
        gs: &mut GameState,
        owner: PlayerId,
        power: i32,
        toughness: i32,
    ) -> ObjectId {
        use crate::types::card::{CardDefinition, CardType, TypeLine};
        let def = CardDefinition {
            name: "Test Creature".into(),
            mana_cost: None,
            type_line: TypeLine {
                supertypes: vec![],
                card_types: vec![CardType::Creature],
                subtypes: vec![],
            },
            oracle_text: String::new(),
            rules_text: vec![],
            text_annotations: vec![],
            power: Some(power),
            toughness: Some(toughness),
            colors: vec![],
        };
        let id = gs.alloc_id();
        let obj = CardObject::new(id, def, owner, Zone::Battlefield);
        gs.battlefield
            .insert(id, PermanentState::new(&obj.definition));
        gs.add_object(obj);
        id
    }

    fn push_damage_trigger(
        gs: &mut GameState,
        target: crate::types::effect::EffectTarget,
        step: crate::types::effect::DamageStep,
    ) {
        use crate::types::effect::EffectStep;
        let stack_id = gs.alloc_stack_id();
        let obj = StackObject {
            id: stack_id,
            payload: StackPayload::TriggeredAbility {
                source_id: ObjectId(99),
                effect: vec![EffectStep::DealDamage(step)],
                label: "test damage".into(),
            },
            controller: PlayerId(0),
            targets: vec![target],
            x_value: None,
            cast_mode: CastMode::Standard,
        };
        gs.stack.push(stack_id);
        gs.stack_objects.insert(stack_id, obj);
    }

    #[test]
    fn lifelink_damage_to_creature_gains_life_for_controller() {
        use crate::types::effect::{DamageStep, EffectTarget};
        let mut gs = make_state();
        let creature_id = make_creature_on_battlefield(&mut gs, PlayerId(1), 2, 4);
        let before_life = gs.get_player(PlayerId(0)).unwrap().life;
        push_damage_trigger(
            &mut gs,
            EffectTarget::Object { id: creature_id },
            DamageStep {
                amount: 3,
                lifelink: true,
                ..Default::default()
            },
        );
        let gs = resolve_top(gs);
        assert_eq!(gs.get_player(PlayerId(0)).unwrap().life, before_life + 3);
        assert_eq!(gs.battlefield[&creature_id].damage_marked, 3);
    }

    #[test]
    fn lifelink_damage_to_player_gains_life_for_controller() {
        use crate::types::effect::{DamageStep, EffectTarget};
        let mut gs = make_state();
        let before_controller_life = gs.get_player(PlayerId(0)).unwrap().life;
        let before_target_life = gs.get_player(PlayerId(1)).unwrap().life;
        push_damage_trigger(
            &mut gs,
            EffectTarget::Player { id: PlayerId(1) },
            DamageStep {
                amount: 3,
                lifelink: true,
                ..Default::default()
            },
        );
        let gs = resolve_top(gs);
        assert_eq!(
            gs.get_player(PlayerId(0)).unwrap().life,
            before_controller_life + 3
        );
        assert_eq!(
            gs.get_player(PlayerId(1)).unwrap().life,
            before_target_life - 3
        );
    }

    #[test]
    fn lifelink_zero_damage_gains_no_life() {
        use crate::types::effect::{DamageStep, EffectTarget};
        let mut gs = make_state();
        let before_life = gs.get_player(PlayerId(0)).unwrap().life;
        push_damage_trigger(
            &mut gs,
            EffectTarget::Player { id: PlayerId(1) },
            DamageStep {
                amount: 0,
                lifelink: true,
                ..Default::default()
            },
        );
        let gs = resolve_top(gs);
        assert_eq!(gs.get_player(PlayerId(0)).unwrap().life, before_life);
    }

    #[test]
    fn deathtouch_nonzero_sets_damaged_by_deathtouch_and_sba_destroys() {
        use crate::types::effect::{DamageStep, EffectTarget};
        let mut gs = make_state();
        // 1/1 creature: 1 deathtouch damage → SBA destroys it.
        let creature_id = make_creature_on_battlefield(&mut gs, PlayerId(1), 1, 1);
        push_damage_trigger(
            &mut gs,
            EffectTarget::Object { id: creature_id },
            DamageStep {
                amount: 1,
                deathtouch: true,
                ..Default::default()
            },
        );
        let gs = resolve_top(gs);
        // SBA ran inside resolve_top — creature removed from battlefield.
        assert!(!gs.battlefield.contains_key(&creature_id));
    }

    #[test]
    fn deathtouch_zero_damage_does_not_set_flag() {
        use crate::types::effect::{DamageStep, EffectTarget};
        let mut gs = make_state();
        let creature_id = make_creature_on_battlefield(&mut gs, PlayerId(1), 2, 2);
        push_damage_trigger(
            &mut gs,
            EffectTarget::Object { id: creature_id },
            DamageStep {
                amount: 0,
                deathtouch: true,
                ..Default::default()
            },
        );
        let gs = resolve_top(gs);
        assert!(!gs.battlefield[&creature_id].damaged_by_deathtouch);
        assert!(gs.battlefield.contains_key(&creature_id));
    }

    #[test]
    fn wither_damage_to_creature_places_minus_one_counters() {
        use crate::types::CounterKind;
        use crate::types::effect::{DamageStep, EffectTarget};
        let mut gs = make_state();
        let creature_id = make_creature_on_battlefield(&mut gs, PlayerId(1), 3, 3);
        push_damage_trigger(
            &mut gs,
            EffectTarget::Object { id: creature_id },
            DamageStep {
                amount: 2,
                wither: true,
                ..Default::default()
            },
        );
        let gs = resolve_top(gs);
        let key = CounterKind::PtModifier {
            power: -1,
            toughness: -1,
        };
        assert_eq!(gs.battlefield[&creature_id].counter_count(&key), 2);
        assert_eq!(gs.battlefield[&creature_id].damage_marked, 0);
    }

    #[test]
    fn wither_damage_to_player_is_regular_life_loss() {
        // CR 702.80a: wither only converts damage to -1/-1 counters on creatures.
        // Wither damage to a player is still regular life loss.
        use crate::types::CounterKind;
        use crate::types::effect::{DamageStep, EffectTarget};
        let mut gs = make_state();
        let before_life = gs.get_player(PlayerId(1)).unwrap().life;
        push_damage_trigger(
            &mut gs,
            EffectTarget::Player { id: PlayerId(1) },
            DamageStep {
                amount: 2,
                wither: true,
                ..Default::default()
            },
        );
        let gs = resolve_top(gs);
        assert_eq!(gs.get_player(PlayerId(1)).unwrap().life, before_life - 2);
        assert_eq!(
            gs.get_player(PlayerId(1))
                .unwrap()
                .counter_count(&CounterKind::Poison),
            0
        );
    }

    #[test]
    fn infect_damage_to_creature_places_minus_one_counters() {
        use crate::types::CounterKind;
        use crate::types::effect::{DamageStep, EffectTarget};
        let mut gs = make_state();
        // 4/4 creature: 3 infect damage → 3 × -1/-1 counters → effective toughness 1.
        // Creature survives SBAs so we can assert on its counter state.
        let creature_id = make_creature_on_battlefield(&mut gs, PlayerId(1), 4, 4);
        push_damage_trigger(
            &mut gs,
            EffectTarget::Object { id: creature_id },
            DamageStep {
                amount: 3,
                infect: true,
                ..Default::default()
            },
        );
        let gs = resolve_top(gs);
        let key = CounterKind::PtModifier {
            power: -1,
            toughness: -1,
        };
        assert_eq!(gs.battlefield[&creature_id].counter_count(&key), 3);
        assert_eq!(gs.battlefield[&creature_id].damage_marked, 0);
    }

    #[test]
    fn infect_damage_to_player_gives_poison_counters_not_life_loss() {
        use crate::types::CounterKind;
        use crate::types::effect::{DamageStep, EffectTarget};
        let mut gs = make_state();
        let before_life = gs.get_player(PlayerId(1)).unwrap().life;
        push_damage_trigger(
            &mut gs,
            EffectTarget::Player { id: PlayerId(1) },
            DamageStep {
                amount: 3,
                infect: true,
                ..Default::default()
            },
        );
        let gs = resolve_top(gs);
        assert_eq!(gs.get_player(PlayerId(1)).unwrap().life, before_life); // no life loss
        assert_eq!(
            gs.get_player(PlayerId(1))
                .unwrap()
                .counter_count(&CounterKind::Poison),
            3
        );
    }

    #[test]
    fn wither_and_deathtouch_combined_on_creature() {
        // CR 702.80a + 702.2b: wither gives -1/-1 counters; deathtouch flag still set.
        // The counter reduces toughness to 1 and deathtouch triggers SBA destruction.
        use crate::types::effect::{DamageStep, EffectTarget};
        let mut gs = make_state();
        let creature_id = make_creature_on_battlefield(&mut gs, PlayerId(1), 2, 2);
        push_damage_trigger(
            &mut gs,
            EffectTarget::Object { id: creature_id },
            DamageStep {
                amount: 1,
                wither: true,
                deathtouch: true,
                ..Default::default()
            },
        );
        let gs = resolve_top(gs);
        // SBA runs — creature toughness is now 2-1=1, damage_marked=0, but
        // damaged_by_deathtouch was set. SBA then destroys the creature.
        assert!(!gs.battlefield.contains_key(&creature_id));
    }

    #[test]
    fn vanilla_deal_damage_to_creature_unchanged() {
        // Regression: all-false flags behave exactly as before.
        use crate::types::effect::{DamageStep, EffectTarget};
        let mut gs = make_state();
        let creature_id = make_creature_on_battlefield(&mut gs, PlayerId(1), 2, 4);
        push_damage_trigger(
            &mut gs,
            EffectTarget::Object { id: creature_id },
            DamageStep {
                amount: 3,
                ..Default::default()
            },
        );
        let gs = resolve_top(gs);
        assert_eq!(gs.battlefield[&creature_id].damage_marked, 3);
    }

    #[test]
    fn vanilla_deal_damage_to_player_unchanged() {
        // Regression: all-false flags behave exactly as before.
        use crate::types::effect::{DamageStep, EffectTarget};
        let mut gs = make_state();
        let before_life = gs.get_player(PlayerId(1)).unwrap().life;
        push_damage_trigger(
            &mut gs,
            EffectTarget::Player { id: PlayerId(1) },
            DamageStep {
                amount: 3,
                ..Default::default()
            },
        );
        let gs = resolve_top(gs);
        assert_eq!(gs.get_player(PlayerId(1)).unwrap().life, before_life - 3);
    }

    #[test]
    fn deal_damage_to_protected_creature_is_prevented() {
        // Creature has protection from blue; source is blue → damage prevented (CR 702.16e).
        use crate::types::ability::{KeywordAbility, ProtectionQuality, Rule, RulesText};
        use crate::types::card::{CardDefinition, CardType, TypeLine};
        use crate::types::effect::DamageStep;
        use crate::types::mana::ManaColor;

        let mut gs = GameState::new(vec![
            Player::new(PlayerId(0), "Alice"),
            Player::new(PlayerId(1), "Bob"),
        ]);
        let creature_def = CardDefinition {
            name: "Protected".into(),
            mana_cost: None,
            type_line: TypeLine {
                supertypes: vec![],
                card_types: vec![CardType::Creature],
                subtypes: vec![],
            },
            oracle_text: String::new(),
            rules_text: vec![RulesText::Active(Rule::Static(
                KeywordAbility::ProtectionFrom(ProtectionQuality::Color(ManaColor::Blue)),
            ))],
            text_annotations: vec![],
            power: Some(2),
            toughness: Some(2),
            colors: vec![],
        };
        let target_id = gs.alloc_id();
        let obj =
            crate::types::CardObject::new(target_id, creature_def, PlayerId(1), Zone::Battlefield);
        gs.battlefield
            .insert(target_id, PermanentState::new(&obj.definition));
        gs.add_object(obj);

        let step = EffectStep::DealDamage(DamageStep {
            amount: 3,
            source_colors: vec![ManaColor::Blue], // blue source
            ..DamageStep::default()
        });
        let targets = vec![crate::types::effect::EffectTarget::Object { id: target_id }];
        let gs = execute_effect_steps(gs, PlayerId(0), &[step], &targets, None);

        // Damage was prevented — creature still at 0 damage_marked
        assert_eq!(gs.battlefield[&target_id].damage_marked, 0);
    }

    #[test]
    fn move_zone_graveyard_to_battlefield_transitions_object() {
        // MoveZone moves a card from the graveyard to the battlefield.
        use crate::types::CardObject;
        use crate::types::effect::{EffectStep, EffectTarget};
        use crate::types::zone::{Zone, ZoneOwner};

        let mut gs = make_state();

        let def = CardDefinition {
            name: "Persist Test".into(),
            mana_cost: None,
            type_line: TypeLine {
                supertypes: vec![],
                card_types: vec![CardType::Creature],
                subtypes: vec![],
            },
            oracle_text: String::new(),
            rules_text: vec![],
            text_annotations: vec![],
            power: Some(2),
            toughness: Some(2),
            colors: vec![],
        };
        let id = gs.alloc_id();
        let obj = CardObject::new(id, def, PlayerId(0), Zone::Graveyard);
        gs.graveyards.get_mut(&PlayerId(0)).unwrap().push(id);
        gs.add_object(obj);

        let targets = vec![EffectTarget::Object { id }];
        let gs = super::execute_effect_steps(
            gs,
            PlayerId(0),
            &[EffectStep::MoveZone {
                from: Zone::Graveyard,
                to: Zone::Battlefield,
                to_player: ZoneOwner::CardOwner,
            }],
            &targets,
            None,
        );

        assert!(
            gs.battlefield.contains_key(&id),
            "object should be on battlefield"
        );
        assert_eq!(gs.objects[&id].zone, Zone::Battlefield);
        assert!(
            !gs.graveyards[&PlayerId(0)].contains(&id),
            "should not be in graveyard"
        );
        assert_eq!(gs.objects[&id].controller, PlayerId(0));
    }

    #[test]
    fn move_zone_noop_when_object_not_in_from_zone() {
        // MoveZone is a no-op if the object is not in the specified `from` zone.
        use crate::types::effect::{EffectStep, EffectTarget};
        use crate::types::zone::{Zone, ZoneOwner};
        use crate::types::{CardObject, PermanentState};

        let mut gs = make_state();

        // Creature is on the battlefield, but from: Graveyard — should be a no-op.
        let def = CardDefinition {
            name: "Battlefield Creature".into(),
            mana_cost: None,
            type_line: TypeLine {
                supertypes: vec![],
                card_types: vec![CardType::Creature],
                subtypes: vec![],
            },
            oracle_text: String::new(),
            rules_text: vec![],
            text_annotations: vec![],
            power: Some(2),
            toughness: Some(2),
            colors: vec![],
        };
        let id = gs.alloc_id();
        let obj = CardObject::new(id, def, PlayerId(0), Zone::Battlefield);
        gs.battlefield
            .insert(id, PermanentState::new(&obj.definition));
        gs.add_object(obj);

        let targets = vec![EffectTarget::Object { id }];
        let gs = super::execute_effect_steps(
            gs,
            PlayerId(0),
            &[EffectStep::MoveZone {
                from: Zone::Graveyard,
                to: Zone::Battlefield,
                to_player: ZoneOwner::CardOwner,
            }],
            &targets,
            None,
        );

        assert!(
            gs.battlefield.contains_key(&id),
            "object should still be on battlefield"
        );
        assert_eq!(gs.objects[&id].zone, Zone::Battlefield);
        assert!(gs.graveyards[&PlayerId(0)].is_empty());
    }

    #[test]
    fn move_zone_to_battlefield_fires_etb_triggers() {
        // When MoveZone moves a card to the battlefield, any ETB triggers are pushed onto the stack.
        use crate::types::ability::{
            Rule, TriggerEvent, TriggerSubjectFilter, TriggerTargetMode, TriggeredAbility,
        };
        use crate::types::effect::{EffectStep, EffectTarget};
        use crate::types::zone::{Zone, ZoneOwner};
        use crate::types::{CardObject, RulesText};

        let mut gs = make_state();
        put_in_library(&mut gs, PlayerId(0));

        // A card in the graveyard with an ETB trigger.
        let def = CardDefinition {
            name: "ETB Creature".into(),
            mana_cost: None,
            type_line: TypeLine {
                supertypes: vec![],
                card_types: vec![CardType::Creature],
                subtypes: vec![],
            },
            oracle_text: "When this enters, draw a card.".into(),
            rules_text: vec![RulesText::Active(Rule::Triggered(TriggeredAbility {
                trigger: TriggerEvent::EntersTheBattlefield {
                    subject: TriggerSubjectFilter {
                        is_self: Some(true),
                        ..Default::default()
                    },
                },
                condition: None,
                target_mode: TriggerTargetMode::None,
                effect: vec![EffectStep::DrawCard(1)],
            }))],
            text_annotations: vec![],
            power: Some(1),
            toughness: Some(1),
            colors: vec![],
        };
        let id = gs.alloc_id();
        let obj = CardObject::new(id, def, PlayerId(0), Zone::Graveyard);
        gs.graveyards.get_mut(&PlayerId(0)).unwrap().push(id);
        gs.add_object(obj);

        let targets = vec![EffectTarget::Object { id }];
        let gs = super::execute_effect_steps(
            gs,
            PlayerId(0),
            &[EffectStep::MoveZone {
                from: Zone::Graveyard,
                to: Zone::Battlefield,
                to_player: ZoneOwner::CardOwner,
            }],
            &targets,
            None,
        );

        assert!(gs.battlefield.contains_key(&id));
        assert_eq!(gs.stack.len(), 1, "ETB trigger should be on the stack");
        // Card not yet drawn — trigger hasn't resolved.
        assert!(gs.hands[&PlayerId(0)].is_empty());
    }

    // ── inject_source_flags unit tests ──────────────────────────────────────────

    #[test]
    fn inject_source_flags_sets_lifelink_from_abilities() {
        use crate::engine::stack::inject_source_flags;
        use crate::types::RulesText;
        use crate::types::ability::{KeywordAbility, Rule};
        use crate::types::effect::{DamageStep, EffectStep};

        let rules_text = vec![RulesText::Active(Rule::Static(KeywordAbility::Lifelink))];
        let effect = vec![EffectStep::DealDamage(DamageStep {
            amount: 2,
            ..Default::default()
        })];
        let result = inject_source_flags(effect, &rules_text, &[], &[], &[]);
        match &result[0] {
            EffectStep::DealDamage(s) => {
                assert!(s.lifelink);
                assert!(!s.deathtouch);
                assert!(!s.wither);
                assert!(!s.infect);
                assert_eq!(s.amount, 2);
            }
            other => panic!("expected DealDamage, got {other:?}"),
        }
    }

    #[test]
    fn inject_source_flags_sets_wither_and_infect() {
        use crate::engine::stack::inject_source_flags;
        use crate::types::RulesText;
        use crate::types::ability::{KeywordAbility, Rule};
        use crate::types::effect::{DamageStep, EffectStep};

        let rules_text = vec![
            RulesText::Active(Rule::Static(KeywordAbility::Wither)),
            RulesText::Active(Rule::Static(KeywordAbility::Infect)),
        ];
        let effect = vec![EffectStep::DealDamage(DamageStep {
            amount: 1,
            ..Default::default()
        })];
        let result = inject_source_flags(effect, &rules_text, &[], &[], &[]);
        match &result[0] {
            EffectStep::DealDamage(s) => {
                assert!(s.wither);
                assert!(s.infect);
            }
            other => panic!("expected DealDamage, got {other:?}"),
        }
    }

    #[test]
    fn inject_source_flags_empty_abilities_leaves_flags_false() {
        use crate::engine::stack::inject_source_flags;
        use crate::types::effect::{DamageStep, EffectStep};

        let effect = vec![EffectStep::DealDamage(DamageStep {
            amount: 5,
            ..Default::default()
        })];
        let result = inject_source_flags(effect, &[], &[], &[], &[]);
        match &result[0] {
            EffectStep::DealDamage(s) => {
                assert!(!s.lifelink && !s.deathtouch && !s.wither && !s.infect);
                assert_eq!(s.amount, 5);
            }
            other => panic!("expected DealDamage, got {other:?}"),
        }
    }

    #[test]
    fn inject_source_flags_non_deal_damage_step_passes_through() {
        use crate::engine::stack::inject_source_flags;
        use crate::types::effect::EffectStep;

        let effect = vec![EffectStep::DrawCard(1)];
        let result = inject_source_flags(effect, &[], &[], &[], &[]);
        assert!(matches!(result[0], EffectStep::DrawCard(1)));
    }

    #[test]
    fn counter_spell_step_counters_targeted_stack_spell() {
        use crate::types::ability::{SpellAbility, SpellFilter, TargetFilter};
        use crate::types::effect::EffectTarget;
        use crate::types::mana::ManaColor;

        let mut gs = make_state();

        // Put a target creature spell on the stack (player 1's Bears).
        let target_def = CardDefinition {
            name: "Bears".into(),
            mana_cost: None,
            type_line: TypeLine {
                supertypes: vec![],
                card_types: vec![CardType::Creature],
                subtypes: vec![],
            },
            oracle_text: String::new(),
            rules_text: vec![],
            text_annotations: vec![],
            power: Some(2),
            toughness: Some(2),
            colors: vec![],
        };
        let bears_card_id = gs.alloc_id();
        let bears_obj = CardObject::new(bears_card_id, target_def, PlayerId(1), Zone::Stack);
        gs.add_object(bears_obj);
        let bears_sid = gs.alloc_stack_id();
        gs.stack.push(bears_sid);
        gs.stack_objects.insert(
            bears_sid,
            StackObject {
                id: bears_sid,
                payload: StackPayload::Spell {
                    card_id: bears_card_id,
                },
                controller: PlayerId(1),
                targets: vec![],
                x_value: None,
                cast_mode: CastMode::Standard,
            },
        );

        // Put a Counterspell on the stack above Bears, targeting Bears.
        let counter_def = CardDefinition {
            name: "Counterspell".into(),
            mana_cost: Some(ManaCost {
                pips: vec![ManaPip::Blue, ManaPip::Blue],
            }),
            type_line: TypeLine {
                supertypes: vec![],
                card_types: vec![CardType::Instant],
                subtypes: vec![],
            },
            oracle_text: "Counter target spell.".into(),
            rules_text: vec![RulesText::Active(Rule::SpellAbility(SpellAbility {
                target_requirements: vec![TargetFilter::Spell(SpellFilter::any())],
                steps: vec![EffectStep::CounterSpell],
            }))],
            text_annotations: vec![],
            power: None,
            toughness: None,
            colors: vec![ManaColor::Blue],
        };
        let counter_card_id = gs.alloc_id();
        let counter_obj = CardObject::new(counter_card_id, counter_def, PlayerId(0), Zone::Stack);
        gs.add_object(counter_obj);
        let counter_sid = gs.alloc_stack_id();
        gs.stack.push(counter_sid);
        gs.stack_objects.insert(
            counter_sid,
            StackObject {
                id: counter_sid,
                payload: StackPayload::Spell {
                    card_id: counter_card_id,
                },
                controller: PlayerId(0),
                targets: vec![EffectTarget::StackObject { id: bears_sid }],
                x_value: None,
                cast_mode: CastMode::Standard,
            },
        );

        // Resolve Counterspell (top of stack).
        let gs = resolve_top(gs);

        // Bears countered: removed from stack, card in player 1's graveyard.
        assert!(!gs.stack.contains(&bears_sid));
        assert!(!gs.stack_objects.contains_key(&bears_sid));
        assert_eq!(gs.objects[&bears_card_id].zone, Zone::Graveyard);
        assert!(gs.graveyards[&PlayerId(1)].contains(&bears_card_id));
        // Counterspell itself resolved to player 0's graveyard.
        assert_eq!(gs.objects[&counter_card_id].zone, Zone::Graveyard);
        assert!(gs.graveyards[&PlayerId(0)].contains(&counter_card_id));
        assert!(gs.stack.is_empty());
    }
}
