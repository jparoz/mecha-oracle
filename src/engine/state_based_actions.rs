use crate::types::{
    CounterKind, GameEvent, GameState, ObjectId, PlayerId, Zone, ability::StaticAbility,
};

/// Repeatedly finds and applies SBAs until no new ones trigger (CR 704.3).
/// Returns the updated GameState and any triggered abilities that fired (CR 603.2).
pub fn check_and_apply_sbas(
    state: GameState,
) -> (GameState, Vec<crate::types::stack::StackObject>) {
    let mut state = state;
    let mut all_triggers: Vec<crate::types::stack::StackObject> = Vec::new();
    loop {
        let sbas = find_sbas(&state);
        if sbas.is_empty() {
            break;
        }
        let (new_state, triggers) = apply_sbas(state, sbas);
        state = new_state;
        all_triggers.extend(triggers);
    }
    (state, all_triggers)
}

#[derive(Debug, Clone)]
enum Sba {
    PlayerLoses(PlayerId),
    MoveToGraveyard(ObjectId),
    CancelCounters(ObjectId, u32),
}

fn find_sbas(state: &GameState) -> Vec<Sba> {
    let mut sbas = vec![];

    // CR 704.5a: player with 0 or less life loses.
    for player in &state.players {
        if !player.has_lost && player.life <= 0 {
            sbas.push(Sba::PlayerLoses(player.id));
        }
    }

    // CR 704.5c / 122.1f: player with 10 or more poison counters loses.
    for player in &state.players {
        if !player.has_lost && player.counter_count(&CounterKind::Poison) >= 10 {
            sbas.push(Sba::PlayerLoses(player.id));
        }
    }

    // CR 704.5g: creature on battlefield with toughness ≤ 0 → graveyard.
    // CR 704.5h: creature with damage ≥ toughness → graveyard.
    // CR 704.5h (deathtouch): creature dealt any deathtouch damage → graveyard.
    // CR 702.12b: Indestructible creatures are exempt from both 704.5g and 704.5h.
    for (&id, perm) in &state.battlefield {
        if perm.is_creature() && !perm.has_keyword(StaticAbility::Indestructible) {
            let lethal_damage = perm
                .effective_toughness()
                .map(|t| t <= 0 || perm.damage_marked as i32 >= t)
                .unwrap_or(false);
            if lethal_damage || perm.damaged_by_deathtouch {
                sbas.push(Sba::MoveToGraveyard(id));
            }
        }
    }

    // CR 704.5q / 122.3: if a permanent has both +1/+1 and -1/-1 counters, remove N of each
    // where N = min of the two counts.
    let plus_key = CounterKind::PtModifier {
        power: 1,
        toughness: 1,
    };
    let minus_key = CounterKind::PtModifier {
        power: -1,
        toughness: -1,
    };
    for (&id, perm) in &state.battlefield {
        let n = perm
            .counter_count(&plus_key)
            .min(perm.counter_count(&minus_key));
        if n > 0 {
            sbas.push(Sba::CancelCounters(id, n));
        }
    }

    sbas
}

fn apply_sbas(
    mut state: GameState,
    sbas: Vec<Sba>,
) -> (GameState, Vec<crate::types::stack::StackObject>) {
    let mut triggers = Vec::new();
    for sba in sbas {
        match sba {
            Sba::PlayerLoses(pid) => {
                if let Some(p) = state.get_player_mut(pid) {
                    p.has_lost = true;
                }
                state.game_over = true;
            }
            Sba::MoveToGraveyard(id) => {
                // CR 603.10a: collect Dies triggers before the zone change so that
                // sources on the battlefield (including the dying creature itself,
                // per LKI) are still visible to the trigger collector.
                let mut t = crate::engine::triggered::collect_triggers_for_event(
                    &mut state,
                    &GameEvent::Dies { subject_id: id },
                );
                state = move_to_graveyard(state, id);
                triggers.append(&mut t);
            }
            Sba::CancelCounters(id, n) => {
                if let Some(perm) = state.battlefield.get_mut(&id) {
                    perm.remove_counters(
                        &CounterKind::PtModifier {
                            power: 1,
                            toughness: 1,
                        },
                        n,
                    );
                    perm.remove_counters(
                        &CounterKind::PtModifier {
                            power: -1,
                            toughness: -1,
                        },
                        n,
                    );
                }
            }
        }
    }
    (state, triggers)
}

