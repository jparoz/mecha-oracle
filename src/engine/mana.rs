use super::EngineError;
use crate::types::{
    GameState, ManaCheckpoint, ManaColor, ManaCost, ManaPip, ManaPool, ObjectId, PaymentPlan,
    PlayerId, Zone,
};

/// Tap a basic land on the battlefield to add one mana to its controller's pool.
pub fn tap_land_for_mana(
    mut state: GameState,
    object_id: ObjectId,
) -> Result<GameState, EngineError> {
    let (controller, color, is_snow) = {
        let obj = state
            .objects
            .get(&object_id)
            .ok_or(EngineError::CardNotFound)?;
        if obj.zone != Zone::Battlefield {
            return Err(EngineError::CardNotOnBattlefield);
        }
        if obj.tapped {
            return Err(EngineError::AlreadyTapped);
        }
        if !obj.is_land() {
            return Err(EngineError::NotALand);
        }
        let is_snow = obj
            .definition
            .type_line
            .supertypes
            .contains(&crate::types::card::Supertype::Snow);
        (
            obj.controller,
            land_produces(&obj.definition.type_line.subtypes),
            is_snow,
        )
    };

    // Lazily create a checkpoint before the first mana tap in this priority window.
    if state.mana_checkpoint.is_none() {
        let pools = state
            .players
            .iter()
            .map(|p| (p.id, p.mana_pool.clone()))
            .collect();
        state.mana_checkpoint = Some(ManaCheckpoint {
            pools,
            tapped_lands: vec![],
        });
    }
    state
        .mana_checkpoint
        .as_mut()
        .unwrap()
        .tapped_lands
        .push(object_id);

    state.objects.get_mut(&object_id).unwrap().tapped = true;
    let player = state.get_player_mut(controller).unwrap();
    if is_snow {
        player.mana_pool.add_snow(color, 1);
    } else {
        player.mana_pool.add(color, 1);
    }
    Ok(state)
}

/// Undo all mana taps made in the current priority window. Restores each player's
/// mana pool and untaps every land recorded in the checkpoint, then clears it.
/// Returns `Err(NoManaCheckpoint)` if no taps have been made since the last commit.
pub fn reset_mana(mut state: GameState) -> Result<GameState, EngineError> {
    let checkpoint = state
        .mana_checkpoint
        .take()
        .ok_or(EngineError::NoManaCheckpoint)?;
    for player in state.players.iter_mut() {
        if let Some(pool) = checkpoint.pools.get(&player.id) {
            player.mana_pool = pool.clone();
        }
    }
    for &id in &checkpoint.tapped_lands {
        if let Some(obj) = state.objects.get_mut(&id) {
            obj.tapped = false;
        }
    }
    Ok(state)
}

fn land_produces(subtypes: &[String]) -> ManaColor {
    for s in subtypes {
        match s.as_str() {
            "Plains" => return ManaColor::White,
            "Island" => return ManaColor::Blue,
            "Swamp" => return ManaColor::Black,
            "Mountain" => return ManaColor::Red,
            "Forest" => return ManaColor::Green,
            _ => {}
        }
    }
    ManaColor::Colorless
}

fn amount_for_color(color: &ManaColor, rem: &ManaPool) -> u32 {
    match color {
        ManaColor::White => rem.white,
        ManaColor::Blue => rem.blue,
        ManaColor::Black => rem.black,
        ManaColor::Red => rem.red,
        ManaColor::Green => rem.green,
        ManaColor::Colorless => rem.colorless,
    }
}

fn deduct_one_color(color: &ManaColor, rem: &mut ManaPool, plan: &mut PaymentPlan) -> Option<()> {
    macro_rules! go {
        ($c:ident) => {{
            if rem.$c == 0 {
                return None;
            }
            rem.$c -= 1;
            plan.$c += 1;
            Some(())
        }};
    }
    match color {
        ManaColor::White => go!(white),
        ManaColor::Blue => go!(blue),
        ManaColor::Black => go!(black),
        ManaColor::Red => go!(red),
        ManaColor::Green => go!(green),
        ManaColor::Colorless => go!(colorless),
    }
}

