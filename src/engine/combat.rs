use super::{EngineError, state_based_actions::check_and_apply_sbas};
use crate::engine::triggered::{collect_attack_triggers, collect_block_triggers};
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

    let cmt = state.controllers_most_recent_turn(player_id);
    for &id in attacker_ids {
        let obj = state.objects.get(&id).ok_or(EngineError::CardNotFound)?;
        if obj.controller != player_id {
            return Err(EngineError::NotYourCard);
        }
        if !obj.is_creature() {
            return Err(EngineError::NotACreature);
        }
        let perm = state
            .battlefield
            .get(&id)
            .ok_or(EngineError::CardNotFound)?;
        if perm.summoning_sick(cmt) && !perm.has_keyword(StaticAbility::Haste) {
            return Err(EngineError::SummoningSick);
        }
        if perm.tapped {
            return Err(EngineError::CreatureTapped);
        }
    }

    state.mana_checkpoint = None;
    for &id in attacker_ids {
        if !state
            .objects
            .get(&id)
            .unwrap()
            .has_keyword(StaticAbility::Vigilance)
        {
            state.battlefield.get_mut(&id).unwrap().tapped = true;
        }
    }
    state.combat.attackers = attacker_ids.to_vec();
    state.combat.blocking_map = attacker_ids.iter().map(|&id| (id, vec![])).collect();
    state.combat.attackers_declared = true;

    // Collect attack-triggered abilities (Exalted, Melee) and push onto stack.
    let triggers = collect_attack_triggers(&mut state);
    for t in triggers {
        let id = t.id;
        state.stack.push(id);
        state.stack_objects.insert(id, t);
    }
    if !state.stack.is_empty() {
        state.consecutive_passes = 0;
        state.priority_player = state.active_player;
    }

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

    for &(blocker_id, attacker_id) in blocks {
        let obj = state
            .objects
            .get(&blocker_id)
            .ok_or(EngineError::CardNotFound)?;
        if obj.controller != player_id {
            return Err(EngineError::NotYourCard);
        }
        if !obj.is_creature() {
            return Err(EngineError::NotACreature);
        }
        let perm = state
            .battlefield
            .get(&blocker_id)
            .ok_or(EngineError::CardNotFound)?;
        if perm.tapped {
            return Err(EngineError::CreatureTapped);
        }
        if !state.combat.attackers.contains(&attacker_id) {
            return Err(EngineError::CannotCastNow);
        }
        // tapped already caught above (distinct error variant); can_block() here gates Decayed et al.
        if !can_block_attacker(&state, blocker_id, attacker_id) {
            return Err(EngineError::InvalidBlocker);
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

    state.mana_checkpoint = None;
    state.combat.blockers_declared = true;

    // Collect block-triggered abilities (Flanking, Bushido N) and push onto stack.
    let triggers = collect_block_triggers(&mut state);
    for t in triggers {
        let id = t.id;
        state.stack.push(id);
        state.stack_objects.insert(id, t);
    }
    if !state.stack.is_empty() {
        state.consecutive_passes = 0;
        state.priority_player = state.active_player;
    }

    Ok(state)
}

/// CR 509.1: returns true if `blocker_id` can legally block `attacker_id`.
/// Checks per-pair evasion rules. Does not check menace (a whole-declaration constraint).
pub fn can_block_attacker(state: &GameState, blocker_id: ObjectId, attacker_id: ObjectId) -> bool {
    let Some(blocker_perm) = state.battlefield.get(&blocker_id) else {
        return false;
    };
    let Some(blocker_obj) = state.objects.get(&blocker_id) else {
        return false;
    };
    let Some(attacker_obj) = state.objects.get(&attacker_id) else {
        return false;
    };
    if !blocker_perm.can_block() {
        return false;
    }
    // CR 702.9b: flying
    if attacker_obj.has_keyword(StaticAbility::Flying)
        && !blocker_obj.has_keyword(StaticAbility::Flying)
        && !blocker_obj.has_keyword(StaticAbility::Reach)
    {
        return false;
    }
    // CR 702.28b: shadow
    if attacker_obj.has_keyword(StaticAbility::Shadow)
        != blocker_obj.has_keyword(StaticAbility::Shadow)
    {
        return false;
    }
    // CR 702.31b: horsemanship
    if attacker_obj.has_keyword(StaticAbility::Horsemanship)
        && !blocker_obj.has_keyword(StaticAbility::Horsemanship)
    {
        return false;
    }
    // CR 702.118b: skulk
    if attacker_obj.has_keyword(StaticAbility::Skulk) {
        let attacker_power = state
            .battlefield
            .get(&attacker_id)
            .and_then(|p| p.effective_power())
            .unwrap_or(0);
        let blocker_power = state
            .battlefield
            .get(&blocker_id)
            .and_then(|p| p.effective_power())
            .unwrap_or(0);
        if blocker_power > attacker_power {
            return false;
        }
    }
    true
}

/// Deal combat damage (CR 510). Handles first strike / double strike two-round system (CR 510.4).
/// If any first/double striker is present and we haven't done the first-strike round yet,
/// only those creatures deal damage and a second CombatDamage step is queued.
/// In the second round, double strikers and vanilla creatures deal damage (not first-strike-only).
/// If no first/double strikers are present, all creatures deal damage in a single round.
/// Also handles Trample (CR 702.19), Deathtouch (CR 702.2c), and Lifelink (CR 702.15).
pub fn deal_combat_damage(mut state: GameState) -> GameState {
    state.mana_checkpoint = None;
    use std::collections::HashSet;

    let defending_player = state.opponent_of(state.active_player);
    let attackers = state.combat.attackers.clone();
    let blocking_map = state.combat.blocking_map.clone();

    let any_first_or_double = attackers
        .iter()
        .chain(blocking_map.values().flatten())
        .any(|&id| {
            state
                .objects
                .get(&id)
                .map(|o| {
                    o.has_keyword(StaticAbility::FirstStrike)
                        || o.has_keyword(StaticAbility::DoubleStrike)
                })
                .unwrap_or(false)
        });

    let first_round = any_first_or_double && !state.combat.first_strike_done;
    let second_round = any_first_or_double && state.combat.first_strike_done;

    if first_round {
        state.combat.first_strike_done = true;
        state.extra_steps.push_back(Step::CombatDamage);
    }

    let deals_this_round = |id: ObjectId| -> bool {
        let Some(obj) = state.objects.get(&id) else {
            return false;
        };
        if first_round {
            obj.has_keyword(StaticAbility::FirstStrike)
                || obj.has_keyword(StaticAbility::DoubleStrike)
        } else if second_round {
            !obj.has_keyword(StaticAbility::FirstStrike)
        } else {
            true
        }
    };

    let mut damage_to_players: HashMap<PlayerId, i32> = HashMap::new();
    let mut damage_to_objects: HashMap<ObjectId, u32> = HashMap::new();
    let mut lifelink_gain: HashMap<PlayerId, i32> = HashMap::new();
    let mut deathtouch_targets: HashSet<ObjectId> = HashSet::new();

    for &attacker_id in &attackers {
        if !deals_this_round(attacker_id) {
            continue;
        }

        let (atk_power, has_trample, has_deathtouch, has_lifelink, atk_controller) = {
            let obj = match state.objects.get(&attacker_id) {
                Some(o) => o,
                None => continue,
            };
            let power = state
                .battlefield
                .get(&attacker_id)
                .and_then(|p| p.effective_power())
                .map(|p| p.max(0) as u32)
                .unwrap_or(0);
            (
                power,
                obj.has_keyword(StaticAbility::Trample),
                obj.has_keyword(StaticAbility::Deathtouch),
                obj.has_keyword(StaticAbility::Lifelink),
                obj.controller,
            )
        };

        let blockers = blocking_map.get(&attacker_id).cloned().unwrap_or_default();
        let mut total_damage_dealt = 0u32;

        if blockers.is_empty() {
            *damage_to_players.entry(defending_player).or_insert(0) += atk_power as i32;
            total_damage_dealt = atk_power;
        } else {
            let mut remaining = atk_power;
            for &blocker_id in &blockers {
                if remaining == 0 {
                    break;
                }
                // Lethal threshold: 1 if attacker has deathtouch (CR 702.2c), else remaining toughness.
                let lethal = if has_deathtouch {
                    1u32
                } else {
                    state
                        .battlefield
                        .get(&blocker_id)
                        .map(|p| {
                            let toughness = p
                                .effective_toughness()
                                .map(|t| t.max(0) as u32)
                                .unwrap_or(0);
                            toughness.saturating_sub(p.damage_marked)
                        })
                        .unwrap_or(0)
                        .max(1)
                };
                let assign = remaining.min(lethal);
                *damage_to_objects.entry(blocker_id).or_insert(0) += assign;
                remaining -= assign;
                total_damage_dealt += assign;
                if has_deathtouch && assign > 0 {
                    deathtouch_targets.insert(blocker_id);
                }
            }
            // Remaining damage: to player if trample, otherwise pile on last blocker.
            if remaining > 0 {
                if has_trample {
                    *damage_to_players.entry(defending_player).or_insert(0) += remaining as i32;
                    total_damage_dealt += remaining;
                } else if let Some(&last) = blockers.last() {
                    *damage_to_objects.entry(last).or_insert(0) += remaining;
                    if has_deathtouch {
                        deathtouch_targets.insert(last);
                    }
                    total_damage_dealt += remaining;
                }
            }
        }

        if has_lifelink && total_damage_dealt > 0 {
            *lifelink_gain.entry(atk_controller).or_insert(0) += total_damage_dealt as i32;
        }

        // Blockers deal their damage back to the attacker.
        for &blocker_id in &blockers {
            if !deals_this_round(blocker_id) {
                continue;
            }
            let (blk_power, blk_deathtouch, blk_lifelink, blk_controller) = {
                let obj = match state.objects.get(&blocker_id) {
                    Some(o) => o,
                    None => continue,
                };
                let power = state
                    .battlefield
                    .get(&blocker_id)
                    .and_then(|p| p.effective_power())
                    .map(|p| p.max(0) as u32)
                    .unwrap_or(0);
                (
                    power,
                    obj.has_keyword(StaticAbility::Deathtouch),
                    obj.has_keyword(StaticAbility::Lifelink),
                    obj.controller,
                )
            };
            if blk_power > 0 {
                *damage_to_objects.entry(attacker_id).or_insert(0) += blk_power;
                if blk_deathtouch {
                    deathtouch_targets.insert(attacker_id);
                }
                if blk_lifelink {
                    *lifelink_gain.entry(blk_controller).or_insert(0) += blk_power as i32;
                }
            }
        }
    }

    // Apply all damage and effects simultaneously.
    for (pid, dmg) in &damage_to_players {
        if let Some(p) = state.get_player_mut(*pid) {
            p.life -= dmg;
        }
    }
    for (oid, dmg) in damage_to_objects {
        if let Some(perm) = state.battlefield.get_mut(&oid) {
            perm.damage_marked += dmg;
        }
    }
    for oid in deathtouch_targets {
        if let Some(perm) = state.battlefield.get_mut(&oid) {
            perm.damaged_by_deathtouch = true;
        }
    }
    for (pid, gain) in lifelink_gain {
        if let Some(p) = state.get_player_mut(pid) {
            p.life += gain;
        }
    }

    check_and_apply_sbas(state)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cards::test_helpers::test_db;
    use crate::types::{CardObject, PermanentState, Player, Zone};

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
        let obj = CardObject::new(id, def, owner, Zone::Battlefield);
        let mut perm = PermanentState::new(&obj.definition);
        perm.controller_since_turn = 0;
        state.battlefield.insert(id, perm);
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
            Ability, CardDefinition, OracleSpan,
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
                .map(|k| OracleSpan::Parsed(Ability::Static(k)))
                .collect(),
            power: Some(power),
            toughness: Some(toughness),
        };
        let obj = crate::types::CardObject::new(id, def, owner, Zone::Battlefield);
        let mut perm = PermanentState::new(&obj.definition);
        perm.controller_since_turn = 0;
        state.battlefield.insert(id, perm);
        state.add_object(obj);
        id
    }

    #[test]
    fn declare_attackers_exalted_puts_trigger_on_stack() {
        use crate::types::OracleSpan;
        use crate::types::ability::{Ability, StaticAbility};
        use crate::types::card::{CardDefinition, CardType, TypeLine};

        let mut gs = make_combat_state();
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
            power: Some(2),
            toughness: Some(2),
        };
        let attacker_id = add_creature(&mut gs, PlayerId(0), plain_def);
        let exalted_def = CardDefinition {
            name: "Exalted Elf".into(),
            mana_cost: None,
            type_line: TypeLine {
                supertypes: vec![],
                card_types: vec![CardType::Creature],
                subtypes: vec![],
            },
            oracle_text: String::new(),
            abilities: vec![OracleSpan::Parsed(Ability::Static(StaticAbility::Exalted))],
            power: Some(1),
            toughness: Some(1),
        };
        add_creature(&mut gs, PlayerId(0), exalted_def);

        let gs = declare_attackers(gs, PlayerId(0), &[attacker_id]).unwrap();

        assert_eq!(gs.stack.len(), 1); // one Exalted trigger
        assert_eq!(gs.consecutive_passes, 0);
    }

    #[test]
    fn declare_attackers_sets_declared_flag() {
        let gs = make_combat_state();
        assert!(!gs.combat.attackers_declared);
        let gs = declare_attackers(gs, PlayerId(0), &[]).unwrap();
        assert!(gs.combat.attackers_declared);
    }

    #[test]
    fn declare_blockers_sets_declared_flag() {
        let gs = make_combat_state();
        assert!(!gs.combat.blockers_declared);
        let gs = declare_blockers(gs, PlayerId(1), &[]).unwrap();
        assert!(gs.combat.blockers_declared);
    }

    #[test]
    fn vigilant_attacker_does_not_tap() {
        use crate::types::ability::StaticAbility;
        let mut gs = make_combat_state();
        let id = keyword_creature(&mut gs, PlayerId(0), 2, 2, vec![StaticAbility::Vigilance]);
        let gs = declare_attackers(gs, PlayerId(0), &[id]).unwrap();
        assert!(!gs.battlefield[&id].tapped); // vigilance: does not tap when attacking
    }

    #[test]
    fn haste_creature_can_attack_while_summoning_sick() {
        use crate::types::ability::StaticAbility;
        let mut gs = make_combat_state();
        let id = keyword_creature(&mut gs, PlayerId(0), 2, 2, vec![StaticAbility::Haste]);
        gs.battlefield.get_mut(&id).unwrap().controller_since_turn = u32::MAX; // still sick
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
        assert!(!gs.battlefield.contains_key(&attacker));
        assert!(!gs.battlefield.contains_key(&blocker));
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

        assert!(gs.battlefield.contains_key(&giant)); // 3/3 survives 2 damage
        assert!(!gs.battlefield.contains_key(&bear)); // 2/2 dies to 3 damage
        assert_eq!(gs.battlefield[&giant].damage_marked, 2);
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
        gs.battlefield
            .get_mut(&bear_id)
            .unwrap()
            .controller_since_turn = u32::MAX;

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
        gs.battlefield.get_mut(&blocker).unwrap().tapped = true;
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
        assert!(gs.battlefield[&bear_id].tapped);
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
    fn first_striker_kills_blocker_before_it_can_deal_damage() {
        // 3/2 First Strike vs 2/2 vanilla:
        // Round 1: first striker deals 3 (lethal to 2/2). 2/2 can't deal back.
        // Round 2: 2/2 is dead, no damage back to first striker.
        use crate::engine::turn::advance_step;
        use crate::types::ability::StaticAbility;
        let mut gs = make_combat_state();
        let attacker =
            keyword_creature(&mut gs, PlayerId(0), 3, 2, vec![StaticAbility::FirstStrike]);
        let blocker = keyword_creature(&mut gs, PlayerId(1), 2, 2, vec![]);
        gs = declare_attackers(gs, PlayerId(0), &[attacker]).unwrap();
        gs.step = Step::DeclareBlockers;
        gs = declare_blockers(gs, PlayerId(1), &[(blocker, attacker)]).unwrap();
        gs.step = Step::CombatDamage;

        // Round 1
        let gs = deal_combat_damage(gs);

        // Blocker should be dead; attacker should be undamaged
        assert!(!gs.battlefield.contains_key(&blocker));
        assert_eq!(gs.battlefield[&attacker].damage_marked, 0);
        // A second CombatDamage step should be queued
        assert!(!gs.extra_steps.is_empty());

        // Advance to second round
        let gs = advance_step(gs); // pops extra_steps → CombatDamage
        let gs = deal_combat_damage(gs);

        // No blockers left — attacker still undamaged; player untouched (attacker was blocked)
        assert_eq!(gs.battlefield[&attacker].damage_marked, 0);
        assert_eq!(gs.get_player(PlayerId(1)).unwrap().life, 20);
    }

    #[test]
    fn double_striker_deals_damage_in_both_rounds() {
        // 2/2 Double Strike vs 3/3:
        // Round 1: double striker deals 2. Round 2: double striker deals another 2.
        // 3/3 deals 3 in round 2. 3/3 has 4 damage total (lethal), double striker has 3 (lethal).
        use crate::engine::turn::advance_step;
        use crate::types::ability::StaticAbility;
        let mut gs = make_combat_state();
        let attacker = keyword_creature(
            &mut gs,
            PlayerId(0),
            2,
            2,
            vec![StaticAbility::DoubleStrike],
        );
        let blocker = keyword_creature(&mut gs, PlayerId(1), 3, 3, vec![]);
        gs = declare_attackers(gs, PlayerId(0), &[attacker]).unwrap();
        gs.step = Step::DeclareBlockers;
        gs = declare_blockers(gs, PlayerId(1), &[(blocker, attacker)]).unwrap();
        gs.step = Step::CombatDamage;

        // Round 1: only double striker deals damage
        let gs = deal_combat_damage(gs);
        assert_eq!(gs.battlefield[&blocker].damage_marked, 2);
        assert_eq!(gs.battlefield[&attacker].damage_marked, 0); // blocker hasn't dealt yet

        // Round 2: double striker AND non-first-strikers (none; blocker is vanilla) deal damage
        let gs = advance_step(gs);
        assert_eq!(gs.step(), Step::CombatDamage);
        let gs = deal_combat_damage(gs);

        // Both die
        assert!(!gs.battlefield.contains_key(&blocker));
        assert!(!gs.battlefield.contains_key(&attacker));
    }

    #[test]
    fn no_first_strikers_means_single_round_and_no_extra_step() {
        // Vanilla combat: no extra step should be queued
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
        gs.step = Step::DeclareBlockers;
        gs = declare_blockers(gs, PlayerId(1), &[(blocker, attacker)]).unwrap();
        gs.step = Step::CombatDamage;

        let gs = deal_combat_damage(gs);

        // Both die; no extra step queued
        assert!(!gs.battlefield.contains_key(&attacker));
        assert!(!gs.battlefield.contains_key(&blocker));
        assert!(gs.extra_steps.is_empty());
    }

    #[test]
    fn trample_sends_excess_to_player() {
        // 5/5 Trample vs 2/2 blocker: 2 to blocker (lethal), 3 tramples through
        use crate::types::ability::StaticAbility;
        let mut gs = make_combat_state();
        let attacker = keyword_creature(&mut gs, PlayerId(0), 5, 5, vec![StaticAbility::Trample]);
        let blocker = keyword_creature(&mut gs, PlayerId(1), 2, 2, vec![]);
        gs = declare_attackers(gs, PlayerId(0), &[attacker]).unwrap();
        gs.step = Step::DeclareBlockers;
        gs = declare_blockers(gs, PlayerId(1), &[(blocker, attacker)]).unwrap();
        gs.step = Step::CombatDamage;

        let gs = deal_combat_damage(gs);

        assert!(!gs.battlefield.contains_key(&blocker));
        assert_eq!(gs.get_player(PlayerId(1)).unwrap().life, 17); // 20 - 3
    }

    #[test]
    fn trample_deathtouch_one_damage_is_lethal_per_blocker() {
        // 5/5 Trample + Deathtouch vs 4/4 blocker: 1 damage is lethal (deathtouch),
        // 4 tramples through to defending player
        use crate::types::ability::StaticAbility;
        let mut gs = make_combat_state();
        let attacker = keyword_creature(
            &mut gs,
            PlayerId(0),
            5,
            5,
            vec![StaticAbility::Trample, StaticAbility::Deathtouch],
        );
        let blocker = keyword_creature(&mut gs, PlayerId(1), 4, 4, vec![]);
        gs = declare_attackers(gs, PlayerId(0), &[attacker]).unwrap();
        gs.step = Step::DeclareBlockers;
        gs = declare_blockers(gs, PlayerId(1), &[(blocker, attacker)]).unwrap();
        gs.step = Step::CombatDamage;

        let gs = deal_combat_damage(gs);

        assert!(!gs.battlefield.contains_key(&blocker)); // 1 deathtouch damage kills 4/4
        assert_eq!(gs.get_player(PlayerId(1)).unwrap().life, 16); // 20 - 4 trample
    }

    #[test]
    fn lifelink_attacker_gains_life_from_combat_damage() {
        // 3/3 Lifelink unblocked: controller gains 3 life
        use crate::types::ability::StaticAbility;
        let mut gs = make_combat_state();
        let attacker = keyword_creature(&mut gs, PlayerId(0), 3, 3, vec![StaticAbility::Lifelink]);
        gs = declare_attackers(gs, PlayerId(0), &[attacker]).unwrap();
        gs.step = Step::DeclareBlockers;
        gs = declare_blockers(gs, PlayerId(1), &[]).unwrap();
        gs.step = Step::CombatDamage;

        let gs = deal_combat_damage(gs);

        assert_eq!(gs.get_player(PlayerId(0)).unwrap().life, 23); // 20 + 3
        assert_eq!(gs.get_player(PlayerId(1)).unwrap().life, 17); // 20 - 3
    }

    #[test]
    fn deathtouch_marks_target_for_sba() {
        // 1/1 Deathtouch vs 4/4: deathtouch creature deals 1 damage, flag set on 4/4
        use crate::types::ability::StaticAbility;
        let mut gs = make_combat_state();
        let attacker =
            keyword_creature(&mut gs, PlayerId(0), 1, 1, vec![StaticAbility::Deathtouch]);
        let blocker = keyword_creature(&mut gs, PlayerId(1), 4, 4, vec![]);
        gs = declare_attackers(gs, PlayerId(0), &[attacker]).unwrap();
        gs.step = Step::DeclareBlockers;
        gs = declare_blockers(gs, PlayerId(1), &[(blocker, attacker)]).unwrap();
        gs.step = Step::CombatDamage;

        let gs = deal_combat_damage(gs);

        // 4/4 received deathtouch damage → SBAs already ran → it should be dead
        assert!(!gs.battlefield.contains_key(&blocker));
        // 1/1 received 4 damage (lethal) → also dead
        assert!(!gs.battlefield.contains_key(&attacker));
    }

    #[test]
    fn declare_blockers_clears_mana_checkpoint() {
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
        gs.step = Step::DeclareBlockers;
        gs.mana_checkpoint = Some(crate::types::ManaCheckpoint {
            pools: std::collections::HashMap::new(),
            tapped_lands: vec![],
        });

        let gs = declare_blockers(gs, PlayerId(1), &[(blocker, attacker)]).unwrap();

        assert!(gs.mana_checkpoint.is_none());
    }

    #[test]
    fn declare_blockers_flanking_attacker_puts_trigger_on_stack() {
        use crate::types::OracleSpan;
        use crate::types::ability::{Ability, StaticAbility};
        use crate::types::card::{CardDefinition, CardType, TypeLine};

        let mut gs = make_combat_state();
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
            power: Some(2),
            toughness: Some(2),
        };
        let plain_def = CardDefinition {
            name: "Blocker".into(),
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
        let attacker = add_creature(&mut gs, PlayerId(0), flanking_def);
        let blocker = add_creature(&mut gs, PlayerId(1), plain_def);
        gs = declare_attackers(gs, PlayerId(0), &[attacker]).unwrap();
        // Clear any attack triggers so we can verify blockers trigger separately.
        gs.stack.clear();
        gs.stack_objects.clear();
        gs.step = crate::types::Step::DeclareBlockers;

        let gs = declare_blockers(gs, PlayerId(1), &[(blocker, attacker)]).unwrap();

        assert_eq!(gs.stack.len(), 1); // Flanking trigger
    }

    #[test]
    fn declare_attackers_clears_mana_checkpoint() {
        let db = test_db();
        let mut gs = make_combat_state();
        let bear_id = add_creature(
            &mut gs,
            PlayerId(0),
            db.get("Grizzly Bears").unwrap().clone(),
        );
        gs.mana_checkpoint = Some(crate::types::ManaCheckpoint {
            pools: std::collections::HashMap::new(),
            tapped_lands: vec![],
        });

        let gs = declare_attackers(gs, PlayerId(0), &[bear_id]).unwrap();

        assert!(gs.mana_checkpoint.is_none());
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
        gs.battlefield.get_mut(&attacker).unwrap().current_power = Some(5);
        gs.battlefield.get_mut(&attacker).unwrap().current_toughness = Some(5);

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
        assert!(!gs.battlefield.contains_key(&block1));
        assert!(!gs.battlefield.contains_key(&block2));
        assert!(gs.battlefield.contains_key(&attacker));
        assert_eq!(gs.battlefield[&attacker].damage_marked, 4);
    }

    #[test]
    fn skulk_cannot_be_blocked_by_greater_power() {
        use crate::types::ability::StaticAbility;
        let mut gs = make_combat_state();
        let attacker = keyword_creature(&mut gs, PlayerId(0), 1, 3, vec![StaticAbility::Skulk]);
        let blocker = keyword_creature(&mut gs, PlayerId(1), 2, 2, vec![]);
        gs = declare_attackers(gs, PlayerId(0), &[attacker]).unwrap();
        gs.step = Step::DeclareBlockers;
        // 2-power blocker can't block 1-power skulk attacker
        assert!(matches!(
            declare_blockers(gs, PlayerId(1), &[(blocker, attacker)]),
            Err(EngineError::InvalidBlocker)
        ));
    }

    #[test]
    fn skulk_can_be_blocked_by_equal_or_lesser_power() {
        use crate::types::ability::StaticAbility;
        let mut gs = make_combat_state();
        let attacker = keyword_creature(&mut gs, PlayerId(0), 2, 3, vec![StaticAbility::Skulk]);
        let equal_blocker = keyword_creature(&mut gs, PlayerId(1), 2, 2, vec![]);
        gs = declare_attackers(gs, PlayerId(0), &[attacker]).unwrap();
        gs.step = Step::DeclareBlockers;
        assert!(declare_blockers(gs, PlayerId(1), &[(equal_blocker, attacker)]).is_ok());
    }

    #[test]
    fn decayed_creature_cannot_block() {
        use crate::types::ability::StaticAbility;
        let mut gs = make_combat_state();
        let attacker = keyword_creature(&mut gs, PlayerId(0), 2, 2, vec![]);
        let decayed = keyword_creature(&mut gs, PlayerId(1), 2, 2, vec![StaticAbility::Decayed]);
        gs = declare_attackers(gs, PlayerId(0), &[attacker]).unwrap();
        gs.step = Step::DeclareBlockers;
        assert!(matches!(
            declare_blockers(gs, PlayerId(1), &[(decayed, attacker)]),
            Err(EngineError::InvalidBlocker)
        ));
    }

    #[test]
    fn can_block_attacker_vanilla_vs_vanilla() {
        let mut gs = make_combat_state();
        let attacker = keyword_creature(&mut gs, PlayerId(0), 2, 2, vec![]);
        let blocker = keyword_creature(&mut gs, PlayerId(1), 2, 2, vec![]);
        gs = declare_attackers(gs, PlayerId(0), &[attacker]).unwrap();
        assert!(can_block_attacker(&gs, blocker, attacker));
    }

    #[test]
    fn can_block_attacker_ground_cannot_block_flier() {
        let mut gs = make_combat_state();
        let attacker = keyword_creature(&mut gs, PlayerId(0), 2, 2, vec![StaticAbility::Flying]);
        let blocker = keyword_creature(&mut gs, PlayerId(1), 2, 2, vec![]);
        gs = declare_attackers(gs, PlayerId(0), &[attacker]).unwrap();
        assert!(!can_block_attacker(&gs, blocker, attacker));
    }

    #[test]
    fn can_block_attacker_reach_can_block_flier() {
        let mut gs = make_combat_state();
        let attacker = keyword_creature(&mut gs, PlayerId(0), 2, 2, vec![StaticAbility::Flying]);
        let blocker = keyword_creature(&mut gs, PlayerId(1), 2, 2, vec![StaticAbility::Reach]);
        gs = declare_attackers(gs, PlayerId(0), &[attacker]).unwrap();
        assert!(can_block_attacker(&gs, blocker, attacker));
    }

    #[test]
    fn can_block_attacker_non_shadow_cannot_block_shadow() {
        let mut gs = make_combat_state();
        let attacker = keyword_creature(&mut gs, PlayerId(0), 2, 2, vec![StaticAbility::Shadow]);
        let blocker = keyword_creature(&mut gs, PlayerId(1), 2, 2, vec![]);
        gs = declare_attackers(gs, PlayerId(0), &[attacker]).unwrap();
        assert!(!can_block_attacker(&gs, blocker, attacker));
    }

    #[test]
    fn can_block_attacker_shadow_cannot_block_non_shadow() {
        let mut gs = make_combat_state();
        let attacker = keyword_creature(&mut gs, PlayerId(0), 2, 2, vec![]);
        let blocker = keyword_creature(&mut gs, PlayerId(1), 2, 2, vec![StaticAbility::Shadow]);
        gs = declare_attackers(gs, PlayerId(0), &[attacker]).unwrap();
        assert!(!can_block_attacker(&gs, blocker, attacker));
    }

    #[test]
    fn can_block_attacker_shadow_can_block_shadow() {
        let mut gs = make_combat_state();
        let attacker = keyword_creature(&mut gs, PlayerId(0), 2, 2, vec![StaticAbility::Shadow]);
        let blocker = keyword_creature(&mut gs, PlayerId(1), 2, 2, vec![StaticAbility::Shadow]);
        gs = declare_attackers(gs, PlayerId(0), &[attacker]).unwrap();
        assert!(can_block_attacker(&gs, blocker, attacker));
    }

    #[test]
    fn can_block_attacker_non_horsemanship_cannot_block_horsemanship() {
        let mut gs = make_combat_state();
        let attacker = keyword_creature(
            &mut gs,
            PlayerId(0),
            2,
            2,
            vec![StaticAbility::Horsemanship],
        );
        let blocker = keyword_creature(&mut gs, PlayerId(1), 2, 2, vec![]);
        gs = declare_attackers(gs, PlayerId(0), &[attacker]).unwrap();
        assert!(!can_block_attacker(&gs, blocker, attacker));
    }

    #[test]
    fn can_block_attacker_skulk_not_blockable_by_greater_power() {
        let mut gs = make_combat_state();
        let attacker = keyword_creature(&mut gs, PlayerId(0), 1, 1, vec![StaticAbility::Skulk]);
        let blocker = keyword_creature(&mut gs, PlayerId(1), 2, 2, vec![]);
        gs = declare_attackers(gs, PlayerId(0), &[attacker]).unwrap();
        assert!(!can_block_attacker(&gs, blocker, attacker));
    }

    #[test]
    fn can_block_attacker_skulk_blockable_by_equal_power() {
        let mut gs = make_combat_state();
        let attacker = keyword_creature(&mut gs, PlayerId(0), 2, 2, vec![StaticAbility::Skulk]);
        let blocker = keyword_creature(&mut gs, PlayerId(1), 2, 2, vec![]);
        gs = declare_attackers(gs, PlayerId(0), &[attacker]).unwrap();
        assert!(can_block_attacker(&gs, blocker, attacker));
    }

    #[test]
    fn can_block_attacker_decayed_cannot_block() {
        let mut gs = make_combat_state();
        let attacker = keyword_creature(&mut gs, PlayerId(0), 2, 2, vec![]);
        let blocker = keyword_creature(&mut gs, PlayerId(1), 2, 2, vec![StaticAbility::Decayed]);
        gs = declare_attackers(gs, PlayerId(0), &[attacker]).unwrap();
        assert!(!can_block_attacker(&gs, blocker, attacker));
    }

    #[test]
    fn can_block_attacker_returns_false_for_unknown_blocker() {
        let mut gs = make_combat_state();
        let attacker = keyword_creature(&mut gs, PlayerId(0), 2, 2, vec![]);
        gs = declare_attackers(gs, PlayerId(0), &[attacker]).unwrap();
        // Use an ObjectId that does not exist in the state
        let unknown = ObjectId(9999);
        assert!(!can_block_attacker(&gs, unknown, attacker));
    }

    #[test]
    fn can_block_attacker_returns_false_for_unknown_attacker() {
        let mut gs = make_combat_state();
        let blocker = keyword_creature(&mut gs, PlayerId(1), 2, 2, vec![]);
        let unknown_attacker = ObjectId(9999);
        assert!(!can_block_attacker(&gs, blocker, unknown_attacker));
    }
}
