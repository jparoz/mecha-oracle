use super::{EngineError, state_based_actions::check_and_apply_sbas};
use crate::types::ability::StaticAbility;
use crate::types::{GameState, ObjectId, PlayerId, Step};
use std::collections::HashMap;

/// Declare attackers: tap them and record in CombatState (CR 508).
pub fn declare_attackers(
    mut state: GameState,
    player_id: PlayerId,
    attacker_ids: &[ObjectId],
) -> Result<GameState, EngineError> {
    if state.active_player != player_id {
        return Err(EngineError::CannotCastNow);
    }
    if state.step != Step::DeclareAttackers {
        return Err(EngineError::CannotCastNow);
    }

    for &id in attacker_ids {
        let obj = state.objects.get(&id).ok_or(EngineError::CardNotFound)?;
        if obj.controller != player_id {
            return Err(EngineError::NotYourCard);
        }
        if obj.summoning_sick && !obj.has_keyword(StaticAbility::Haste) {
            return Err(EngineError::SummoningSick);
        }
        if obj.tapped {
            return Err(EngineError::CreatureTapped);
        }
        if !obj.is_creature() {
            return Err(EngineError::NotACreature);
        }
    }

    for &id in attacker_ids {
        if !state
            .objects
            .get(&id)
            .unwrap()
            .has_keyword(StaticAbility::Vigilance)
        {
            state.objects.get_mut(&id).unwrap().tapped = true;
        }
    }
    state.combat.attackers = attacker_ids.to_vec();
    state.combat.blocking_map = attacker_ids.iter().map(|&id| (id, vec![])).collect();

    Ok(state)
}

/// Declare blockers. `blocks` is a list of (blocker_id, attacker_id) pairs (CR 509).
/// The declaration order determines the damage-assignment order for each attacker.
pub fn declare_blockers(
    mut state: GameState,
    player_id: PlayerId,
    blocks: &[(ObjectId, ObjectId)],
) -> Result<GameState, EngineError> {
    let defending_player = state.opponent_of(state.active_player);
    if player_id != defending_player {
        return Err(EngineError::CannotCastNow);
    }
    if state.step != Step::DeclareBlockers {
        return Err(EngineError::CannotCastNow);
    }

    for &(blocker_id, attacker_id) in blocks {
        let obj = state
            .objects
            .get(&blocker_id)
            .ok_or(EngineError::CardNotFound)?;
        if obj.controller != player_id {
            return Err(EngineError::NotYourCard);
        }
        if obj.tapped {
            return Err(EngineError::CreatureTapped);
        }
        if !obj.is_creature() {
            return Err(EngineError::NotACreature);
        }
        if !state.combat.attackers.contains(&attacker_id) {
            return Err(EngineError::CannotCastNow);
        }
        // CR 702.9b: a creature with flying can only be blocked by creatures with flying or reach.
        if state
            .objects
            .get(&attacker_id)
            .map(|a| a.has_keyword(StaticAbility::Flying))
            .unwrap_or(false)
        {
            if !obj.has_keyword(StaticAbility::Flying) && !obj.has_keyword(StaticAbility::Reach) {
                return Err(EngineError::InvalidBlocker);
            }
        }
    }

    // Re-build blocking_map from declarations; declaration order = damage assignment order.
    for attackers_blockers in state.combat.blocking_map.values_mut() {
        attackers_blockers.clear();
    }
    for &(blocker_id, attacker_id) in blocks {
        state
            .combat
            .blocking_map
            .entry(attacker_id)
            .or_default()
            .push(blocker_id);
    }

    // CR 702.111b: a creature with menace can't be blocked by exactly one creature.
    for &attacker_id in &state.combat.attackers {
        if state
            .objects
            .get(&attacker_id)
            .map(|a| a.has_keyword(StaticAbility::Menace))
            .unwrap_or(false)
        {
            let num_blockers = state
                .combat
                .blocking_map
                .get(&attacker_id)
                .map(|v| v.len())
                .unwrap_or(0);
            if num_blockers == 1 {
                return Err(EngineError::MenaceRequiresTwoBlockers);
            }
        }
    }

    Ok(state)
}

