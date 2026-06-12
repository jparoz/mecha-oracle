use super::{
    EngineError,
    state_based_actions::check_and_apply_sbas,
    triggered::collect_etb_triggers,
    turn::{advance_step, draw_card},
};
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

// CR 608.2b: execute each effect step for the given controller.
// Shared by instant/sorcery spell resolution and triggered/activated ability resolution.
fn execute_effect_steps(
    mut state: GameState,
    controller: PlayerId,
    steps: &[EffectStep],
    targets: &[crate::types::effect::EffectTarget],
) -> GameState {
    use crate::types::effect::EffectTarget;
    for step in steps {
        match step {
            EffectStep::DrawCard(n) => {
                for _ in 0..*n {
                    state = draw_card(state, controller);
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
                if let Some(EffectTarget::Object { id }) = targets.first()
                    && let Some(perm) = state.battlefield.get_mut(id)
                {
                    perm.pt_boost_until_eot.power += delta.power;
                    perm.pt_boost_until_eot.toughness += delta.toughness;
                }
            }
            // TODO CR 702.2c/702.15a: deathtouch and lifelink propagation not yet
            // implemented; DealDamage carries no source-keyword context.
            EffectStep::DealDamage(n) => match targets.first() {
                Some(EffectTarget::Object { id }) => {
                    if let Some(perm) = state.battlefield.get_mut(id) {
                        perm.damage_marked += n;
                    }
                }
                Some(EffectTarget::Player { id }) => {
                    if let Some(player) = state.get_player_mut(*id) {
                        player.life -= *n as i32;
                    }
                }
                None => {}
            },
            EffectStep::Unimplemented(_) => {}
        }
    }
    state
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

    match stack_obj.payload {
        StackPayload::Spell { card_id } => {
            let controller = stack_obj.controller;
            let is_permanent = state
                .objects
                .get(&card_id)
                .map(|o| o.definition.type_line.is_permanent())
                .unwrap_or(false);

            if is_permanent {
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

                // CR 603.3: collect ETB triggers and push onto stack (CR 405.3 APNAP order —
                // for a single entering permanent, all triggers share the same controller
                // so order is trivial; multi-permanent APNAP ordering is a future concern).
                let triggers = collect_etb_triggers(&mut state, card_id);
                for trigger in triggers {
                    let id = trigger.id;
                    state.stack.push(id);
                    state.stack_objects.insert(id, trigger);
                }
            } else {
                // CR 608.2b: instant/sorcery — execute effects, then move to graveyard.
                let steps: Vec<EffectStep> = state
                    .objects
                    .get(&card_id)
                    .map(|obj| {
                        obj.definition
                            .abilities
                            .iter()
                            .filter_map(|span| match span {
                                crate::types::OracleSpan::Parsed(
                                    crate::types::Ability::SpellEffect(spell_ability),
                                ) => Some(spell_ability.steps.clone()),
                                _ => None,
                            })
                            .flatten()
                            .collect()
                    })
                    .unwrap_or_default();

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
                    return check_and_apply_sbas(state);
                }

                state = execute_effect_steps(state, controller, &steps, &targets);

                if let Some(obj) = state.objects.get_mut(&card_id) {
                    obj.zone = Zone::Graveyard;
                }
                if let Some(gy) = state.graveyards.get_mut(&controller) {
                    gy.push(card_id);
                }
            }

            // CR 117.3b: after triggered abilities are put on the stack, active player
            // receives priority (distinct from CR 117.3c where the caster retains priority
            // after casting a spell or activating an ability).
            state.consecutive_passes = 0;
            state.priority_player = state.active_player;
            check_and_apply_sbas(state)
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
                return check_and_apply_sbas(state);
            }
            state = execute_effect_steps(state, controller, &effect, &targets);
            state.consecutive_passes = 0;
            state.priority_player = state.active_player;
            check_and_apply_sbas(state)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cards::test_helpers::test_db;
    use crate::types::card::{CardDefinition, CardType, TypeLine};
    use crate::types::effect::EffectStep;
    use crate::types::mana::{ManaCost, ManaPip};
    use crate::types::stack::{StackObject, StackPayload};
    use crate::types::{Ability, CardObject, ObjectId, OracleSpan, Player, Step};

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
            abilities: vec![OracleSpan::Parsed(Ability::SpellEffect(SpellAbility {
                target_requirements: vec![],
                steps,
            }))],
            power: None,
            toughness: None,
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
            abilities: vec![OracleSpan::Parsed(Ability::SpellEffect(SpellAbility {
                target_requirements: vec![],
                steps,
            }))],
            power: None,
            toughness: None,
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
        use crate::types::ability::{TriggerEvent, TriggeredAbility};

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
            abilities: vec![OracleSpan::Parsed(Ability::Triggered(TriggeredAbility {
                trigger: TriggerEvent::EntersTheBattlefield {
                    subject_is_self: true,
                },
                effect: vec![EffectStep::DrawCard(1)],
            }))],
            power: Some(1),
            toughness: Some(1),
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
            abilities: vec![],
            power: Some(2),
            toughness: Some(2),
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
        };
        gs.stack.push(stack_id);
        gs.stack_objects.insert(stack_id, stack_obj);

        let gs = resolve_top(gs);

        assert_eq!(gs.battlefield[&id].effective_power(), Some(3));
        assert_eq!(gs.battlefield[&id].effective_toughness(), Some(3));
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
            abilities: vec![],
            power: Some(2),
            toughness: Some(4),
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
                effect: vec![EffectStep::DealDamage(3)],
                label: "test damage".into(),
            },
            controller: PlayerId(0),
            targets: vec![EffectTarget::Object { id }],
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
                effect: vec![EffectStep::DealDamage(3)],
                label: "test damage".into(),
            },
            controller: PlayerId(0),
            targets: vec![EffectTarget::Player { id: PlayerId(1) }],
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
            abilities: vec![],
            power: Some(1),
            toughness: Some(1),
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
            abilities: vec![OracleSpan::Parsed(Ability::SpellEffect(SpellAbility {
                target_requirements: vec![TargetFilter::Creature],
                steps: vec![EffectStep::BoostPermanentPT(PTDelta {
                    power: 3,
                    toughness: 3,
                })],
            }))],
            power: None,
            toughness: None,
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
                effect: vec![EffectStep::DealDamage(3)],
                label: "test damage".into(),
            },
            controller: PlayerId(0),
            targets: vec![EffectTarget::Player { id: PlayerId(1) }],
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
}