pub fn move_to_graveyard(mut state: GameState, object_id: ObjectId) -> GameState {
    state.battlefield.remove(&object_id);
    if let Some(obj) = state.objects.get_mut(&object_id) {
        let owner = obj.owner;
        obj.zone = Zone::Graveyard;
        if let Some(gy) = state.graveyards.get_mut(&owner) {
            gy.push(object_id);
        }
    }
    state
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cards::test_helpers::test_db;
    use crate::types::{CardObject, PermanentState, Player, Zone};

    fn make_state() -> GameState {
        GameState::new(vec![
            Player::new(PlayerId(0), "Alice"),
            Player::new(PlayerId(1), "Bob"),
        ])
    }

    fn add_creature_to_battlefield(
        state: &mut GameState,
        owner: PlayerId,
        def: crate::types::CardDefinition,
    ) -> ObjectId {
        let id = state.alloc_id();
        let obj = CardObject::new(id, def, owner, Zone::Battlefield);
        let mut perm = PermanentState::new(&obj.definition);
        perm.controller_since_turn = 0;
        state.battlefield.insert(id, perm);
        state.add_object(obj);
        id
    }

    #[test]
    fn creature_with_lethal_damage_goes_to_graveyard() {
        let db = test_db();
        let mut gs = make_state();
        let bear_id = add_creature_to_battlefield(
            &mut gs,
            PlayerId(0),
            db.get("Grizzly Bears").unwrap().clone(),
        );
        gs.battlefield.get_mut(&bear_id).unwrap().damage_marked = 2; // toughness = 2, lethal

        let (gs, _) = check_and_apply_sbas(gs);

        assert!(!gs.battlefield.contains_key(&bear_id));
        assert!(gs.graveyards[&PlayerId(0)].contains(&bear_id));
        assert_eq!(gs.objects[&bear_id].zone, Zone::Graveyard);
    }

    #[test]
    fn creature_below_lethal_damage_survives() {
        let db = test_db();
        let mut gs = make_state();
        let bear_id = add_creature_to_battlefield(
            &mut gs,
            PlayerId(0),
            db.get("Grizzly Bears").unwrap().clone(),
        );
        gs.battlefield.get_mut(&bear_id).unwrap().damage_marked = 1; // toughness = 2, survives

        let (gs, _) = check_and_apply_sbas(gs);

        assert!(gs.battlefield.contains_key(&bear_id));
    }

    #[test]
    fn player_at_zero_life_loses() {
        let mut gs = make_state();
        gs.get_player_mut(PlayerId(1)).unwrap().life = 0;

        let (gs, _) = check_and_apply_sbas(gs);

        assert!(gs.is_game_over());
        assert_eq!(gs.winner(), Some(PlayerId(0)));
    }

    #[test]
    fn player_at_negative_life_loses() {
        let mut gs = make_state();
        gs.get_player_mut(PlayerId(0)).unwrap().life = -3;

        let (gs, _) = check_and_apply_sbas(gs);

        assert_eq!(gs.winner(), Some(PlayerId(1)));
    }

    fn keyword_creature_on_battlefield(
        state: &mut GameState,
        owner: PlayerId,
        power: i32,
        toughness: i32,
        keywords: Vec<crate::types::ability::StaticAbility>,
    ) -> ObjectId {
        use crate::types::{CardDefinition, CardType, Rule, RulesText, TypeLine};
        let id = state.alloc_id();
        let def = CardDefinition {
            name: "Test".into(),
            mana_cost: None,
            type_line: TypeLine {
                supertypes: vec![],
                card_types: vec![CardType::Creature],
                subtypes: vec![],
            },
            oracle_text: String::new(),
            rules_text: keywords
                .into_iter()
                .map(|k| RulesText::Active(Rule::Static(k)))
                .collect(),
            text_annotations: vec![],
            power: Some(power),
            toughness: Some(toughness),
            colors: vec![],
        };
        let obj = CardObject::new(id, def, owner, Zone::Battlefield);
        let mut perm = PermanentState::new(&obj.definition);
        perm.controller_since_turn = 0;
        state.battlefield.insert(id, perm);
        state.add_object(obj);
        id
    }

    #[test]
    fn indestructible_survives_lethal_damage() {
        use crate::types::ability::StaticAbility;
        let mut gs = make_state();
        let id = keyword_creature_on_battlefield(
            &mut gs,
            PlayerId(0),
            2,
            2,
            vec![StaticAbility::Indestructible],
        );
        gs.battlefield.get_mut(&id).unwrap().damage_marked = 5; // more than toughness

        let (gs, _) = check_and_apply_sbas(gs);

        assert!(gs.battlefield.contains_key(&id)); // survives
    }

    #[test]
    fn deathtouch_damage_kills_non_indestructible_creature() {
        let mut gs = make_state();
        let db = test_db();
        let id = add_creature_to_battlefield(
            &mut gs,
            PlayerId(0),
            db.get("Hill Giant").unwrap().clone(),
        );
        gs.battlefield.get_mut(&id).unwrap().damaged_by_deathtouch = true;

        let (gs, _) = check_and_apply_sbas(gs);

        assert!(!gs.battlefield.contains_key(&id));
    }

    #[test]
    fn indestructible_survives_deathtouch_damage() {
        use crate::types::ability::StaticAbility;
        let mut gs = make_state();
        let id = keyword_creature_on_battlefield(
            &mut gs,
            PlayerId(0),
            2,
            2,
            vec![StaticAbility::Indestructible],
        );
        gs.battlefield.get_mut(&id).unwrap().damaged_by_deathtouch = true;

        let (gs, _) = check_and_apply_sbas(gs);

        assert!(gs.battlefield.contains_key(&id)); // indestructible ignores both 704.5g and 704.5h
    }

    #[test]
    fn multiple_dying_creatures_all_go_to_graveyard() {
        let db = test_db();
        let mut gs = make_state();
        let bear1 = add_creature_to_battlefield(
            &mut gs,
            PlayerId(0),
            db.get("Grizzly Bears").unwrap().clone(),
        );
        let bear2 = add_creature_to_battlefield(
            &mut gs,
            PlayerId(0),
            db.get("Grizzly Bears").unwrap().clone(),
        );
        gs.battlefield.get_mut(&bear1).unwrap().damage_marked = 5;
        gs.battlefield.get_mut(&bear2).unwrap().damage_marked = 5;

        let (gs, _) = check_and_apply_sbas(gs);

        assert!(gs.battlefield.is_empty());
        assert_eq!(gs.graveyards[&PlayerId(0)].len(), 2);
    }

    #[test]
    fn sba_cancels_equal_plus_and_minus_one_counters() {
        use crate::types::CounterKind;
        let db = test_db();
        let mut gs = make_state();
        let id = add_creature_to_battlefield(
            &mut gs,
            PlayerId(0),
            db.get("Grizzly Bears").unwrap().clone(),
        );
        gs.battlefield.get_mut(&id).unwrap().add_counters(
            CounterKind::PtModifier {
                power: 1,
                toughness: 1,
            },
            3,
        );
        gs.battlefield.get_mut(&id).unwrap().add_counters(
            CounterKind::PtModifier {
                power: -1,
                toughness: -1,
            },
            3,
        );

        let (gs, _) = check_and_apply_sbas(gs);

        assert_eq!(
            gs.battlefield[&id].counter_count(&CounterKind::PtModifier {
                power: 1,
                toughness: 1
            }),
            0
        );
        assert_eq!(
            gs.battlefield[&id].counter_count(&CounterKind::PtModifier {
                power: -1,
                toughness: -1
            }),
            0
        );
    }

    #[test]
    fn sba_removes_min_of_unequal_counter_counts() {
        use crate::types::CounterKind;
        let db = test_db();
        let mut gs = make_state();
        let id = add_creature_to_battlefield(
            &mut gs,
            PlayerId(0),
            db.get("Grizzly Bears").unwrap().clone(),
        );
        gs.battlefield.get_mut(&id).unwrap().add_counters(
            CounterKind::PtModifier {
                power: 1,
                toughness: 1,
            },
            5,
        );
        gs.battlefield.get_mut(&id).unwrap().add_counters(
            CounterKind::PtModifier {
                power: -1,
                toughness: -1,
            },
            2,
        );

        let (gs, _) = check_and_apply_sbas(gs);

        assert_eq!(
            gs.battlefield[&id].counter_count(&CounterKind::PtModifier {
                power: 1,
                toughness: 1
            }),
            3
        );
        assert_eq!(
            gs.battlefield[&id].counter_count(&CounterKind::PtModifier {
                power: -1,
                toughness: -1
            }),
            0
        );
    }

    #[test]
    fn sba_does_not_cancel_mismatched_pt_modifier_counters() {
        // CR 122.3 only cancels +1/+1 against -1/-1; other PtModifier pairs are unaffected.
        use crate::types::CounterKind;
        let db = test_db();
        let mut gs = make_state();
        let id = add_creature_to_battlefield(
            &mut gs,
            PlayerId(0),
            db.get("Grizzly Bears").unwrap().clone(),
        );
        gs.battlefield.get_mut(&id).unwrap().add_counters(
            CounterKind::PtModifier {
                power: 2,
                toughness: 0,
            },
            1,
        );
        gs.battlefield.get_mut(&id).unwrap().add_counters(
            CounterKind::PtModifier {
                power: -1,
                toughness: -1,
            },
            1,
        );

        let (gs, _) = check_and_apply_sbas(gs);

        assert_eq!(
            gs.battlefield[&id].counter_count(&CounterKind::PtModifier {
                power: 2,
                toughness: 0
            }),
            1
        );
        assert_eq!(
            gs.battlefield[&id].counter_count(&CounterKind::PtModifier {
                power: -1,
                toughness: -1
            }),
            1
        );
    }

    #[test]
    fn ten_poison_counters_causes_loss() {
        use crate::types::CounterKind;
        let mut gs = make_state();
        gs.get_player_mut(PlayerId(1))
            .unwrap()
            .add_counters(CounterKind::Poison, 10);

        let (gs, _) = check_and_apply_sbas(gs);

        assert!(gs.is_game_over());
        assert_eq!(gs.winner(), Some(PlayerId(0)));
    }

    #[test]
    fn sba_counter_cancellation_and_toughness_zero_chain() {
        // A 1/1 with one +1/+1 and two -1/-1 counters:
        // First SBA loop: cancel one of each → 0 +1/+1, one -1/-1 remain → effective toughness 0.
        // Second SBA loop: toughness ≤ 0 → move to graveyard.
        use crate::types::CounterKind;
        let mut gs = make_state();
        let id = keyword_creature_on_battlefield(
            &mut gs,
            PlayerId(0),
            1, // power
            1, // toughness
            vec![],
        );
        gs.battlefield.get_mut(&id).unwrap().add_counters(
            CounterKind::PtModifier {
                power: 1,
                toughness: 1,
            },
            1,
        );
        gs.battlefield.get_mut(&id).unwrap().add_counters(
            CounterKind::PtModifier {
                power: -1,
                toughness: -1,
            },
            2,
        );

        let (gs, _) = check_and_apply_sbas(gs);

        // Should be in graveyard — died from effective toughness 0 after counter cancellation.
        assert!(!gs.battlefield.contains_key(&id));
        assert!(gs.graveyards[&PlayerId(0)].contains(&id));
    }

    #[test]
    fn nine_poison_counters_does_not_cause_loss() {
        use crate::types::CounterKind;
        let mut gs = make_state();
        gs.get_player_mut(PlayerId(1))
            .unwrap()
            .add_counters(CounterKind::Poison, 9);

        let (gs, _) = check_and_apply_sbas(gs);

        assert!(!gs.is_game_over());
    }

    #[test]
    fn check_and_apply_sbas_returns_dies_trigger_when_creature_dies() {
        use crate::types::RulesText;
        use crate::types::ability::{
            Rule, TriggerEvent, TriggerSubjectFilter, TriggerTargetMode, TriggeredAbility,
        };
        use crate::types::card::{CardDefinition, CardType, TypeLine};
        use crate::types::effect::EffectStep;

        let mut state = make_state();

        // A permanent with "when this dies, draw a card"
        let watcher_def = CardDefinition {
            name: "Doomed Watcher".into(),
            mana_cost: None,
            type_line: TypeLine {
                supertypes: vec![],
                card_types: vec![CardType::Creature],
                subtypes: vec![],
            },
            oracle_text: String::new(),
            rules_text: vec![RulesText::Active(Rule::Triggered(TriggeredAbility {
                trigger: TriggerEvent::Dies {
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
        let watcher_id = add_creature_to_battlefield(&mut state, PlayerId(0), watcher_def);
        // Mark it as having lethal damage so SBA kills it.
        state
            .battlefield
            .get_mut(&watcher_id)
            .unwrap()
            .damage_marked = 99;

        let (new_state, triggers) = check_and_apply_sbas(state);

        assert!(
            !new_state.battlefield.contains_key(&watcher_id),
            "creature should be dead"
        );
        assert_eq!(triggers.len(), 1, "should have one Dies trigger");
    }

    #[test]
    fn persist_creature_returns_with_minus_counter_after_dying() {
        // CR 702.79: a 2/2 Persist creature (no -1/-1 counters) dies and returns
        // to the battlefield under its owner's control with a -1/-1 counter.
        use crate::types::ability::{Rule, StaticAbility};
        use crate::types::mana::ManaCost;
        use crate::types::{CardDefinition, CardType, CounterKind, RulesText, TypeLine, Zone};

        let mut state = make_state();

        let def = CardDefinition {
            name: "Young Wolf".into(),
            mana_cost: Some(ManaCost { pips: vec![] }),
            type_line: TypeLine {
                supertypes: vec![],
                card_types: vec![CardType::Creature],
                subtypes: vec!["Wolf".into()],
            },
            oracle_text: "Persist".into(),
            rules_text: vec![RulesText::Active(Rule::Static(StaticAbility::Persist))],
            text_annotations: vec![],
            power: Some(2),
            toughness: Some(2),
            colors: vec![],
        };
        let id = add_creature_to_battlefield(&mut state, PlayerId(0), def);
        state.battlefield.get_mut(&id).unwrap().damage_marked = 5;

        // SBA: dies, Persist trigger returned.
        let (mut state, triggers) = check_and_apply_sbas(state);
        assert!(
            !state.battlefield.contains_key(&id),
            "creature should have died"
        );
        assert!(state.graveyards[&PlayerId(0)].contains(&id));
        assert_eq!(triggers.len(), 1, "exactly one Persist trigger");

        // Push trigger onto the stack.
        for t in triggers {
            let tid = t.id;
            state.stack.push(tid);
            state.stack_objects.insert(tid, t);
        }

        // Resolve the trigger: MoveZone (gy→bf) + AddCounter (-1/-1).
        let state = crate::engine::stack::resolve_top(state);

        assert!(
            state.battlefield.contains_key(&id),
            "creature should be back on battlefield"
        );
        assert_eq!(state.objects[&id].zone, Zone::Battlefield);
        assert!(!state.graveyards[&PlayerId(0)].contains(&id));
        assert_eq!(
            state.battlefield[&id].counter_count(&CounterKind::PtModifier {
                power: -1,
                toughness: -1
            }),
            1,
            "Persist creature should have exactly one -1/-1 counter"
        );
        assert_eq!(state.objects[&id].controller, PlayerId(0));
    }

    #[test]
    fn persist_does_not_trigger_when_minus_counter_present() {
        // CR 702.79: a Persist creature that already has a -1/-1 counter dies permanently.
        use crate::types::ability::{Rule, StaticAbility};
        use crate::types::mana::ManaCost;
        use crate::types::{CardDefinition, CardType, CounterKind, RulesText, TypeLine};

        let mut state = make_state();

        let def = CardDefinition {
            name: "Young Wolf".into(),
            mana_cost: Some(ManaCost { pips: vec![] }),
            type_line: TypeLine {
                supertypes: vec![],
                card_types: vec![CardType::Creature],
                subtypes: vec![],
            },
            oracle_text: "Persist".into(),
            rules_text: vec![RulesText::Active(Rule::Static(StaticAbility::Persist))],
            text_annotations: vec![],
            power: Some(2),
            toughness: Some(2),
            colors: vec![],
        };
        let id = add_creature_to_battlefield(&mut state, PlayerId(0), def);
        state.battlefield.get_mut(&id).unwrap().add_counters(
            CounterKind::PtModifier {
                power: -1,
                toughness: -1,
            },
            1,
        );
        state.battlefield.get_mut(&id).unwrap().damage_marked = 5;

        let (state, triggers) = check_and_apply_sbas(state);

        assert!(!state.battlefield.contains_key(&id));
        assert!(state.graveyards[&PlayerId(0)].contains(&id));
        assert!(
            triggers.is_empty(),
            "Persist must not trigger when -1/-1 counter present"
        );
    }

    #[test]
    fn undying_creature_returns_with_plus_counter_after_dying() {
        // CR 702.93: a 2/1 Undying creature (no +1/+1 counters) dies and returns
        // under its owner's control with a +1/+1 counter (becomes 3/2).
        use crate::types::ability::{Rule, StaticAbility};
        use crate::types::mana::ManaCost;
        use crate::types::{CardDefinition, CardType, CounterKind, RulesText, TypeLine, Zone};

        let mut state = make_state();

        let def = CardDefinition {
            name: "Strangleroot Geist".into(),
            mana_cost: Some(ManaCost { pips: vec![] }),
            type_line: TypeLine {
                supertypes: vec![],
                card_types: vec![CardType::Creature],
                subtypes: vec!["Spirit".into()],
            },
            oracle_text: "Undying".into(),
            rules_text: vec![RulesText::Active(Rule::Static(StaticAbility::Undying))],
            text_annotations: vec![],
            power: Some(2),
            toughness: Some(1),
            colors: vec![],
        };
        let id = add_creature_to_battlefield(&mut state, PlayerId(0), def);
        state.battlefield.get_mut(&id).unwrap().damage_marked = 5;

        let (mut state, triggers) = check_and_apply_sbas(state);
        assert!(!state.battlefield.contains_key(&id));
        assert!(state.graveyards[&PlayerId(0)].contains(&id));
        assert_eq!(triggers.len(), 1, "exactly one Undying trigger");

        for t in triggers {
            let tid = t.id;
            state.stack.push(tid);
            state.stack_objects.insert(tid, t);
        }

        let state = crate::engine::stack::resolve_top(state);

        assert!(
            state.battlefield.contains_key(&id),
            "creature should be back on battlefield"
        );
        assert_eq!(state.objects[&id].zone, Zone::Battlefield);
        assert!(!state.graveyards[&PlayerId(0)].contains(&id));
        assert_eq!(
            state.battlefield[&id].counter_count(&CounterKind::PtModifier {
                power: 1,
                toughness: 1
            }),
            1,
            "Undying creature should have exactly one +1/+1 counter"
        );
        assert_eq!(state.objects[&id].controller, PlayerId(0));
    }

    #[test]
    fn undying_does_not_trigger_when_plus_counter_present() {
        // CR 702.93: an Undying creature that already has a +1/+1 counter dies permanently.
        use crate::types::ability::{Rule, StaticAbility};
        use crate::types::mana::ManaCost;
        use crate::types::{CardDefinition, CardType, CounterKind, RulesText, TypeLine};

        let mut state = make_state();

        let def = CardDefinition {
            name: "Strangleroot Geist".into(),
            mana_cost: Some(ManaCost { pips: vec![] }),
            type_line: TypeLine {
                supertypes: vec![],
                card_types: vec![CardType::Creature],
                subtypes: vec![],
            },
            oracle_text: "Undying".into(),
            rules_text: vec![RulesText::Active(Rule::Static(StaticAbility::Undying))],
            text_annotations: vec![],
            power: Some(2),
            toughness: Some(1),
            colors: vec![],
        };
        let id = add_creature_to_battlefield(&mut state, PlayerId(0), def);
        state.battlefield.get_mut(&id).unwrap().add_counters(
            CounterKind::PtModifier {
                power: 1,
                toughness: 1,
            },
            1,
        );
        state.battlefield.get_mut(&id).unwrap().damage_marked = 5;

        let (state, triggers) = check_and_apply_sbas(state);

        assert!(!state.battlefield.contains_key(&id));
        assert!(state.graveyards[&PlayerId(0)].contains(&id));
        assert!(
            triggers.is_empty(),
            "Undying must not trigger when +1/+1 counter present"
        );
    }

    #[test]
    fn check_and_apply_sbas_no_dies_trigger_when_creature_survives() {
        use crate::types::RulesText;
        use crate::types::ability::{
            Rule, TriggerEvent, TriggerSubjectFilter, TriggerTargetMode, TriggeredAbility,
        };
        use crate::types::card::{CardDefinition, CardType, TypeLine};
        use crate::types::effect::EffectStep;

        let mut state = make_state();

        // A permanent with "when this dies, draw a card"
        let survivor_def = CardDefinition {
            name: "Survivor".into(),
            mana_cost: None,
            type_line: TypeLine {
                supertypes: vec![],
                card_types: vec![CardType::Creature],
                subtypes: vec![],
            },
            oracle_text: String::new(),
            rules_text: vec![RulesText::Active(Rule::Triggered(TriggeredAbility {
                trigger: TriggerEvent::Dies {
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
            power: Some(2),
            toughness: Some(2),
            colors: vec![],
        };
        let survivor_id = add_creature_to_battlefield(&mut state, PlayerId(0), survivor_def);
        // Mark it as having 1 damage (below lethal for a 2/2, so it survives)
        state
            .battlefield
            .get_mut(&survivor_id)
            .unwrap()
            .damage_marked = 1;

        let (new_state, triggers) = check_and_apply_sbas(state);

        assert!(
            new_state.battlefield.contains_key(&survivor_id),
            "creature should survive"
        );
        assert!(
            triggers.is_empty(),
            "no Dies trigger should fire when creature survives"
        );
    }
}
