use super::{EngineError, state_based_actions::check_and_apply_sbas};
use crate::types::ability::StaticAbility;
use crate::types::{GameEvent, GameState, ObjectId, PlayerId, Step};
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

    // CR 603.2: fire Attacks event for each attacker; collect triggered abilities.
    let mut attack_triggers = Vec::new();
    for &attacker_id in &state.combat.attackers.clone() {
        let mut t = crate::engine::triggered::collect_triggers_for_event(
            &mut state,
            &GameEvent::Attacks {
                subject_id: attacker_id,
            },
        );
        attack_triggers.append(&mut t);
    }
    for trigger in attack_triggers {
        let id = trigger.id;
        state.stack.push(id);
        state.stack_objects.insert(id, trigger);
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

    // CR 603.2: fire Blocks and BecomesBlocked events; collect triggered abilities.
    let blocking_map_snapshot: Vec<(ObjectId, Vec<ObjectId>)> = state
        .combat
        .blocking_map
        .iter()
        .map(|(&a, bs)| (a, bs.clone()))
        .collect();

    let mut block_triggers = Vec::new();

    // Fire Blocks event for each blocker.
    for (_, blockers) in &blocking_map_snapshot {
        for &blocker_id in blockers {
            let mut t = crate::engine::triggered::collect_triggers_for_event(
                &mut state,
                &crate::types::GameEvent::Blocks {
                    subject_id: blocker_id,
                },
            );
            block_triggers.append(&mut t);
        }
    }

    // Fire BecomesBlocked event for each attacker that has at least one blocker.
    for (attacker_id, blockers) in &blocking_map_snapshot {
        if !blockers.is_empty() {
            let mut t = crate::engine::triggered::collect_triggers_for_event(
                &mut state,
                &crate::types::GameEvent::BecomesBlocked {
                    subject_id: *attacker_id,
                },
            );
            block_triggers.append(&mut t);
        }
    }

    for trigger in block_triggers {
        let id = trigger.id;
        state.stack.push(id);
        state.stack_objects.insert(id, trigger);
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
    if !state.battlefield.contains_key(&attacker_id) {
        return false;
    }
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
        let atk_cont = super::continuous_pt_bonus(state, attacker_id);
        let attacker_power = state
            .battlefield
            .get(&attacker_id)
            .and_then(|p| p.effective_power(atk_cont.power))
            .unwrap_or(0);
        let blk_cont = super::continuous_pt_bonus(state, blocker_id);
        let blocker_power = state
            .battlefield
            .get(&blocker_id)
            .and_then(|p| p.effective_power(blk_cont.power))
            .unwrap_or(0);
        if blocker_power > attacker_power {
            return false;
        }
    }
    // CR 702.36b: Fear — can't be blocked except by artifact or black creatures
    if attacker_obj.has_keyword(StaticAbility::Fear) {
        let blocker_is_artifact = blocker_obj
            .definition
            .type_line
            .card_types
            .contains(&crate::types::card::CardType::Artifact);
        let blocker_is_black = blocker_obj
            .definition
            .colors
            .contains(&crate::types::mana::ManaColor::Black);
        if !blocker_is_artifact && !blocker_is_black {
            return false;
        }
    }
    // CR 702.13b: Intimidate — can't be blocked except by artifact or same-color creature
    if attacker_obj.has_keyword(StaticAbility::Intimidate) {
        let blocker_is_artifact = blocker_obj
            .definition
            .type_line
            .card_types
            .contains(&crate::types::card::CardType::Artifact);
        let attacker_colors = &attacker_obj.definition.colors;
        let blocker_colors = &blocker_obj.definition.colors;
        let shares_color = attacker_colors.iter().any(|c| blocker_colors.contains(c));
        if !blocker_is_artifact && !shares_color {
            return false;
        }
    }
    // CR 702.14c: Landwalk — can't be blocked if defending player controls matching land
    {
        use crate::types::ability::{LandwalkKind, Rule, StaticAbility as SA};
        let defending_player = state.opponent_of(state.active_player);
        for span in &attacker_obj.definition.rules_text {
            if let crate::types::RulesText::Active(Rule::Static(SA::Landwalk(kind))) = span {
                let defender_has_land = state.battlefield.iter().any(|(land_id, _)| {
                    let land_obj = match state.objects.get(land_id) {
                        Some(o) => o,
                        None => return false,
                    };
                    if land_obj.controller != defending_player {
                        return false;
                    }
                    if !land_obj.definition.type_line.is_land() {
                        return false;
                    }
                    match kind {
                        LandwalkKind::LandType(t) => {
                            land_obj.definition.type_line.subtypes.contains(t)
                        }
                        LandwalkKind::Nonbasic => !land_obj
                            .definition
                            .type_line
                            .supertypes
                            .contains(&crate::types::card::Supertype::Basic),
                    }
                });
                if defender_has_land {
                    return false;
                }
            }
        }
    }
    // CR 702.16f: Protection — can't be blocked by creatures with the protected quality
    {
        use crate::types::ability::{Rule, StaticAbility as SA};
        let blocker_colors = &blocker_obj.definition.colors;
        for span in &attacker_obj.definition.rules_text {
            if let crate::types::RulesText::Active(Rule::Static(SA::ProtectionFromColor(c))) = span
                && blocker_colors.contains(c)
            {
                return false;
            }
        }
    }
    true
}

