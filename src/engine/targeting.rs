use crate::types::ability::{StaticAbility, TargetFilter};
use crate::types::effect::EffectTarget;
use crate::types::{GameState, PlayerId, Zone};

// CR 115.4: a target is legal if it exists in the targeted zone, satisfies the
// filter, and is not protected by Shroud (CR 702.18) or Hexproof (CR 702.11).
pub fn is_legal_target(
    state: &GameState,
    target: &EffectTarget,
    filter: TargetFilter,
    caster: PlayerId,
) -> bool {
    match target {
        EffectTarget::Object { id } => {
            let obj = match state.objects.get(id) {
                Some(o) => o,
                None => return false,
            };
            if obj.zone != Zone::Battlefield {
                return false;
            }
            let passes_filter = match filter {
                TargetFilter::Creature => obj.is_creature(),
                TargetFilter::Player => false,
                TargetFilter::Any => obj.is_creature(), // planeswalkers/battles: future
            };
            if !passes_filter {
                return false;
            }
            // CR 702.18: Shroud prevents targeting by anyone
            if obj.has_keyword(StaticAbility::Shroud) {
                return false;
            }
            // CR 702.11: Hexproof prevents targeting by opponents
            if obj.has_keyword(StaticAbility::Hexproof) && obj.controller != caster {
                return false;
            }
            true
        }
        EffectTarget::Player { id } => {
            let player = match state.get_player(*id) {
                Some(p) => p,
                None => return false,
            };
            if player.has_lost {
                return false;
            }
            matches!(filter, TargetFilter::Player | TargetFilter::Any)
        }
    }
}

/// Returns all legal targets for `filter` from `caster`'s point of view.
pub fn legal_targets(
    state: &GameState,
    filter: TargetFilter,
    caster: PlayerId,
) -> Vec<EffectTarget> {
    let mut result = Vec::new();
    if matches!(filter, TargetFilter::Creature | TargetFilter::Any) {
        for &id in state.battlefield.keys() {
            let t = EffectTarget::Object { id };
            if is_legal_target(state, &t, filter, caster) {
                result.push(t);
            }
        }
    }
    if matches!(filter, TargetFilter::Player | TargetFilter::Any) {
        for player in &state.players {
            let t = EffectTarget::Player { id: player.id };
            if is_legal_target(state, &t, filter, caster) {
                result.push(t);
            }
        }
    }
    result
}

