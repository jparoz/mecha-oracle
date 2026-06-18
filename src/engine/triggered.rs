use crate::types::ability::{Ability, CastFilter, StaticAbility, TriggerEvent};
use crate::types::effect::EffectStep;
use crate::types::stack::{StackObject, StackPayload};
use crate::types::{GameState, ObjectId, OracleSpan, PTDelta, PlayerId};

// CR 603.2: collect ETB triggers from `entering_id` into stack objects.
// Returns Vec<StackObject> to be pushed onto the stack by the caller.
pub fn collect_etb_triggers(state: &mut GameState, entering_id: ObjectId) -> Vec<StackObject> {
    let entries: Vec<(PlayerId, Vec<crate::types::EffectStep>, String)> = {
        let obj = match state.objects.get(&entering_id) {
            Some(o) => o,
            None => return vec![],
        };
        obj.definition
            .abilities
            .iter()
            .filter_map(|span| match span {
                OracleSpan::Parsed(Ability::Triggered(t))
                    if matches!(
                        t.trigger,
                        TriggerEvent::EntersTheBattlefield {
                            subject_is_self: true
                        }
                    ) =>
                {
                    let label = format!("{}: ETB trigger", obj.definition.name);
                    Some((obj.controller, t.effect.clone(), label))
                }
                _ => None,
            })
            .collect()
    };

    let source_abilities: Vec<crate::types::OracleSpan> = state
        .objects
        .get(&entering_id)
        .map(|o| o.definition.abilities.clone())
        .unwrap_or_default();
    entries
        .into_iter()
        .map(|(controller, effect, label)| {
            let id = state.alloc_stack_id();
            StackObject {
                id,
                payload: StackPayload::TriggeredAbility {
                    source_id: entering_id,
                    effect: crate::engine::stack::inject_source_flags(effect, &source_abilities),
                    label,
                },
                controller,
                targets: vec![], // ETB effects use DrawCard/GainLife; never targeted
                x_value: None,
            }
        })
        .collect()
}

/// CR 702.108b: collect triggered abilities that fire when a spell is cast.
/// Currently handles: Prowess (noncreature filter → +1/+1 until EOT on each Prowess creature).
/// Add additional StaticAbility branches here as new cast-triggered keywords are implemented.
pub fn collect_cast_triggers(
    state: &mut GameState,
    caster: PlayerId,
    spell_id: ObjectId,
    filter: &CastFilter,
) -> Vec<StackObject> {
    // Check whether the cast spell satisfies the filter.
    let spell_types: Vec<crate::types::card::CardType> = state
        .objects
        .get(&spell_id)
        .map(|o| o.definition.type_line.card_types.clone())
        .unwrap_or_default();
    if !filter.matches(&spell_types) {
        return vec![];
    }

    // Collect permanents that have cast-triggered abilities.
    let prowess_creature_ids: Vec<(ObjectId, PlayerId)> = state
        .battlefield
        .keys()
        .filter_map(|&id| {
            let obj = state.objects.get(&id)?;
            if obj.controller != caster {
                return None;
            }
            let perm = state.battlefield.get(&id)?;
            if perm.is_creature() && perm.has_keyword(StaticAbility::Prowess) {
                Some((id, obj.controller))
            } else {
                None
            }
        })
        .collect();

    prowess_creature_ids
        .into_iter()
        .map(|(creature_id, controller)| {
            let sid = state.alloc_stack_id();
            use crate::types::effect::EffectTarget;
            StackObject {
                id: sid,
                payload: StackPayload::TriggeredAbility {
                    source_id: creature_id,
                    effect: vec![EffectStep::BoostPermanentPT(PTDelta {
                        power: 1,
                        toughness: 1,
                    })],
                    label: "Prowess".into(),
                },
                controller,
                targets: vec![EffectTarget::Object { id: creature_id }],
                x_value: None,
            }
        })
        .collect()
}

/// Collect triggered abilities that fire when blockers are declared.
/// Handles: Flanking (CR 702.25b), Bushido N (CR 702.45b).
pub fn collect_block_triggers(state: &mut GameState) -> Vec<StackObject> {
    let attacking_player = state.active_player;
    let defending_player = state.opponent_of(attacking_player);
    let blocking_map: Vec<(ObjectId, Vec<ObjectId>)> = state
        .combat
        .blocking_map
        .iter()
        .map(|(&a, bs)| (a, bs.clone()))
        .collect();
    let mut result = Vec::new();

    for (attacker_id, blockers) in &blocking_map {
        // Flanking (CR 702.25b): each non-Flanking blocker gets -1/-1.
        if state
            .battlefield
            .get(attacker_id)
            .map(|p| p.has_keyword(StaticAbility::Flanking))
            .unwrap_or(false)
        {
            for &blocker_id in blockers {
                let blocker_has_flanking = state
                    .battlefield
                    .get(&blocker_id)
                    .map(|p| p.has_keyword(StaticAbility::Flanking))
                    .unwrap_or(false);
                if !blocker_has_flanking {
                    let sid = state.alloc_stack_id();
                    use crate::types::effect::EffectTarget;
                    result.push(StackObject {
                        id: sid,
                        payload: StackPayload::TriggeredAbility {
                            source_id: *attacker_id,
                            effect: vec![EffectStep::BoostPermanentPT(PTDelta {
                                power: -1,
                                toughness: -1,
                            })],
                            label: "Flanking".into(),
                        },
                        controller: attacking_player,
                        targets: vec![EffectTarget::Object { id: blocker_id }],
                        x_value: None,
                    });
                }
            }
        }

        // Bushido N on attacker: fires if attacker has at least one blocker.
        if let Some(n) = state
            .battlefield
            .get(attacker_id)
            .and_then(|p| p.bushido_n())
            && !blockers.is_empty()
        {
            let sid = state.alloc_stack_id();
            use crate::types::effect::EffectTarget;
            result.push(StackObject {
                id: sid,
                payload: StackPayload::TriggeredAbility {
                    source_id: *attacker_id,
                    effect: vec![EffectStep::BoostPermanentPT(PTDelta {
                        power: n as i32,
                        toughness: n as i32,
                    })],
                    label: format!("Bushido {n}"),
                },
                controller: attacking_player,
                targets: vec![EffectTarget::Object { id: *attacker_id }],
                x_value: None,
            });
        }

        // Bushido N on each blocker: fires for every blocker with Bushido.
        for &blocker_id in blockers {
            if let Some(n) = state
                .battlefield
                .get(&blocker_id)
                .and_then(|p| p.bushido_n())
            {
                let sid = state.alloc_stack_id();
                use crate::types::effect::EffectTarget;
                result.push(StackObject {
                    id: sid,
                    payload: StackPayload::TriggeredAbility {
                        source_id: blocker_id,
                        effect: vec![EffectStep::BoostPermanentPT(PTDelta {
                            power: n as i32,
                            toughness: n as i32,
                        })],
                        label: format!("Bushido {n}"),
                    },
                    controller: defending_player,
                    targets: vec![EffectTarget::Object { id: blocker_id }],
                    x_value: None,
                });
            }
        }
    }

    result
}