/// Deal combat damage simultaneously (CR 510).
/// Each attacking creature deals damage equal to power.
/// Each blocking creature deals damage equal to power to the attacker.
/// Multiple blockers: attacker assigns at least lethal to each before moving to next.
pub fn deal_combat_damage(mut state: GameState) -> GameState {
    let defending_player = state.opponent_of(state.active_player);
    let attackers = state.combat.attackers.clone();
    let blocking_map = state.combat.blocking_map.clone();

    let mut damage_to_players: HashMap<PlayerId, i32> = HashMap::new();
    let mut damage_to_objects: HashMap<ObjectId, u32> = HashMap::new();

    for &attacker_id in &attackers {
        let attacker_power = state
            .objects
            .get(&attacker_id)
            .and_then(|o| o.effective_power())
            .map(|p| p.max(0) as u32)
            .unwrap_or(0);

        let blockers = blocking_map.get(&attacker_id).cloned().unwrap_or_default();

        if blockers.is_empty() {
            *damage_to_players.entry(defending_player).or_insert(0) += attacker_power as i32;
        } else {
            // Assign damage in order: at least lethal to each before the next (CR 510.1c).
            let mut remaining = attacker_power;
            for (i, &blocker_id) in blockers.iter().enumerate() {
                if remaining == 0 {
                    break;
                }
                let is_last = i == blockers.len() - 1;
                let assign = if is_last {
                    remaining
                } else {
                    let toughness = state
                        .objects
                        .get(&blocker_id)
                        .and_then(|o| o.effective_toughness())
                        .map(|t| t.max(0) as u32)
                        .unwrap_or(0);
                    // Must assign lethal; if we can't reach lethal, assign all.
                    remaining.min(toughness.max(1))
                };
                *damage_to_objects.entry(blocker_id).or_insert(0) += assign;
                remaining -= assign;
            }
        }

        // Every blocker deals its power to the attacker.
        for &blocker_id in &blockers {
            let blocker_power = state
                .objects
                .get(&blocker_id)
                .and_then(|o| o.effective_power())
                .map(|p| p.max(0) as u32)
                .unwrap_or(0);
            *damage_to_objects.entry(attacker_id).or_insert(0) += blocker_power;
        }
    }

    // Apply all damage simultaneously.
    for (pid, dmg) in damage_to_players {
        if let Some(p) = state.get_player_mut(pid) {
            p.life -= dmg;
        }
    }
    for (oid, dmg) in damage_to_objects {
        if let Some(obj) = state.objects.get_mut(&oid) {
            obj.damage_marked += dmg;
        }
    }

    check_and_apply_sbas(state)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cards::test_helpers::test_db;
    use crate::types::{CardObject, Player, Zone};

    fn make_combat_state() -> GameState {
        let mut gs = GameState::new(vec![
            Player::new(PlayerId(0), "Alice"),
            Player::new(PlayerId(1), "Bob"),
        ]);
        gs.step = Step::DeclareAttackers;
        gs
    }

    fn add_creature(
        state: &mut GameState,
        owner: PlayerId,
        def: crate::types::CardDefinition,
    ) -> ObjectId {
        let id = state.alloc_id();
        let mut obj = CardObject::new(id, def, owner, Zone::Battlefield);
        obj.summoning_sick = false;
        state.battlefield.push(id);
        state.add_object(obj);
        id
    }

    fn keyword_creature(
        state: &mut GameState,
        owner: PlayerId,
        power: i32,
        toughness: i32,
        keywords: Vec<crate::types::ability::StaticAbility>,
    ) -> ObjectId {
        use crate::types::{
            AbilityAST, CardDefinition,
            card::{CardType, TypeLine},
        };
        let id = state.alloc_id();
        let def = CardDefinition {
            name: "Test Creature".into(),
            mana_cost: None,
            type_line: TypeLine {
                supertypes: vec![],
                card_types: vec![CardType::Creature],
                subtypes: vec![],
            },
            oracle_text: String::new(),
            abilities: keywords
                .into_iter()
                .map(|k| AbilityAST::Static(k))
                .collect(),
            power: Some(power),
            toughness: Some(toughness),
        };
        let mut obj = crate::types::CardObject::new(id, def, owner, Zone::Battlefield);
        obj.summoning_sick = false;
        state.battlefield.push(id);
        state.add_object(obj);
        id
    }

    #[test]
    fn vigilant_attacker_does_not_tap() {
        use crate::types::ability::StaticAbility;
        let mut gs = make_combat_state();
        let id = keyword_creature(&mut gs, PlayerId(0), 2, 2, vec![StaticAbility::Vigilance]);
        let gs = declare_attackers(gs, PlayerId(0), &[id]).unwrap();
        assert!(!gs.objects[&id].tapped); // vigilance: does not tap when attacking
    }

    #[test]
    fn haste_creature_can_attack_while_summoning_sick() {
        use crate::types::ability::StaticAbility;
        let mut gs = make_combat_state();
        let id = keyword_creature(&mut gs, PlayerId(0), 2, 2, vec![StaticAbility::Haste]);
        gs.objects.get_mut(&id).unwrap().summoning_sick = true; // still sick
        // Should be able to declare it as attacker
        let gs = declare_attackers(gs, PlayerId(0), &[id]).unwrap();
        assert!(gs.combat.attackers.contains(&id));
    }

    #[test]
    fn unblocked_attacker_deals_damage_to_player() {
        let db = test_db();
        let mut gs = make_combat_state();
        let bear_id = add_creature(
            &mut gs,
            PlayerId(0),
            db.get("Grizzly Bears").unwrap().clone(),
        );
        gs = declare_attackers(gs, PlayerId(0), &[bear_id]).unwrap();
        gs.step = Step::DeclareBlockers;
        gs = declare_blockers(gs, PlayerId(1), &[]).unwrap();
        gs.step = Step::CombatDamage;

        let gs = deal_combat_damage(gs);

        assert_eq!(gs.get_player(PlayerId(1)).unwrap().life, 18); // 20 - 2
    }

    #[test]
    fn blocked_creatures_deal_damage_to_each_other() {
        let db = test_db();
        let mut gs = make_combat_state();
        let attacker = add_creature(
            &mut gs,
            PlayerId(0),
            db.get("Grizzly Bears").unwrap().clone(),
        ); // 2/2
        let blocker = add_creature(
            &mut gs,
            PlayerId(1),
            db.get("Grizzly Bears").unwrap().clone(),
        ); // 2/2
        gs = declare_attackers(gs, PlayerId(0), &[attacker]).unwrap();
        gs.step = Step::DeclareBlockers;
        gs = declare_blockers(gs, PlayerId(1), &[(blocker, attacker)]).unwrap();
        gs.step = Step::CombatDamage;

        let gs = deal_combat_damage(gs);

        // Both take lethal damage and go to graveyard.
        assert!(!gs.battlefield.contains(&attacker));
        assert!(!gs.battlefield.contains(&blocker));
        assert_eq!(gs.get_player(PlayerId(1)).unwrap().life, 20); // no damage to player
    }

    #[test]
    fn larger_creature_kills_smaller_and_survives() {
        let db = test_db();
        let mut gs = make_combat_state();
        let giant = add_creature(&mut gs, PlayerId(0), db.get("Hill Giant").unwrap().clone()); // 3/3
        let bear = add_creature(
            &mut gs,
            PlayerId(1),
            db.get("Grizzly Bears").unwrap().clone(),
        ); // 2/2
        gs = declare_attackers(gs, PlayerId(0), &[giant]).unwrap();
        gs.step = Step::DeclareBlockers;
        gs = declare_blockers(gs, PlayerId(1), &[(bear, giant)]).unwrap();
        gs.step = Step::CombatDamage;

        let gs = deal_combat_damage(gs);

        assert!(gs.battlefield.contains(&giant)); // 3/3 survives 2 damage
        assert!(!gs.battlefield.contains(&bear)); // 2/2 dies to 3 damage
        assert_eq!(gs.objects[&giant].damage_marked, 2);
    }

    #[test]
    fn summoning_sick_creature_cannot_attack() {
        let db = test_db();
        let mut gs = make_combat_state();
        let bear_id = add_creature(
            &mut gs,
            PlayerId(0),
            db.get("Grizzly Bears").unwrap().clone(),
        );
        gs.objects.get_mut(&bear_id).unwrap().summoning_sick = true;

        assert!(matches!(
            declare_attackers(gs, PlayerId(0), &[bear_id]),
            Err(EngineError::SummoningSick)
        ));
    }

    #[test]
    fn tapped_creature_cannot_block() {
        let db = test_db();
        let mut gs = make_combat_state();
        let attacker = add_creature(
            &mut gs,
            PlayerId(0),
            db.get("Grizzly Bears").unwrap().clone(),
        );
        let blocker = add_creature(
            &mut gs,
            PlayerId(1),
            db.get("Grizzly Bears").unwrap().clone(),
        );
        gs = declare_attackers(gs, PlayerId(0), &[attacker]).unwrap();
        gs.objects.get_mut(&blocker).unwrap().tapped = true;
        gs.step = Step::DeclareBlockers;

        assert!(matches!(
            declare_blockers(gs, PlayerId(1), &[(blocker, attacker)]),
            Err(EngineError::CreatureTapped)
        ));
    }

    #[test]
    fn attacking_taps_the_attacker() {
        let db = test_db();
        let mut gs = make_combat_state();
        let bear_id = add_creature(
            &mut gs,
            PlayerId(0),
            db.get("Grizzly Bears").unwrap().clone(),
        );
        let gs = declare_attackers(gs, PlayerId(0), &[bear_id]).unwrap();
        assert!(gs.objects[&bear_id].tapped);
    }

    #[test]
    fn non_flier_cannot_block_flier() {
        use crate::types::ability::StaticAbility;
        let mut gs = make_combat_state();
        let attacker = keyword_creature(&mut gs, PlayerId(0), 2, 2, vec![StaticAbility::Flying]);
        let blocker = keyword_creature(&mut gs, PlayerId(1), 2, 2, vec![]); // no flying/reach
        gs = declare_attackers(gs, PlayerId(0), &[attacker]).unwrap();
        gs.step = Step::DeclareBlockers;
        assert!(matches!(
            declare_blockers(gs, PlayerId(1), &[(blocker, attacker)]),
            Err(EngineError::InvalidBlocker)
        ));
    }

    #[test]
    fn flier_can_block_flier() {
        use crate::types::ability::StaticAbility;
        let mut gs = make_combat_state();
        let attacker = keyword_creature(&mut gs, PlayerId(0), 2, 2, vec![StaticAbility::Flying]);
        let blocker = keyword_creature(&mut gs, PlayerId(1), 2, 2, vec![StaticAbility::Flying]);
        gs = declare_attackers(gs, PlayerId(0), &[attacker]).unwrap();
        gs.step = Step::DeclareBlockers;
        assert!(declare_blockers(gs, PlayerId(1), &[(blocker, attacker)]).is_ok());
    }

    #[test]
    fn reach_creature_can_block_flier() {
        use crate::types::ability::StaticAbility;
        let mut gs = make_combat_state();
        let attacker = keyword_creature(&mut gs, PlayerId(0), 2, 2, vec![StaticAbility::Flying]);
        let blocker = keyword_creature(&mut gs, PlayerId(1), 2, 2, vec![StaticAbility::Reach]);
        gs = declare_attackers(gs, PlayerId(0), &[attacker]).unwrap();
        gs.step = Step::DeclareBlockers;
        assert!(declare_blockers(gs, PlayerId(1), &[(blocker, attacker)]).is_ok());
    }

    #[test]
    fn menace_requires_two_or_more_blockers() {
        use crate::types::ability::StaticAbility;
        let mut gs = make_combat_state();
        let attacker = keyword_creature(&mut gs, PlayerId(0), 2, 2, vec![StaticAbility::Menace]);
        let blocker = keyword_creature(&mut gs, PlayerId(1), 2, 2, vec![]);
        gs = declare_attackers(gs, PlayerId(0), &[attacker]).unwrap();
        gs.step = Step::DeclareBlockers;
        // Exactly one blocker → illegal
        assert!(matches!(
            declare_blockers(gs, PlayerId(1), &[(blocker, attacker)]),
            Err(EngineError::MenaceRequiresTwoBlockers)
        ));
    }

    #[test]
    fn menace_allows_two_blockers() {
        use crate::types::ability::StaticAbility;
        let mut gs = make_combat_state();
        let attacker = keyword_creature(&mut gs, PlayerId(0), 4, 4, vec![StaticAbility::Menace]);
        let blocker1 = keyword_creature(&mut gs, PlayerId(1), 2, 2, vec![]);
        let blocker2 = keyword_creature(&mut gs, PlayerId(1), 2, 2, vec![]);
        gs = declare_attackers(gs, PlayerId(0), &[attacker]).unwrap();
        gs.step = Step::DeclareBlockers;
        assert!(
            declare_blockers(
                gs,
                PlayerId(1),
                &[(blocker1, attacker), (blocker2, attacker)]
            )
            .is_ok()
        );
    }

    #[test]
    fn menace_allows_zero_blockers() {
        use crate::types::ability::StaticAbility;
        let mut gs = make_combat_state();
        let attacker = keyword_creature(&mut gs, PlayerId(0), 2, 2, vec![StaticAbility::Menace]);
        gs = declare_attackers(gs, PlayerId(0), &[attacker]).unwrap();
        gs.step = Step::DeclareBlockers;
        // No blockers declared — legal (creature is unblocked)
        assert!(declare_blockers(gs, PlayerId(1), &[]).is_ok());
    }

    #[test]
    fn multiple_blockers_take_damage_in_order() {
        // Attacker: 5/5. Blockers: two 2/2s.
        // Damage assignment: 2 to first (lethal), 3 to second.
        let db = test_db();
        let mut gs = make_combat_state();

        // Use Hill Giant as a base and override P/T for this test
        let mut giant_def = db.get("Hill Giant").unwrap().clone();
        giant_def.power = Some(5);
        giant_def.toughness = Some(5);
        let attacker = add_creature(&mut gs, PlayerId(0), giant_def);
        gs.objects.get_mut(&attacker).unwrap().current_power = Some(5);
        gs.objects.get_mut(&attacker).unwrap().current_toughness = Some(5);

        let block1 = add_creature(
            &mut gs,
            PlayerId(1),
            db.get("Grizzly Bears").unwrap().clone(),
        );
        let block2 = add_creature(
            &mut gs,
            PlayerId(1),
            db.get("Grizzly Bears").unwrap().clone(),
        );

        gs = declare_attackers(gs, PlayerId(0), &[attacker]).unwrap();
        gs.step = Step::DeclareBlockers;
        gs = declare_blockers(gs, PlayerId(1), &[(block1, attacker), (block2, attacker)]).unwrap();
        gs.step = Step::CombatDamage;

        let gs = deal_combat_damage(gs);

        // Both blockers should die. Attacker takes 2+2=4 damage, survives (5/5 with 4 damage).
        assert!(!gs.battlefield.contains(&block1));
        assert!(!gs.battlefield.contains(&block2));
        assert!(gs.battlefield.contains(&attacker));
        assert_eq!(gs.objects[&attacker].damage_marked, 4);
    }
}