fn spend_generic_rem(mut n: u32, rem: &mut ManaPool, plan: &mut PaymentPlan) {
    macro_rules! spend {
        ($f:ident) => {
            let s = n.min(rem.$f);
            rem.$f -= s;
            plan.$f += s;
            n -= s;
        };
    }
    spend!(white);
    spend!(blue);
    spend!(black);
    spend!(red);
    spend!(green);
    spend!(colorless);
    let _ = n;
}

fn pip_color_available(color: &ManaColor, plan: &PaymentPlan) -> bool {
    match color {
        ManaColor::White => plan.white > 0,
        ManaColor::Blue => plan.blue > 0,
        ManaColor::Black => plan.black > 0,
        ManaColor::Red => plan.red > 0,
        ManaColor::Green => plan.green > 0,
        ManaColor::Colorless => plan.colorless > 0,
    }
}

fn deduct_pip_color(color: &ManaColor, plan: &mut PaymentPlan) -> Option<()> {
    let field = match color {
        ManaColor::White => &mut plan.white,
        ManaColor::Blue => &mut plan.blue,
        ManaColor::Black => &mut plan.black,
        ManaColor::Red => &mut plan.red,
        ManaColor::Green => &mut plan.green,
        ManaColor::Colorless => &mut plan.colorless,
    };
    if *field == 0 {
        return None;
    }
    *field -= 1;
    Some(())
}

fn spend_from_rem(mut n: u32, rem: &mut PaymentPlan) {
    macro_rules! spend {
        ($f:ident) => {
            let s = n.min(rem.$f);
            rem.$f -= s;
            n -= s;
        };
    }
    spend!(white);
    spend!(blue);
    spend!(black);
    spend!(red);
    spend!(green);
    spend!(colorless);
    let _ = n;
}