/// Collect triggered abilities that fire when creatures are declared as attackers.
/// Handles: Exalted (CR 702.83b), Melee (CR 702.121b), Battle Cry (CR 702.91b).
pub fn collect_attack_triggers(state: &mut GameState) -> Vec<StackObject> {
    let attackers = state.combat.attackers.clone();
    let attacking_player = state.active_player;
    let mut result = Vec::new();

    // Exalted (CR 702.83b): fires once per Exalted permanent when exactly one creature attacks.
    if attackers.len() == 1 {
        let attacker_id = attackers[0];
        let exalted_sources: Vec<ObjectId> = state
            .battlefield
            .keys()
            .filter(|&&id| {
                state
                    .objects
                    .get(&id)
                    .map(|o| o.controller == attacking_player)
                    .unwrap_or(false)
                    && state
                        .battlefield
                        .get(&id)
                        .map(|p| p.has_keyword(StaticAbility::Exalted))
                        .unwrap_or(false)
            })
            .copied()
            .collect();
        for source_id in exalted_sources {
            let sid = state.alloc_stack_id();
            use crate::types::effect::EffectTarget;
            result.push(StackObject {
                id: sid,
                payload: StackPayload::TriggeredAbility {
                    source_id,
                    effect: vec![EffectStep::BoostPermanentPT(PTDelta {
                        power: 1,
                        toughness: 1,
                    })],
                    label: "Exalted".into(),
                },
                controller: attacking_player,
                targets: vec![EffectTarget::Object { id: attacker_id }],
                x_value: None,
            });
        }
    }

    // Melee (CR 702.121b): +1/+1 per opponent attacked; 2-player = always 1 opponent.
    let melee_attackers: Vec<ObjectId> = attackers
        .iter()
        .filter(|&&id| {
            state
                .battlefield
                .get(&id)
                .map(|p| p.has_keyword(StaticAbility::Melee))
                .unwrap_or(false)
        })
        .copied()
        .collect();
    for attacker_id in melee_attackers {
        let sid = state.alloc_stack_id();
        use crate::types::effect::EffectTarget;
        result.push(StackObject {
            id: sid,
            payload: StackPayload::TriggeredAbility {
                source_id: attacker_id,
                effect: vec![EffectStep::BoostPermanentPT(PTDelta {
                    power: 1,
                    toughness: 1,
                })],
                label: "Melee".into(),
            },
            controller: attacking_player,
            targets: vec![EffectTarget::Object { id: attacker_id }],
            x_value: None,
        });
    }

    // Battle Cry (CR 702.91b): each other attacking creature gets +1/+0 until end of turn.
    let battle_cry_attackers: Vec<ObjectId> = attackers
        .iter()
        .filter(|&&id| {
            state
                .battlefield
                .get(&id)
                .map(|p| p.has_keyword(StaticAbility::BattleCry))
                .unwrap_or(false)
        })
        .copied()
        .collect();
    for source_id in battle_cry_attackers {
        for &other_id in attackers.iter().filter(|&&id| id != source_id) {
            let sid = state.alloc_stack_id();
            use crate::types::effect::EffectTarget;
            result.push(StackObject {
                id: sid,
                payload: StackPayload::TriggeredAbility {
                    source_id,
                    effect: vec![EffectStep::BoostPermanentPT(PTDelta {
                        power: 1,
                        toughness: 0,
                    })],
                    label: "Battle Cry".into(),
                },
                controller: attacking_player,
                targets: vec![EffectTarget::Object { id: other_id }],
                x_value: None,
            });
        }
    }

    // Training (CR 702.149a): +1/+1 counter when attacking alongside a creature with greater power.
    for &attacker_id in &attackers {
        let has_training = state
            .battlefield
            .get(&attacker_id)
            .map(|p| p.has_keyword(StaticAbility::Training))
            .unwrap_or(false);
        if !has_training {
            continue;
        }
        let my_power = state
            .battlefield
            .get(&attacker_id)
            .and_then(|p| p.effective_power())
            .unwrap_or(0);
        let has_greater_power_ally = attackers
            .iter()
            .filter(|&&id| id != attacker_id)
            .any(|&id| {
                state
                    .battlefield
                    .get(&id)
                    .and_then(|p| p.effective_power())
                    .map(|p| p > my_power)
                    .unwrap_or(false)
            });
        if !has_greater_power_ally {
            continue;
        }
        let sid = state.alloc_stack_id();
        use crate::types::CounterKind;
        use crate::types::effect::EffectTarget;
        result.push(StackObject {
            id: sid,
            payload: StackPayload::TriggeredAbility {
                source_id: attacker_id,
                effect: vec![EffectStep::AddCounter {
                    kind: CounterKind::PtModifier {
                        power: 1,
                        toughness: 1,
                    },
                    count: 1,
                }],
                label: "Training".into(),
            },
            controller: attacking_player,
            targets: vec![EffectTarget::Object { id: attacker_id }],
            x_value: None,
        });
    }

    result
}

/// CR 702.100b: Collect Evolve triggers for battlefield permanents when `entering_id` ETBs.
pub fn collect_evolve_triggers(state: &mut GameState, entering_id: ObjectId) -> Vec<StackObject> {
    use crate::types::CounterKind;
    use crate::types::effect::EffectTarget;

    let entering_power = state
        .battlefield
        .get(&entering_id)
        .and_then(|p| p.effective_power());
    let entering_toughness = state
        .battlefield
        .get(&entering_id)
        .and_then(|p| p.effective_toughness());

    let Some(controller) = state.objects.get(&entering_id).map(|o| o.controller) else {
        return vec![];
    };

    let evolve_ids: Vec<ObjectId> = state
        .battlefield
        .keys()
        .filter(|&&id| {
            id != entering_id
                && state
                    .objects
                    .get(&id)
                    .map(|o| o.controller == controller)
                    .unwrap_or(false)
                && state
                    .battlefield
                    .get(&id)
                    .map(|p| p.has_keyword(StaticAbility::Evolve))
                    .unwrap_or(false)
        })
        .copied()
        .collect();

    evolve_ids
        .into_iter()
        .filter_map(|evolve_id| {
            let perm = state.battlefield.get(&evolve_id)?;
            let my_power = perm.effective_power().unwrap_or(0);
            let my_toughness = perm.effective_toughness().unwrap_or(0);
            let qualifies = entering_power.map(|ep| ep > my_power).unwrap_or(false)
                || entering_toughness
                    .map(|et| et > my_toughness)
                    .unwrap_or(false);
            if !qualifies {
                return None;
            }
            let sid = state.alloc_stack_id();
            Some(StackObject {
                id: sid,
                payload: StackPayload::TriggeredAbility {
                    source_id: evolve_id,
                    effect: vec![EffectStep::AddCounter {
                        kind: CounterKind::PtModifier {
                            power: 1,
                            toughness: 1,
                        },
                        count: 1,
                    }],
                    label: "Evolve".into(),
                },
                controller,
                targets: vec![EffectTarget::Object { id: evolve_id }],
                x_value: None,
            })
        })
        .collect()
}

