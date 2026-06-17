use crate::types::{CounterKind, GameState, ObjectId, PlayerId, Zone, ability::StaticAbility};

/// Repeatedly finds and applies SBAs until no new ones trigger (CR 704.3).
pub fn check_and_apply_sbas(state: GameState) -> GameState {
    let mut state = state;
    loop {
        let sbas = find_sbas(&state);
        if sbas.is_empty() {
            break;
        }
        state = apply_sbas(state, sbas);
    }
    state
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

    // CR 122.3: if a permanent has both +1/+1 and -1/-1 counters, remove N of each
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

fn apply_sbas(mut state: GameState, sbas: Vec<Sba>) -> GameState {
    for sba in sbas {
        match sba {
            Sba::PlayerLoses(pid) => {
                if let Some(p) = state.get_player_mut(pid) {
                    p.has_lost = true;
                }
                state.game_over = true;
            }
            Sba::MoveToGraveyard(id) => {
                state = move_to_graveyard(state, id);
            }
            Sba::CancelCounters(id, n) => {
                if let Some(perm) = state.battlefield.get_mut(&id) {
                    let plus_key = CounterKind::PtModifier {
                        power: 1,
                        toughness: 1,
                    };
                    let minus_key = CounterKind::PtModifier {
                        power: -1,
                        toughness: -1,
                    };
                    for key in [plus_key, minus_key] {
                        let new_val = perm.counter_count(&key).saturating_sub(n);
                        if new_val == 0 {
                            perm.counters.remove(&key);
                        } else {
                            perm.counters.insert(key, new_val);
                        }
                    }
                }
            }
        }
    }
    state
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

        let gs = check_and_apply_sbas(gs);

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

        let gs = check_and_apply_sbas(gs);

        assert!(gs.battlefield.contains_key(&bear_id));
    }

    #[test]
    fn player_at_zero_life_loses() {
        let mut gs = make_state();
        gs.get_player_mut(PlayerId(1)).unwrap().life = 0;

        let gs = check_and_apply_sbas(gs);

        assert!(gs.is_game_over());
        assert_eq!(gs.winner(), Some(PlayerId(0)));
    }

    #[test]
    fn player_at_negative_life_loses() {
        let mut gs = make_state();
        gs.get_player_mut(PlayerId(0)).unwrap().life = -3;

        let gs = check_and_apply_sbas(gs);

        assert_eq!(gs.winner(), Some(PlayerId(1)));
    }

    fn keyword_creature_on_battlefield(
        state: &mut GameState,
        owner: PlayerId,
        power: i32,
        toughness: i32,
        keywords: Vec<crate::types::ability::StaticAbility>,
    ) -> ObjectId {
        use crate::types::{Ability, CardDefinition, CardType, OracleSpan, TypeLine};
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
            abilities: keywords
                .into_iter()
                .map(|k| OracleSpan::Parsed(Ability::Static(k)))
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

        let gs = check_and_apply_sbas(gs);

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

        let gs = check_and_apply_sbas(gs);

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

        let gs = check_and_apply_sbas(gs);

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

        let gs = check_and_apply_sbas(gs);

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

        let gs = check_and_apply_sbas(gs);

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

        let gs = check_and_apply_sbas(gs);

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

        let gs = check_and_apply_sbas(gs);

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
}
