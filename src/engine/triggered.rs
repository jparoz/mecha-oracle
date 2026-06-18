use crate::types::ability::{Ability, StaticAbility, TriggerEvent, TriggeredAbility};
use crate::types::effect::EffectStep;
use crate::types::stack::{StackObject, StackPayload};
use crate::types::{
    CounterKind, GameEvent, GameState, ObjectId, OracleSpan, PTDelta, PlayerId, TriggerCondition,
    TriggerSubjectFilter, TriggerTargetMode, TurnOwner,
};

/// Returns true if filter matches the given subject.
/// source_id: the permanent that owns the triggered ability.
/// source_controller: that permanent's controller.
fn subject_filter_matches(
    filter: &TriggerSubjectFilter,
    subject_id: Option<ObjectId>,
    source_id: ObjectId,
    source_controller: PlayerId,
    state: &GameState,
) -> bool {
    let sid = match subject_id {
        Some(id) => id,
        // No subject satisfies any non-empty filter.
        None => return filter == &TriggerSubjectFilter::default(),
    };

    if let Some(is_self) = filter.is_self
        && is_self != (sid == source_id)
    {
        return false;
    }

    if let Some(ref required_owner) = filter.controller {
        let subject_controller = state.objects.get(&sid).map(|o| o.controller);
        let ok = match required_owner {
            TurnOwner::You => subject_controller == Some(source_controller),
            TurnOwner::Opponent => subject_controller
                .map(|c| c != source_controller)
                .unwrap_or(false),
            TurnOwner::Any => true,
        };
        if !ok {
            return false;
        }
    }

    if let Some(obj) = state.objects.get(&sid) {
        if !filter.card_types.is_empty()
            && !filter
                .card_types
                .iter()
                .any(|t| obj.definition.type_line.card_types.contains(t))
        {
            return false;
        }
        if !filter.subtypes.is_empty()
            && !filter
                .subtypes
                .iter()
                .all(|t| obj.definition.type_line.subtypes.contains(t))
        {
            return false;
        }
    }

    true
}

/// Returns true if the trigger condition is satisfied given the current game state and event subject.
fn trigger_condition_satisfied(
    condition: &TriggerCondition,
    subject_id: Option<ObjectId>,
    source_id: ObjectId,
    state: &GameState,
) -> bool {
    match condition {
        TriggerCondition::ExactlyOneAttacker => state.combat.attackers.len() == 1,

        TriggerCondition::AttackingAlongsideGreaterPowerCreature => {
            let my_power = state
                .battlefield
                .get(&source_id)
                .and_then(|p| p.effective_power())
                .unwrap_or(0);
            state
                .combat
                .attackers
                .iter()
                .filter(|&&id| id != source_id)
                .any(|&id| {
                    state
                        .battlefield
                        .get(&id)
                        .and_then(|p| p.effective_power())
                        .map(|p| p > my_power)
                        .unwrap_or(false)
                })
        }

        TriggerCondition::EnteringCreatureHasGreaterPower => {
            let sid = match subject_id {
                Some(id) => id,
                None => return false,
            };
            let entering_power = match state
                .battlefield
                .get(&sid)
                .and_then(|p| p.effective_power())
            {
                Some(p) => p,
                None => return false,
            };
            let my_power = state
                .battlefield
                .get(&source_id)
                .and_then(|p| p.effective_power())
                .unwrap_or(0);
            entering_power > my_power
        }

        TriggerCondition::EnteringCreatureHasGreaterToughness => {
            let sid = match subject_id {
                Some(id) => id,
                None => return false,
            };
            let entering_toughness = match state
                .battlefield
                .get(&sid)
                .and_then(|p| p.effective_toughness())
            {
                Some(t) => t,
                None => return false,
            };
            let my_toughness = state
                .battlefield
                .get(&source_id)
                .and_then(|p| p.effective_toughness())
                .unwrap_or(0);
            entering_toughness > my_toughness
        }

        TriggerCondition::EnteringCreatureHasGreaterPowerOrToughness => {
            let sid = match subject_id {
                Some(id) => id,
                None => return false,
            };
            let ep = state
                .battlefield
                .get(&sid)
                .and_then(|p| p.effective_power());
            let et = state
                .battlefield
                .get(&sid)
                .and_then(|p| p.effective_toughness());
            let mp = state
                .battlefield
                .get(&source_id)
                .and_then(|p| p.effective_power())
                .unwrap_or(0);
            let mt = state
                .battlefield
                .get(&source_id)
                .and_then(|p| p.effective_toughness())
                .unwrap_or(0);
            ep.map(|p| p > mp).unwrap_or(false) || et.map(|t| t > mt).unwrap_or(false)
        }

        TriggerCondition::SubjectLacksKeyword(kw) => {
            let sid = match subject_id {
                Some(id) => id,
                None => return false,
            };
            !state
                .battlefield
                .get(&sid)
                .map(|p| p.has_keyword(kw.clone()))
                .unwrap_or(false)
        }
    }
}

/// CR 702.108a: Prowess — whenever you cast a noncreature spell, this creature gets +1/+1 until EOT.
pub fn prowess_triggered_ability() -> TriggeredAbility {
    use crate::types::ability::SpellFilter;
    TriggeredAbility {
        trigger: TriggerEvent::SpellCast {
            caster: TurnOwner::You,
            filter: SpellFilter::noncreature(),
        },
        condition: None,
        target_mode: TriggerTargetMode::Source,
        effect: vec![EffectStep::BoostPermanentPT(PTDelta {
            power: 1,
            toughness: 1,
        })],
    }
}

