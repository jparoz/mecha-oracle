use crate::types::{GameState, ObjectId, PlayerId, Zone, ability::StaticAbility};

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
    for &id in &state.battlefield {
        if let Some(obj) = state.objects.get(&id) {
            if obj.is_creature() && !obj.has_keyword(StaticAbility::Indestructible) {
                let lethal_damage = obj
                    .effective_toughness()
                    .map(|t| t <= 0 || obj.damage_marked as i32 >= t)
                    .unwrap_or(false);
                if lethal_damage || obj.damaged_by_deathtouch {
                    sbas.push(Sba::MoveToGraveyard(id));
                }
            }
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
        }
    }
    state
}

pub fn move_to_graveyard(mut state: GameState, object_id: ObjectId) -> GameState {
    state.battlefield.retain(|&id| id != object_id);
    if let Some(obj) = state.objects.get_mut(&object_id) {
        let owner = obj.owner;
        obj.zone = Zone::Graveyard;
        obj.damage_marked = 0;
        obj.damaged_by_deathtouch = false;
        obj.tapped = false;
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
    use crate::types::{CardObject, Player, Zone};

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
        let mut obj = CardObject::new(id, def, owner, Zone::Battlefield);
        obj.summoning_sick = false;
        state.battlefield.push(id);
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
        gs.objects.get_mut(&bear_id).unwrap().damage_marked = 2; // toughness = 2, lethal

        let gs = check_and_apply_sbas(gs);

        assert!(!gs.battlefield.contains(&bear_id));
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
        gs.objects.get_mut(&bear_id).unwrap().damage_marked = 1; // toughness = 2, survives

        let gs = check_and_apply_sbas(gs);

        assert!(gs.battlefield.contains(&bear_id));
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
        use crate::types::{AbilityAST, CardDefinition, CardType, TypeLine};
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
                .map(|k| AbilityAST::Static(k))
                .collect(),
            power: Some(power),
            toughness: Some(toughness),
        };
        let mut obj = CardObject::new(id, def, owner, Zone::Battlefield);
        obj.summoning_sick = false;
        state.battlefield.push(id);
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
        gs.objects.get_mut(&id).unwrap().damage_marked = 5; // more than toughness

        let gs = check_and_apply_sbas(gs);

        assert!(gs.battlefield.contains(&id)); // survives
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
        gs.objects.get_mut(&id).unwrap().damaged_by_deathtouch = true;

        let gs = check_and_apply_sbas(gs);

        assert!(!gs.battlefield.contains(&id));
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
        gs.objects.get_mut(&id).unwrap().damaged_by_deathtouch = true;

        let gs = check_and_apply_sbas(gs);

        assert!(gs.battlefield.contains(&id)); // indestructible ignores both 704.5g and 704.5h
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
        gs.objects.get_mut(&bear1).unwrap().damage_marked = 5;
        gs.objects.get_mut(&bear2).unwrap().damage_marked = 5;

        let gs = check_and_apply_sbas(gs);

        assert!(gs.battlefield.is_empty());
        assert_eq!(gs.graveyards[&PlayerId(0)].len(), 2);
    }
}
