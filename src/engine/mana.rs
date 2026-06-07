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

/// Counts simple pip requirements. Returns Err if cost contains hybrid/phyrexian/snow
/// (those require the full greedy plan — see greedy_payment_plan).
fn tally_simple_pips(cost: &ManaCost) -> Result<(u32, u32, u32, u32, u32, u32, u32), ()> {
    let (mut nw, mut nu, mut nb, mut nr, mut ng, mut nc, mut generic) = (0, 0, 0, 0, 0, 0, 0);
    for pip in &cost.pips {
        match pip {
            ManaPip::White => nw += 1,
            ManaPip::Blue => nu += 1,
            ManaPip::Black => nb += 1,
            ManaPip::Red => nr += 1,
            ManaPip::Green => ng += 1,
            ManaPip::Colorless => nc += 1,
            ManaPip::Generic(n) => generic += n,
            ManaPip::X => {}
            _ => return Err(()),
        }
    }
    Ok((nw, nu, nb, nr, ng, nc, generic))
}

fn amount_for_color(color: &ManaColor, w: u32, u: u32, b: u32, r: u32, g: u32, c: u32) -> u32 {
    match color {
        ManaColor::White => w,
        ManaColor::Blue => u,
        ManaColor::Black => b,
        ManaColor::Red => r,
        ManaColor::Green => g,
        ManaColor::Colorless => c,
    }
}

fn deduct_one_color(
    color: &ManaColor,
    rw: &mut u32,
    ru: &mut u32,
    rb: &mut u32,
    rr: &mut u32,
    rg: &mut u32,
    rc: &mut u32,
    pw: &mut u32,
    pu: &mut u32,
    pb: &mut u32,
    pr: &mut u32,
    pg: &mut u32,
    pc: &mut u32,
) -> Option<()> {
    macro_rules! go {
        ($ra:expr, $pa:expr) => {{
            if *$ra == 0 {
                return None;
            }
            *$ra -= 1;
            *$pa += 1;
            Some(())
        }};
    }
    match color {
        ManaColor::White => go!(rw, pw),
        ManaColor::Blue => go!(ru, pu),
        ManaColor::Black => go!(rb, pb),
        ManaColor::Red => go!(rr, pr),
        ManaColor::Green => go!(rg, pg),
        ManaColor::Colorless => go!(rc, pc),
    }
}

fn spend_generic_rem(
    mut n: u32,
    rw: &mut u32,
    ru: &mut u32,
    rb: &mut u32,
    rr: &mut u32,
    rg: &mut u32,
    rc: &mut u32,
    pw: &mut u32,
    pu: &mut u32,
    pb: &mut u32,
    pr: &mut u32,
    pg: &mut u32,
    pc: &mut u32,
) {
    macro_rules! spend {
        ($r:expr, $p:expr) => {
            let s = n.min(*$r);
            *$r -= s;
            *$p += s;
            n -= s;
        };
    }
    spend!(rw, pw);
    spend!(ru, pu);
    spend!(rb, pb);
    spend!(rr, pr);
    spend!(rg, pg);
    spend!(rc, pc);
}

