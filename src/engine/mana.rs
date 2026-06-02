use crate::types::{GameState, ManaCost, ManaColor, ObjectId, PlayerId, Zone};
use super::EngineError;

/// Tap a basic land on the battlefield to add one mana to its controller's pool.
pub fn tap_land_for_mana(mut state: GameState, object_id: ObjectId) -> Result<GameState, EngineError> {
    let (controller, color) = {
        let obj = state.objects.get(&object_id).ok_or(EngineError::CardNotFound)?;
        if obj.zone != Zone::Battlefield { return Err(EngineError::CardNotOnBattlefield); }
        if obj.tapped { return Err(EngineError::AlreadyTapped); }
        if !obj.is_land() { return Err(EngineError::NotALand); }
        (obj.controller, land_produces(&obj.definition.type_line.subtypes))
    };

    state.objects.get_mut(&object_id).unwrap().tapped = true;
    state.get_player_mut(controller).unwrap().mana_pool.add(color, 1);
    Ok(state)
}

fn land_produces(subtypes: &[String]) -> ManaColor {
    for s in subtypes {
        match s.as_str() {
            "Plains"   => return ManaColor::White,
            "Island"   => return ManaColor::Blue,
            "Swamp"    => return ManaColor::Black,
            "Mountain" => return ManaColor::Red,
            "Forest"   => return ManaColor::Green,
            _          => {}
        }
    }
    ManaColor::Colorless
}

/// Deduct a mana cost from a player's pool. Colored mana is paid first;
/// the generic portion is satisfied by any remaining mana.
#[allow(unused_assignments)]
pub fn pay_mana_cost(mut state: GameState, player_id: PlayerId, cost: &ManaCost) -> Result<GameState, EngineError> {
    {
        let pool = &state.get_player(player_id).ok_or(EngineError::CardNotFound)?.mana_pool;
        if pool.white < cost.white || pool.blue < cost.blue || pool.black < cost.black
            || pool.red < cost.red || pool.green < cost.green || pool.colorless < cost.colorless
        {
            return Err(EngineError::InsufficientMana);
        }
        let after_colored = pool.total() - cost.total_colored();
        if after_colored < cost.generic { return Err(EngineError::InsufficientMana); }
    }

    let player = state.get_player_mut(player_id).unwrap();
    player.mana_pool.white    -= cost.white;
    player.mana_pool.blue     -= cost.blue;
    player.mana_pool.black    -= cost.black;
    player.mana_pool.red      -= cost.red;
    player.mana_pool.green    -= cost.green;
    player.mana_pool.colorless -= cost.colorless;

    let mut remaining = cost.generic;
    let pool = &mut player.mana_pool;
    macro_rules! spend {
        ($field:ident) => {
            let s = remaining.min(pool.$field);
            pool.$field -= s;
            remaining   -= s;
        };
    }
    spend!(white); spend!(blue); spend!(black);
    spend!(red);   spend!(green); spend!(colorless);

    Ok(state)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::{CardDefinition, CardObject, ManaCost, Player};

    fn make_state() -> GameState {
        GameState::new(vec![
            Player::new(PlayerId(0), "Alice"),
            Player::new(PlayerId(1), "Bob"),
        ])
    }

    fn add_land(state: &mut GameState, owner: PlayerId, def: CardDefinition) -> ObjectId {
        let id = state.alloc_id();
        let mut obj = CardObject::new(id, def, owner, Zone::Battlefield);
        obj.summoning_sick = false;
        state.battlefield.push(id);
        state.add_object(obj);
        id
    }

    #[test]
    fn tap_forest_adds_green_mana() {
        let mut gs = make_state();
        let forest_id = add_land(&mut gs, PlayerId(0), CardDefinition::forest());

        let gs = tap_land_for_mana(gs, forest_id).unwrap();

        assert!(gs.objects[&forest_id].tapped);
        assert_eq!(gs.get_player(PlayerId(0)).unwrap().mana_pool.green, 1);
    }

    #[test]
    fn cannot_tap_already_tapped_land() {
        let mut gs = make_state();
        let forest_id = add_land(&mut gs, PlayerId(0), CardDefinition::forest());
        gs.objects.get_mut(&forest_id).unwrap().tapped = true;

        assert!(matches!(tap_land_for_mana(gs, forest_id), Err(EngineError::AlreadyTapped)));
    }

    #[test]
    fn pay_1g_with_green_and_any() {
        let mut gs = make_state();
        gs.get_player_mut(PlayerId(0)).unwrap().mana_pool.green += 2;
        let cost = ManaCost { generic: 1, green: 1, ..Default::default() };

        let gs = pay_mana_cost(gs, PlayerId(0), &cost).unwrap();

        assert!(gs.get_player(PlayerId(0)).unwrap().mana_pool.is_empty());
    }

    #[test]
    fn cannot_pay_1g_with_only_1_green() {
        let mut gs = make_state();
        gs.get_player_mut(PlayerId(0)).unwrap().mana_pool.green += 1;
        let cost = ManaCost { generic: 1, green: 1, ..Default::default() };

        assert!(matches!(pay_mana_cost(gs, PlayerId(0), &cost), Err(EngineError::InsufficientMana)));
    }

    #[test]
    fn cannot_pay_g_with_red_mana() {
        let mut gs = make_state();
        gs.get_player_mut(PlayerId(0)).unwrap().mana_pool.red += 2;
        let cost = ManaCost { green: 1, ..Default::default() };

        assert!(matches!(pay_mana_cost(gs, PlayerId(0), &cost), Err(EngineError::InsufficientMana)));
    }

    #[test]
    fn generic_cost_satisfied_by_any_color() {
        let mut gs = make_state();
        gs.get_player_mut(PlayerId(0)).unwrap().mana_pool.red   += 1;
        gs.get_player_mut(PlayerId(0)).unwrap().mana_pool.green += 1;
        let cost = ManaCost { generic: 2, ..Default::default() };

        let gs = pay_mana_cost(gs, PlayerId(0), &cost).unwrap();

        assert!(gs.get_player(PlayerId(0)).unwrap().mana_pool.is_empty());
    }
}