/// Build a greedy payment plan for `cost` given current `pool` and player `life`.
/// Returns `None` if no valid plan exists.
/// X is treated as 0 (caller must override x_value if needed).
/// CR 107.4: handles all pip types including hybrid, Phyrexian, snow.
pub fn greedy_payment_plan(cost: &ManaCost, pool: &ManaPool, life: i32) -> Option<PaymentPlan> {
    use crate::types::mana::ManaPip::*;
    let mut plan = PaymentPlan::default();
    let mut rem = pool.clone();
    let mut rem_life = life;

    if cost.pips.iter().any(|p| matches!(p, X)) {
        plan.x_value = Some(0);
    }

    for pip in &cost.pips {
        match pip {
            White => deduct_one_color(&ManaColor::White, &mut rem, &mut plan)?,
            Blue => deduct_one_color(&ManaColor::Blue, &mut rem, &mut plan)?,
            Black => deduct_one_color(&ManaColor::Black, &mut rem, &mut plan)?,
            Red => deduct_one_color(&ManaColor::Red, &mut rem, &mut plan)?,
            Green => deduct_one_color(&ManaColor::Green, &mut rem, &mut plan)?,
            Colorless => deduct_one_color(&ManaColor::Colorless, &mut rem, &mut plan)?,
            X => {} // x_value already set to 0
            Snow => {
                // Pick first available snow-tagged color (CR 107.4k)
                if rem.snow_white > 0 && rem.white > 0 {
                    rem.white -= 1;
                    rem.snow_white -= 1;
                    plan.white += 1;
                    plan.snow_white += 1;
                } else if rem.snow_blue > 0 && rem.blue > 0 {
                    rem.blue -= 1;
                    rem.snow_blue -= 1;
                    plan.blue += 1;
                    plan.snow_blue += 1;
                } else if rem.snow_black > 0 && rem.black > 0 {
                    rem.black -= 1;
                    rem.snow_black -= 1;
                    plan.black += 1;
                    plan.snow_black += 1;
                } else if rem.snow_red > 0 && rem.red > 0 {
                    rem.red -= 1;
                    rem.snow_red -= 1;
                    plan.red += 1;
                    plan.snow_red += 1;
                } else if rem.snow_green > 0 && rem.green > 0 {
                    rem.green -= 1;
                    rem.snow_green -= 1;
                    plan.green += 1;
                    plan.snow_green += 1;
                } else if rem.snow_colorless > 0 && rem.colorless > 0 {
                    rem.colorless -= 1;
                    rem.snow_colorless -= 1;
                    plan.colorless += 1;
                    plan.snow_colorless += 1;
                } else {
                    return None;
                }
            }
            Phyrexian(c) => {
                // CR 107.4f: may pay 2 life instead of colored mana; prefer blood when enough life
                if rem_life >= 2 {
                    rem_life -= 2;
                    plan.blood += 1;
                } else {
                    deduct_one_color(c, &mut rem, &mut plan)?;
                }
            }
            HybridPhyrexian(c1, c2) => {
                // CR 107.4g: pay either color or 2 life
                if rem_life >= 2 {
                    rem_life -= 2;
                    plan.blood += 1;
                } else {
                    let a1 = amount_for_color(c1, &rem);
                    let a2 = amount_for_color(c2, &rem);
                    let chosen = if a1 >= a2 { c1 } else { c2 };
                    deduct_one_color(chosen, &mut rem, &mut plan)?;
                }
            }
            Hybrid(c1, c2) => {
                // CR 107.4b: pay either color; prefer the side with more available
                let a1 = amount_for_color(c1, &rem);
                let a2 = amount_for_color(c2, &rem);
                if a1 == 0 && a2 == 0 {
                    return None;
                }
                let chosen = if a1 >= a2 { c1 } else { c2 };
                deduct_one_color(chosen, &mut rem, &mut plan)?;
            }
            ColorlessHybrid(c) => {
                // CR 107.4d: pay 1 colorless or 1 of the specified color
                if rem.colorless > 0 {
                    rem.colorless -= 1;
                    plan.colorless += 1;
                } else {
                    deduct_one_color(c, &mut rem, &mut plan)?;
                }
            }
            GenericHybrid(n, c) => {
                // CR 107.4c: pay N generic or 1 of the specified color
                let ca = amount_for_color(c, &rem);
                if ca > 0 {
                    deduct_one_color(c, &mut rem, &mut plan)?;
                } else {
                    let total =
                        rem.white + rem.blue + rem.black + rem.red + rem.green + rem.colorless;
                    if total < *n {
                        return None;
                    }
                    spend_generic_rem(*n, &mut rem, &mut plan);
                }
            }
            Generic(n) => {
                let total = rem.white + rem.blue + rem.black + rem.red + rem.green + rem.colorless;
                if total < *n {
                    return None;
                }
                spend_generic_rem(*n, &mut rem, &mut plan);
            }
        }
    }
    Some(plan)
}

/// Returns true if the pool can pay the cost given current life total.
/// Handles all CR 107.4 pip types via greedy_payment_plan.
pub fn can_pay_mana(cost: &ManaCost, pool: &ManaPool, life: i32) -> bool {
    greedy_payment_plan(cost, pool, life).is_some()
}