/// CR 702.21a: Collect ward triggered abilities for any declared targets that are
/// opponent-controlled permanents with Ward. Each Ward ability on such a target generates
/// one TriggeredAbility with a Payment effect pushed above the triggering spell/ability on the stack.
/// The trigger is controlled by the Ward permanent's controller (CR 603.3a).
pub fn collect_ward_triggers(
    state: &mut GameState,
    triggering_stack_id: crate::types::stack::StackId,
    acting_player: PlayerId,
    targets: &[crate::types::effect::EffectTarget],
) -> Vec<crate::types::stack::StackObject> {
    use crate::types::ability::{Ability, CostComponent, OracleSpan, StaticAbility};
    use crate::types::effect::EffectTarget;
    use crate::types::stack::{StackObject, StackPayload};

    let mut triggers = Vec::new();
    for target in targets {
        let target_obj_id = match target {
            EffectTarget::Object { id } => *id,
            EffectTarget::Player { .. } | EffectTarget::StackObject { .. } => continue,
        };
        if !state.battlefield.contains_key(&target_obj_id) {
            continue;
        }
        let target_obj = match state.objects.get(&target_obj_id) {
            Some(o) => o,
            None => continue,
        };
        if target_obj.controller == acting_player {
            continue;
        }
        let ward_permanent_controller = target_obj.controller;
        let ward_cost_sets: Vec<Vec<CostComponent>> = target_obj
            .definition
            .abilities
            .iter()
            .filter_map(|span| match span {
                OracleSpan::Parsed(Ability::Static(StaticAbility::Ward(components))) => {
                    Some(components.clone())
                }
                _ => None,
            })
            .collect();
        for cost in ward_cost_sets {
            let sid = state.alloc_stack_id();
            let label = if cost.len() == 1 {
                match &cost[0] {
                    CostComponent::Mana(m) => format!("Ward \u{2014} {m}"),
                    CostComponent::PayLife(n) => format!("Ward \u{2014} Pay {n} life"),
                    _ => "Ward".to_string(),
                }
            } else {
                "Ward".to_string()
            };
            triggers.push(StackObject {
                id: sid,
                payload: StackPayload::TriggeredAbility {
                    source_id: target_obj_id,
                    effect: vec![crate::types::effect::EffectStep::Payment {
                        cost,
                        on_paid: vec![],
                        on_declined: vec![crate::types::effect::EffectStep::CounterSpell],
                    }],
                    label,
                },
                controller: ward_permanent_controller,
                targets: vec![crate::types::effect::EffectTarget::StackObject {
                    id: triggering_stack_id,
                }],
                x_value: None,
            });
        }
    }
    triggers
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::ability::{Ability, TriggerEvent, TriggeredAbility};
    use crate::types::card::{CardDefinition, CardType, TypeLine};
    use crate::types::effect::EffectStep;
    use crate::types::mana::ManaCost;
    use crate::types::{
        CardObject, GameState, ObjectId, OracleSpan, PermanentState, Player, PlayerId, Zone,
    };

    fn two_player_state() -> GameState {
        GameState::new(vec![
            Player::new(PlayerId(0), "Alice"),
            Player::new(PlayerId(1), "Bob"),
        ])
    }

    fn place_on_battlefield(
        state: &mut GameState,
        def: CardDefinition,
        owner: PlayerId,
    ) -> ObjectId {
        let id = state.alloc_id();
        let obj = CardObject::new(id, def, owner, Zone::Battlefield);
        state
            .battlefield
            .insert(id, PermanentState::new(&obj.definition));
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

    fn etb_draw_def() -> CardDefinition {
        CardDefinition {
            name: "Elvish Visionary".into(),
            mana_cost: Some(ManaCost { pips: vec![] }),
            type_line: TypeLine {
                supertypes: vec![],
                card_types: vec![CardType::Creature],
                subtypes: vec!["Elf".into(), "Scout".into()],
            },
            oracle_text: "When this enters, draw a card.".into(),
            abilities: vec![OracleSpan::Parsed(Ability::Triggered(TriggeredAbility {
                trigger: TriggerEvent::EntersTheBattlefield {
                    subject_is_self: true,
                },
                effect: vec![EffectStep::DrawCard(1)],
            }))],
            text_annotations: vec![],
            power: Some(1),
            toughness: Some(1),
            colors: vec![],
        }
    }

    fn etb_gain_life_def() -> CardDefinition {
        CardDefinition {
            name: "Pelakka Wurm".into(),
            mana_cost: Some(ManaCost { pips: vec![] }),
            type_line: TypeLine {
                supertypes: vec![],
                card_types: vec![CardType::Creature],
                subtypes: vec!["Wurm".into()],
            },
            oracle_text: "When this enters, you gain 7 life.".into(),
            abilities: vec![OracleSpan::Parsed(Ability::Triggered(TriggeredAbility {
                trigger: TriggerEvent::EntersTheBattlefield {
                    subject_is_self: true,
                },
                effect: vec![EffectStep::GainLife(7)],
            }))],
            text_annotations: vec![],
            power: Some(7),
            toughness: Some(7),
            colors: vec![],
        }
    }

    fn keyword_attacker(
        state: &mut GameState,
        owner: PlayerId,
        power: i32,
        toughness: i32,
        keywords: Vec<StaticAbility>,
    ) -> ObjectId {
        use crate::types::OracleSpan;
        use crate::types::ability::Ability;
        use crate::types::card::{CardType, TypeLine};
        let def = CardDefinition {
            name: "Test Attacker".into(),
            mana_cost: None,
            type_line: TypeLine {
                supertypes: vec![],
                card_types: vec![CardType::Creature],
                subtypes: vec![],
            },
            oracle_text: String::new(),
            abilities: keywords
                .into_iter()
                .map(|k| OracleSpan::Parsed(Ability::Static(k)))
                .collect(),
            text_annotations: vec![],
            power: Some(power),
            toughness: Some(toughness),
            colors: vec![],
        };
        place_on_battlefield(state, def, owner)
    }

    fn enter_creature_on_battlefield(
        state: &mut GameState,
        owner: PlayerId,
        power: i32,
        toughness: i32,
        keywords: Vec<StaticAbility>,
    ) -> ObjectId {
        use crate::types::OracleSpan;
        use crate::types::ability::Ability;
        use crate::types::card::{CardType, TypeLine};
        let def = CardDefinition {
            name: "Test".into(),
            mana_cost: None,
            type_line: TypeLine {
                supertypes: vec![],
                card_types: vec![CardType::Creature],
                subtypes: vec![],
            },
            oracle_text: String::new(),
            abilities: keywords
                .into_iter()
                .map(|k| OracleSpan::Parsed(Ability::Static(k)))
                .collect(),
            text_annotations: vec![],
            power: Some(power),
            toughness: Some(toughness),
            colors: vec![],
        };
        let id = state.alloc_id();
        let obj = CardObject::new(id, def, owner, Zone::Battlefield);
        let mut perm = PermanentState::new(&obj.definition);
        perm.controller_since_turn = 0;
        state.battlefield.insert(id, perm);
        state.add_object(obj);
        id
    }

    #[test]
    fn training_triggers_when_attacking_with_higher_power_ally() {
        // CR 702.149a: Training fires when attacking alongside a creature with greater power.
        use crate::types::ability::StaticAbility;
        use crate::types::stack::StackPayload;
        let mut gs = two_player_state();
        let training_id =
            keyword_attacker(&mut gs, PlayerId(0), 2, 2, vec![StaticAbility::Training]);
        let ally_id = keyword_attacker(&mut gs, PlayerId(0), 3, 3, vec![]);
        gs.combat.attackers = vec![training_id, ally_id];

        let triggers = collect_attack_triggers(&mut gs);

        assert_eq!(
            triggers.iter().filter(|t| {
                matches!(&t.payload, StackPayload::TriggeredAbility { label, .. } if label == "Training")
            }).count(),
            1,
            "Should have exactly one Training trigger"
        );
    }

    #[test]
    fn training_does_not_trigger_when_no_ally_with_greater_power() {
        // CR 702.149a: Training requires the ally to have GREATER power; equal doesn't count.
        use crate::types::ability::StaticAbility;
        use crate::types::stack::StackPayload;
        let mut gs = two_player_state();
        let training_id =
            keyword_attacker(&mut gs, PlayerId(0), 2, 2, vec![StaticAbility::Training]);
        let ally_id = keyword_attacker(&mut gs, PlayerId(0), 2, 2, vec![]);
        gs.combat.attackers = vec![training_id, ally_id];

        let triggers = collect_attack_triggers(&mut gs);

        assert_eq!(
            triggers.iter().filter(|t| {
                matches!(&t.payload, StackPayload::TriggeredAbility { label, .. } if label == "Training")
            }).count(),
            0,
            "Training should not trigger when ally power equals training creature's power"
        );
    }

    #[test]
    fn training_does_not_trigger_when_attacking_alone() {
        // CR 702.149a: No trigger if attacking alone (no other creatures attacking).
        use crate::types::ability::StaticAbility;
        use crate::types::stack::StackPayload;
        let mut gs = two_player_state();
        let training_id =
            keyword_attacker(&mut gs, PlayerId(0), 2, 2, vec![StaticAbility::Training]);
        gs.combat.attackers = vec![training_id];

        let triggers = collect_attack_triggers(&mut gs);

        let training_count = triggers.iter().filter(|t| {
            matches!(&t.payload, StackPayload::TriggeredAbility { label, .. } if label == "Training")
        }).count();
        assert_eq!(
            training_count, 0,
            "Training should not trigger when attacking alone"
        );
    }

    #[test]
    fn training_trigger_targets_training_creature_itself() {
        // The +1/+1 counter should go on the Training creature, not the ally.
        use crate::types::CounterKind;
        use crate::types::ability::StaticAbility;
        use crate::types::effect::EffectTarget;
        use crate::types::stack::StackPayload;
        let mut gs = two_player_state();
        let training_id =
            keyword_attacker(&mut gs, PlayerId(0), 2, 2, vec![StaticAbility::Training]);
        let ally_id = keyword_attacker(&mut gs, PlayerId(0), 3, 3, vec![]);
        gs.combat.attackers = vec![training_id, ally_id];

        let triggers = collect_attack_triggers(&mut gs);

        let training_trigger = triggers.iter().find(|t| {
            matches!(&t.payload, StackPayload::TriggeredAbility { label, .. } if label == "Training")
        }).expect("should have a Training trigger");

        // Check the trigger targets the Training creature itself.
        assert!(
            training_trigger
                .targets
                .iter()
                .any(|t| matches!(t, EffectTarget::Object { id } if *id == training_id)),
            "Training trigger should target the Training creature"
        );

        // Check the effect is AddCounter +1/+1 count 1.
        if let StackPayload::TriggeredAbility { effect, .. } = &training_trigger.payload {
            assert!(effect.iter().any(|step| matches!(
                step,
                EffectStep::AddCounter {
                    kind: CounterKind::PtModifier {
                        power: 1,
                        toughness: 1
                    },
                    count: 1
                }
            )));
        }
    }

    #[test]
    fn evolve_triggers_when_creature_with_greater_power_enters() {
        // CR 702.100b: Evolve fires if entering creature has greater power.
        use crate::types::CounterKind;
        use crate::types::ability::StaticAbility;
        use crate::types::effect::EffectTarget;
        use crate::types::stack::StackPayload;
        let mut gs = two_player_state();
        let evolve_id =
            enter_creature_on_battlefield(&mut gs, PlayerId(0), 2, 2, vec![StaticAbility::Evolve]);
        let entering_id = enter_creature_on_battlefield(&mut gs, PlayerId(0), 3, 2, vec![]);

        let triggers = collect_evolve_triggers(&mut gs, entering_id);

        assert_eq!(triggers.len(), 1);
        let t = &triggers[0];
        assert!(
            t.targets
                .iter()
                .any(|tgt| matches!(tgt, EffectTarget::Object { id } if *id == evolve_id))
        );
        if let StackPayload::TriggeredAbility { effect, .. } = &t.payload {
            assert!(effect.iter().any(|step| matches!(
                step,
                EffectStep::AddCounter {
                    kind: CounterKind::PtModifier {
                        power: 1,
                        toughness: 1
                    },
                    count: 1
                }
            )));
        }
    }

    #[test]
    fn evolve_triggers_when_creature_with_greater_toughness_enters() {
        // CR 702.100b: Also triggers on greater toughness.
        use crate::types::ability::StaticAbility;
        use crate::types::effect::EffectTarget;
        let mut gs = two_player_state();
        let evolve_id =
            enter_creature_on_battlefield(&mut gs, PlayerId(0), 2, 2, vec![StaticAbility::Evolve]);
        let entering_id = enter_creature_on_battlefield(&mut gs, PlayerId(0), 2, 3, vec![]);

        let triggers = collect_evolve_triggers(&mut gs, entering_id);

        assert_eq!(triggers.len(), 1);
        assert!(
            triggers[0]
                .targets
                .iter()
                .any(|tgt| matches!(tgt, EffectTarget::Object { id } if *id == evolve_id))
        );
    }

    #[test]
    fn evolve_does_not_trigger_when_equal_power_and_toughness_enters() {
        // CR 702.100b: "greater power or greater toughness" — equal doesn't qualify.
        use crate::types::ability::StaticAbility;
        let mut gs = two_player_state();
        let _evolve_id =
            enter_creature_on_battlefield(&mut gs, PlayerId(0), 2, 2, vec![StaticAbility::Evolve]);
        let entering_id = enter_creature_on_battlefield(&mut gs, PlayerId(0), 2, 2, vec![]);

        let triggers = collect_evolve_triggers(&mut gs, entering_id);

        assert_eq!(triggers.len(), 0);
    }

    #[test]
    fn evolve_does_not_trigger_for_opponent_creature_etb() {
        // CR 702.100b: Only triggers on creatures entering under YOUR control.
        use crate::types::ability::StaticAbility;
        let mut gs = two_player_state();
        let _evolve_id =
            enter_creature_on_battlefield(&mut gs, PlayerId(0), 2, 2, vec![StaticAbility::Evolve]);
        let entering_id = enter_creature_on_battlefield(&mut gs, PlayerId(1), 5, 5, vec![]);

        let triggers = collect_evolve_triggers(&mut gs, entering_id);

        assert_eq!(
            triggers.len(),
            0,
            "Opponent's creature entering should not trigger your Evolve"
        );
    }

    #[test]
    fn evolve_does_not_trigger_on_itself() {
        // An Evolve creature ETBing should not trigger its own Evolve.
        use crate::types::ability::StaticAbility;
        let mut gs = two_player_state();
        let evolve_id =
            enter_creature_on_battlefield(&mut gs, PlayerId(0), 5, 5, vec![StaticAbility::Evolve]);

        let triggers = collect_evolve_triggers(&mut gs, evolve_id);

        assert_eq!(
            triggers.len(),
            0,
            "Evolve creature should not trigger on its own ETB"
        );
    }

    #[test]
    fn collect_etb_draw_trigger_returns_stack_object() {
        use crate::types::stack::StackPayload;
        let mut gs = two_player_state();
        put_in_library(&mut gs, PlayerId(0));
        let creature_id = place_on_battlefield(&mut gs, etb_draw_def(), PlayerId(0));

        let triggers = collect_etb_triggers(&mut gs, creature_id);

        assert_eq!(triggers.len(), 1);
        assert_eq!(triggers[0].controller, PlayerId(0));
        let StackPayload::TriggeredAbility {
            source_id, effect, ..
        } = &triggers[0].payload
        else {
            panic!("expected TriggeredAbility");
        };
        assert_eq!(source_id, &creature_id);
        assert_eq!(*effect, vec![EffectStep::DrawCard(1)]);
    }

    #[test]
    fn collect_etb_gain_life_trigger_returns_stack_object() {
        use crate::types::stack::StackPayload;
        let mut gs = two_player_state();
        let creature_id = place_on_battlefield(&mut gs, etb_gain_life_def(), PlayerId(0));

        let triggers = collect_etb_triggers(&mut gs, creature_id);

        assert_eq!(triggers.len(), 1);
        let StackPayload::TriggeredAbility {
            source_id, effect, ..
        } = &triggers[0].payload
        else {
            panic!("expected TriggeredAbility");
        };
        assert_eq!(source_id, &creature_id);
        assert_eq!(*effect, vec![EffectStep::GainLife(7)]);
    }

    #[test]
    fn collect_etb_no_triggers_returns_empty() {
        let mut gs = two_player_state();
        let def = CardDefinition {
            name: "Vanilla".into(),
            mana_cost: None,
            type_line: TypeLine {
                supertypes: vec![],
                card_types: vec![CardType::Creature],
                subtypes: vec![],
            },
            oracle_text: String::new(),
            abilities: vec![],
            text_annotations: vec![],
            power: Some(2),
            toughness: Some(2),
            colors: vec![],
        };
        let creature_id = place_on_battlefield(&mut gs, def, PlayerId(0));

        let triggers = collect_etb_triggers(&mut gs, creature_id);

        assert!(triggers.is_empty());
    }

    #[test]
    fn collect_cast_triggers_prowess_fires_on_noncreature() {
        use crate::engine::triggered::collect_cast_triggers;
        use crate::types::ability::{Ability, CastFilter, StaticAbility};
        use crate::types::card::{CardDefinition, CardType, TypeLine};
        use crate::types::mana::ManaCost;
        use crate::types::{CardObject, OracleSpan, Zone};

        let mut gs = two_player_state();

        // A creature with Prowess on the battlefield.
        let prowess_def = CardDefinition {
            name: "Prowess Monk".into(),
            mana_cost: None,
            type_line: TypeLine {
                supertypes: vec![],
                card_types: vec![CardType::Creature],
                subtypes: vec![],
            },
            oracle_text: "Prowess".into(),
            abilities: vec![OracleSpan::Parsed(Ability::Static(StaticAbility::Prowess))],
            text_annotations: vec![],
            power: Some(1),
            toughness: Some(1),
            colors: vec![],
        };
        let creature_id = place_on_battlefield(&mut gs, prowess_def, PlayerId(0));

        // A noncreature spell on the stack (instant).
        let instant_def = CardDefinition {
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
        let spell_id = gs.alloc_id();
        let spell_obj = CardObject::new(spell_id, instant_def, PlayerId(0), Zone::Stack);
        gs.add_object(spell_obj);

        let triggers =
            collect_cast_triggers(&mut gs, PlayerId(0), spell_id, &CastFilter::noncreature());

        assert_eq!(triggers.len(), 1);
        assert_eq!(triggers[0].controller, PlayerId(0));
        use crate::types::stack::StackPayload;
        let StackPayload::TriggeredAbility {
            source_id, effect, ..
        } = &triggers[0].payload
        else {
            panic!("expected TriggeredAbility");
        };
        assert_eq!(source_id, &creature_id);
        use crate::types::PTDelta;
        use crate::types::effect::{EffectStep, EffectTarget};
        assert_eq!(
            *effect,
            vec![EffectStep::BoostPermanentPT(PTDelta {
                power: 1,
                toughness: 1
            })]
        );
        assert_eq!(
            triggers[0].targets,
            vec![EffectTarget::Object { id: creature_id }]
        );
    }

    #[test]
    fn collect_cast_triggers_prowess_silent_on_creature_spell() {
        use crate::engine::triggered::collect_cast_triggers;
        use crate::types::ability::{Ability, CastFilter, StaticAbility};
        use crate::types::card::{CardDefinition, CardType, TypeLine};
        use crate::types::{CardObject, OracleSpan, Zone};

        let mut gs = two_player_state();

        let prowess_def = CardDefinition {
            name: "Prowess Monk".into(),
            mana_cost: None,
            type_line: TypeLine {
                supertypes: vec![],
                card_types: vec![CardType::Creature],
                subtypes: vec![],
            },
            oracle_text: String::new(),
            abilities: vec![OracleSpan::Parsed(Ability::Static(StaticAbility::Prowess))],
            text_annotations: vec![],
            power: Some(1),
            toughness: Some(1),
            colors: vec![],
        };
        place_on_battlefield(&mut gs, prowess_def, PlayerId(0));

        // A creature spell.
        let creature_spell_def = CardDefinition {
            name: "Bear".into(),
            mana_cost: None,
            type_line: TypeLine {
                supertypes: vec![],
                card_types: vec![CardType::Creature],
                subtypes: vec![],
            },
            oracle_text: String::new(),
            abilities: vec![],
            text_annotations: vec![],
            power: Some(2),
            toughness: Some(2),
            colors: vec![],
        };
        let spell_id = gs.alloc_id();
        let spell_obj = CardObject::new(spell_id, creature_spell_def, PlayerId(0), Zone::Stack);
        gs.add_object(spell_obj);

        let triggers =
            collect_cast_triggers(&mut gs, PlayerId(0), spell_id, &CastFilter::noncreature());
        assert!(triggers.is_empty());
    }

    #[test]
    fn collect_attack_triggers_exalted_single_attacker() {
        use crate::engine::triggered::collect_attack_triggers;
        use crate::types::OracleSpan;
        use crate::types::ability::{Ability, StaticAbility};
        use crate::types::card::{CardDefinition, CardType, TypeLine};

        let mut gs = two_player_state();
        // A 2/2 attacker (no Exalted).
        let attacker_def = CardDefinition {
            name: "Attacker".into(),
            mana_cost: None,
            type_line: TypeLine {
                supertypes: vec![],
                card_types: vec![CardType::Creature],
                subtypes: vec![],
            },
            oracle_text: String::new(),
            abilities: vec![],
            text_annotations: vec![],
            power: Some(2),
            toughness: Some(2),
            colors: vec![],
        };
        let attacker_id = place_on_battlefield(&mut gs, attacker_def, PlayerId(0));
        // An Exalted creature also controlled by P0.
        let exalted_def = CardDefinition {
            name: "Exalted Permanent".into(),
            mana_cost: None,
            type_line: TypeLine {
                supertypes: vec![],
                card_types: vec![CardType::Creature],
                subtypes: vec![],
            },
            oracle_text: String::new(),
            abilities: vec![OracleSpan::Parsed(Ability::Static(StaticAbility::Exalted))],
            text_annotations: vec![],
            power: Some(1),
            toughness: Some(1),
            colors: vec![],
        };
        let _exalted_id = place_on_battlefield(&mut gs, exalted_def, PlayerId(0));
        gs.combat.attackers = vec![attacker_id];

        let triggers = collect_attack_triggers(&mut gs);

        assert_eq!(triggers.len(), 1);
        use crate::types::stack::StackPayload;
        let StackPayload::TriggeredAbility { effect, .. } = &triggers[0].payload else {
            panic!("expected TriggeredAbility");
        };
        use crate::types::{
            PTDelta,
            effect::{EffectStep, EffectTarget},
        };
        assert_eq!(
            *effect,
            vec![EffectStep::BoostPermanentPT(PTDelta {
                power: 1,
                toughness: 1
            })]
        );
        assert_eq!(
            triggers[0].targets,
            vec![EffectTarget::Object { id: attacker_id }]
        );
    }

    #[test]
    fn collect_attack_triggers_exalted_multiple_attackers_no_trigger() {
        use crate::engine::triggered::collect_attack_triggers;
        use crate::types::OracleSpan;
        use crate::types::ability::{Ability, StaticAbility};
        use crate::types::card::{CardDefinition, CardType, TypeLine};

        let mut gs = two_player_state();
        let make_def = |name: &str| CardDefinition {
            name: name.into(),
            mana_cost: None,
            type_line: TypeLine {
                supertypes: vec![],
                card_types: vec![CardType::Creature],
                subtypes: vec![],
            },
            oracle_text: String::new(),
            abilities: vec![OracleSpan::Parsed(Ability::Static(StaticAbility::Exalted))],
            text_annotations: vec![],
            power: Some(1),
            toughness: Some(1),
            colors: vec![],
        };
        let a = place_on_battlefield(&mut gs, make_def("A"), PlayerId(0));
        let b = place_on_battlefield(&mut gs, make_def("B"), PlayerId(0));
        gs.combat.attackers = vec![a, b]; // two attackers — not alone

        let triggers = collect_attack_triggers(&mut gs);
        assert!(triggers.is_empty());
    }

    #[test]
    fn collect_attack_triggers_two_exalted_permanents_give_two_triggers() {
        use crate::engine::triggered::collect_attack_triggers;
        use crate::types::OracleSpan;
        use crate::types::ability::{Ability, StaticAbility};
        use crate::types::card::{CardDefinition, CardType, TypeLine};

        let mut gs = two_player_state();
        let plain_def = CardDefinition {
            name: "Attacker".into(),
            mana_cost: None,
            type_line: TypeLine {
                supertypes: vec![],
                card_types: vec![CardType::Creature],
                subtypes: vec![],
            },
            oracle_text: String::new(),
            abilities: vec![],
            text_annotations: vec![],
            power: Some(2),
            toughness: Some(2),
            colors: vec![],
        };
        let attacker_id = place_on_battlefield(&mut gs, plain_def, PlayerId(0));
        let exalted_def = CardDefinition {
            name: "Exalted".into(),
            mana_cost: None,
            type_line: TypeLine {
                supertypes: vec![],
                card_types: vec![CardType::Creature],
                subtypes: vec![],
            },
            oracle_text: String::new(),
            abilities: vec![OracleSpan::Parsed(Ability::Static(StaticAbility::Exalted))],
            text_annotations: vec![],
            power: Some(1),
            toughness: Some(1),
            colors: vec![],
        };
        place_on_battlefield(&mut gs, exalted_def.clone(), PlayerId(0));
        place_on_battlefield(&mut gs, exalted_def, PlayerId(0));
        gs.combat.attackers = vec![attacker_id];

        let triggers = collect_attack_triggers(&mut gs);
        assert_eq!(triggers.len(), 2); // one per Exalted permanent
    }

    #[test]
    fn collect_attack_triggers_melee_in_two_player_gives_one_boost() {
        use crate::engine::triggered::collect_attack_triggers;
        use crate::types::OracleSpan;
        use crate::types::ability::{Ability, StaticAbility};
        use crate::types::card::{CardDefinition, CardType, TypeLine};

        let mut gs = two_player_state();
        let melee_def = CardDefinition {
            name: "Melee Creature".into(),
            mana_cost: None,
            type_line: TypeLine {
                supertypes: vec![],
                card_types: vec![CardType::Creature],
                subtypes: vec![],
            },
            oracle_text: String::new(),
            abilities: vec![OracleSpan::Parsed(Ability::Static(StaticAbility::Melee))],
            text_annotations: vec![],
            power: Some(2),
            toughness: Some(2),
            colors: vec![],
        };
        let attacker_id = place_on_battlefield(&mut gs, melee_def, PlayerId(0));
        gs.combat.attackers = vec![attacker_id];

        let triggers = collect_attack_triggers(&mut gs);

        assert_eq!(triggers.len(), 1);
        use crate::types::stack::StackPayload;
        use crate::types::{
            PTDelta,
            effect::{EffectStep, EffectTarget},
        };
        let StackPayload::TriggeredAbility { effect, .. } = &triggers[0].payload else {
            panic!();
        };
        assert_eq!(
            *effect,
            vec![EffectStep::BoostPermanentPT(PTDelta {
                power: 1,
                toughness: 1
            })]
        );
        assert_eq!(
            triggers[0].targets,
            vec![EffectTarget::Object { id: attacker_id }]
        );
    }

    #[test]
    fn collect_block_triggers_flanking_gives_minus_one_to_non_flanking_blocker() {
        use crate::engine::triggered::collect_block_triggers;
        use crate::types::OracleSpan;
        use crate::types::ability::{Ability, StaticAbility};
        use crate::types::card::{CardDefinition, CardType, TypeLine};

        let mut gs = two_player_state();
        let flanking_def = CardDefinition {
            name: "Flanking Attacker".into(),
            mana_cost: None,
            type_line: TypeLine {
                supertypes: vec![],
                card_types: vec![CardType::Creature],
                subtypes: vec![],
            },
            oracle_text: String::new(),
            abilities: vec![OracleSpan::Parsed(Ability::Static(StaticAbility::Flanking))],
            text_annotations: vec![],
            power: Some(2),
            toughness: Some(2),
            colors: vec![],
        };
        let attacker_id = place_on_battlefield(&mut gs, flanking_def, PlayerId(0));
        let plain_def = CardDefinition {
            name: "Plain Blocker".into(),
            mana_cost: None,
            type_line: TypeLine {
                supertypes: vec![],
                card_types: vec![CardType::Creature],
                subtypes: vec![],
            },
            oracle_text: String::new(),
            abilities: vec![],
            text_annotations: vec![],
            power: Some(2),
            toughness: Some(2),
            colors: vec![],
        };
        let blocker_id = place_on_battlefield(&mut gs, plain_def, PlayerId(1));

        gs.combat.attackers = vec![attacker_id];
        gs.combat.blocking_map = [(attacker_id, vec![blocker_id])].into();

        let triggers = collect_block_triggers(&mut gs);

        assert_eq!(triggers.len(), 1);
        use crate::types::stack::StackPayload;
        use crate::types::{
            PTDelta,
            effect::{EffectStep, EffectTarget},
        };
        let StackPayload::TriggeredAbility { effect, .. } = &triggers[0].payload else {
            panic!();
        };
        assert_eq!(
            *effect,
            vec![EffectStep::BoostPermanentPT(PTDelta {
                power: -1,
                toughness: -1,
            })]
        );
        assert_eq!(
            triggers[0].targets,
            vec![EffectTarget::Object { id: blocker_id }]
        );
    }

    #[test]
    fn collect_block_triggers_flanking_no_trigger_for_flanking_blocker() {
        use crate::engine::triggered::collect_block_triggers;
        use crate::types::OracleSpan;
        use crate::types::ability::{Ability, StaticAbility};
        use crate::types::card::{CardDefinition, CardType, TypeLine};

        let mut gs = two_player_state();
        let flanking_def = |name: &str| CardDefinition {
            name: name.into(),
            mana_cost: None,
            type_line: TypeLine {
                supertypes: vec![],
                card_types: vec![CardType::Creature],
                subtypes: vec![],
            },
            oracle_text: String::new(),
            abilities: vec![OracleSpan::Parsed(Ability::Static(StaticAbility::Flanking))],
            text_annotations: vec![],
            power: Some(2),
            toughness: Some(2),
            colors: vec![],
        };
        let attacker_id = place_on_battlefield(&mut gs, flanking_def("Attacker"), PlayerId(0));
        let blocker_id = place_on_battlefield(&mut gs, flanking_def("Blocker"), PlayerId(1));
        gs.combat.attackers = vec![attacker_id];
        gs.combat.blocking_map = [(attacker_id, vec![blocker_id])].into();

        let triggers = collect_block_triggers(&mut gs);
        assert!(triggers.is_empty()); // blocker also has Flanking → no trigger
    }

    #[test]
    fn collect_block_triggers_bushido_boosts_attacker_and_blocker() {
        use crate::engine::triggered::collect_block_triggers;
        use crate::types::OracleSpan;
        use crate::types::ability::{Ability, StaticAbility};
        use crate::types::card::{CardDefinition, CardType, TypeLine};

        let mut gs = two_player_state();
        let bushido_def = CardDefinition {
            name: "Bushido Attacker".into(),
            mana_cost: None,
            type_line: TypeLine {
                supertypes: vec![],
                card_types: vec![CardType::Creature],
                subtypes: vec![],
            },
            oracle_text: String::new(),
            abilities: vec![OracleSpan::Parsed(Ability::Static(
                StaticAbility::BushidoN(2),
            ))],
            text_annotations: vec![],
            power: Some(3),
            toughness: Some(3),
            colors: vec![],
        };
        let attacker_id = place_on_battlefield(&mut gs, bushido_def, PlayerId(0));
        let bushido_blocker_def = CardDefinition {
            name: "Bushido Blocker".into(),
            mana_cost: None,
            type_line: TypeLine {
                supertypes: vec![],
                card_types: vec![CardType::Creature],
                subtypes: vec![],
            },
            oracle_text: String::new(),
            abilities: vec![OracleSpan::Parsed(Ability::Static(
                StaticAbility::BushidoN(1),
            ))],
            text_annotations: vec![],
            power: Some(2),
            toughness: Some(2),
            colors: vec![],
        };
        let blocker_id = place_on_battlefield(&mut gs, bushido_blocker_def, PlayerId(1));
        gs.combat.attackers = vec![attacker_id];
        gs.combat.blocking_map = [(attacker_id, vec![blocker_id])].into();

        let triggers = collect_block_triggers(&mut gs);
        assert_eq!(triggers.len(), 2); // one for attacker (Bushido 2), one for blocker (Bushido 1)

        use crate::types::stack::StackPayload;
        use crate::types::{
            PTDelta,
            effect::{EffectStep, EffectTarget},
        };
        let effects: Vec<_> = triggers
            .iter()
            .map(|t| {
                let StackPayload::TriggeredAbility { effect, .. } = &t.payload else {
                    panic!();
                };
                effect.clone()
            })
            .collect();
        let all_targets: Vec<_> = triggers.iter().map(|t| t.targets.clone()).collect();
        assert!(
            effects.contains(&vec![EffectStep::BoostPermanentPT(PTDelta {
                power: 2,
                toughness: 2,
            })])
        );
        assert!(
            effects.contains(&vec![EffectStep::BoostPermanentPT(PTDelta {
                power: 1,
                toughness: 1,
            })])
        );
        assert!(all_targets.contains(&vec![EffectTarget::Object { id: attacker_id }]));
        assert!(all_targets.contains(&vec![EffectTarget::Object { id: blocker_id }]));
    }

    #[test]
    fn collect_block_triggers_bushido_no_trigger_when_unblocked() {
        use crate::engine::triggered::collect_block_triggers;
        use crate::types::OracleSpan;
        use crate::types::ability::{Ability, StaticAbility};
        use crate::types::card::{CardDefinition, CardType, TypeLine};

        let mut gs = two_player_state();
        let bushido_def = CardDefinition {
            name: "Bushido".into(),
            mana_cost: None,
            type_line: TypeLine {
                supertypes: vec![],
                card_types: vec![CardType::Creature],
                subtypes: vec![],
            },
            oracle_text: String::new(),
            abilities: vec![OracleSpan::Parsed(Ability::Static(
                StaticAbility::BushidoN(2),
            ))],
            text_annotations: vec![],
            power: Some(3),
            toughness: Some(3),
            colors: vec![],
        };
        let attacker_id = place_on_battlefield(&mut gs, bushido_def, PlayerId(0));
        gs.combat.attackers = vec![attacker_id];
        gs.combat.blocking_map = [(attacker_id, vec![])].into(); // unblocked

        let triggers = collect_block_triggers(&mut gs);
        assert!(triggers.is_empty());
    }

    #[test]
    fn collect_etb_assigns_unique_stack_ids() {
        let mut gs = two_player_state();
        put_in_library(&mut gs, PlayerId(0));
        // creature with two ETB triggers
        let def = CardDefinition {
            name: "Multi Trigger".into(),
            mana_cost: None,
            type_line: TypeLine {
                supertypes: vec![],
                card_types: vec![CardType::Creature],
                subtypes: vec![],
            },
            oracle_text: String::new(),
            abilities: vec![
                OracleSpan::Parsed(Ability::Triggered(TriggeredAbility {
                    trigger: TriggerEvent::EntersTheBattlefield {
                        subject_is_self: true,
                    },
                    effect: vec![EffectStep::DrawCard(1)],
                })),
                OracleSpan::Parsed(Ability::Triggered(TriggeredAbility {
                    trigger: TriggerEvent::EntersTheBattlefield {
                        subject_is_self: true,
                    },
                    effect: vec![EffectStep::GainLife(2)],
                })),
            ],
            text_annotations: vec![],
            power: Some(1),
            toughness: Some(1),
            colors: vec![],
        };
        let creature_id = place_on_battlefield(&mut gs, def, PlayerId(0));

        let triggers = collect_etb_triggers(&mut gs, creature_id);

        assert_eq!(triggers.len(), 2);
        assert_ne!(triggers[0].id, triggers[1].id);
    }

    #[test]
    fn collect_ward_triggers_emits_triggered_ability_with_payment() {
        use crate::engine::triggered::collect_ward_triggers;
        use crate::types::OracleSpan;
        use crate::types::ability::{Ability, CostComponent, StaticAbility};
        use crate::types::card::{CardDefinition, CardType, TypeLine};
        use crate::types::effect::{EffectStep, EffectTarget};
        use crate::types::mana::{ManaCost, ManaPip};
        use crate::types::stack::{StackObject, StackPayload};

        let mut gs = two_player_state();

        // A creature with Ward {2} controlled by P1
        let ward_cost = vec![CostComponent::Mana(ManaCost {
            pips: vec![ManaPip::Generic(2)],
        })];
        let ward_def = CardDefinition {
            name: "Ward Creature".into(),
            mana_cost: None,
            type_line: TypeLine {
                supertypes: vec![],
                card_types: vec![CardType::Creature],
                subtypes: vec![],
            },
            oracle_text: String::new(),
            abilities: vec![OracleSpan::Parsed(Ability::Static(StaticAbility::Ward(
                ward_cost.clone(),
            )))],
            text_annotations: vec![],
            power: Some(2),
            toughness: Some(2),
            colors: vec![],
        };
        let ward_id = place_on_battlefield(&mut gs, ward_def, PlayerId(1));

        // A spell on the stack controlled by P0 targeting the ward creature
        let triggering_sid = gs.alloc_stack_id();
        let spell_card_id = gs.alloc_id();
        gs.stack_objects.insert(
            triggering_sid,
            StackObject {
                id: triggering_sid,
                payload: StackPayload::Spell {
                    card_id: spell_card_id,
                },
                controller: PlayerId(0),
                targets: vec![],
                x_value: None,
            },
        );
        gs.stack.push(triggering_sid);

        let targets = vec![EffectTarget::Object { id: ward_id }];
        let triggers = collect_ward_triggers(&mut gs, triggering_sid, PlayerId(0), &targets);

        assert_eq!(triggers.len(), 1);
        let trigger = &triggers[0];
        assert_eq!(trigger.controller, PlayerId(1));
        // Must be TriggeredAbility (not WardTrigger)
        let StackPayload::TriggeredAbility { effect, .. } = &trigger.payload else {
            panic!("expected TriggeredAbility, got something else");
        };
        assert_eq!(effect.len(), 1);
        assert!(matches!(&effect[0], EffectStep::Payment { .. }));
        // The target of the ward trigger should be the triggering spell
        assert_eq!(
            trigger.targets,
            vec![EffectTarget::StackObject { id: triggering_sid }]
        );
    }

    #[test]
    fn battle_cry_boosts_other_attackers_not_self() {
        // CR 702.91b: each OTHER attacking creature gets +1/+0
        use crate::types::OracleSpan;
        use crate::types::ability::{Ability, StaticAbility};
        use crate::types::card::{CardDefinition, CardType, TypeLine};
        use crate::types::effect::EffectTarget;
        use crate::types::stack::StackPayload;

        let mut gs = two_player_state();

        let battle_cry_def = CardDefinition {
            name: "Battle Cry Creature".into(),
            mana_cost: None,
            type_line: TypeLine {
                supertypes: vec![],
                card_types: vec![CardType::Creature],
                subtypes: vec![],
            },
            oracle_text: String::new(),
            abilities: vec![OracleSpan::Parsed(Ability::Static(
                StaticAbility::BattleCry,
            ))],
            text_annotations: vec![],
            power: Some(2),
            toughness: Some(2),
            colors: vec![],
        };
        let battle_cry_id = place_on_battlefield(&mut gs, battle_cry_def, PlayerId(0));

        let ally_def = CardDefinition {
            name: "Ally".into(),
            mana_cost: None,
            type_line: TypeLine {
                supertypes: vec![],
                card_types: vec![CardType::Creature],
                subtypes: vec![],
            },
            oracle_text: String::new(),
            abilities: vec![],
            text_annotations: vec![],
            power: Some(1),
            toughness: Some(1),
            colors: vec![],
        };
        let ally_id = place_on_battlefield(&mut gs, ally_def, PlayerId(0));

        gs.combat.attackers = vec![battle_cry_id, ally_id];

        // Act
        let triggers = collect_attack_triggers(&mut gs);

        // Assert: exactly one trigger from battle_cry_id, targeting the ally (not itself)
        let battle_cry_triggers: Vec<_> = triggers
            .iter()
            .filter(|t| {
                matches!(&t.payload, StackPayload::TriggeredAbility { source_id, .. } if *source_id == battle_cry_id)
            })
            .collect();
        assert_eq!(
            battle_cry_triggers.len(),
            1,
            "Battle Cry should generate exactly one boost trigger (for the ally)"
        );

        let trigger = &battle_cry_triggers[0];
        assert_eq!(
            trigger.targets,
            vec![EffectTarget::Object { id: ally_id }],
            "Battle Cry boost should target the ally, not the Battle Cry creature"
        );
        if let StackPayload::TriggeredAbility { effect, .. } = &trigger.payload {
            assert!(
                matches!(
                    effect[0],
                    EffectStep::BoostPermanentPT(PTDelta {
                        power: 1,
                        toughness: 0
                    })
                ),
                "Battle Cry boost should have +1/+0 modifier"
            );
        } else {
            panic!("Expected TriggeredAbility payload");
        }
    }
}