/// CR 702.83b: Exalted — when exactly one creature you control attacks, that creature gets +1/+1.
pub fn exalted_triggered_ability() -> TriggeredAbility {
    TriggeredAbility {
        trigger: TriggerEvent::Attacks {
            subject: TriggerSubjectFilter {
                controller: Some(TurnOwner::You),
                ..Default::default()
            },
        },
        condition: Some(TriggerCondition::ExactlyOneAttacker),
        target_mode: TriggerTargetMode::Subject,
        effect: vec![EffectStep::BoostPermanentPT(PTDelta {
            power: 1,
            toughness: 1,
        })],
    }
}

/// CR 702.121b: Melee — when this attacks, it gets +1/+1 until end of turn (2-player = 1 opponent always).
pub fn melee_triggered_ability() -> TriggeredAbility {
    TriggeredAbility {
        trigger: TriggerEvent::Attacks {
            subject: TriggerSubjectFilter {
                is_self: Some(true),
                ..Default::default()
            },
        },
        condition: None,
        target_mode: TriggerTargetMode::Source,
        effect: vec![EffectStep::BoostPermanentPT(PTDelta {
            power: 1,
            toughness: 1,
        })],
    }
}

/// CR 702.91b: Battle Cry — when this attacks, each other attacking creature gets +1/+0 until end of turn.
pub fn battle_cry_triggered_ability() -> TriggeredAbility {
    TriggeredAbility {
        trigger: TriggerEvent::Attacks {
            subject: TriggerSubjectFilter {
                is_self: Some(true),
                ..Default::default()
            },
        },
        condition: None,
        target_mode: TriggerTargetMode::AllOtherAttackers,
        effect: vec![EffectStep::BoostPermanentPT(PTDelta {
            power: 1,
            toughness: 0,
        })],
    }
}

/// CR 702.149a: Training — when this attacks alongside a creature with greater power, put a +1/+1 counter on it.
pub fn training_triggered_ability() -> TriggeredAbility {
    TriggeredAbility {
        trigger: TriggerEvent::Attacks {
            subject: TriggerSubjectFilter {
                is_self: Some(true),
                ..Default::default()
            },
        },
        condition: Some(TriggerCondition::AttackingAlongsideGreaterPowerCreature),
        target_mode: TriggerTargetMode::Source,
        effect: vec![EffectStep::AddCounter {
            kind: CounterKind::PtModifier {
                power: 1,
                toughness: 1,
            },
            count: 1,
        }],
    }
}