/// Deduct a mana cost from a player's pool using an explicit PaymentPlan.
/// Validates the plan against both the pool contents and the cost pips before applying.
/// CR 107.4: handles all pip types including hybrid, Phyrexian, snow.
pub fn pay_mana_cost(
    mut state: GameState,
    player_id: PlayerId,
    cost: &ManaCost,
    plan: &PaymentPlan,
) -> Result<GameState, EngineError> {
    // Validate plan against pool
    {
        let player = state
            .get_player(player_id)
            .ok_or(EngineError::CardNotFound)?;
        let pool = &player.mana_pool;
        if pool.white < plan.white
            || pool.blue < plan.blue
            || pool.black < plan.black
            || pool.red < plan.red
            || pool.green < plan.green
            || pool.colorless < plan.colorless
        {
            return Err(EngineError::InvalidPaymentPlan);
        }
        if pool.snow_white < plan.snow_white
            || pool.snow_blue < plan.snow_blue
            || pool.snow_black < plan.snow_black
            || pool.snow_red < plan.snow_red
            || pool.snow_green < plan.snow_green
            || pool.snow_colorless < plan.snow_colorless
        {
            return Err(EngineError::InvalidPaymentPlan);
        }
        if player.life < (plan.blood as i32) * 2 {
            return Err(EngineError::InvalidPaymentPlan);
        }
    }

    // Validate plan satisfies cost (pip-by-pip)
    {
        // Guard: plan's snow shadow invariant must hold
        if plan.snow_white > plan.white
            || plan.snow_blue > plan.blue
            || plan.snow_black > plan.black
            || plan.snow_red > plan.red
            || plan.snow_green > plan.green
            || plan.snow_colorless > plan.colorless
        {
            return Err(EngineError::InvalidPaymentPlan);
        }

        let mut rem = plan.clone();
        let mut blood_left = plan.blood;

        for pip in &cost.pips {
            match pip {
                ManaPip::White => {
                    if rem.white == 0 {
                        return Err(EngineError::InvalidPaymentPlan);
                    }
                    rem.white -= 1;
                }
                ManaPip::Blue => {
                    if rem.blue == 0 {
                        return Err(EngineError::InvalidPaymentPlan);
                    }
                    rem.blue -= 1;
                }
                ManaPip::Black => {
                    if rem.black == 0 {
                        return Err(EngineError::InvalidPaymentPlan);
                    }
                    rem.black -= 1;
                }
                ManaPip::Red => {
                    if rem.red == 0 {
                        return Err(EngineError::InvalidPaymentPlan);
                    }
                    rem.red -= 1;
                }
                ManaPip::Green => {
                    if rem.green == 0 {
                        return Err(EngineError::InvalidPaymentPlan);
                    }
                    rem.green -= 1;
                }
                ManaPip::Colorless => {
                    if rem.colorless == 0 {
                        return Err(EngineError::InvalidPaymentPlan);
                    }
                    rem.colorless -= 1;
                }
                ManaPip::Snow => {
                    let snow_total = rem.snow_white
                        + rem.snow_blue
                        + rem.snow_black
                        + rem.snow_red
                        + rem.snow_green
                        + rem.snow_colorless;
                    if snow_total == 0 {
                        return Err(EngineError::InvalidPaymentPlan);
                    }
                    if rem.snow_white > 0 {
                        rem.snow_white -= 1;
                        rem.white -= 1;
                    } else if rem.snow_blue > 0 {
                        rem.snow_blue -= 1;
                        rem.blue -= 1;
                    } else if rem.snow_black > 0 {
                        rem.snow_black -= 1;
                        rem.black -= 1;
                    } else if rem.snow_red > 0 {
                        rem.snow_red -= 1;
                        rem.red -= 1;
                    } else if rem.snow_green > 0 {
                        rem.snow_green -= 1;
                        rem.green -= 1;
                    } else {
                        rem.snow_colorless -= 1;
                        rem.colorless -= 1;
                    }
                }
                ManaPip::Phyrexian(c) => {
                    if blood_left > 0 {
                        blood_left -= 1;
                    } else {
                        deduct_pip_color(c, &mut rem).ok_or(EngineError::InvalidPaymentPlan)?;
                    }
                }
                ManaPip::HybridPhyrexian(c1, c2) => {
                    if blood_left > 0 {
                        blood_left -= 1;
                    } else if pip_color_available(c1, &rem) {
                        deduct_pip_color(c1, &mut rem).unwrap();
                    } else if pip_color_available(c2, &rem) {
                        deduct_pip_color(c2, &mut rem).unwrap();
                    } else {
                        return Err(EngineError::InvalidPaymentPlan);
                    }
                }
                ManaPip::Hybrid(c1, c2) => {
                    if pip_color_available(c1, &rem) {
                        deduct_pip_color(c1, &mut rem).unwrap();
                    } else if pip_color_available(c2, &rem) {
                        deduct_pip_color(c2, &mut rem).unwrap();
                    } else {
                        return Err(EngineError::InvalidPaymentPlan);
                    }
                }
                ManaPip::ColorlessHybrid(c) => {
                    if rem.colorless > 0 {
                        rem.colorless -= 1;
                    } else {
                        deduct_pip_color(c, &mut rem).ok_or(EngineError::InvalidPaymentPlan)?;
                    }
                }
                ManaPip::GenericHybrid(n, c) => {
                    if pip_color_available(c, &rem) {
                        deduct_pip_color(c, &mut rem).unwrap();
                    } else {
                        let total =
                            rem.white + rem.blue + rem.black + rem.red + rem.green + rem.colorless;
                        if total < *n {
                            return Err(EngineError::InvalidPaymentPlan);
                        }
                        spend_from_rem(*n, &mut rem);
                    }
                }
                ManaPip::Generic(n) => {
                    let total =
                        rem.white + rem.blue + rem.black + rem.red + rem.green + rem.colorless;
                    if total < *n {
                        return Err(EngineError::InvalidPaymentPlan);
                    }
                    spend_from_rem(*n, &mut rem);
                }
                ManaPip::X => {
                    let x_val = plan.x_value.ok_or(EngineError::InvalidPaymentPlan)?;
                    let total =
                        rem.white + rem.blue + rem.black + rem.red + rem.green + rem.colorless;
                    if total < x_val {
                        return Err(EngineError::InvalidPaymentPlan);
                    }
                    spend_from_rem(x_val, &mut rem);
                }
            }
        }
    }

    // Apply plan atomically
    let player = state.get_player_mut(player_id).unwrap();
    player.mana_pool.white -= plan.white;
    player.mana_pool.blue -= plan.blue;
    player.mana_pool.black -= plan.black;
    player.mana_pool.red -= plan.red;
    player.mana_pool.green -= plan.green;
    player.mana_pool.colorless -= plan.colorless;
    player.mana_pool.snow_white -= plan.snow_white;
    player.mana_pool.snow_blue -= plan.snow_blue;
    player.mana_pool.snow_black -= plan.snow_black;
    player.mana_pool.snow_red -= plan.snow_red;
    player.mana_pool.snow_green -= plan.snow_green;
    player.mana_pool.snow_colorless -= plan.snow_colorless;
    player.life -= (plan.blood as i32) * 2;

    Ok(state)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cards::test_helpers::test_db;
    use crate::types::{CardObject, ManaColor, ManaCost, ManaPip, ManaPool, PaymentPlan, Player};

    fn make_state() -> GameState {
        GameState::new(vec![
            Player::new(PlayerId(0), "Alice"),
            Player::new(PlayerId(1), "Bob"),
        ])
    }

    fn add_land(
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
    fn tap_forest_adds_green_mana() {
        let db = test_db();
        let mut gs = make_state();
        let forest_id = add_land(&mut gs, PlayerId(0), db.get("Forest").unwrap().clone());

        let gs = tap_land_for_mana(gs, forest_id).unwrap();

        assert!(gs.objects[&forest_id].tapped);
        assert_eq!(gs.get_player(PlayerId(0)).unwrap().mana_pool.green, 1);
    }

    #[test]
    fn cannot_tap_already_tapped_land() {
        let db = test_db();
        let mut gs = make_state();
        let forest_id = add_land(&mut gs, PlayerId(0), db.get("Forest").unwrap().clone());
        gs.objects.get_mut(&forest_id).unwrap().tapped = true;

        assert!(matches!(
            tap_land_for_mana(gs, forest_id),
            Err(EngineError::AlreadyTapped)
        ));
    }

    #[test]
    fn pay_1g_with_green_and_any() {
        let mut gs = make_state();
        gs.get_player_mut(PlayerId(0)).unwrap().mana_pool.green += 2;
        let cost = ManaCost {
            pips: vec![ManaPip::Generic(1), ManaPip::Green],
        };
        let plan =
            greedy_payment_plan(&cost, &gs.get_player(PlayerId(0)).unwrap().mana_pool, 20).unwrap();
        let gs = pay_mana_cost(gs, PlayerId(0), &cost, &plan).unwrap();

        assert!(gs.get_player(PlayerId(0)).unwrap().mana_pool.is_empty());
    }

    #[test]
    fn cannot_pay_1g_with_only_1_green() {
        let mut gs = make_state();
        gs.get_player_mut(PlayerId(0)).unwrap().mana_pool.green += 1;
        let cost = ManaCost {
            pips: vec![ManaPip::Generic(1), ManaPip::Green],
        };
        let plan = greedy_payment_plan(&cost, &gs.get_player(PlayerId(0)).unwrap().mana_pool, 20);
        assert!(plan.is_none());
    }

    #[test]
    fn cannot_pay_g_with_red_mana() {
        let mut gs = make_state();
        gs.get_player_mut(PlayerId(0)).unwrap().mana_pool.red += 2;
        let cost = ManaCost {
            pips: vec![ManaPip::Green],
        };
        let plan = greedy_payment_plan(&cost, &gs.get_player(PlayerId(0)).unwrap().mana_pool, 20);
        assert!(plan.is_none());
    }

    #[test]
    fn generic_cost_satisfied_by_any_color() {
        let mut gs = make_state();
        gs.get_player_mut(PlayerId(0)).unwrap().mana_pool.red += 1;
        gs.get_player_mut(PlayerId(0)).unwrap().mana_pool.green += 1;
        let cost = ManaCost {
            pips: vec![ManaPip::Generic(2)],
        };
        let plan =
            greedy_payment_plan(&cost, &gs.get_player(PlayerId(0)).unwrap().mana_pool, 20).unwrap();
        let gs = pay_mana_cost(gs, PlayerId(0), &cost, &plan).unwrap();

        assert!(gs.get_player(PlayerId(0)).unwrap().mana_pool.is_empty());
    }

    #[test]
    fn pay_with_hybrid_plan() {
        let mut gs = make_state();
        gs.get_player_mut(PlayerId(0)).unwrap().mana_pool.green = 1;
        let cost = ManaCost {
            pips: vec![ManaPip::Hybrid(ManaColor::Black, ManaColor::Green)],
        };
        let plan = PaymentPlan {
            green: 1,
            ..Default::default()
        };
        let gs = pay_mana_cost(gs, PlayerId(0), &cost, &plan).unwrap();
        assert!(gs.get_player(PlayerId(0)).unwrap().mana_pool.is_empty());
    }

    #[test]
    fn pay_with_phyrexian_blood_plan() {
        let mut gs = make_state();
        gs.get_player_mut(PlayerId(0)).unwrap().life = 20;
        let cost = ManaCost {
            pips: vec![ManaPip::Phyrexian(ManaColor::Blue)],
        };
        let plan = PaymentPlan {
            blood: 1,
            ..Default::default()
        };
        let gs = pay_mana_cost(gs, PlayerId(0), &cost, &plan).unwrap();
        assert_eq!(gs.get_player(PlayerId(0)).unwrap().life, 18);
    }

    #[test]
    fn pay_with_snow_plan() {
        let mut gs = make_state();
        gs.get_player_mut(PlayerId(0))
            .unwrap()
            .mana_pool
            .add_snow(ManaColor::Green, 1);
        let cost = ManaCost {
            pips: vec![ManaPip::Snow],
        };
        let plan = PaymentPlan {
            green: 1,
            snow_green: 1,
            ..Default::default()
        };
        let gs = pay_mana_cost(gs, PlayerId(0), &cost, &plan).unwrap();
        let pool = &gs.get_player(PlayerId(0)).unwrap().mana_pool;
        assert_eq!(pool.green, 0);
        assert_eq!(pool.snow_green, 0);
    }

    #[test]
    fn invalid_plan_returns_error() {
        let mut gs = make_state();
        gs.get_player_mut(PlayerId(0)).unwrap().mana_pool.red = 1;
        let cost = ManaCost {
            pips: vec![ManaPip::Green],
        };
        // plan says spend 1 green but pool has 0 green
        let plan = PaymentPlan {
            green: 1,
            ..Default::default()
        };
        assert!(matches!(
            pay_mana_cost(gs, PlayerId(0), &cost, &plan),
            Err(EngineError::InvalidPaymentPlan)
        ));
    }

    #[test]
    fn tap_land_for_mana_creates_checkpoint_on_first_tap() {
        let db = test_db();
        let mut gs = make_state();
        let forest_id = add_land(&mut gs, PlayerId(0), db.get("Forest").unwrap().clone());

        assert!(gs.mana_checkpoint.is_none());
        let gs = tap_land_for_mana(gs, forest_id).unwrap();

        let cp = gs.mana_checkpoint.as_ref().expect("checkpoint created");
        assert_eq!(cp.tapped_lands, vec![forest_id]);
        assert!(cp.pools[&PlayerId(0)].is_empty()); // pool was empty before the tap
    }

    #[test]
    fn second_tap_appends_to_existing_checkpoint() {
        let db = test_db();
        let mut gs = make_state();
        let f1 = add_land(&mut gs, PlayerId(0), db.get("Forest").unwrap().clone());
        let f2 = add_land(&mut gs, PlayerId(0), db.get("Forest").unwrap().clone());

        let gs = tap_land_for_mana(gs, f1).unwrap();
        let gs = tap_land_for_mana(gs, f2).unwrap();

        let cp = gs.mana_checkpoint.as_ref().unwrap();
        assert_eq!(cp.tapped_lands, vec![f1, f2]);
        assert!(cp.pools[&PlayerId(0)].is_empty()); // pool at checkpoint was empty
    }

    #[test]
    fn reset_mana_restores_pools_and_untaps_lands() {
        use super::reset_mana;
        let db = test_db();
        let mut gs = make_state();
        let forest_id = add_land(&mut gs, PlayerId(0), db.get("Forest").unwrap().clone());

        let gs = tap_land_for_mana(gs, forest_id).unwrap();
        assert!(gs.objects[&forest_id].tapped);
        assert_eq!(gs.get_player(PlayerId(0)).unwrap().mana_pool.green, 1);

        let gs = reset_mana(gs).unwrap();

        assert!(!gs.objects[&forest_id].tapped, "land untapped after reset");
        assert!(
            gs.get_player(PlayerId(0)).unwrap().mana_pool.is_empty(),
            "pool restored"
        );
        assert!(gs.mana_checkpoint.is_none(), "checkpoint cleared");
    }

    #[test]
    fn reset_mana_returns_err_with_no_checkpoint() {
        use super::reset_mana;
        let gs = make_state();
        assert!(matches!(
            reset_mana(gs),
            Err(super::EngineError::NoManaCheckpoint)
        ));
    }

    #[test]
    fn greedy_plan_covers_hybrid_pip() {
        let cost = ManaCost {
            pips: vec![ManaPip::Hybrid(ManaColor::Black, ManaColor::Green)],
        };
        let pool = ManaPool {
            green: 1,
            ..Default::default()
        };
        let plan = super::greedy_payment_plan(&cost, &pool, 20).unwrap();
        assert_eq!(plan.green, 1);
        assert_eq!(plan.black, 0);
    }

    #[test]
    fn greedy_plan_hybrid_prefers_larger_side() {
        let cost = ManaCost {
            pips: vec![ManaPip::Hybrid(ManaColor::Black, ManaColor::Green)],
        };
        let pool = ManaPool {
            black: 1,
            green: 3,
            ..Default::default()
        };
        let plan = super::greedy_payment_plan(&cost, &pool, 20).unwrap();
        assert_eq!(plan.green, 1);
        assert_eq!(plan.black, 0);
    }

    #[test]
    fn greedy_plan_phyrexian_prefers_blood() {
        let cost = ManaCost {
            pips: vec![ManaPip::Phyrexian(ManaColor::Blue)],
        };
        let pool = ManaPool {
            blue: 2,
            ..Default::default()
        };
        let plan = super::greedy_payment_plan(&cost, &pool, 20).unwrap();
        assert_eq!(plan.blood, 1);
        assert_eq!(plan.blue, 0);
    }

    #[test]
    fn greedy_plan_phyrexian_falls_back_to_color_if_low_life() {
        let cost = ManaCost {
            pips: vec![ManaPip::Phyrexian(ManaColor::Blue)],
        };
        let pool = ManaPool {
            blue: 1,
            ..Default::default()
        };
        let plan = super::greedy_payment_plan(&cost, &pool, 1).unwrap();
        assert_eq!(plan.blood, 0);
        assert_eq!(plan.blue, 1);
    }

    #[test]
    fn greedy_plan_snow_pip() {
        let cost = ManaCost {
            pips: vec![ManaPip::Snow],
        };
        let mut pool = ManaPool::default();
        pool.add_snow(ManaColor::Green, 1);
        let plan = super::greedy_payment_plan(&cost, &pool, 20).unwrap();
        assert_eq!(plan.snow_green, 1);
        assert_eq!(plan.green, 1);
    }

    #[test]
    fn greedy_plan_returns_none_if_insufficient() {
        let cost = ManaCost {
            pips: vec![ManaPip::Green],
        };
        let pool = ManaPool::default();
        assert!(super::greedy_payment_plan(&cost, &pool, 20).is_none());
    }

    #[test]
    fn can_pay_mana_true_for_hybrid_with_one_side() {
        let cost = ManaCost {
            pips: vec![ManaPip::Hybrid(ManaColor::Red, ManaColor::Green)],
        };
        let pool = ManaPool {
            red: 1,
            ..Default::default()
        };
        assert!(super::can_pay_mana(&cost, &pool, 20));
    }

    #[test]
    fn can_pay_mana_phyrexian_true_with_2_life_and_no_mana() {
        let cost = ManaCost {
            pips: vec![ManaPip::Phyrexian(ManaColor::White)],
        };
        let pool = ManaPool::default();
        assert!(super::can_pay_mana(&cost, &pool, 20));
        assert!(!super::can_pay_mana(&cost, &pool, 1));
    }

    #[test]
    fn tap_snow_forest_adds_snow_tagged_green() {
        use crate::types::card::{CardDefinition, CardType, Supertype, TypeLine};
        let snow_forest_def = CardDefinition {
            name: "Snow-Covered Forest".into(),
            mana_cost: None,
            type_line: TypeLine {
                supertypes: vec![Supertype::Basic, Supertype::Snow],
                card_types: vec![CardType::Land],
                subtypes: vec!["Forest".into()],
            },
            oracle_text: "({T}: Add {G}.)".into(),
            abilities: vec![],
            power: None,
            toughness: None,
        };
        let mut gs = make_state();
        let id = add_land(&mut gs, PlayerId(0), snow_forest_def);
        let gs = tap_land_for_mana(gs, id).unwrap();
        let pool = &gs.get_player(PlayerId(0)).unwrap().mana_pool;
        assert_eq!(pool.green, 1);
        assert_eq!(pool.snow_green, 1);
    }

    #[test]
    fn tap_regular_forest_does_not_add_snow_tag() {
        let db = test_db();
        let mut gs = make_state();
        let id = add_land(&mut gs, PlayerId(0), db.get("Forest").unwrap().clone());
        let gs = tap_land_for_mana(gs, id).unwrap();
        let pool = &gs.get_player(PlayerId(0)).unwrap().mana_pool;
        assert_eq!(pool.green, 1);
        assert_eq!(pool.snow_green, 0);
    }
}