/// Deal combat damage (CR 510). Handles first strike / double strike two-round system (CR 510.4).
/// If any first/double striker is present and we haven't done the first-strike round yet,
/// only those creatures deal damage and a second CombatDamage step is queued.
/// In the second round, double strikers and vanilla creatures deal damage (not first-strike-only).
/// If no first/double strikers are present, all creatures deal damage in a single round.
/// Also handles Trample (CR 702.19), Deathtouch (CR 702.2c), Lifelink (CR 702.15),
/// Wither (CR 702.80), Infect (CR 702.90), and Toxic N (CR 702.164).
pub fn deal_combat_damage(mut state: GameState) -> GameState {
    state.mana_checkpoint = None;
    use crate::types::CounterKind;
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
    // Wither (CR 702.80a) / Infect (CR 702.90a): creature damage as -1/-1 counters.
    let mut wither_to_objects: HashMap<ObjectId, u32> = HashMap::new();
    // Infect (CR 702.90a) / Toxic N (CR 702.164a): player damage as poison counters.
    let mut poison_to_players: HashMap<PlayerId, u32> = HashMap::new();
    // CR 603.2: track (attacker_id, DamageTargetKind) for DealsCombatDamage event emission.
    let mut combat_damage_events: Vec<(ObjectId, crate::types::DamageTargetKind)> = Vec::new();

    for &attacker_id in &attackers {
        if !deals_this_round(attacker_id) {
            continue;
        }

        let (
            atk_power,
            has_trample,
            has_deathtouch,
            has_lifelink,
            has_wither,
            has_infect,
            atk_controller,
        ) = {
            let obj = match state.objects.get(&attacker_id) {
                Some(o) => o,
                None => continue,
            };
            let atk_cont = super::continuous_pt_bonus(&state, attacker_id);
            let power = state
                .battlefield
                .get(&attacker_id)
                .and_then(|p| p.effective_power(atk_cont.power))
                .map(|p| p.max(0) as u32)
                .unwrap_or(0);
            (
                power,
                obj.has_keyword(StaticAbility::Trample),
                obj.has_keyword(StaticAbility::Deathtouch),
                obj.has_keyword(StaticAbility::Lifelink),
                obj.has_keyword(StaticAbility::Wither),
                obj.has_keyword(StaticAbility::Infect),
                obj.controller,
            )
        };
        let toxic_n = state
            .battlefield
            .get(&attacker_id)
            .and_then(|p| p.toxic_n());

        let blockers = blocking_map.get(&attacker_id).cloned().unwrap_or_default();
        let mut total_damage_dealt = 0u32;
        let mut attacked_player: Option<PlayerId> = None;

        if blockers.is_empty() {
            if has_infect {
                *poison_to_players.entry(defending_player).or_insert(0) += atk_power;
            } else {
                *damage_to_players.entry(defending_player).or_insert(0) += atk_power as i32;
            }
            total_damage_dealt = atk_power;
            if atk_power > 0 {
                attacked_player = Some(defending_player);
            }
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
                    let blk_cont = super::continuous_pt_bonus(&state, blocker_id);
                    state
                        .battlefield
                        .get(&blocker_id)
                        .map(|p| {
                            let toughness = p
                                .effective_toughness(blk_cont.toughness)
                                .map(|t| t.max(0) as u32)
                                .unwrap_or(0);
                            toughness.saturating_sub(p.damage_marked)
                        })
                        .unwrap_or(0)
                        .max(1)
                };
                let assign = remaining.min(lethal);
                if has_wither || has_infect {
                    *wither_to_objects.entry(blocker_id).or_insert(0) += assign;
                } else {
                    *damage_to_objects.entry(blocker_id).or_insert(0) += assign;
                }
                remaining -= assign;
                total_damage_dealt += assign;
                if has_deathtouch && assign > 0 {
                    deathtouch_targets.insert(blocker_id);
                }
            }
            // Remaining damage: to player if trample, otherwise pile on last blocker.
            if remaining > 0 {
                if has_trample {
                    if has_infect {
                        *poison_to_players.entry(defending_player).or_insert(0) += remaining;
                    } else {
                        *damage_to_players.entry(defending_player).or_insert(0) += remaining as i32;
                    }
                    total_damage_dealt += remaining;
                    attacked_player = Some(defending_player);
                } else if let Some(&last) = blockers.last() {
                    if has_wither || has_infect {
                        *wither_to_objects.entry(last).or_insert(0) += remaining;
                    } else {
                        *damage_to_objects.entry(last).or_insert(0) += remaining;
                    }
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

        // CR 702.164a: Toxic N — additional poison counters when this deals combat damage to a player.
        if let (Some(n), Some(pid)) = (toxic_n, attacked_player) {
            *poison_to_players.entry(pid).or_insert(0) += n;
        }

        // CR 603.2: record DealsCombatDamage events for later emission.
        if attacked_player.is_some() {
            combat_damage_events.push((attacker_id, crate::types::DamageTargetKind::Player));
        }
        if total_damage_dealt > 0 && !blockers.is_empty() {
            combat_damage_events.push((attacker_id, crate::types::DamageTargetKind::Creature));
        }

        // Blockers deal their damage back to the attacker.
        for &blocker_id in &blockers {
            if !deals_this_round(blocker_id) {
                continue;
            }
            let (blk_power, blk_wither, blk_infect, blk_deathtouch, blk_lifelink, blk_controller) = {
                let obj = match state.objects.get(&blocker_id) {
                    Some(o) => o,
                    None => continue,
                };
                let blk_cont = super::continuous_pt_bonus(&state, blocker_id);
                let power = state
                    .battlefield
                    .get(&blocker_id)
                    .and_then(|p| p.effective_power(blk_cont.power))
                    .map(|p| p.max(0) as u32)
                    .unwrap_or(0);
                (
                    power,
                    obj.has_keyword(StaticAbility::Wither),
                    obj.has_keyword(StaticAbility::Infect),
                    obj.has_keyword(StaticAbility::Deathtouch),
                    obj.has_keyword(StaticAbility::Lifelink),
                    obj.controller,
                )
            };
            if blk_power > 0 {
                if blk_wither || blk_infect {
                    *wither_to_objects.entry(attacker_id).or_insert(0) += blk_power;
                } else {
                    *damage_to_objects.entry(attacker_id).or_insert(0) += blk_power;
                }
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
    // Apply wither/infect counter damage to creatures.
    for (oid, n) in wither_to_objects {
        if let Some(perm) = state.battlefield.get_mut(&oid) {
            perm.add_counters(
                CounterKind::PtModifier {
                    power: -1,
                    toughness: -1,
                },
                n,
            );
        }
    }
    // Apply infect/toxic poison counters to players.
    for (pid, n) in poison_to_players {
        if let Some(p) = state.get_player_mut(pid) {
            p.add_counters(CounterKind::Poison, n);
        }
    }

    let (mut state, sba_triggers) = check_and_apply_sbas(state);
    for t in sba_triggers {
        let id = t.id;
        state.stack.push(id);
        state.stack_objects.insert(id, t);
    }

    // CR 603.2: fire DealsCombatDamage events and collect triggered abilities.
    // Fired after SBAs so that creatures destroyed by combat damage are already removed,
    // matching the rule that triggered abilities wait for SBAs before going on the stack.
    let mut combat_triggers = Vec::new();
    for (attacker_id, target_kind) in combat_damage_events {
        let mut t = crate::engine::triggered::collect_triggers_for_event(
            &mut state,
            &crate::types::GameEvent::DealsCombatDamage {
                subject_id: attacker_id,
                to: target_kind,
            },
        );
        combat_triggers.append(&mut t);
    }
    for trigger in combat_triggers {
        let id = trigger.id;
        state.stack.push(id);
        state.stack_objects.insert(id, trigger);
    }
    if !state.stack.is_empty() {
        state.consecutive_passes = 0;
        state.priority_player = state.active_player;
    }

    state
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
            CardDefinition, Rule, RulesText,
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
            rules_text: keywords
                .into_iter()
                .map(|k| RulesText::Active(Rule::Static(k)))
                .collect(),
            text_annotations: vec![],
            power: Some(power),
            toughness: Some(toughness),
            colors: vec![],
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
        use crate::engine::triggered::exalted_triggered_ability;
        use crate::types::RulesText;
        use crate::types::ability::Rule;
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
            rules_text: vec![],
            text_annotations: vec![],
            power: Some(2),
            toughness: Some(2),
            colors: vec![],
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
            rules_text: vec![RulesText::Active(Rule::Triggered(
                exalted_triggered_ability(),
            ))],
            text_annotations: vec![],
            power: Some(1),
            toughness: Some(1),
            colors: vec![],
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
        // CR 702.25a: Flanking trigger fires when a non-Flanking creature blocks this.
        use crate::engine::triggered::flanking_triggered_ability;
        use crate::types::RulesText;
        use crate::types::ability::Rule;
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
            rules_text: vec![RulesText::Active(Rule::Triggered(
                flanking_triggered_ability(),
            ))],
            text_annotations: vec![],
            power: Some(2),
            toughness: Some(2),
            colors: vec![],
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
            rules_text: vec![],
            text_annotations: vec![],
            power: Some(2),
            toughness: Some(2),
            colors: vec![],
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

    fn place_creature_with_colors(
        state: &mut GameState,
        owner: PlayerId,
        rules_text: Vec<crate::types::RulesText>,
        colors: Vec<crate::types::mana::ManaColor>,
    ) -> ObjectId {
        use crate::types::card::{CardDefinition, CardType, TypeLine};
        let def = CardDefinition {
            name: "Creature".into(),
            mana_cost: None,
            type_line: TypeLine {
                supertypes: vec![],
                card_types: vec![CardType::Creature],
                subtypes: vec![],
            },
            oracle_text: String::new(),
            rules_text,
            text_annotations: vec![],
            power: Some(2),
            toughness: Some(2),
            colors,
        };
        let id = state.alloc_id();
        let obj = crate::types::CardObject::new(id, def, owner, Zone::Battlefield);
        let mut perm = PermanentState::new(&obj.definition);
        perm.controller_since_turn = 0;
        state.battlefield.insert(id, perm);
        state.add_object(obj);
        id
    }

    #[test]
    fn fear_blocks_non_artifact_non_black_creature() {
        use crate::types::RulesText;
        use crate::types::ability::{Rule, StaticAbility};
        use crate::types::mana::ManaColor;
        let mut gs = make_combat_state();
        let attacker = place_creature_with_colors(
            &mut gs,
            PlayerId(0),
            vec![RulesText::Active(Rule::Static(StaticAbility::Fear))],
            vec![ManaColor::Black],
        );
        gs.combat.attackers = vec![attacker];
        gs.active_player = PlayerId(0);
        // Green blocker — not artifact, not black
        let blocker =
            place_creature_with_colors(&mut gs, PlayerId(1), vec![], vec![ManaColor::Green]);
        assert!(!can_block_attacker(&gs, blocker, attacker));
    }

    #[test]
    fn fear_allows_black_creature_to_block() {
        use crate::types::RulesText;
        use crate::types::ability::{Rule, StaticAbility};
        use crate::types::mana::ManaColor;
        let mut gs = make_combat_state();
        let attacker = place_creature_with_colors(
            &mut gs,
            PlayerId(0),
            vec![RulesText::Active(Rule::Static(StaticAbility::Fear))],
            vec![],
        );
        gs.combat.attackers = vec![attacker];
        gs.active_player = PlayerId(0);
        let blocker =
            place_creature_with_colors(&mut gs, PlayerId(1), vec![], vec![ManaColor::Black]);
        assert!(can_block_attacker(&gs, blocker, attacker));
    }

    #[test]
    fn intimidate_blocks_different_color_non_artifact() {
        use crate::types::RulesText;
        use crate::types::ability::{Rule, StaticAbility};
        use crate::types::mana::ManaColor;
        let mut gs = make_combat_state();
        let attacker = place_creature_with_colors(
            &mut gs,
            PlayerId(0),
            vec![RulesText::Active(Rule::Static(StaticAbility::Intimidate))],
            vec![ManaColor::Red],
        );
        gs.combat.attackers = vec![attacker];
        gs.active_player = PlayerId(0);
        // Blue blocker shares no color with Red attacker
        let blocker =
            place_creature_with_colors(&mut gs, PlayerId(1), vec![], vec![ManaColor::Blue]);
        assert!(!can_block_attacker(&gs, blocker, attacker));
    }

    #[test]
    fn intimidate_allows_same_color_blocker() {
        use crate::types::RulesText;
        use crate::types::ability::{Rule, StaticAbility};
        use crate::types::mana::ManaColor;
        let mut gs = make_combat_state();
        let attacker = place_creature_with_colors(
            &mut gs,
            PlayerId(0),
            vec![RulesText::Active(Rule::Static(StaticAbility::Intimidate))],
            vec![ManaColor::Red],
        );
        gs.combat.attackers = vec![attacker];
        gs.active_player = PlayerId(0);
        let blocker = place_creature_with_colors(
            &mut gs,
            PlayerId(1),
            vec![],
            vec![ManaColor::Red, ManaColor::Green],
        );
        assert!(can_block_attacker(&gs, blocker, attacker));
    }

    #[test]
    fn islandwalk_unblockable_when_defender_controls_island() {
        use crate::types::RulesText;
        use crate::types::ability::{LandwalkKind, Rule, StaticAbility};
        use crate::types::card::{CardDefinition, CardType, Supertype, TypeLine};
        let mut gs = make_combat_state();
        let attacker = place_creature_with_colors(
            &mut gs,
            PlayerId(0),
            vec![RulesText::Active(Rule::Static(StaticAbility::Landwalk(
                LandwalkKind::LandType("Island".to_string()),
            )))],
            vec![],
        );
        gs.combat.attackers = vec![attacker];
        gs.active_player = PlayerId(0);
        // Place an Island under PlayerId(1)'s control
        let island_def = CardDefinition {
            name: "Island".into(),
            mana_cost: None,
            type_line: TypeLine {
                supertypes: vec![Supertype::Basic],
                card_types: vec![CardType::Land],
                subtypes: vec!["Island".to_string()],
            },
            oracle_text: String::new(),
            rules_text: vec![],
            text_annotations: vec![],
            power: None,
            toughness: None,
            colors: vec![],
        };
        let land_id = gs.alloc_id();
        let obj = CardObject::new(land_id, island_def, PlayerId(1), Zone::Battlefield);
        gs.battlefield
            .insert(land_id, PermanentState::new(&obj.definition));
        gs.add_object(obj);
        let blocker = place_creature_with_colors(&mut gs, PlayerId(1), vec![], vec![]);
        assert!(!can_block_attacker(&gs, blocker, attacker));
    }

    #[test]
    fn islandwalk_blockable_when_no_island_on_battlefield() {
        use crate::types::RulesText;
        use crate::types::ability::{LandwalkKind, Rule, StaticAbility};
        let mut gs = make_combat_state();
        let attacker = place_creature_with_colors(
            &mut gs,
            PlayerId(0),
            vec![RulesText::Active(Rule::Static(StaticAbility::Landwalk(
                LandwalkKind::LandType("Island".to_string()),
            )))],
            vec![],
        );
        gs.combat.attackers = vec![attacker];
        gs.active_player = PlayerId(0);
        let blocker = place_creature_with_colors(&mut gs, PlayerId(1), vec![], vec![]);
        assert!(can_block_attacker(&gs, blocker, attacker));
    }

    #[test]
    fn protection_from_red_blocks_red_blocker() {
        use crate::types::RulesText;
        use crate::types::ability::{Rule, StaticAbility};
        use crate::types::mana::ManaColor;
        let mut gs = make_combat_state();
        let attacker = place_creature_with_colors(
            &mut gs,
            PlayerId(0),
            vec![RulesText::Active(Rule::Static(
                StaticAbility::ProtectionFromColor(ManaColor::Red),
            ))],
            vec![],
        );
        gs.combat.attackers = vec![attacker];
        gs.active_player = PlayerId(0);
        let blocker =
            place_creature_with_colors(&mut gs, PlayerId(1), vec![], vec![ManaColor::Red]);
        assert!(!can_block_attacker(&gs, blocker, attacker));
    }

    #[test]
    fn protection_from_red_allows_blue_blocker() {
        use crate::types::RulesText;
        use crate::types::ability::{Rule, StaticAbility};
        use crate::types::mana::ManaColor;
        let mut gs = make_combat_state();
        let attacker = place_creature_with_colors(
            &mut gs,
            PlayerId(0),
            vec![RulesText::Active(Rule::Static(
                StaticAbility::ProtectionFromColor(ManaColor::Red),
            ))],
            vec![],
        );
        gs.combat.attackers = vec![attacker];
        gs.active_player = PlayerId(0);
        let blocker =
            place_creature_with_colors(&mut gs, PlayerId(1), vec![], vec![ManaColor::Blue]);
        assert!(can_block_attacker(&gs, blocker, attacker));
    }

    #[test]
    fn wither_deals_minus_counters_to_blocker_not_marked_damage() {
        // CR 702.80a: Wither routes creature damage as -1/-1 counters.
        use crate::types::CounterKind;
        let mut gs = make_combat_state();
        let attacker_id = keyword_creature(&mut gs, PlayerId(0), 2, 2, vec![StaticAbility::Wither]);
        let blocker_id = keyword_creature(&mut gs, PlayerId(1), 3, 3, vec![]);
        gs.combat.attackers = vec![attacker_id];
        gs.combat.blocking_map = [(attacker_id, vec![blocker_id])].into();

        let gs = deal_combat_damage(gs);

        let perm = &gs.battlefield[&blocker_id];
        assert_eq!(
            perm.damage_marked, 0,
            "Wither damage must not be marked damage"
        );
        assert_eq!(
            perm.counter_count(&CounterKind::PtModifier {
                power: -1,
                toughness: -1
            }),
            2,
            "Wither attacker (power 2) should give 2 × -1/-1 counters to blocker"
        );
    }

    #[test]
    fn wither_unblocked_still_deals_life_damage_to_player() {
        // CR 702.80a: Wither only changes creature damage; player damage is still life loss.
        use crate::types::CounterKind;
        let mut gs = make_combat_state();
        let attacker_id = keyword_creature(&mut gs, PlayerId(0), 2, 2, vec![StaticAbility::Wither]);
        gs.combat.attackers = vec![attacker_id];
        gs.combat.blocking_map = [(attacker_id, vec![])].into();

        let gs = deal_combat_damage(gs);

        let defender = gs.players.iter().find(|p| p.id == PlayerId(1)).unwrap();
        assert_eq!(
            defender.life, 18,
            "Wither unblocked attacker deals normal life damage to player"
        );
        assert_eq!(defender.counter_count(&CounterKind::Poison), 0);
    }

    #[test]
    fn infect_deals_minus_counters_to_blocker_and_poison_to_player() {
        // CR 702.90a: Infect → -1/-1 to creatures, poison to players.
        use crate::types::CounterKind;
        let mut gs = make_combat_state();
        let attacker_id = keyword_creature(&mut gs, PlayerId(0), 3, 3, vec![StaticAbility::Infect]);
        gs.combat.attackers = vec![attacker_id];
        gs.combat.blocking_map = [(attacker_id, vec![])].into(); // unblocked → hits player

        let gs = deal_combat_damage(gs);

        let defender = gs.players.iter().find(|p| p.id == PlayerId(1)).unwrap();
        assert_eq!(defender.life, 20, "Infect does not reduce player life");
        assert_eq!(
            defender.counter_count(&CounterKind::Poison),
            3,
            "Infect gives poison counters equal to damage"
        );
    }

    #[test]
    fn infect_blocked_deals_minus_counters_not_marked_damage() {
        use crate::types::CounterKind;
        let mut gs = make_combat_state();
        let attacker_id = keyword_creature(&mut gs, PlayerId(0), 2, 2, vec![StaticAbility::Infect]);
        let blocker_id = keyword_creature(&mut gs, PlayerId(1), 3, 3, vec![]);
        gs.combat.attackers = vec![attacker_id];
        gs.combat.blocking_map = [(attacker_id, vec![blocker_id])].into();

        let gs = deal_combat_damage(gs);

        let perm = &gs.battlefield[&blocker_id];
        assert_eq!(perm.damage_marked, 0);
        assert_eq!(
            perm.counter_count(&CounterKind::PtModifier {
                power: -1,
                toughness: -1
            }),
            2
        );
    }

    #[test]
    fn toxic_adds_poison_counters_in_addition_to_life_damage() {
        // CR 702.164a: Toxic N gives N additional poison counters when dealing combat damage to a player.
        use crate::types::CounterKind;
        let mut gs = make_combat_state();
        let attacker_id =
            keyword_creature(&mut gs, PlayerId(0), 2, 2, vec![StaticAbility::ToxicN(2)]);
        gs.combat.attackers = vec![attacker_id];
        gs.combat.blocking_map = [(attacker_id, vec![])].into();

        let gs = deal_combat_damage(gs);

        let defender = gs.players.iter().find(|p| p.id == PlayerId(1)).unwrap();
        assert_eq!(defender.life, 18, "Toxic does not suppress life damage");
        assert_eq!(
            defender.counter_count(&CounterKind::Poison),
            2,
            "Toxic 2 gives 2 poison counters"
        );
    }

    #[test]
    fn infect_and_toxic_together_stack_poison() {
        // A creature with both Infect and Toxic 2 that deals 3 combat damage to a player:
        // 3 poison from Infect + 2 poison from Toxic 2 = 5 poison total, 0 life loss.
        use crate::types::CounterKind;
        let mut gs = make_combat_state();
        let attacker_id = keyword_creature(
            &mut gs,
            PlayerId(0),
            3,
            3,
            vec![StaticAbility::Infect, StaticAbility::ToxicN(2)],
        );
        gs.combat.attackers = vec![attacker_id];
        gs.combat.blocking_map = [(attacker_id, vec![])].into();

        let gs = deal_combat_damage(gs);

        let defender = gs.players.iter().find(|p| p.id == PlayerId(1)).unwrap();
        assert_eq!(defender.life, 20);
        assert_eq!(defender.counter_count(&CounterKind::Poison), 5);
    }

    #[test]
    fn lifelink_still_triggers_with_wither() {
        // CR 702.15a: Lifelink counts total damage dealt regardless of form.
        let mut gs = make_combat_state();
        let attacker_id = keyword_creature(
            &mut gs,
            PlayerId(0),
            2,
            2,
            vec![StaticAbility::Wither, StaticAbility::Lifelink],
        );
        let blocker_id = keyword_creature(&mut gs, PlayerId(1), 3, 3, vec![]);
        gs.combat.attackers = vec![attacker_id];
        gs.combat.blocking_map = [(attacker_id, vec![blocker_id])].into();

        let gs = deal_combat_damage(gs);

        let attacker_controller = gs.players.iter().find(|p| p.id == PlayerId(0)).unwrap();
        assert_eq!(
            attacker_controller.life, 22,
            "Lifelink should gain 2 life (2 wither damage dealt)"
        );
    }

    #[test]
    fn wither_blocker_deals_minus_counters_to_attacker() {
        // CR 702.80a: Wither applies to damage from any source with the keyword.
        use crate::types::CounterKind;
        let mut gs = make_combat_state();
        let attacker_id = keyword_creature(&mut gs, PlayerId(0), 3, 3, vec![]);
        let blocker_id = keyword_creature(&mut gs, PlayerId(1), 2, 2, vec![StaticAbility::Wither]);
        gs.combat.attackers = vec![attacker_id];
        gs.combat.blocking_map = [(attacker_id, vec![blocker_id])].into();

        let gs = deal_combat_damage(gs);

        let perm = &gs.battlefield[&attacker_id];
        assert_eq!(
            perm.damage_marked, 0,
            "Blocker with Wither should not leave marked damage"
        );
        assert_eq!(
            perm.counter_count(&CounterKind::PtModifier {
                power: -1,
                toughness: -1
            }),
            2
        );
    }

    // --- DealsCombatDamage event emission tests ---

    #[test]
    fn unblocked_attacker_fires_deals_combat_damage_to_player_trigger() {
        // CR 603.2: a DealsCombatDamage event is fired for each attacker that deals combat
        // damage to a player. A creature with a DealsCombatDamage trigger should put a stack
        // object on the stack.
        use crate::engine::triggered::deals_combat_damage_to_player_triggered_ability;
        use crate::types::RulesText;
        use crate::types::ability::Rule;

        let mut gs = make_combat_state();
        // A 2/2 creature with "whenever this deals combat damage to a player, draw a card".
        let ability_span = RulesText::Active(Rule::Triggered(
            deals_combat_damage_to_player_triggered_ability(),
        ));
        use crate::types::card::{CardDefinition, CardType, TypeLine};
        let def = CardDefinition {
            name: "Coastal Piracy Creature".into(),
            mana_cost: None,
            type_line: TypeLine {
                supertypes: vec![],
                card_types: vec![CardType::Creature],
                subtypes: vec![],
            },
            oracle_text: "Whenever this deals combat damage to a player, draw a card.".into(),
            rules_text: vec![ability_span],
            text_annotations: vec![],
            power: Some(2),
            toughness: Some(2),
            colors: vec![],
        };
        let attacker_id = add_creature(&mut gs, PlayerId(0), def);
        gs.combat.attackers = vec![attacker_id];
        gs.combat.blocking_map = [(attacker_id, vec![])].into();

        let gs = deal_combat_damage(gs);

        // A trigger should have been put on the stack.
        assert_eq!(
            gs.stack.len(),
            1,
            "DealsCombatDamage trigger should be on the stack"
        );
    }

    #[test]
    fn blocked_attacker_deals_no_player_damage_no_trigger() {
        // A creature that is fully blocked (no trample) should not fire a
        // DealsCombatDamage-to-player trigger.
        use crate::engine::triggered::deals_combat_damage_to_player_triggered_ability;
        use crate::types::RulesText;
        use crate::types::ability::Rule;
        use crate::types::card::{CardDefinition, CardType, TypeLine};

        let mut gs = make_combat_state();
        let ability_span = RulesText::Active(Rule::Triggered(
            deals_combat_damage_to_player_triggered_ability(),
        ));
        let def = CardDefinition {
            name: "Piracy Creature".into(),
            mana_cost: None,
            type_line: TypeLine {
                supertypes: vec![],
                card_types: vec![CardType::Creature],
                subtypes: vec![],
            },
            oracle_text: String::new(),
            rules_text: vec![ability_span],
            text_annotations: vec![],
            power: Some(2),
            toughness: Some(2),
            colors: vec![],
        };
        let attacker_id = add_creature(&mut gs, PlayerId(0), def);
        let blocker_id = keyword_creature(&mut gs, PlayerId(1), 2, 2, vec![]);
        gs.combat.attackers = vec![attacker_id];
        gs.combat.blocking_map = [(attacker_id, vec![blocker_id])].into();

        let gs = deal_combat_damage(gs);

        // No trigger should fire — attacker dealt no damage to a player.
        assert_eq!(
            gs.stack.len(),
            0,
            "No DealsCombatDamage trigger should fire when blocked with no trample"
        );
    }

    #[test]
    fn trample_attacker_fires_deals_combat_damage_to_player_trigger() {
        // Trample attacker: excess goes to player, so trigger fires.
        use crate::engine::triggered::deals_combat_damage_to_player_triggered_ability;
        use crate::types::RulesText;
        use crate::types::ability::Rule;
        use crate::types::card::{CardDefinition, CardType, TypeLine};

        let mut gs = make_combat_state();
        let ability_span = RulesText::Active(Rule::Triggered(
            deals_combat_damage_to_player_triggered_ability(),
        ));
        let def = CardDefinition {
            name: "Trample Piracy".into(),
            mana_cost: None,
            type_line: TypeLine {
                supertypes: vec![],
                card_types: vec![CardType::Creature],
                subtypes: vec![],
            },
            oracle_text: String::new(),
            rules_text: vec![
                RulesText::Active(Rule::Static(StaticAbility::Trample)),
                ability_span,
            ],
            text_annotations: vec![],
            power: Some(5),
            toughness: Some(5),
            colors: vec![],
        };
        let attacker_id = add_creature(&mut gs, PlayerId(0), def);
        let blocker_id = keyword_creature(&mut gs, PlayerId(1), 2, 2, vec![]);
        gs.combat.attackers = vec![attacker_id];
        gs.combat.blocking_map = [(attacker_id, vec![blocker_id])].into();

        let gs = deal_combat_damage(gs);

        assert!(
            !gs.stack.is_empty(),
            "Trample attacker should fire DealsCombatDamage trigger (excess to player)"
        );
    }

    #[test]
    fn anthem_increases_attacker_damage() {
        // Attacker: 2/2 Grizzly Bears with a "creatures you control get +1/+0" anthem.
        // Expected attacker power = 3, expected to deal 3 damage to the player.
        use crate::types::{
            CardObject, ContinuousEffect, ControllerFilter, PTDelta, PermanentFilter,
            PermanentState, Rule, RulesText, Zone,
            card::{CardDefinition, CardType, TypeLine},
            mana::ManaColor,
        };

        let db = test_db();

        let anthem_def = CardDefinition {
            name: "Power Anthem".into(),
            mana_cost: None,
            type_line: TypeLine {
                supertypes: vec![],
                card_types: vec![CardType::Enchantment],
                subtypes: vec![],
            },
            oracle_text: "Creatures you control get +1/+0.".into(),
            rules_text: vec![RulesText::Active(Rule::Continuous(ContinuousEffect {
                subject_filter: PermanentFilter {
                    controller: ControllerFilter::You,
                    card_types: vec![CardType::Creature],
                    ..Default::default()
                },
                pt_modification: Some(PTDelta {
                    power: 1,
                    toughness: 0,
                }),
            }))],
            text_annotations: vec![],
            power: None,
            toughness: None,
            colors: vec![ManaColor::White],
        };

        let mut gs = make_combat_state();
        let bear_id = add_creature(
            &mut gs,
            PlayerId(0),
            db.get("Grizzly Bears").unwrap().clone(),
        );
        let anthem_id = gs.alloc_id();
        let anthem_obj = CardObject::new(anthem_id, anthem_def, PlayerId(0), Zone::Battlefield);
        gs.battlefield
            .insert(anthem_id, PermanentState::new(&anthem_obj.definition));
        gs.add_object(anthem_obj);

        gs = declare_attackers(gs, PlayerId(0), &[bear_id]).unwrap();
        gs.step = Step::DeclareBlockers;
        gs = declare_blockers(gs, PlayerId(1), &[]).unwrap();
        gs.step = Step::CombatDamage;

        let gs = deal_combat_damage(gs);

        // Bears deals 2+1=3 damage to player 1.
        assert_eq!(gs.get_player(PlayerId(1)).unwrap().life, 17);
    }
}