/// Build a greedy payment plan for `cost` given current `pool` and player `life`.
/// Returns `None` if no valid plan exists.
/// X is treated as 0 (caller must override x_value if needed).
/// CR 107.4: handles all pip types including hybrid, Phyrexian, snow.
pub fn greedy_payment_plan(cost: &ManaCost, pool: &ManaPool, life: i32) -> Option<PaymentPlan> {
    use crate::types::mana::ManaPip::*;
    let mut plan = PaymentPlan::default();
    let mut rem_w = pool.white;
    let mut rem_u = pool.blue;
    let mut rem_b = pool.black;
    let mut rem_r = pool.red;
    let mut rem_g = pool.green;
    let mut rem_c = pool.colorless;
    let mut rem_sw = pool.snow_white;
    let mut rem_su = pool.snow_blue;
    let mut rem_sb = pool.snow_black;
    let mut rem_sr = pool.snow_red;
    let mut rem_sg = pool.snow_green;
    let mut rem_sc = pool.snow_colorless;
    let mut rem_life = life;

    if cost.pips.iter().any(|p| matches!(p, X)) {
        plan.x_value = Some(0);
    }

    for pip in &cost.pips {
        match pip {
            White => deduct_one_color(
                &ManaColor::White,
                &mut rem_w,
                &mut rem_u,
                &mut rem_b,
                &mut rem_r,
                &mut rem_g,
                &mut rem_c,
                &mut plan.white,
                &mut plan.blue,
                &mut plan.black,
                &mut plan.red,
                &mut plan.green,
                &mut plan.colorless,
            )?,
            Blue => deduct_one_color(
                &ManaColor::Blue,
                &mut rem_w,
                &mut rem_u,
                &mut rem_b,
                &mut rem_r,
                &mut rem_g,
                &mut rem_c,
                &mut plan.white,
                &mut plan.blue,
                &mut plan.black,
                &mut plan.red,
                &mut plan.green,
                &mut plan.colorless,
            )?,
            Black => deduct_one_color(
                &ManaColor::Black,
                &mut rem_w,
                &mut rem_u,
                &mut rem_b,
                &mut rem_r,
                &mut rem_g,
                &mut rem_c,
                &mut plan.white,
                &mut plan.blue,
                &mut plan.black,
                &mut plan.red,
                &mut plan.green,
                &mut plan.colorless,
            )?,
            Red => deduct_one_color(
                &ManaColor::Red,
                &mut rem_w,
                &mut rem_u,
                &mut rem_b,
                &mut rem_r,
                &mut rem_g,
                &mut rem_c,
                &mut plan.white,
                &mut plan.blue,
                &mut plan.black,
                &mut plan.red,
                &mut plan.green,
                &mut plan.colorless,
            )?,
            Green => deduct_one_color(
                &ManaColor::Green,
                &mut rem_w,
                &mut rem_u,
                &mut rem_b,
                &mut rem_r,
                &mut rem_g,
                &mut rem_c,
                &mut plan.white,
                &mut plan.blue,
                &mut plan.black,
                &mut plan.red,
                &mut plan.green,
                &mut plan.colorless,
            )?,
            Colorless => deduct_one_color(
                &ManaColor::Colorless,
                &mut rem_w,
                &mut rem_u,
                &mut rem_b,
                &mut rem_r,
                &mut rem_g,
                &mut rem_c,
                &mut plan.white,
                &mut plan.blue,
                &mut plan.black,
                &mut plan.red,
                &mut plan.green,
                &mut plan.colorless,
            )?,
            X => {} // x_value already set to 0
            Snow => {
                // Pick first available snow-tagged color (CR 107.4k)
                if rem_sw > 0 {
                    rem_w -= 1;
                    rem_sw -= 1;
                    plan.white += 1;
                    plan.snow_white += 1;
                } else if rem_su > 0 {
                    rem_u -= 1;
                    rem_su -= 1;
                    plan.blue += 1;
                    plan.snow_blue += 1;
                } else if rem_sb > 0 {
                    rem_b -= 1;
                    rem_sb -= 1;
                    plan.black += 1;
                    plan.snow_black += 1;
                } else if rem_sr > 0 {
                    rem_r -= 1;
                    rem_sr -= 1;
                    plan.red += 1;
                    plan.snow_red += 1;
                } else if rem_sg > 0 {
                    rem_g -= 1;
                    rem_sg -= 1;
                    plan.green += 1;
                    plan.snow_green += 1;
                } else if rem_sc > 0 {
                    rem_c -= 1;
                    rem_sc -= 1;
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
                    deduct_one_color(
                        c,
                        &mut rem_w,
                        &mut rem_u,
                        &mut rem_b,
                        &mut rem_r,
                        &mut rem_g,
                        &mut rem_c,
                        &mut plan.white,
                        &mut plan.blue,
                        &mut plan.black,
                        &mut plan.red,
                        &mut plan.green,
                        &mut plan.colorless,
                    )?;
                }
            }
            HybridPhyrexian(c1, c2) => {
                // CR 107.4g: pay either color or 2 life
                if rem_life >= 2 {
                    rem_life -= 2;
                    plan.blood += 1;
                } else {
                    let a1 = amount_for_color(c1, rem_w, rem_u, rem_b, rem_r, rem_g, rem_c);
                    let a2 = amount_for_color(c2, rem_w, rem_u, rem_b, rem_r, rem_g, rem_c);
                    let chosen = if a1 >= a2 { c1 } else { c2 };
                    deduct_one_color(
                        chosen,
                        &mut rem_w,
                        &mut rem_u,
                        &mut rem_b,
                        &mut rem_r,
                        &mut rem_g,
                        &mut rem_c,
                        &mut plan.white,
                        &mut plan.blue,
                        &mut plan.black,
                        &mut plan.red,
                        &mut plan.green,
                        &mut plan.colorless,
                    )?;
                }
            }
            Hybrid(c1, c2) => {
                // CR 107.4b: pay either color; prefer the side with more available
                let a1 = amount_for_color(c1, rem_w, rem_u, rem_b, rem_r, rem_g, rem_c);
                let a2 = amount_for_color(c2, rem_w, rem_u, rem_b, rem_r, rem_g, rem_c);
                if a1 == 0 && a2 == 0 {
                    return None;
                }
                let chosen = if a1 >= a2 { c1 } else { c2 };
                deduct_one_color(
                    chosen,
                    &mut rem_w,
                    &mut rem_u,
                    &mut rem_b,
                    &mut rem_r,
                    &mut rem_g,
                    &mut rem_c,
                    &mut plan.white,
                    &mut plan.blue,
                    &mut plan.black,
                    &mut plan.red,
                    &mut plan.green,
                    &mut plan.colorless,
                )?;
            }
            ColorlessHybrid(c) => {
                // CR 107.4d: pay 1 colorless or 1 of the specified color
                if rem_c > 0 {
                    rem_c -= 1;
                    plan.colorless += 1;
                } else {
                    deduct_one_color(
                        c,
                        &mut rem_w,
                        &mut rem_u,
                        &mut rem_b,
                        &mut rem_r,
                        &mut rem_g,
                        &mut rem_c,
                        &mut plan.white,
                        &mut plan.blue,
                        &mut plan.black,
                        &mut plan.red,
                        &mut plan.green,
                        &mut plan.colorless,
                    )?;
                }
            }
            GenericHybrid(n, c) => {
                // CR 107.4c: pay N generic or 1 of the specified color
                let ca = amount_for_color(c, rem_w, rem_u, rem_b, rem_r, rem_g, rem_c);
                if ca > 0 {
                    deduct_one_color(
                        c,
                        &mut rem_w,
                        &mut rem_u,
                        &mut rem_b,
                        &mut rem_r,
                        &mut rem_g,
                        &mut rem_c,
                        &mut plan.white,
                        &mut plan.blue,
                        &mut plan.black,
                        &mut plan.red,
                        &mut plan.green,
                        &mut plan.colorless,
                    )?;
                } else {
                    let total = rem_w + rem_u + rem_b + rem_r + rem_g + rem_c;
                    if total < *n {
                        return None;
                    }
                    spend_generic_rem(
                        *n,
                        &mut rem_w,
                        &mut rem_u,
                        &mut rem_b,
                        &mut rem_r,
                        &mut rem_g,
                        &mut rem_c,
                        &mut plan.white,
                        &mut plan.blue,
                        &mut plan.black,
                        &mut plan.red,
                        &mut plan.green,
                        &mut plan.colorless,
                    );
                }
            }
            Generic(n) => {
                let total = rem_w + rem_u + rem_b + rem_r + rem_g + rem_c;
                if total < *n {
                    return None;
                }
                spend_generic_rem(
                    *n,
                    &mut rem_w,
                    &mut rem_u,
                    &mut rem_b,
                    &mut rem_r,
                    &mut rem_g,
                    &mut rem_c,
                    &mut plan.white,
                    &mut plan.blue,
                    &mut plan.black,
                    &mut plan.red,
                    &mut plan.green,
                    &mut plan.colorless,
                );
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

/// Deduct a mana cost from a player's pool using simple pip tallying.
/// Colored mana is paid first; the generic portion is satisfied by any remaining mana.
#[allow(unused_assignments)]
pub fn pay_mana_cost(
    mut state: GameState,
    player_id: PlayerId,
    cost: &ManaCost,
) -> Result<GameState, EngineError> {
    let (nw, nu, nb, nr, ng, nc, generic) = {
        let player = state
            .get_player(player_id)
            .ok_or(EngineError::CardNotFound)?;
        let pool = &player.mana_pool;
        let tallied = tally_simple_pips(cost).map_err(|_| EngineError::InsufficientMana)?;
        let (nw, nu, nb, nr, ng, nc, generic) = tallied;
        if pool.white < nw
            || pool.blue < nu
            || pool.black < nb
            || pool.red < nr
            || pool.green < ng
            || pool.colorless < nc
        {
            return Err(EngineError::InsufficientMana);
        }
        let remaining = pool.total() - nw - nu - nb - nr - ng - nc;
        if remaining < generic {
            return Err(EngineError::InsufficientMana);
        }
        (nw, nu, nb, nr, ng, nc, generic)
    };

    let player = state.get_player_mut(player_id).unwrap();
    player.mana_pool.white -= nw;
    player.mana_pool.blue -= nu;
    player.mana_pool.black -= nb;
    player.mana_pool.red -= nr;
    player.mana_pool.green -= ng;
    player.mana_pool.colorless -= nc;

    let mut remaining = generic;
    let pool = &mut player.mana_pool;
    macro_rules! spend {
        ($field:ident) => {
            let s = remaining.min(pool.$field);
            pool.$field -= s;
            remaining -= s;
        };
    }
    spend!(white);
    spend!(blue);
    spend!(black);
    spend!(red);
    spend!(green);
    spend!(colorless);

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

        let gs = pay_mana_cost(gs, PlayerId(0), &cost).unwrap();

        assert!(gs.get_player(PlayerId(0)).unwrap().mana_pool.is_empty());
    }

    #[test]
    fn cannot_pay_1g_with_only_1_green() {
        let mut gs = make_state();
        gs.get_player_mut(PlayerId(0)).unwrap().mana_pool.green += 1;
        let cost = ManaCost {
            pips: vec![ManaPip::Generic(1), ManaPip::Green],
        };

        assert!(matches!(
            pay_mana_cost(gs, PlayerId(0), &cost),
            Err(EngineError::InsufficientMana)
        ));
    }

    #[test]
    fn cannot_pay_g_with_red_mana() {
        let mut gs = make_state();
        gs.get_player_mut(PlayerId(0)).unwrap().mana_pool.red += 2;
        let cost = ManaCost {
            pips: vec![ManaPip::Green],
        };

        assert!(matches!(
            pay_mana_cost(gs, PlayerId(0), &cost),
            Err(EngineError::InsufficientMana)
        ));
    }

    #[test]
    fn generic_cost_satisfied_by_any_color() {
        let mut gs = make_state();
        gs.get_player_mut(PlayerId(0)).unwrap().mana_pool.red += 1;
        gs.get_player_mut(PlayerId(0)).unwrap().mana_pool.green += 1;
        let cost = ManaCost {
            pips: vec![ManaPip::Generic(2)],
        };

        let gs = pay_mana_cost(gs, PlayerId(0), &cost).unwrap();

        assert!(gs.get_player(PlayerId(0)).unwrap().mana_pool.is_empty());
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
        assert_eq!(cp.pools[&PlayerId(0)].is_empty(), true); // pool at checkpoint was empty
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
        let mut pool = ManaPool::default();
        pool.green = 1;
        let plan = super::greedy_payment_plan(&cost, &pool, 20).unwrap();
        assert_eq!(plan.green, 1);
        assert_eq!(plan.black, 0);
    }

    #[test]
    fn greedy_plan_hybrid_prefers_larger_side() {
        let cost = ManaCost {
            pips: vec![ManaPip::Hybrid(ManaColor::Black, ManaColor::Green)],
        };
        let mut pool = ManaPool::default();
        pool.black = 1;
        pool.green = 3;
        let plan = super::greedy_payment_plan(&cost, &pool, 20).unwrap();
        assert_eq!(plan.green, 1);
        assert_eq!(plan.black, 0);
    }

    #[test]
    fn greedy_plan_phyrexian_prefers_blood() {
        let cost = ManaCost {
            pips: vec![ManaPip::Phyrexian(ManaColor::Blue)],
        };
        let mut pool = ManaPool::default();
        pool.blue = 2;
        let plan = super::greedy_payment_plan(&cost, &pool, 20).unwrap();
        assert_eq!(plan.blood, 1);
        assert_eq!(plan.blue, 0);
    }

    #[test]
    fn greedy_plan_phyrexian_falls_back_to_color_if_low_life() {
        let cost = ManaCost {
            pips: vec![ManaPip::Phyrexian(ManaColor::Blue)],
        };
        let mut pool = ManaPool::default();
        pool.blue = 1;
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
        let mut pool = ManaPool::default();
        pool.red = 1;
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