/// CR 603.2: collect all triggered abilities on the battlefield that fire for the given game event.
pub fn collect_triggers_for_event(state: &mut GameState, event: &GameEvent) -> Vec<StackObject> {
    use crate::engine::stack::inject_source_flags;
    use crate::types::effect::EffectTarget;

    // Snapshot source IDs to avoid borrow conflicts during iteration.
    let source_ids: Vec<ObjectId> = state.battlefield.keys().copied().collect();
    let mut result = Vec::new();

    for source_id in source_ids {
        let (controller, abilities) = match state.objects.get(&source_id) {
            Some(o) => (o.controller, o.definition.abilities.clone()),
            None => continue,
        };

        for span in &abilities {
            let triggered = match span {
                OracleSpan::Parsed(Ability::Triggered(t)) => t,
                _ => continue,
            };

            // Match event discriminant and subject filter.
            let subject_id: Option<ObjectId> = match (event, &triggered.trigger) {
                (
                    GameEvent::EntersTheBattlefield { subject_id },
                    TriggerEvent::EntersTheBattlefield { subject },
                ) if subject_filter_matches(
                    subject,
                    Some(*subject_id),
                    source_id,
                    controller,
                    state,
                ) =>
                {
                    Some(*subject_id)
                }
                (GameEvent::Dies { subject_id }, TriggerEvent::Dies { subject })
                    if subject_filter_matches(
                        subject,
                        Some(*subject_id),
                        source_id,
                        controller,
                        state,
                    ) =>
                {
                    Some(*subject_id)
                }
                (GameEvent::Attacks { subject_id }, TriggerEvent::Attacks { subject })
                    if subject_filter_matches(
                        subject,
                        Some(*subject_id),
                        source_id,
                        controller,
                        state,
                    ) =>
                {
                    Some(*subject_id)
                }
                (GameEvent::Blocks { subject_id }, TriggerEvent::Blocks { subject })
                    if subject_filter_matches(
                        subject,
                        Some(*subject_id),
                        source_id,
                        controller,
                        state,
                    ) =>
                {
                    Some(*subject_id)
                }
                (
                    GameEvent::BecomesBlocked { subject_id },
                    TriggerEvent::BecomesBlocked { subject },
                ) if subject_filter_matches(
                    subject,
                    Some(*subject_id),
                    source_id,
                    controller,
                    state,
                ) =>
                {
                    Some(*subject_id)
                }
                (
                    GameEvent::SpellCast { caster, spell_id },
                    TriggerEvent::SpellCast {
                        caster: required_caster,
                        filter,
                    },
                ) => {
                    let caster_ok = match required_caster {
                        TurnOwner::You => *caster == controller,
                        TurnOwner::Opponent => *caster != controller,
                        TurnOwner::Any => true,
                    };
                    if !caster_ok {
                        continue;
                    }
                    let spell_ok = state
                        .objects
                        .get(spell_id)
                        .map(|o| {
                            filter.matches(
                                &o.definition.type_line.card_types,
                                o.definition
                                    .mana_cost
                                    .as_ref()
                                    .map(|c| c.mana_value())
                                    .unwrap_or(0),
                                &o.definition.colors,
                            )
                        })
                        .unwrap_or(false);
                    if !spell_ok {
                        continue;
                    }
                    None
                }
                (
                    GameEvent::TargetedBy {
                        target_id,
                        acting_player,
                    },
                    TriggerEvent::TargetedBy {
                        controller: required,
                    },
                ) => {
                    if *target_id != source_id {
                        continue;
                    }
                    let ok = match required {
                        TurnOwner::Opponent => *acting_player != controller,
                        TurnOwner::You => *acting_player == controller,
                        TurnOwner::Any => true,
                    };
                    if !ok {
                        continue;
                    }
                    None
                }
                // CR 603.2: DrawsCard event fires once per card drawn.
                (GameEvent::DrawsCard { player }, TriggerEvent::DrawsCard { who }) => {
                    let ok = match who {
                        TurnOwner::You => *player == controller,
                        TurnOwner::Opponent => *player != controller,
                        TurnOwner::Any => true,
                    };
                    if !ok {
                        continue;
                    }
                    None
                }
                // CR 603.2b: PhaseStep event fires at the beginning of each step/phase.
                (
                    GameEvent::PhaseStep {
                        step: event_step,
                        active_player,
                    },
                    TriggerEvent::PhaseStep {
                        step: trigger_step,
                        whose_turn,
                    },
                ) => {
                    if event_step != trigger_step {
                        continue;
                    }
                    let ok = match whose_turn {
                        TurnOwner::You => *active_player == controller,
                        TurnOwner::Opponent => *active_player != controller,
                        TurnOwner::Any => true,
                    };
                    if !ok {
                        continue;
                    }
                    None
                }
                _ => continue,
            };

            // Check condition.
            if let Some(cond) = &triggered.condition
                && !trigger_condition_satisfied(cond, subject_id, source_id, state)
            {
                continue;
            }

            // Resolve targets.
            let triggered_clone = triggered.clone();
            let targets: Vec<EffectTarget> = match &triggered_clone.target_mode {
                TriggerTargetMode::None => vec![],
                TriggerTargetMode::Source => vec![EffectTarget::Object { id: source_id }],
                TriggerTargetMode::Subject => match subject_id {
                    Some(sid) => vec![EffectTarget::Object { id: sid }],
                    None => vec![],
                },
                TriggerTargetMode::AllOtherAttackers => state
                    .combat
                    .attackers
                    .iter()
                    .filter(|&&id| id != source_id)
                    .map(|&id| EffectTarget::Object { id })
                    .collect(),
            };

            let effect = inject_source_flags(triggered_clone.effect, &abilities);
            let sid = state.alloc_stack_id();
            let label = format!(
                "{}: trigger",
                state
                    .objects
                    .get(&source_id)
                    .map(|o| o.definition.name.as_str())
                    .unwrap_or("?")
            );
            result.push(StackObject {
                id: sid,
                payload: StackPayload::TriggeredAbility {
                    source_id,
                    effect,
                    label,
                },
                controller,
                targets,
                x_value: None,
            });
        }

        // Handle StaticAbility::Evolve — fires when an ETB event has a subject under the
        // same controller with greater power or toughness (CR 702.100b).
        if let GameEvent::EntersTheBattlefield {
            subject_id: entering_id,
        } = event
        {
            let entering_id = *entering_id;
            if source_id != entering_id
                && state
                    .objects
                    .get(&source_id)
                    .map(|o| o.controller == controller)
                    .unwrap_or(false)
                && state
                    .battlefield
                    .get(&source_id)
                    .map(|p| p.has_keyword(StaticAbility::Evolve))
                    .unwrap_or(false)
            {
                let entering_controller = state.objects.get(&entering_id).map(|o| o.controller);
                if entering_controller == Some(controller) {
                    let entering_power = state
                        .battlefield
                        .get(&entering_id)
                        .and_then(|p| p.effective_power());
                    let entering_toughness = state
                        .battlefield
                        .get(&entering_id)
                        .and_then(|p| p.effective_toughness());
                    let my_power = state
                        .battlefield
                        .get(&source_id)
                        .and_then(|p| p.effective_power())
                        .unwrap_or(0);
                    let my_toughness = state
                        .battlefield
                        .get(&source_id)
                        .and_then(|p| p.effective_toughness())
                        .unwrap_or(0);
                    let qualifies = entering_power.map(|ep| ep > my_power).unwrap_or(false)
                        || entering_toughness
                            .map(|et| et > my_toughness)
                            .unwrap_or(false);
                    if qualifies {
                        use crate::types::CounterKind;
                        let sid = state.alloc_stack_id();
                        result.push(StackObject {
                            id: sid,
                            payload: StackPayload::TriggeredAbility {
                                source_id,
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
                            targets: vec![EffectTarget::Object { id: source_id }],
                            x_value: None,
                        });
                    }
                }
            }
        }
    }

    result
}

/// CR 702.25a: Flanking — when a non-Flanking creature blocks this, it gets -1/-1.
pub fn flanking_triggered_ability() -> TriggeredAbility {
    TriggeredAbility {
        trigger: TriggerEvent::Blocks {
            subject: TriggerSubjectFilter {
                controller: Some(TurnOwner::Opponent),
                ..Default::default()
            },
        },
        condition: Some(TriggerCondition::SubjectLacksKeyword(
            StaticAbility::Flanking,
        )),
        target_mode: TriggerTargetMode::Subject,
        effect: vec![EffectStep::BoostPermanentPT(PTDelta {
            power: -1,
            toughness: -1,
        })],
    }
}

/// CR 702.45a: Bushido N (attacker) — fires when this becomes blocked.
pub fn bushido_attacker_triggered_ability(n: u32) -> TriggeredAbility {
    TriggeredAbility {
        trigger: TriggerEvent::BecomesBlocked {
            subject: TriggerSubjectFilter {
                is_self: Some(true),
                ..Default::default()
            },
        },
        condition: None,
        target_mode: TriggerTargetMode::Source,
        effect: vec![EffectStep::BoostPermanentPT(PTDelta {
            power: n as i32,
            toughness: n as i32,
        })],
    }
}

/// CR 702.45a: Bushido N (blocker) — fires when this blocks.
pub fn bushido_blocker_triggered_ability(n: u32) -> TriggeredAbility {
    TriggeredAbility {
        trigger: TriggerEvent::Blocks {
            subject: TriggerSubjectFilter {
                is_self: Some(true),
                ..Default::default()
            },
        },
        condition: None,
        target_mode: TriggerTargetMode::Source,
        effect: vec![EffectStep::BoostPermanentPT(PTDelta {
            power: n as i32,
            toughness: n as i32,
        })],
    }
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
        use crate::types::ability::{TriggerSubjectFilter, TriggerTargetMode};
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
        }
    }

    fn etb_gain_life_def() -> CardDefinition {
        use crate::types::ability::{TriggerSubjectFilter, TriggerTargetMode};
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
                    subject: TriggerSubjectFilter {
                        is_self: Some(true),
                        ..Default::default()
                    },
                },
                condition: None,
                target_mode: TriggerTargetMode::None,
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

    /// Place a creature on the battlefield carrying the given TriggeredAbility oracle spans.
    fn triggered_attacker(
        state: &mut GameState,
        owner: PlayerId,
        power: i32,
        toughness: i32,
        abilities: Vec<OracleSpan>,
    ) -> ObjectId {
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
            abilities,
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
        use crate::types::GameEvent;
        use crate::types::stack::StackPayload;
        let mut gs = two_player_state();
        let training_span = OracleSpan::Parsed(Ability::Triggered(training_triggered_ability()));
        let training_id = triggered_attacker(&mut gs, PlayerId(0), 2, 2, vec![training_span]);
        let ally_id = keyword_attacker(&mut gs, PlayerId(0), 3, 3, vec![]);
        gs.combat.attackers = vec![training_id, ally_id];

        let triggers = collect_triggers_for_event(
            &mut gs,
            &GameEvent::Attacks {
                subject_id: training_id,
            },
        );

        assert_eq!(
            triggers.iter().filter(|t| {
                matches!(&t.payload, StackPayload::TriggeredAbility { source_id, .. } if *source_id == training_id)
            }).count(),
            1,
            "Should have exactly one Training trigger"
        );
    }

    #[test]
    fn training_does_not_trigger_when_no_ally_with_greater_power() {
        // CR 702.149a: Training requires the ally to have GREATER power; equal doesn't count.
        use crate::types::GameEvent;
        use crate::types::stack::StackPayload;
        let mut gs = two_player_state();
        let training_span = OracleSpan::Parsed(Ability::Triggered(training_triggered_ability()));
        let training_id = triggered_attacker(&mut gs, PlayerId(0), 2, 2, vec![training_span]);
        let ally_id = keyword_attacker(&mut gs, PlayerId(0), 2, 2, vec![]);
        gs.combat.attackers = vec![training_id, ally_id];

        let triggers = collect_triggers_for_event(
            &mut gs,
            &GameEvent::Attacks {
                subject_id: training_id,
            },
        );

        assert_eq!(
            triggers.iter().filter(|t| {
                matches!(&t.payload, StackPayload::TriggeredAbility { source_id, .. } if *source_id == training_id)
            }).count(),
            0,
            "Training should not trigger when ally power equals training creature's power"
        );
    }

    #[test]
    fn training_does_not_trigger_when_attacking_alone() {
        // CR 702.149a: No trigger if attacking alone (no other creatures attacking).
        use crate::types::GameEvent;
        use crate::types::stack::StackPayload;
        let mut gs = two_player_state();
        let training_span = OracleSpan::Parsed(Ability::Triggered(training_triggered_ability()));
        let training_id = triggered_attacker(&mut gs, PlayerId(0), 2, 2, vec![training_span]);
        gs.combat.attackers = vec![training_id];

        let triggers = collect_triggers_for_event(
            &mut gs,
            &GameEvent::Attacks {
                subject_id: training_id,
            },
        );

        let training_count = triggers.iter().filter(|t| {
            matches!(&t.payload, StackPayload::TriggeredAbility { source_id, .. } if *source_id == training_id)
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
        use crate::types::GameEvent;
        use crate::types::effect::EffectTarget;
        use crate::types::stack::StackPayload;
        let mut gs = two_player_state();
        let training_span = OracleSpan::Parsed(Ability::Triggered(training_triggered_ability()));
        let training_id = triggered_attacker(&mut gs, PlayerId(0), 2, 2, vec![training_span]);
        let ally_id = keyword_attacker(&mut gs, PlayerId(0), 3, 3, vec![]);
        gs.combat.attackers = vec![training_id, ally_id];

        let triggers = collect_triggers_for_event(
            &mut gs,
            &GameEvent::Attacks {
                subject_id: training_id,
            },
        );

        let training_trigger = triggers.iter().find(|t| {
            matches!(&t.payload, StackPayload::TriggeredAbility { source_id, .. } if *source_id == training_id)
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

        let triggers = collect_triggers_for_event(
            &mut gs,
            &GameEvent::EntersTheBattlefield {
                subject_id: entering_id,
            },
        );

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

        let triggers = collect_triggers_for_event(
            &mut gs,
            &GameEvent::EntersTheBattlefield {
                subject_id: entering_id,
            },
        );

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

        let triggers = collect_triggers_for_event(
            &mut gs,
            &GameEvent::EntersTheBattlefield {
                subject_id: entering_id,
            },
        );

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

        let triggers = collect_triggers_for_event(
            &mut gs,
            &GameEvent::EntersTheBattlefield {
                subject_id: entering_id,
            },
        );

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

        let triggers = collect_triggers_for_event(
            &mut gs,
            &GameEvent::EntersTheBattlefield {
                subject_id: evolve_id,
            },
        );

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

        let triggers = collect_triggers_for_event(
            &mut gs,
            &GameEvent::EntersTheBattlefield {
                subject_id: creature_id,
            },
        );

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

        let triggers = collect_triggers_for_event(
            &mut gs,
            &GameEvent::EntersTheBattlefield {
                subject_id: creature_id,
            },
        );

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

        let triggers = collect_triggers_for_event(
            &mut gs,
            &GameEvent::EntersTheBattlefield {
                subject_id: creature_id,
            },
        );

        assert!(triggers.is_empty());
    }

    #[test]
    fn collect_cast_triggers_prowess_fires_on_noncreature() {
        use crate::types::ability::Ability;
        use crate::types::card::{CardDefinition, CardType, TypeLine};
        use crate::types::mana::ManaCost;
        use crate::types::{CardObject, GameEvent, OracleSpan, Zone};

        let mut gs = two_player_state();

        // A creature with Prowess (as TriggeredAbility) on the battlefield.
        let prowess_def = CardDefinition {
            name: "Prowess Monk".into(),
            mana_cost: None,
            type_line: TypeLine {
                supertypes: vec![],
                card_types: vec![CardType::Creature],
                subtypes: vec![],
            },
            oracle_text: "Prowess".into(),
            abilities: vec![OracleSpan::Parsed(Ability::Triggered(
                prowess_triggered_ability(),
            ))],
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

        let triggers = collect_triggers_for_event(
            &mut gs,
            &GameEvent::SpellCast {
                caster: PlayerId(0),
                spell_id,
            },
        );

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
        use crate::types::ability::Ability;
        use crate::types::card::{CardDefinition, CardType, TypeLine};
        use crate::types::{CardObject, GameEvent, OracleSpan, Zone};

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
            abilities: vec![OracleSpan::Parsed(Ability::Triggered(
                prowess_triggered_ability(),
            ))],
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

        let triggers = collect_triggers_for_event(
            &mut gs,
            &GameEvent::SpellCast {
                caster: PlayerId(0),
                spell_id,
            },
        );
        assert!(triggers.is_empty());
    }

    #[test]
    fn collect_attack_triggers_exalted_single_attacker() {
        use crate::types::GameEvent;
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
        // An Exalted permanent also controlled by P0.
        let exalted_span = OracleSpan::Parsed(Ability::Triggered(exalted_triggered_ability()));
        let _exalted_id = triggered_attacker(&mut gs, PlayerId(0), 1, 1, vec![exalted_span]);
        gs.combat.attackers = vec![attacker_id];

        let triggers = collect_triggers_for_event(
            &mut gs,
            &GameEvent::Attacks {
                subject_id: attacker_id,
            },
        );

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
        use crate::types::GameEvent;

        let mut gs = two_player_state();
        let exalted_span_a = OracleSpan::Parsed(Ability::Triggered(exalted_triggered_ability()));
        let exalted_span_b = OracleSpan::Parsed(Ability::Triggered(exalted_triggered_ability()));
        let a = triggered_attacker(&mut gs, PlayerId(0), 1, 1, vec![exalted_span_a]);
        let b = triggered_attacker(&mut gs, PlayerId(0), 1, 1, vec![exalted_span_b]);
        gs.combat.attackers = vec![a, b]; // two attackers — ExactlyOneAttacker condition fails

        // Fire Attacks event for each attacker; neither should trigger because condition fails.
        let mut triggers =
            collect_triggers_for_event(&mut gs, &GameEvent::Attacks { subject_id: a });
        triggers.extend(collect_triggers_for_event(
            &mut gs,
            &GameEvent::Attacks { subject_id: b },
        ));
        assert!(triggers.is_empty());
    }

    #[test]
    fn collect_attack_triggers_two_exalted_permanents_give_two_triggers() {
        use crate::types::GameEvent;
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
        let exalted_span_a = OracleSpan::Parsed(Ability::Triggered(exalted_triggered_ability()));
        let exalted_span_b = OracleSpan::Parsed(Ability::Triggered(exalted_triggered_ability()));
        triggered_attacker(&mut gs, PlayerId(0), 1, 1, vec![exalted_span_a]);
        triggered_attacker(&mut gs, PlayerId(0), 1, 1, vec![exalted_span_b]);
        gs.combat.attackers = vec![attacker_id];

        let triggers = collect_triggers_for_event(
            &mut gs,
            &GameEvent::Attacks {
                subject_id: attacker_id,
            },
        );
        assert_eq!(triggers.len(), 2); // one per Exalted permanent
    }

    #[test]
    fn collect_attack_triggers_melee_in_two_player_gives_one_boost() {
        use crate::types::GameEvent;

        let mut gs = two_player_state();
        let melee_span = OracleSpan::Parsed(Ability::Triggered(melee_triggered_ability()));
        let attacker_id = triggered_attacker(&mut gs, PlayerId(0), 2, 2, vec![melee_span]);
        gs.combat.attackers = vec![attacker_id];

        let triggers = collect_triggers_for_event(
            &mut gs,
            &GameEvent::Attacks {
                subject_id: attacker_id,
            },
        );

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

    /// Place a creature on the battlefield carrying the given TriggeredAbility oracle spans.
    fn triggered_blocker(
        state: &mut GameState,
        owner: PlayerId,
        power: i32,
        toughness: i32,
        abilities: Vec<OracleSpan>,
    ) -> ObjectId {
        use crate::types::card::{CardType, TypeLine};
        let def = CardDefinition {
            name: "Test Blocker".into(),
            mana_cost: None,
            type_line: TypeLine {
                supertypes: vec![],
                card_types: vec![CardType::Creature],
                subtypes: vec![],
            },
            oracle_text: String::new(),
            abilities,
            text_annotations: vec![],
            power: Some(power),
            toughness: Some(toughness),
            colors: vec![],
        };
        place_on_battlefield(state, def, owner)
    }

    #[test]
    fn collect_block_triggers_flanking_gives_minus_one_to_non_flanking_blocker() {
        // CR 702.25a: Flanking fires when a non-Flanking blocker blocks the Flanking creature.
        use crate::types::GameEvent;
        use crate::types::effect::EffectTarget;
        use crate::types::stack::StackPayload;

        let mut gs = two_player_state();
        let flanking_span = OracleSpan::Parsed(Ability::Triggered(flanking_triggered_ability()));
        let attacker_id = triggered_attacker(&mut gs, PlayerId(0), 2, 2, vec![flanking_span]);
        let blocker_id = triggered_blocker(&mut gs, PlayerId(1), 2, 2, vec![]);

        gs.combat.attackers = vec![attacker_id];
        gs.combat.blocking_map = [(attacker_id, vec![blocker_id])].into();

        // Fire Blocks event for the blocker (subject = blocker).
        let triggers = collect_triggers_for_event(
            &mut gs,
            &GameEvent::Blocks {
                subject_id: blocker_id,
            },
        );

        assert_eq!(triggers.len(), 1);
        use crate::types::{PTDelta, effect::EffectStep};
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
        // CR 702.25a: Flanking blocker also has Flanking — SubjectLacksKeyword condition fails.
        use crate::types::GameEvent;

        let mut gs = two_player_state();
        let flanking_span_a = OracleSpan::Parsed(Ability::Triggered(flanking_triggered_ability()));
        let flanking_span_b = OracleSpan::Parsed(Ability::Static(StaticAbility::Flanking));
        let attacker_id = triggered_attacker(&mut gs, PlayerId(0), 2, 2, vec![flanking_span_a]);
        // Blocker also has Flanking (as StaticAbility so has_keyword check fires).
        let blocker_id = triggered_blocker(&mut gs, PlayerId(1), 2, 2, vec![flanking_span_b]);

        gs.combat.attackers = vec![attacker_id];
        gs.combat.blocking_map = [(attacker_id, vec![blocker_id])].into();

        let triggers = collect_triggers_for_event(
            &mut gs,
            &GameEvent::Blocks {
                subject_id: blocker_id,
            },
        );
        assert!(triggers.is_empty()); // blocker also has Flanking → condition fails → no trigger
    }

    #[test]
    fn collect_block_triggers_bushido_boosts_attacker_and_blocker() {
        // CR 702.45a: Bushido fires on both blocks and becomes-blocked.
        use crate::types::GameEvent;
        use crate::types::effect::EffectTarget;
        use crate::types::stack::StackPayload;

        let mut gs = two_player_state();

        // Attacker: Bushido 2 — fires on BecomesBlocked.
        let attacker_id = triggered_attacker(
            &mut gs,
            PlayerId(0),
            3,
            3,
            vec![
                OracleSpan::Parsed(Ability::Triggered(bushido_attacker_triggered_ability(2))),
                OracleSpan::Parsed(Ability::Triggered(bushido_blocker_triggered_ability(2))),
            ],
        );

        // Blocker: Bushido 1 — fires on Blocks.
        let blocker_id = triggered_blocker(
            &mut gs,
            PlayerId(1),
            2,
            2,
            vec![
                OracleSpan::Parsed(Ability::Triggered(bushido_attacker_triggered_ability(1))),
                OracleSpan::Parsed(Ability::Triggered(bushido_blocker_triggered_ability(1))),
            ],
        );

        gs.combat.attackers = vec![attacker_id];
        gs.combat.blocking_map = [(attacker_id, vec![blocker_id])].into();

        // Fire Blocks event for blocker → blocker's bushido_blocker fires (is_self = true).
        let mut triggers = collect_triggers_for_event(
            &mut gs,
            &GameEvent::Blocks {
                subject_id: blocker_id,
            },
        );
        // Fire BecomesBlocked for attacker → attacker's bushido_attacker fires (is_self = true).
        triggers.extend(collect_triggers_for_event(
            &mut gs,
            &GameEvent::BecomesBlocked {
                subject_id: attacker_id,
            },
        ));

        assert_eq!(triggers.len(), 2); // one for attacker (Bushido 2), one for blocker (Bushido 1)

        use crate::types::{PTDelta, effect::EffectStep};
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
        // CR 702.45a: Bushido on attacker fires on BecomesBlocked — no event fired if unblocked.
        use crate::types::GameEvent;

        let mut gs = two_player_state();
        let attacker_id = triggered_attacker(
            &mut gs,
            PlayerId(0),
            3,
            3,
            vec![
                OracleSpan::Parsed(Ability::Triggered(bushido_attacker_triggered_ability(2))),
                OracleSpan::Parsed(Ability::Triggered(bushido_blocker_triggered_ability(2))),
            ],
        );
        gs.combat.attackers = vec![attacker_id];
        gs.combat.blocking_map = [(attacker_id, vec![])].into(); // unblocked — no BecomesBlocked event

        // No BecomesBlocked event is fired; an Attacks-but-no-blocks scenario.
        let triggers = collect_triggers_for_event(
            &mut gs,
            &GameEvent::Attacks {
                subject_id: attacker_id,
            },
        );
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
                        subject: crate::types::ability::TriggerSubjectFilter {
                            is_self: Some(true),
                            ..Default::default()
                        },
                    },
                    condition: None,
                    target_mode: crate::types::ability::TriggerTargetMode::None,
                    effect: vec![EffectStep::DrawCard(1)],
                })),
                OracleSpan::Parsed(Ability::Triggered(TriggeredAbility {
                    trigger: TriggerEvent::EntersTheBattlefield {
                        subject: crate::types::ability::TriggerSubjectFilter {
                            is_self: Some(true),
                            ..Default::default()
                        },
                    },
                    condition: None,
                    target_mode: crate::types::ability::TriggerTargetMode::None,
                    effect: vec![EffectStep::GainLife(2)],
                })),
            ],
            text_annotations: vec![],
            power: Some(1),
            toughness: Some(1),
            colors: vec![],
        };
        let creature_id = place_on_battlefield(&mut gs, def, PlayerId(0));

        let triggers = collect_triggers_for_event(
            &mut gs,
            &GameEvent::EntersTheBattlefield {
                subject_id: creature_id,
            },
        );

        assert_eq!(triggers.len(), 2);
        assert_ne!(triggers[0].id, triggers[1].id);
    }

    #[test]
    fn collect_ward_triggers_emits_triggered_ability_with_payment() {
        // CR 702.21a: Ward is now a TriggeredAbility dispatched via collect_triggers_for_event
        // with GameEvent::TargetedBy { target_id, acting_player }.
        use crate::types::GameEvent;
        use crate::types::OracleSpan;
        use crate::types::ability::{
            Ability, CostComponent, TriggerEvent, TriggerTargetMode, TriggeredAbility, TurnOwner,
        };
        use crate::types::card::{CardDefinition, CardType, TypeLine};
        use crate::types::effect::EffectStep;
        use crate::types::mana::{ManaCost, ManaPip};
        use crate::types::stack::{StackObject, StackPayload};

        let mut gs = two_player_state();

        // Ward cost: {2}
        let ward_cost = vec![CostComponent::Mana(ManaCost {
            pips: vec![ManaPip::Generic(2)],
        })];
        // A creature with Ward {2} as a TriggeredAbility controlled by P1
        let ward_def = CardDefinition {
            name: "Ward Creature".into(),
            mana_cost: None,
            type_line: TypeLine {
                supertypes: vec![],
                card_types: vec![CardType::Creature],
                subtypes: vec![],
            },
            oracle_text: String::new(),
            abilities: vec![OracleSpan::Parsed(Ability::Triggered(TriggeredAbility {
                trigger: TriggerEvent::TargetedBy {
                    controller: TurnOwner::Opponent,
                },
                condition: None,
                target_mode: TriggerTargetMode::None,
                effect: vec![EffectStep::Payment {
                    cost: ward_cost.clone(),
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

        // Fire TargetedBy event through the general dispatch.
        let triggers = collect_triggers_for_event(
            &mut gs,
            &GameEvent::TargetedBy {
                target_id: ward_id,
                acting_player: PlayerId(0),
            },
        );

        assert_eq!(triggers.len(), 1);
        let trigger = &triggers[0];
        assert_eq!(trigger.controller, PlayerId(1));
        // Must be TriggeredAbility with a Payment step
        let StackPayload::TriggeredAbility { effect, .. } = &trigger.payload else {
            panic!("expected TriggeredAbility, got something else");
        };
        assert_eq!(effect.len(), 1);
        assert!(matches!(&effect[0], EffectStep::Payment { .. }));
    }

    #[test]
    fn collect_targeted_by_does_not_trigger_for_own_permanent() {
        // CR 702.21a: Ward only fires when an OPPONENT targets the permanent.
        use crate::types::GameEvent;
        use crate::types::OracleSpan;
        use crate::types::ability::{
            Ability, CostComponent, TriggerEvent, TriggerTargetMode, TriggeredAbility, TurnOwner,
        };
        use crate::types::card::{CardDefinition, CardType, TypeLine};
        use crate::types::effect::EffectStep;
        use crate::types::mana::{ManaCost, ManaPip};

        let mut gs = two_player_state();

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
            abilities: vec![OracleSpan::Parsed(Ability::Triggered(TriggeredAbility {
                trigger: TriggerEvent::TargetedBy {
                    controller: TurnOwner::Opponent,
                },
                condition: None,
                target_mode: TriggerTargetMode::None,
                effect: vec![EffectStep::Payment {
                    cost: ward_cost.clone(),
                    on_paid: vec![],
                    on_declined: vec![EffectStep::CounterSpell],
                }],
            }))],
            text_annotations: vec![],
            power: Some(2),
            toughness: Some(2),
            colors: vec![],
        };
        // Ward permanent is controlled by P0.
        let ward_id = place_on_battlefield(&mut gs, ward_def, PlayerId(0));

        // P0 targets their own permanent — should NOT trigger Ward.
        let triggers = collect_triggers_for_event(
            &mut gs,
            &GameEvent::TargetedBy {
                target_id: ward_id,
                acting_player: PlayerId(0),
            },
        );
        assert!(
            triggers.is_empty(),
            "Ward should not trigger when controller targets their own permanent"
        );
    }

    #[test]
    fn collect_triggers_for_event_etb_fires_draw_trigger() {
        use crate::types::GameEvent;
        let mut gs = two_player_state();
        put_in_library(&mut gs, PlayerId(0));
        let creature_id = place_on_battlefield(&mut gs, etb_draw_def(), PlayerId(0));

        let triggers = collect_triggers_for_event(
            &mut gs,
            &GameEvent::EntersTheBattlefield {
                subject_id: creature_id,
            },
        );

        assert_eq!(triggers.len(), 1);
        use crate::types::stack::StackPayload;
        let StackPayload::TriggeredAbility {
            source_id, effect, ..
        } = &triggers[0].payload
        else {
            panic!("expected TriggeredAbility");
        };
        assert_eq!(*source_id, creature_id);
        assert_eq!(*effect, vec![EffectStep::DrawCard(1)]);
    }

    #[test]
    fn collect_triggers_for_event_etb_does_not_fire_for_other_events() {
        use crate::types::GameEvent;
        let mut gs = two_player_state();
        put_in_library(&mut gs, PlayerId(0));
        let creature_id = place_on_battlefield(&mut gs, etb_draw_def(), PlayerId(0));

        let triggers = collect_triggers_for_event(
            &mut gs,
            &GameEvent::Attacks {
                subject_id: creature_id,
            },
        );

        assert!(
            triggers.is_empty(),
            "ETB trigger should not fire on Attacks event"
        );
    }

    #[test]
    fn collect_triggers_for_event_evolve_fires_on_etb_with_greater_power() {
        use crate::types::GameEvent;
        let mut gs = two_player_state();
        let evolve_id =
            enter_creature_on_battlefield(&mut gs, PlayerId(0), 2, 2, vec![StaticAbility::Evolve]);
        let entering_id = enter_creature_on_battlefield(&mut gs, PlayerId(0), 3, 2, vec![]);

        let triggers = collect_triggers_for_event(
            &mut gs,
            &GameEvent::EntersTheBattlefield {
                subject_id: entering_id,
            },
        );

        assert_eq!(triggers.len(), 1);
        use crate::types::effect::EffectTarget;
        assert!(
            triggers[0]
                .targets
                .iter()
                .any(|t| matches!(t, EffectTarget::Object { id } if *id == evolve_id))
        );
    }

    #[test]
    fn battle_cry_boosts_other_attackers_not_self() {
        // CR 702.91b: each OTHER attacking creature gets +1/+0
        use crate::types::GameEvent;
        use crate::types::card::{CardDefinition, CardType, TypeLine};
        use crate::types::effect::EffectTarget;
        use crate::types::stack::StackPayload;

        let mut gs = two_player_state();

        let battle_cry_span =
            OracleSpan::Parsed(Ability::Triggered(battle_cry_triggered_ability()));
        let battle_cry_id = triggered_attacker(&mut gs, PlayerId(0), 2, 2, vec![battle_cry_span]);

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

        // Act: fire Attacks event for battle_cry_id (the source of Battle Cry)
        let triggers = collect_triggers_for_event(
            &mut gs,
            &GameEvent::Attacks {
                subject_id: battle_cry_id,
            },
        );

        // Assert: exactly one trigger from battle_cry_id, targeting the ally (not itself).
        // Battle Cry uses AllOtherAttackers target mode, so the single StackObject has
        // targets = [ally_id] (all attackers != source_id).
        let battle_cry_triggers: Vec<_> = triggers
            .iter()
            .filter(|t| {
                matches!(&t.payload, StackPayload::TriggeredAbility { source_id, .. } if *source_id == battle_cry_id)
            })
            .collect();
        assert_eq!(
            battle_cry_triggers.len(),
            1,
            "Battle Cry should generate exactly one boost trigger (with all other attackers as targets)"
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