// CR 608.2b: targets are still legal if the object/player still exists in the
// required zone. (Does not re-check Shroud/Hexproof — those apply at declaration
// time, not at resolution time.)
pub fn targets_still_legal(state: &GameState, targets: &[EffectTarget]) -> bool {
    if targets.is_empty() {
        return true;
    }
    targets.iter().all(|t| match t {
        EffectTarget::Object { id } => state
            .objects
            .get(id)
            .map(|o| o.zone == Zone::Battlefield)
            .unwrap_or(false),
        EffectTarget::Player { id } => state.get_player(*id).map(|p| !p.has_lost).unwrap_or(false),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::ability::Ability;
    use crate::types::card::{CardDefinition, CardType, TypeLine};
    use crate::types::{CardObject, OracleSpan, PermanentState, Player};

    fn two_player_state() -> GameState {
        GameState::new(vec![
            Player::new(PlayerId(0), "Alice"),
            Player::new(PlayerId(1), "Bob"),
        ])
    }

    fn place_creature(
        state: &mut GameState,
        owner: PlayerId,
        abilities: Vec<OracleSpan>,
    ) -> crate::types::ObjectId {
        let def = CardDefinition {
            name: "Test Creature".into(),
            mana_cost: None,
            type_line: TypeLine {
                supertypes: vec![],
                card_types: vec![CardType::Creature],
                subtypes: vec![],
            },
            oracle_text: String::new(),
            abilities,
            text_annotations: vec![],
            power: Some(2),
            toughness: Some(2),
            colors: vec![],
        };
        let id = state.alloc_id();
        let obj = CardObject::new(id, def, owner, Zone::Battlefield);
        state
            .battlefield
            .insert(id, PermanentState::new(&obj.definition));
        state.add_object(obj);
        id
    }

    #[test]
    fn creature_on_battlefield_is_legal_target_for_creature_filter() {
        let mut gs = two_player_state();
        let id = place_creature(&mut gs, PlayerId(1), vec![]);
        let target = EffectTarget::Object { id };
        assert!(is_legal_target(
            &gs,
            &target,
            TargetFilter::Creature,
            PlayerId(0)
        ));
    }

    #[test]
    fn nonexistent_object_is_not_legal_target() {
        use crate::types::ObjectId;
        let gs = two_player_state();
        let target = EffectTarget::Object { id: ObjectId(999) };
        assert!(!is_legal_target(
            &gs,
            &target,
            TargetFilter::Creature,
            PlayerId(0)
        ));
    }

    #[test]
    fn object_not_on_battlefield_is_not_legal_target() {
        let mut gs = two_player_state();
        let def = CardDefinition {
            name: "Hand Card".into(),
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
        let id = gs.alloc_id();
        let obj = CardObject::new(id, def, PlayerId(1), Zone::Hand);
        gs.add_object(obj);
        let target = EffectTarget::Object { id };
        assert!(!is_legal_target(
            &gs,
            &target,
            TargetFilter::Creature,
            PlayerId(0)
        ));
    }

    #[test]
    fn creature_with_shroud_is_not_legal_target_for_anyone() {
        let mut gs = two_player_state();
        let id = place_creature(
            &mut gs,
            PlayerId(1),
            vec![OracleSpan::Parsed(Ability::Static(StaticAbility::Shroud))],
        );
        let target = EffectTarget::Object { id };
        assert!(!is_legal_target(
            &gs,
            &target,
            TargetFilter::Creature,
            PlayerId(0)
        ));
        assert!(!is_legal_target(
            &gs,
            &target,
            TargetFilter::Creature,
            PlayerId(1)
        ));
    }

    #[test]
    fn creature_with_hexproof_is_not_legal_target_for_opponent() {
        let mut gs = two_player_state();
        let id = place_creature(
            &mut gs,
            PlayerId(1),
            vec![OracleSpan::Parsed(Ability::Static(StaticAbility::Hexproof))],
        );
        let target = EffectTarget::Object { id };
        assert!(!is_legal_target(
            &gs,
            &target,
            TargetFilter::Creature,
            PlayerId(0)
        ));
    }

    #[test]
    fn creature_with_hexproof_is_legal_target_for_controller() {
        let mut gs = two_player_state();
        let id = place_creature(
            &mut gs,
            PlayerId(1),
            vec![OracleSpan::Parsed(Ability::Static(StaticAbility::Hexproof))],
        );
        let target = EffectTarget::Object { id };
        assert!(is_legal_target(
            &gs,
            &target,
            TargetFilter::Creature,
            PlayerId(1)
        ));
    }

    #[test]
    fn active_player_is_legal_player_target() {
        let gs = two_player_state();
        let target = EffectTarget::Player { id: PlayerId(0) };
        assert!(is_legal_target(
            &gs,
            &target,
            TargetFilter::Player,
            PlayerId(1)
        ));
    }

    #[test]
    fn any_filter_includes_creatures_and_players() {
        let mut gs = two_player_state();
        let creature_id = place_creature(&mut gs, PlayerId(1), vec![]);
        let targets = legal_targets(&gs, TargetFilter::Any, PlayerId(0));
        assert!(targets.contains(&EffectTarget::Object { id: creature_id }));
        assert!(targets.contains(&EffectTarget::Player { id: PlayerId(0) }));
        assert!(targets.contains(&EffectTarget::Player { id: PlayerId(1) }));
    }

    #[test]
    fn creature_filter_excludes_players() {
        let mut gs = two_player_state();
        let creature_id = place_creature(&mut gs, PlayerId(1), vec![]);
        let targets = legal_targets(&gs, TargetFilter::Creature, PlayerId(0));
        assert!(targets.contains(&EffectTarget::Object { id: creature_id }));
        assert!(!targets.contains(&EffectTarget::Player { id: PlayerId(0) }));
    }

    #[test]
    fn player_filter_excludes_creatures() {
        let mut gs = two_player_state();
        let creature_id = place_creature(&mut gs, PlayerId(1), vec![]);
        let targets = legal_targets(&gs, TargetFilter::Player, PlayerId(0));
        assert!(!targets.contains(&EffectTarget::Object { id: creature_id }));
        assert!(targets.contains(&EffectTarget::Player { id: PlayerId(0) }));
        assert!(targets.contains(&EffectTarget::Player { id: PlayerId(1) }));
    }

    #[test]
    fn targets_still_legal_true_when_creature_on_battlefield() {
        let mut gs = two_player_state();
        let id = place_creature(&mut gs, PlayerId(1), vec![]);
        let targets = vec![EffectTarget::Object { id }];
        assert!(targets_still_legal(&gs, &targets));
    }

    #[test]
    fn targets_still_legal_false_when_creature_off_battlefield() {
        let mut gs = two_player_state();
        let id = gs.alloc_id();
        let targets = vec![EffectTarget::Object { id }];
        assert!(!targets_still_legal(&gs, &targets));
    }

    #[test]
    fn targets_still_legal_true_for_player_alive() {
        let gs = two_player_state();
        let targets = vec![EffectTarget::Player { id: PlayerId(0) }];
        assert!(targets_still_legal(&gs, &targets));
    }

    #[test]
    fn targets_still_legal_false_for_player_who_lost() {
        let mut gs = two_player_state();
        gs.get_player_mut(PlayerId(1)).unwrap().has_lost = true;
        let targets = vec![EffectTarget::Player { id: PlayerId(1) }];
        assert!(!targets_still_legal(&gs, &targets));
    }

    #[test]
    fn targets_still_legal_true_for_empty_slice() {
        let gs = two_player_state();
        assert!(targets_still_legal(&gs, &[]));
    }
}
