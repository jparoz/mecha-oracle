//! Comprehensive Rules (CR) examples as integration tests.
//!
//! Each `#[test]` function corresponds to exactly one `Example:` block in docs/CR.txt.
//! Tests are named `cr_<rule>_ex<n>` where <rule> is the CR rule number (dots → underscores)
//! and <n> distinguishes multiple examples under the same rule.
//!
//! Fully-implemented tests contain assertions against live engine behaviour.
//! Tests for unimplemented features are marked `#[ignore = "requires: <feature>"]` and
//! contain a comment describing what the test would verify once the feature exists.

#![allow(dead_code)]

use mecha_oracle::engine::mana::greedy_payment_plan;
use mecha_oracle::types::card::{CardType, Supertype, TypeLine};
use mecha_oracle::types::mana::{ManaColor, ManaCost, ManaPip, ManaPool};
use mecha_oracle::types::permanent::{PTDelta, PermanentState};
use mecha_oracle::types::{CardDefinition, Rule, RulesText, StaticAbility};
use mecha_oracle::types::{CardObject, GameState, Player, PlayerId, Zone};

// ── helpers ──────────────────────────────────────────────────────────────────

/// Build a ManaCost from a slice of pips.
fn cost(pips: &[ManaPip]) -> ManaCost {
    ManaCost {
        pips: pips.to_vec(),
    }
}

/// Build a ManaPool with exact values per color.
fn pool_from(w: u32, u: u32, b: u32, r: u32, g: u32, c: u32) -> ManaPool {
    ManaPool {
        white: w,
        blue: u,
        black: b,
        red: r,
        green: g,
        colorless: c,
        ..Default::default()
    }
}

/// Derive which ManaColors are present in a ManaCost (CR 202.2).
/// Returns the set of colors implied by colored mana symbols in the cost.
fn colors_from_cost(c: &ManaCost) -> Vec<ManaColor> {
    let mut seen = vec![];
    for pip in &c.pips {
        let add = |col: ManaColor, seen: &mut Vec<ManaColor>| {
            if !seen.contains(&col) {
                seen.push(col);
            }
        };
        match pip {
            ManaPip::White => add(ManaColor::White, &mut seen),
            ManaPip::Blue => add(ManaColor::Blue, &mut seen),
            ManaPip::Black => add(ManaColor::Black, &mut seen),
            ManaPip::Red => add(ManaColor::Red, &mut seen),
            ManaPip::Green => add(ManaColor::Green, &mut seen),
            ManaPip::Hybrid(a, b) => {
                add(*a, &mut seen);
                add(*b, &mut seen);
            }
            ManaPip::GenericHybrid(_, c) => add(*c, &mut seen),
            ManaPip::ColorlessHybrid(c) => add(*c, &mut seen),
            ManaPip::Phyrexian(c) => add(*c, &mut seen),
            ManaPip::HybridPhyrexian(a, b) => {
                add(*a, &mut seen);
                add(*b, &mut seen);
            }
            ManaPip::Generic(_) | ManaPip::Colorless | ManaPip::X | ManaPip::Snow => {}
        }
    }
    seen
}

/// Build a minimal CardDefinition suitable for constructing a PermanentState.
fn make_creature_def(power: i32, toughness: i32) -> CardDefinition {
    CardDefinition {
        name: "Test Creature".to_string(),
        mana_cost: None,
        type_line: TypeLine {
            supertypes: vec![],
            card_types: vec![CardType::Creature],
            subtypes: vec![],
        },
        oracle_text: String::new(),
        rules_text: vec![],
        text_annotations: vec![],
        power: Some(power),
        toughness: Some(toughness),
        colors: vec![],
    }
}

// ── CR 101 ────────────────────────────────────────────────────────────────────

/// CR 101.2 – "can't" beats "may" for land-play effects.
/// "If one effect reads 'You may play an additional land this turn' and another reads
/// 'You can't play lands this turn,' the effect that precludes you from playing lands wins."
#[test]
#[ignore = "requires: land-play permission/restriction layer system"]
fn cr_101_2_ex1_cant_overrides_may_for_land_play() {
    // Would test: player with "may play additional land" AND "can't play lands" cannot play any land.
}

/// CR 101.4 – active player chooses first in simultaneous "each player" effects.
/// "A card reads 'Each player sacrifices a creature.' First, the active player chooses..."
#[test]
#[ignore = "requires: APNAP simultaneous-choice resolution"]
fn cr_101_4_ex1_each_player_sacrifices_active_player_chooses_first() {
    // Would test: when resolving "each player sacrifices a creature", the active player
    // selects first, then each nonactive player in turn order, all sacrificed simultaneously.
}

// ── CR 106 ────────────────────────────────────────────────────────────────────

/// CR 106.5 – Meteor Crater produces no mana when controlling no colored permanents.
#[test]
#[ignore = "requires: custom mana-ability implementation (Meteor Crater)"]
fn cr_106_5_ex1_meteor_crater_no_colored_permanents_produces_no_mana() {
    // Would test: activating Meteor Crater's ability with no colored permanents on the
    // battlefield results in no mana being added to the pool.
}

/// CR 106.6 – Doubling Cube doubles restricted mana and the restriction carries through.
#[test]
#[ignore = "requires: mana-restriction tags and Doubling Cube ability"]
fn cr_106_6_ex1_doubling_cube_preserves_spending_restriction() {
    // Would test: pool {R}{G} (creature-spells only) → after Doubling Cube → {R}{R}{G}{G},
    // where the original {R}{G} can be spent on anything but the new {R}{G} retains the restriction.
}

/// CR 106.7 – Exotic Orchard produces no mana when no lands produce colored mana.
#[test]
#[ignore = "requires: cross-player mana-production query ability (Exotic Orchard)"]
fn cr_106_7_ex1_exotic_orchard_opponent_no_lands_produces_nothing() {
    // Would test: Exotic Orchard produces nothing when opponent controls no lands.
}

/// CR 106.7 – Exotic Orchard mutual-orchard deadlock produces no mana.
#[test]
#[ignore = "requires: cross-player mana-production query ability (Exotic Orchard)"]
fn cr_106_7_ex2_exotic_orchard_mutual_orchard_deadlock() {
    // Would test: if both players control only Exotic Orchards, neither produces mana.
}

/// CR 106.7 – Exotic Orchard when opponent controls an Orchard and you control a Forest.
#[test]
#[ignore = "requires: cross-player mana-production query ability (Exotic Orchard)"]
fn cr_106_7_ex3_exotic_orchard_with_forest_and_opponents_orchard() {
    // Would test: you have Forest + Orchard, opponent has Orchard → both Orchards produce {G}.
}

// ── CR 107 ────────────────────────────────────────────────────────────────────

/// CR 107.1b – A 3/4 creature with -5/-0 applied becomes -2/4.
/// "If a 3/4 creature gets -5/-0, it's a -2/4 creature. It doesn't assign damage in combat.
/// Its total power and toughness is 2."
#[test]
fn cr_107_1b_ex1_negative_power_from_boost() {
    // CR 107.1b: "if the power or toughness of a creature is less than 0, it's still that
    // value (not 0)." This test checks the engine stores and returns negative values.
    let def = make_creature_def(3, 4);
    let mut perm = PermanentState::new(&def);
    perm.pt_boost_until_eot = PTDelta {
        power: -5,
        toughness: 0,
    };

    assert_eq!(perm.effective_power(), Some(-2), "3 + (-5) = -2");
    assert_eq!(perm.effective_toughness(), Some(4), "toughness unchanged");
    // The "total power and toughness" described in the CR (−2 + 4 = 2) is arithmetic sum.
    let total = perm.effective_power().unwrap() + perm.effective_toughness().unwrap();
    assert_eq!(total, 2);
}

/// CR 107.1b – Viridian Joiner with -2 power adds 0 mana (negative power = 0 for mana).
/// "An effect gives it -2/-0, then its ability is activated. The ability adds no mana."
#[test]
#[ignore = "requires: dynamic-power mana ability (Viridian Joiner: add {G} equal to power)"]
fn cr_107_1b_ex2_negative_power_viridian_joiner_adds_zero_mana() {
    // Would test: Viridian Joiner at negative power produces 0 {G} when its ability activates.
    // Negative power is treated as 0 for the purpose of "add mana equal to power."
}

/// CR 107.1b – Chameleon Colossus with -6 power: self-pump {2}{G}{G} where X=power stays 0.
/// "An effect gives it -6/-0, then its ability is activated. It remains a -2/4 creature."
#[test]
#[ignore = "requires: self-referential X-equals-power activated ability (Chameleon Colossus)"]
fn cr_107_1b_ex3_negative_power_chameleon_colossus_pump_stays_zero() {
    // Would test: Colossus at -2 power activates +X/+X (X=power); since power < 0, X=0,
    // so the creature gains +0/+0 and remains -2/4.
}

/// CR 107.4e – {G/W}{G/W} can be paid with {G}{G}.
#[test]
fn cr_107_4e_ex1_hybrid_gw_gw_payable_with_gg() {
    let c = cost(&[
        ManaPip::Hybrid(ManaColor::Green, ManaColor::White),
        ManaPip::Hybrid(ManaColor::Green, ManaColor::White),
    ]);
    let p = pool_from(0, 0, 0, 0, 2, 0);
    let plan = greedy_payment_plan(&c, &p, 20, None);
    assert!(
        plan.is_some(),
        "{{G/W}}{{G/W}} should be payable with {{G}}{{G}}"
    );
    let plan = plan.unwrap();
    assert_eq!(plan.green, 2);
    assert_eq!(plan.white, 0);
    assert_eq!(plan.blood, 0);
}

/// CR 107.4e – {G/W}{G/W} can be paid with {G}{W}.
#[test]
fn cr_107_4e_ex2_hybrid_gw_gw_payable_with_gw() {
    let c = cost(&[
        ManaPip::Hybrid(ManaColor::Green, ManaColor::White),
        ManaPip::Hybrid(ManaColor::Green, ManaColor::White),
    ]);
    // Provide exactly 1G and 1W to force the split payment.
    let p = pool_from(1, 0, 0, 0, 1, 0);
    let plan = greedy_payment_plan(&c, &p, 20, None);
    assert!(
        plan.is_some(),
        "{{G/W}}{{G/W}} should be payable with {{G}}{{W}}"
    );
    let plan = plan.unwrap();
    // Greedy picks the more-available color; with 1G=1W it resolves to 1G+1W.
    assert_eq!(plan.green + plan.white, 2);
    assert_eq!(plan.blood, 0);
}

/// CR 107.4e – {G/W}{G/W} can be paid with {W}{W}.
#[test]
fn cr_107_4e_ex3_hybrid_gw_gw_payable_with_ww() {
    let c = cost(&[
        ManaPip::Hybrid(ManaColor::Green, ManaColor::White),
        ManaPip::Hybrid(ManaColor::Green, ManaColor::White),
    ]);
    let p = pool_from(2, 0, 0, 0, 0, 0);
    let plan = greedy_payment_plan(&c, &p, 20, None);
    assert!(
        plan.is_some(),
        "{{G/W}}{{G/W}} should be payable with {{W}}{{W}}"
    );
    let plan = plan.unwrap();
    assert_eq!(plan.white, 2);
    assert_eq!(plan.green, 0);
}

/// CR 107.4f – {W/P}{W/P} can be paid by spending {W}{W} (when life cannot pay).
#[test]
fn cr_107_4f_ex1_phyrexian_wp_wp_paid_with_ww() {
    let c = cost(&[
        ManaPip::Phyrexian(ManaColor::White),
        ManaPip::Phyrexian(ManaColor::White),
    ]);
    // 0 life forces the greedy algorithm to use mana instead.
    let p = pool_from(2, 0, 0, 0, 0, 0);
    let plan = greedy_payment_plan(&c, &p, 0, None);
    assert!(
        plan.is_some(),
        "{{W/P}}{{W/P}} should be payable with {{W}}{{W}} when life is 0"
    );
    let plan = plan.unwrap();
    assert_eq!(plan.white, 2);
    assert_eq!(plan.blood, 0);
}

/// CR 107.4f – {W/P}{W/P} can be paid by spending {W} and paying 2 life.
#[test]
fn cr_107_4f_ex2_phyrexian_wp_wp_paid_with_w_and_2_life() {
    let c = cost(&[
        ManaPip::Phyrexian(ManaColor::White),
        ManaPip::Phyrexian(ManaColor::White),
    ]);
    // Exactly 2 life: greedy uses life for first pip, then mana (no life left) for second.
    let p = pool_from(2, 0, 0, 0, 0, 0);
    let plan = greedy_payment_plan(&c, &p, 2, None);
    assert!(
        plan.is_some(),
        "{{W/P}}{{W/P}} should be payable with {{W}} + 2 life"
    );
    let plan = plan.unwrap();
    assert_eq!(plan.blood, 1, "one Phyrexian pip paid with 2 life");
    assert_eq!(plan.white, 1, "one Phyrexian pip paid with {{W}}");
}

/// CR 107.4f – {W/P}{W/P} can be paid by paying 4 life (no mana in pool).
#[test]
fn cr_107_4f_ex3_phyrexian_wp_wp_paid_with_4_life() {
    let c = cost(&[
        ManaPip::Phyrexian(ManaColor::White),
        ManaPip::Phyrexian(ManaColor::White),
    ]);
    let p = pool_from(0, 0, 0, 0, 0, 0);
    let plan = greedy_payment_plan(&c, &p, 4, None);
    assert!(
        plan.is_some(),
        "{{W/P}}{{W/P}} should be payable with 4 life"
    );
    let plan = plan.unwrap();
    assert_eq!(
        plan.blood, 2,
        "both pips paid with 2 life each = 4 life total"
    );
    assert_eq!(plan.white, 0);
}

// ── CR 110 ────────────────────────────────────────────────────────────────────

/// CR 110.5c – Dimir Doppelganger copy ability; flip card interaction.
#[test]
#[ignore = "requires: flip-card support and copy-effect engine"]
fn cr_110_5c_ex1_dimir_doppelganger_flip_card_copy() {
    // Would test: Doppelganger becomes a copy of Jushi Apprentice, flips to Tomoya
    // the Revealer via its ability while retaining the Doppelganger copy ability.
}

// ── CR 111 ────────────────────────────────────────────────────────────────────

/// CR 111.3 – Token created by Jade Mage has no mana cost, supertypes, rules text, or abilities.
#[test]
#[ignore = "requires: token-creation engine"]
fn cr_111_3_ex1_jade_mage_saproling_token_properties() {
    // Would test: token created by Jade Mage is a 1/1 green Saproling with no mana cost,
    // no supertypes, no oracle text, and no abilities.
}

/// CR 111.4 – Token name from type: "Dwarven Reinforcements" → "Dwarf Berserker Token".
#[test]
#[ignore = "requires: token-creation engine with type-derived naming"]
fn cr_111_4_ex1_dwarf_berserker_token_name() {
    // Would test: token from Dwarven Reinforcements is named "Dwarf Berserker Token"
    // with subtypes Dwarf and Berserker.
}

/// CR 111.4 – Minsc's Boo token has a custom name, not "Hamster Token".
#[test]
#[ignore = "requires: token-creation engine with explicit naming override"]
fn cr_111_4_ex2_boo_token_name_overrides_type() {
    // Would test: token named "Boo" (Hamster subtype) is not called "Hamster Token".
}

/// CR 111.4 – Spitting Image token copies all copiable characteristics, not named "Human Token".
#[test]
#[ignore = "requires: token-creation engine and copy-effect engine"]
fn cr_111_4_ex3_spitting_image_copy_token_name() {
    // Would test: copy-token of Doomed Dissenter is named "Doomed Dissenter", not "Human Token".
}

/// CR 111.11 – Disa the Restless creates a Tarmogoyf token matching oracle reference.
#[test]
#[ignore = "requires: token-from-oracle-reference engine (Tarmogoyf token)"]
fn cr_111_11_ex1_disa_tarmogoyf_token() {
    // Would test: token created by Disa the Restless matches the current oracle characteristics
    // of a card named Tarmogoyf.
}

/// CR 111.12 – Mimic Vat creates no token if nothing has been exiled with it.
#[test]
#[ignore = "requires: imprint/exile mechanic (Mimic Vat)"]
fn cr_111_12_ex1_mimic_vat_no_exiled_card_no_token() {
    // Would test: Mimic Vat's activated ability produces no token when no card is imprinted.
}

// ── CR 112 ────────────────────────────────────────────────────────────────────

/// CR 112.4 – Color-change effect applies on entry and lasts the duration.
#[test]
#[ignore = "requires: color-modification continuous-effect layer"]
fn cr_112_4_ex1_effect_changes_creature_color_on_entry() {
    // Would test: an effect that changes a black creature spell to white results in a white
    // creature entering the battlefield, remaining white for the effect's duration.
}

// ── CR 113 ────────────────────────────────────────────────────────────────────

/// CR 113.2a – "This creature can't block" is an ability (static ability on the object).
#[test]
#[ignore = "requires: CantBlock static ability variant in the ability enum"]
fn cr_113_2a_ex1_cant_block_is_an_ability() {
    // Would test: oracle text "This creature can't block." is parsed as a StaticAbility
    // (or equivalent), not as reminder text or rules text.
}

/// CR 113.6k – Absolver Thrull's trigger functions from the exile zone.
#[test]
#[ignore = "requires: triggered-ability-active-in-exile-zone support"]
fn cr_113_6k_ex1_absolver_thrull_trigger_from_exile() {
    // Would test: Absolver Thrull's "when the creature it haunts dies" trigger fires from exile.
}

/// CR 113.6m – Reassembling Skeleton can only be activated from graveyard.
#[test]
#[ignore = "requires: zone-restricted activated ability (graveyard-only activation)"]
fn cr_113_6m_ex1_reassembling_skeleton_graveyard_only_activation() {
    // Would test: activating Reassembling Skeleton's ability while it's in a non-graveyard
    // zone is illegal; the ability is only available from the graveyard.
}

/// CR 113.12 – Muraganda Petroglyphs: vanilla Runeclaw Bear enchanted by flight Aura
/// does NOT get the +2/+2 (the Aura grants an ability, so the Bear has abilities).
#[test]
#[ignore = "requires: Muraganda Petroglyphs ability and static PT boost for vanilla creatures"]
fn cr_113_12_ex1_muraganda_vanilla_with_aura_granting_ability() {
    // Would test: Runeclaw Bear enchanted by an Aura granting flying has abilities,
    // so Muraganda Petroglyphs does NOT grant it +2/+2.
}

/// CR 113.12 – Muraganda Petroglyphs: Bear enchanted by "is red" Aura DOES get +2/+2
/// because changing colour is not an ability.
#[test]
#[ignore = "requires: Muraganda Petroglyphs ability and colour-change Aura"]
fn cr_113_12_ex2_muraganda_vanilla_with_color_change_aura_gets_boost() {
    // Would test: Runeclaw Bear enchanted by "enchanted creature is red" still has no abilities,
    // so it receives the +2/+2 from Muraganda Petroglyphs.
}

// ── CR 115 ────────────────────────────────────────────────────────────────────

/// CR 115.1a – A sorcery card's cycling triggered ability is targeted; the card itself is not.
#[test]
#[ignore = "requires: cycling-triggered-ability targeting distinction"]
fn cr_115_1a_ex1_cycling_triggered_ability_is_targeted_card_is_not() {
    // Would test: a sorcery with "When you cycle this card, target creature gets -1/-1" is
    // itself not a targeted spell, only the triggered cycling ability is targeted.
}

// ── CR 115 / 601 ──────────────────────────────────────────────────────────────

/// CR 115.7e – Arc Trail targets can be swapped via Redirect.
#[test]
#[ignore = "requires: target-redirection spell effect (Redirect)"]
fn cr_115_7e_ex1_arc_trail_targets_swapped_via_redirect() {
    // Would test: Redirect changes first target of Arc Trail from Bear to Elves and vice versa,
    // which is legal because the two new targets are different objects.
}

// ── CR 118 ────────────────────────────────────────────────────────────────────

/// CR 118.11 – Psychic Vortex cumulative upkeep: draw replaced by Obstinate Familiar skip.
#[test]
#[ignore = "requires: cumulative-upkeep mechanic and draw-replacement effect"]
fn cr_118_11_ex1_psychic_vortex_cumulative_upkeep_draw_replaced() {
    // Would test: paying Psychic Vortex's cumulative upkeep cost allows the player to use
    // Obstinate Familiar's effect to skip the resulting draws.
}

/// CR 118.12 – Standstill triggered ability can't sacrifice itself if already exiled.
#[test]
#[ignore = "requires: inline-payment obligation (PendingPayment) with missing permanent"]
fn cr_118_12_ex1_standstill_already_exiled_sacrifice_fails() {
    // Would test: if Standstill is exiled before its triggered ability resolves, the
    // "sacrifice Standstill" cost can't be paid, so no cards are drawn.
}

/// CR 118.12 – Gather Specimens redirects token from Dermoplasm cost+choice.
#[test]
#[ignore = "requires: Gather Specimens replacement effect and Dermoplasm morph ability"]
fn cr_118_12_ex2_gather_specimens_dermoplasm_return_still_happens() {
    // Would test: Dermoplasm returns to hand (the cost was paid) even though the morph creature
    // token enters under the opponent's control due to Gather Specimens.
}

// ── CR 120 ────────────────────────────────────────────────────────────────────

/// CR 120.4d – Boon Reflection doubles lifelink damage; damage event sequence.
#[test]
#[ignore = "requires: replacement-effect engine (Boon Reflection), wither, lifelink, prevention"]
fn cr_120_4d_ex1_boon_reflection_wither_lifelink_prevention() {
    // Would test the full damage-event processing chain:
    // 3 damage → prevention reduces to 1 → wither gives -1/-1 counter → lifelink gains 1 life →
    // Boon Reflection doubles to 2 life.
}

/// CR 120.4d – Worship prevents lethal reduction; Awe Strike life prevention interaction.
#[test]
#[ignore = "requires: replacement-effect engine (Worship, Awe Strike) and damage processing"]
fn cr_120_4d_ex2_worship_awe_strike_prevention_life_gain() {
    // Would test: player at 2 life with Worship, attacked by two 5/5s; Awe Strike prevents
    // 5 damage and gains 5 life; net loss is 0 due to prevention, Worship not needed.
}

/// CR 120.5 – Lightning Bolt's damage causes SBA destruction; Bolt itself did not destroy.
#[test]
#[ignore = "requires: damage-attribution tracking (SBA destruction vs direct spell destruction)"]
fn cr_120_5_ex1_lightning_bolt_damage_sba_destroys_not_bolt() {
    // Would test: after Lightning Bolt deals 3 damage to a 2/2 creature, state-based actions
    // destroy it. The creature was NOT destroyed by Lightning Bolt — it was destroyed by SBAs.
}

// ── CR 123 ────────────────────────────────────────────────────────────────────

/// CR 123.6b – Name sticker placement: player chooses word position.
#[test]
#[ignore = "requires: stickers mechanic (Unfinity/Acorn)"]
fn cr_123_6b_ex1_name_sticker_position_choice() {}

/// CR 123.6c – Name sticker position preserved through Adventure exile and return.
#[test]
#[ignore = "requires: stickers mechanic and adventure mechanic"]
fn cr_123_6c_ex1_name_sticker_adventure_exile_position_retained() {}

/// CR 123.6c – Name sticker on a copy-target creature: position after copy.
#[test]
#[ignore = "requires: stickers mechanic and copy effects"]
fn cr_123_6c_ex2_name_sticker_copy_effect_position() {}

/// CR 123.6c – Witness Protection text-change overrides sticker name.
#[test]
#[ignore = "requires: stickers mechanic and Witness Protection text-change effect"]
fn cr_123_6c_ex3_witness_protection_overrides_name_sticker() {}

// ── CR 201 ────────────────────────────────────────────────────────────────────

/// CR 201.2b – Liliana's Contract: four Demons with different names (face-down = no name).
#[test]
#[ignore = "requires: face-down permanents with no name and Liliana's Contract upkeep trigger"]
fn cr_201_2b_ex1_lilianas_contract_face_down_has_no_name() {
    // Would test: a face-down creature with no name does not share a name with others,
    // so four Demons including one face-down do NOT satisfy "four Demons with different names."
}

/// CR 201.4a – "Choose an artifact card name" allows any artifact name, even out-of-format.
#[test]
#[ignore = "requires: name-choice effect implementation (Dispossess)"]
fn cr_201_4a_ex1_dispossess_choose_any_artifact_name() {
    // Would test: a player can name "Black Lotus" for Dispossess even if it's not legal
    // in the format; they cannot choose "Island" even if it's been made an artifact.
}

/// CR 201.5a – Gutter Grime token ability refers to the creating Gutter Grime only.
#[test]
#[ignore = "requires: token-creation engine with source-tracking for self-referential ability"]
fn cr_201_5a_ex1_gutter_grime_token_refers_to_creating_enchantment() {}

/// CR 201.5b – Quicksilver Elemental copies Skithiryx ability; regeneration targets Elemental.
#[test]
#[ignore = "requires: copy-ability mechanic (Quicksilver Elemental) and self-reference resolution"]
fn cr_201_5b_ex1_quicksilver_elemental_copied_ability_self_reference() {}

/// CR 201.5b – Glacial Ray spliced onto Kodama's Reach: the Reach deals damage.
#[test]
#[ignore = "requires: splice-onto-Arcane mechanic"]
fn cr_201_5b_ex2_glacial_ray_splice_reach_deals_damage() {}

/// CR 201.5b – The Ever-Changing 'Dane ability becomes self-referential after copy.
#[test]
#[ignore = "requires: copy-ability self-reference (The Ever-Changing 'Dane)"]
fn cr_201_5b_ex3_ever_changing_dane_self_reference_after_copy() {}

// ── CR 202 ────────────────────────────────────────────────────────────────────

/// CR 202.2a – {2}{W} produces a white object.
#[test]
fn cr_202_2a_ex1_two_w_is_white() {
    let c = cost(&[ManaPip::Generic(2), ManaPip::White]);
    let colors = colors_from_cost(&c);
    assert_eq!(colors, vec![ManaColor::White]);
}

/// CR 202.2a – {2} produces a colorless object (no colored mana symbols).
#[test]
fn cr_202_2a_ex2_generic_only_is_colorless() {
    let c = cost(&[ManaPip::Generic(2)]);
    let colors = colors_from_cost(&c);
    assert!(
        colors.is_empty(),
        "{{2}} has no colored symbols → colorless"
    );
}

/// CR 202.2a – {2}{W}{B} produces a white-and-black object.
#[test]
fn cr_202_2a_ex3_two_w_b_is_white_and_black() {
    let c = cost(&[ManaPip::Generic(2), ManaPip::White, ManaPip::Black]);
    let colors = colors_from_cost(&c);
    assert!(colors.contains(&ManaColor::White));
    assert!(colors.contains(&ManaColor::Black));
    assert_eq!(colors.len(), 2);
}

/// CR 202.3 – {3}{U}{U} has mana value 5.
#[test]
fn cr_202_3_ex1_three_uu_mana_value_5() {
    let c = cost(&[ManaPip::Generic(3), ManaPip::Blue, ManaPip::Blue]);
    assert_eq!(c.mana_value(), 5);
}

/// CR 202.3b – Huntmaster of the Fells: {2}{R}{G} has mana value 4.
#[test]
fn cr_202_3b_ex1_huntmaster_two_rg_mana_value_4() {
    let c = cost(&[ManaPip::Generic(2), ManaPip::Red, ManaPip::Green]);
    assert_eq!(c.mana_value(), 4);
}

/// CR 202.3b – A Clone copying a card with no mana cost has mana value 0.
/// "A Clone enters the battlefield as a copy of Ravager of the Fells. Its mana value is 0."
#[test]
fn cr_202_3b_ex2_no_mana_cost_has_mana_value_zero() {
    // Ravager of the Fells is the back face of a DFC and has no mana cost.
    // We model this as ManaCost::default() (empty pips) representing no mana cost.
    // CR 202.3a: "the total mana value of a permanent with no mana cost is 0."
    let c = ManaCost::default();
    assert_eq!(c.mana_value(), 0);
}

/// CR 202.3b – Insectile Aberration (back face) copying Ravager of the Fells: mana value 0.
#[test]
fn cr_202_3b_ex3_back_face_copy_no_cost_mana_value_zero() {
    // Same rule as ex2: when a permanent's copiable value is a card with no mana cost,
    // the copy's mana value is 0.
    let c = ManaCost::default();
    assert_eq!(c.mana_value(), 0);
}

/// CR 202.3f – {1}{W/U}{W/U} has mana value 3.
#[test]
fn cr_202_3f_ex1_hybrid_1_wu_wu_mana_value_3() {
    let c = cost(&[
        ManaPip::Generic(1),
        ManaPip::Hybrid(ManaColor::White, ManaColor::Blue),
        ManaPip::Hybrid(ManaColor::White, ManaColor::Blue),
    ]);
    assert_eq!(c.mana_value(), 3);
}

/// CR 202.3f – {2/B}{2/B}{2/B} has mana value 6.
#[test]
fn cr_202_3f_ex2_generic_hybrid_2b_2b_2b_mana_value_6() {
    let c = cost(&[
        ManaPip::GenericHybrid(2, ManaColor::Black),
        ManaPip::GenericHybrid(2, ManaColor::Black),
        ManaPip::GenericHybrid(2, ManaColor::Black),
    ]);
    assert_eq!(c.mana_value(), 6);
}

/// CR 202.3g – {1}{W/P}{W/P} has mana value 3.
#[test]
fn cr_202_3g_ex1_phyrexian_1_wp_wp_mana_value_3() {
    let c = cost(&[
        ManaPip::Generic(1),
        ManaPip::Phyrexian(ManaColor::White),
        ManaPip::Phyrexian(ManaColor::White),
    ]);
    assert_eq!(c.mana_value(), 3);
}

// ── CR 205 ────────────────────────────────────────────────────────────────────

/// CR 205.1b – "All lands are 1/1 creatures" leaves land card types intact.
#[test]
#[ignore = "requires: continuous-effect layer adding creature type to lands"]
fn cr_205_1b_ex1_all_lands_1_1_creatures_retain_land_type() {
    // Would test: under "all lands are 1/1 creatures that are still lands", a land that
    // was also an artifact before the effect remains an artifact land creature.
}

/// CR 205.1b – "All artifacts are 1/1 artifact creatures" includes artifact+enchantment.
#[test]
#[ignore = "requires: continuous-effect layer adding creature type to artifacts"]
fn cr_205_1b_ex2_all_artifacts_1_1_artifact_creatures_preserve_enchantment() {
    // Would test: under "all artifacts are 1/1 artifact creatures", a permanent that is
    // both an artifact and an enchantment becomes an artifact enchantment creature.
}

/// CR 205.3b – "Basic Land — Mountain" type line: land, subtype Mountain, not a creature.
#[test]
fn cr_205_3b_ex1_basic_land_mountain_type_line() {
    let tl = TypeLine {
        supertypes: vec![Supertype::Basic],
        card_types: vec![CardType::Land],
        subtypes: vec!["Mountain".to_string()],
    };
    assert!(tl.is_land());
    assert!(!tl.is_creature());
    assert!(tl.subtypes.contains(&"Mountain".to_string()));
    assert!(tl.supertypes.contains(&Supertype::Basic));
}

/// CR 205.3c – Dryad Arbor: "Land Creature — Forest Dryad" is both a land and a creature.
#[test]
fn cr_205_3c_ex1_dryad_arbor_land_and_creature() {
    use mecha_oracle::cards::CardDatabase;
    use std::path::Path;
    let db = CardDatabase::from_path(Path::new("tests/fixtures/oracle_cards_test.json")).unwrap();
    let dryad = db
        .get("Dryad Arbor")
        .expect("Dryad Arbor must be in test fixture");
    assert!(dryad.type_line.is_land(), "Dryad Arbor is a land");
    assert!(dryad.type_line.is_creature(), "Dryad Arbor is a creature");
    assert!(
        dryad.type_line.subtypes.contains(&"Forest".to_string()),
        "subtype Forest"
    );
    assert!(
        dryad.type_line.subtypes.contains(&"Dryad".to_string()),
        "subtype Dryad"
    );
}

/// CR 205.3e – "Merfolk" and "Wizard" are acceptable creature types; "Human Soldier" is not.
#[test]
#[ignore = "requires: creature-type legality check (whitelist of official creature types)"]
fn cr_205_3e_ex1_merfolk_wizard_acceptable_human_soldier_not_two_words() {
    // Would test: when choosing a creature type, "Merfolk" or "Wizard" are valid single-word
    // types, but "Human Soldier" is not (it's two creature types, not one).
}

/// CR 205.4b – "All lands are 1/1 creatures" gives lands creature supertype structure.
#[test]
#[ignore = "requires: continuous-effect layer giving lands power/toughness"]
fn cr_205_4b_ex1_all_lands_1_1_creature_card_type_structure() {
    // Would test: card type "creature" is added to lands; they get 1/1 base P/T.
}

// ── CR 208 ────────────────────────────────────────────────────────────────────

/// CR 208.2a – Lost Order of Jarkeld with 1+* power/toughness.
#[test]
#[ignore = "requires: characteristic-defining ability (CDA) for * power/toughness"]
fn cr_208_2a_ex1_lost_order_of_jarkeld_star_power_toughness() {
    // Would test: Lost Order of Jarkeld's power and toughness are each 1 plus the number
    // of creatures the defending player controls.
}

/// CR 208.3a – Veteran Motorist's pump ability applies before vehicle's P/T is determined.
#[test]
#[ignore = "requires: crew mechanic and vehicle card type with entering-crew effect"]
fn cr_208_3a_ex1_veteran_motorist_pump_before_vehicle_pt() {}

// ── CR 302 ────────────────────────────────────────────────────────────────────

/// CR 302.3 – "Creature — Goblin Wizard" means the card has subtypes Goblin and Wizard.
#[test]
fn cr_302_3_ex1_goblin_wizard_subtypes() {
    let tl = TypeLine {
        supertypes: vec![],
        card_types: vec![CardType::Creature],
        subtypes: vec!["Goblin".to_string(), "Wizard".to_string()],
    };
    assert!(tl.is_creature());
    assert!(tl.subtypes.contains(&"Goblin".to_string()));
    assert!(tl.subtypes.contains(&"Wizard".to_string()));
}

// ── CR 305 ────────────────────────────────────────────────────────────────────

/// CR 305.5 – "Basic Land — Mountain" means land with subtype Mountain, not "Mountain Basic".
#[test]
fn cr_305_5_ex1_basic_land_mountain_subtype() {
    let tl = TypeLine {
        supertypes: vec![Supertype::Basic],
        card_types: vec![CardType::Land],
        subtypes: vec!["Mountain".to_string()],
    };
    assert!(tl.is_land());
    assert!(tl.subtypes.contains(&"Mountain".to_string()));
    // Supertype and subtype are separate from card type.
    assert!(tl.supertypes.contains(&Supertype::Basic));
    assert!(!tl.card_types.contains(&CardType::Creature));
}

// ── CR 400 ────────────────────────────────────────────────────────────────────

/// CR 400.6 – Exquisite Archangel: when player would die from combined sources,
/// replacement chooses whether to exile Archangel or put it in graveyard.
#[test]
#[ignore = "requires: replacement-effect engine and simultaneous SBA interaction"]
fn cr_400_6_ex1_exquisite_archangel_replacement_simultaneous_sba() {
    // Would test: player loses from two simultaneous causes while controlling Exquisite Archangel;
    // controller chooses whether Archangel moves to graveyard or exile.
}

// ── CR 500 ────────────────────────────────────────────────────────────────────

/// CR 500.10 – Obeka adds beginning phases (not just upkeep steps) after this phase.
#[test]
#[ignore = "requires: extra-phase addition (Obeka, Splitter of Seconds) and step-skipping"]
fn cr_500_10_ex1_obeka_adds_beginning_phases_skipping_untap_and_draw() {}

// ── CR 508 ────────────────────────────────────────────────────────────────────

/// CR 508.1c – Two creatures with "must attack" but with contradictory restrictions.
#[test]
#[ignore = "requires: attack-requirement and attack-restriction interaction"]
fn cr_508_1c_ex1_two_attack_if_able_creatures_with_restrictions() {}

/// CR 508.1d – One creature "attacks if able" + one with no abilities; menace attacker.
#[test]
#[ignore = "requires: attack-requirement with menace-restriction interaction"]
fn cr_508_1d_ex1_attacks_if_able_plus_menace_restriction() {}

/// CR 508.2a – Permanent ability triggers when a green creature attacks.
#[test]
#[ignore = "requires: triggered ability keyed on creature colour while attacking"]
fn cr_508_2a_ex1_trigger_when_green_creature_attacks() {}

// ── CR 509 ────────────────────────────────────────────────────────────────────

/// CR 509.1b – Flying + shadow cannot be blocked by a creature with flying but no shadow.
#[test]
fn cr_509_1b_ex1_flying_and_shadow_evasion_cumulative() {
    use mecha_oracle::engine::combat::can_block_attacker;
    use mecha_oracle::engine::turn::skip_to_first_main;

    fn def_with_keywords(kws: &[StaticAbility]) -> CardDefinition {
        let mut d = make_creature_def(1, 1);
        d.rules_text = kws
            .iter()
            .map(|k| RulesText::Active(Rule::Static(k.clone())))
            .collect();
        d
    }

    let p0 = PlayerId(0);
    let p1 = PlayerId(1);
    let mut gs = skip_to_first_main(GameState::new(vec![
        Player::new(p0, "Alice"),
        Player::new(p1, "Bob"),
    ]));

    // Place flying+shadow attacker for Alice.
    let att_id = gs.alloc_id();
    let att_def = def_with_keywords(&[StaticAbility::Flying, StaticAbility::Shadow]);
    let att_obj = CardObject::new(att_id, att_def, p0, Zone::Battlefield);
    let mut att_perm = PermanentState::new(&att_obj.definition);
    att_perm.controller_since_turn = 0;
    gs.battlefield.insert(att_id, att_perm);
    gs.add_object(att_obj);

    // Place flying-only blocker for Bob.
    let blk_id = gs.alloc_id();
    let blk_def = def_with_keywords(&[StaticAbility::Flying]);
    let blk_obj = CardObject::new(blk_id, blk_def, p1, Zone::Battlefield);
    let blk_perm = PermanentState::new(&blk_obj.definition);
    gs.battlefield.insert(blk_id, blk_perm);
    gs.add_object(blk_obj);

    // CR 509.1b: flying+shadow creature can't be blocked by a flying-only creature.
    assert!(
        !can_block_attacker(&gs, blk_id, att_id),
        "flying+shadow attacker cannot be blocked by flying-only creature"
    );
}

/// CR 509.1c – "blocks if able" + menace: must block with both or violates restriction.
#[test]
#[ignore = "requires: declare-blockers legality check with 'blocks if able' requirement"]
fn cr_509_1c_ex1_blocks_if_able_and_menace_must_block_with_both() {}

/// CR 509.3f – Creature ability triggers when it becomes blocked by a particular condition.
#[test]
#[ignore = "requires: becomes-blocked triggered ability with condition filtering"]
fn cr_509_3f_ex1_becomes_blocked_trigger_condition() {}

// ── CR 510 ────────────────────────────────────────────────────────────────────

/// CR 510.1c – Elvish Regrower (4/3) blocked by Vampire Spawn (2/3) and Helpful Hunter (1/1):
/// controller may assign damage in any legal division.
#[test]
#[ignore = "requires: multi-blocker damage assignment validation"]
fn cr_510_1c_ex1_blocked_by_two_creatures_legal_damage_assignments() {
    // Would test: Elvish Regrower can assign: 4 to Hunter, 1+3, 2+2, 3+1, or 4 to Spawn.
    // All are legal; would be illegal to assign less than lethal to a blocker if there's
    // enough damage to continue to the next (per trample rules — but no trample here).
}

// ── CR 601 ────────────────────────────────────────────────────────────────────

/// CR 601.2c – "Tap two target creatures" can't target the same creature twice.
#[test]
#[ignore = "requires: targeting-distinctness rule for spells with multiple targets"]
fn cr_601_2c_ex1_tap_two_targets_must_be_distinct() {}

/// CR 601.2h – Altar's Reap: additional cost (sacrifice a creature) paid on casting.
#[test]
#[ignore = "requires: additional-cost (sacrifice) payment during casting"]
fn cr_601_2h_ex1_altars_reap_sacrifice_additional_cost() {}

/// CR 601.3a – Void Winnower restricts casting spells with even mana value.
#[test]
#[ignore = "requires: casting-restriction continuous effect (Void Winnower)"]
fn cr_601_3a_ex1_void_winnower_restricts_even_mv_spells() {}

/// CR 601.3b – Aura spells granted flash can be cast at instant speed.
#[test]
#[ignore = "requires: flash-granting effect for specific spell types"]
fn cr_601_3b_ex1_aura_spells_cast_as_if_they_had_flash() {}

/// CR 601.3e – Garruk's Horde: cast creature spell from top of library.
#[test]
#[ignore = "requires: cast-from-top-of-library permission effect"]
fn cr_601_3e_ex1_garruk_horde_cast_from_top_of_library() {}

/// CR 601.3e – Melek: cast instant/sorcery from top of library.
#[test]
#[ignore = "requires: cast-from-top-of-library for instant/sorcery"]
fn cr_601_3e_ex2_melek_cast_instant_sorcery_from_top() {}

/// CR 601.4 – Modal spell with kicker: Inscription of Abundance modes.
#[test]
#[ignore = "requires: modal spells and kicker mechanic"]
fn cr_601_4_ex1_inscription_of_abundance_modal_with_kicker() {}

// ── CR 602 ────────────────────────────────────────────────────────────────────

/// CR 602.1a – Activation cost "{2}, {T}" means pay 2 generic + tap in any order.
#[test]
#[ignore = "requires: multi-component activation cost with flexible ordering"]
fn cr_602_1a_ex1_activation_cost_pay_and_tap_any_order() {}

// ── CR 603 ────────────────────────────────────────────────────────────────────

/// CR 603.2c – "Whenever a land becomes tapped" trigger fires on tap for mana.
#[test]
#[ignore = "requires: tap-for-mana triggered ability (TriggerEvent::LandTappedForMana)"]
fn cr_603_2c_ex1_trigger_when_land_becomes_tapped_for_mana() {}

/// CR 603.2e – "Becomes tapped" trigger fires only when transitioning from untapped.
#[test]
#[ignore = "requires: state-change trigger (untapped→tapped only, not tapped→tapped)"]
fn cr_603_2e_ex1_becomes_tapped_only_on_state_change() {}

/// CR 603.2g – Damage trigger doesn't fire if all damage is prevented.
#[test]
#[ignore = "requires: damage-prevention effect and trigger-cancellation on zero damage"]
fn cr_603_2g_ex1_damage_trigger_skipped_if_all_prevented() {}

/// CR 603.4 – Felidar Sovereign upkeep trigger: fires on controller's upkeep.
#[test]
#[ignore = "requires: upkeep-conditional win trigger (Felidar Sovereign)"]
fn cr_603_4_ex1_felidar_sovereign_upkeep_trigger_check_life() {}

/// CR 603.6b – Token from "if a creature would enter" effect triggered by land.
#[test]
#[ignore = "requires: ETB replacement effect producing tokens (complex interaction)"]
fn cr_603_6b_ex1_would_enter_trigger_produces_token_for_land() {}

/// CR 603.7a – "When this creature leaves the battlefield" trigger from wrong zone.
#[test]
#[ignore = "requires: leaves-battlefield trigger zone tracking"]
fn cr_603_7a_ex1_when_creature_leaves_battlefield_not_in_bf() {}

/// CR 603.7a – "When this creature becomes untapped" trigger won't fire if card never on BF.
#[test]
#[ignore = "requires: zone-tracking for would-trigger-in-wrong-zone"]
fn cr_603_7a_ex2_becomes_untapped_trigger_requires_on_battlefield() {}

/// CR 603.7c – Delayed trigger from delayed-triggered ability at beginning of next end step.
#[test]
#[ignore = "requires: delayed-triggered-ability engine"]
fn cr_603_7c_ex1_delayed_trigger_at_beginning_of_next_end_step() {}

/// CR 603.8 – "Whenever you have no cards in hand, draw a card" self-triggers.
#[test]
#[ignore = "requires: self-referential draw trigger keyed on hand-size state"]
fn cr_603_8_ex1_no_cards_in_hand_draw_self_trigger() {}

/// CR 603.10a – Two simultaneous triggers from same event, same controller.
#[test]
#[ignore = "requires: multiple-simultaneous-triggers ordering (APNAP)"]
fn cr_603_10a_ex1_two_simultaneous_triggers_same_controller() {}

/// CR 603.11 – "Reveal first card drawn each turn" trigger during opponent's turn.
#[test]
#[ignore = "requires: reveal-first-draw-per-turn triggered ability across turns"]
fn cr_603_11_ex1_reveal_first_card_drawn_each_turn() {}

/// CR 603.12 – Heart-Piercer Manticore ETB ability.
#[test]
#[ignore = "requires: ETB triggered ability with optional sacrifice cost (Heart-Piercer Manticore)"]
fn cr_603_12_ex1_heart_piercer_manticore_etb_sacrifice() {}

// ── CR 605 ────────────────────────────────────────────────────────────────────

/// CR 605.2 – Mana ability produces mana for each creature you control.
#[test]
#[ignore = "requires: dynamic mana-production ability keyed on creature count"]
fn cr_605_2_ex1_mana_ability_g_for_each_creature_you_control() {}

/// CR 605.4a – "Whenever a player taps a land for mana" triggers on opponents' lands too.
#[test]
#[ignore = "requires: triggered ability on mana tapping (mana ability distinction)"]
fn cr_605_4a_ex1_trigger_on_any_player_tapping_land() {}

// ── CR 606 ────────────────────────────────────────────────────────────────────

/// CR 606.5 – Carth the Lion increases planeswalker loyalty ability costs.
#[test]
#[ignore = "requires: loyalty-ability cost modification (Carth the Lion)"]
fn cr_606_5_ex1_carth_the_lion_loyalty_cost_modification() {}

// ── CR 607 ────────────────────────────────────────────────────────────────────

/// CR 607.2i – Stormscape Battlemage: kicker with two independent kicked abilities.
#[test]
#[ignore = "requires: multi-kicker-mode ability resolution (Stormscape Battlemage)"]
fn cr_607_2i_ex1_stormscape_battlemage_kicker_modes() {}

/// CR 607.4 – Paradise Plume: choose colour on entry, relevant abilities link to that choice.
#[test]
#[ignore = "requires: as-this-enters choice binding to linked ability"]
fn cr_607_4_ex1_paradise_plume_choice_linked_abilities() {}

/// CR 607.5 – Arc-Slogger: activation limit tracks exiled count.
#[test]
#[ignore = "requires: activation-limit mechanic (Arc-Slogger exile 10 cards)"]
fn cr_607_5_ex1_arc_slogger_activation_limit() {}

/// CR 607.5a – Voice of All copied by Unstable Shapeshifter on entry.
#[test]
#[ignore = "requires: copy-effect engine and as-this-enters choice propagation"]
fn cr_607_5a_ex1_voice_of_all_copy_choice_propagation() {}

/// CR 607.5a – Vesuvan Doppelganger copies Voice of All; Doppelganger makes its own choice.
#[test]
#[ignore = "requires: copy-effect engine and as-this-enters independent choice"]
fn cr_607_5a_ex2_vesuvan_doppelganger_copies_voice_of_all_independent_choice() {}

// ── CR 608 ────────────────────────────────────────────────────────────────────

/// CR 608.2b – Sorin's Thirst: deals damage and creates life link simultaneously.
#[test]
#[ignore = "requires: spell-resolution multi-step effect (damage + controller gains life)"]
fn cr_608_2b_ex1_sorins_thirst_simultaneous_damage_and_life_loss() {}

/// CR 608.2b – Plague Spores targets vanish before resolution: only remaining targets affected.
#[test]
#[ignore = "requires: target-validity check on resolution (CR 608.2b illegal-target handling)"]
fn cr_608_2b_ex2_plague_spores_illegal_target_on_resolution() {}

/// CR 608.2d – Optional sacrifice during resolution: player may choose not to sacrifice.
#[test]
#[ignore = "requires: optional-during-resolution sacrifice effect"]
fn cr_608_2d_ex1_optional_sacrifice_during_resolution() {}

/// CR 608.2f – Blatant Thievery: separate control-gain effect for each opponent.
#[test]
#[ignore = "requires: per-opponent control-gaining spell (multiplayer)"]
fn cr_608_2f_ex1_blatant_thievery_per_opponent_control_gain() {}

/// CR 608.2f – Soulfire Eruption choosing any number of targets.
#[test]
#[ignore = "requires: variable-target spell resolution (Soulfire Eruption)"]
fn cr_608_2f_ex2_soulfire_eruption_any_number_of_targets() {}

/// CR 608.2i – Bear Cub mid-turn becomes unattackable; effect still resolves.
#[test]
#[ignore = "requires: mid-resolution zone-change and effect continuation"]
fn cr_608_2i_ex1_effect_continues_after_target_leaves_attacking() {}

/// CR 608.2j – "Destroy all black creatures" destroys white-black multicoloured creature.
#[test]
#[ignore = "requires: colour-checking against permanent's colour (multi-colour destroy)"]
fn cr_608_2j_ex1_destroy_all_black_destroys_white_black_creature() {}

/// CR 608.2k – Wall of Tears returns blocker to hand when it blocks.
#[test]
#[ignore = "requires: blocks-triggered ability returning blocking creature"]
fn cr_608_2k_ex1_wall_of_tears_returns_blocker() {}

/// CR 608.3e – "Lands can't enter the battlefield" prevents playing lands that turn.
#[test]
#[ignore = "requires: land-entry prevention continuous effect (Worms of the Earth)"]
fn cr_608_3e_ex1_lands_cant_enter_battlefield_prevention() {}

// ── CR 609 ────────────────────────────────────────────────────────────────────

/// CR 609.2 – "All lands are creatures" doesn't affect cards in hand/library.
#[test]
#[ignore = "requires: continuous-effect layer scope (battlefield only, not other zones)"]
fn cr_609_2_ex1_all_lands_creatures_only_affects_battlefield() {}

/// CR 609.3 – "Discard two cards" when player has only one: player discards one, fails to discard more.
#[test]
#[ignore = "requires: discard-effect with insufficient hand size handling"]
fn cr_609_3_ex1_discard_two_with_only_one_card_in_hand() {}

/// CR 609.4a – Vedalken Orrery grants flash to all nonland spells.
#[test]
#[ignore = "requires: flash-granting static ability from artifact (Vedalken Orrery)"]
fn cr_609_4a_ex1_vedalken_orrery_flash_all_nonland_spells() {}

// ── CR 610 ────────────────────────────────────────────────────────────────────

/// CR 610.3d – Two Banisher Priests destroyed simultaneously; exiled cards return.
#[test]
#[ignore = "requires: delayed-return from Banisher Priest exile and simultaneous destruction"]
fn cr_610_3d_ex1_two_banisher_priests_destroyed_simultaneously() {}

// ── CR 611 ────────────────────────────────────────────────────────────────────

/// CR 611.2b – Master Thief: gain control on entry.
#[test]
#[ignore = "requires: gain-control continuous effect (until leaves battlefield)"]
fn cr_611_2b_ex1_master_thief_gain_control_on_enter() {}

/// CR 611.2c – "All white creatures get +1/+1 until end of turn": later ETB creatures affected.
#[test]
#[ignore = "requires: ongoing until-end-of-turn effect applying to creatures that enter later"]
fn cr_611_2c_ex1_eot_pt_boost_applies_to_later_entrants() {}

/// CR 611.2c – "Prevent all damage creatures would deal this turn" persists.
#[test]
#[ignore = "requires: ongoing prevention effect for all creature damage that turn"]
fn cr_611_2c_ex2_prevent_all_creature_damage_eot_effect() {}

/// CR 611.2e – Arbiter of the Ideal manifests top card onto battlefield.
#[test]
#[ignore = "requires: manifest mechanic and top-of-library reveal"]
fn cr_611_2e_ex1_arbiter_of_the_ideal_manifest() {}

/// CR 611.3b – "All white creatures get +1/+1" static ability generates continuous effect.
#[test]
#[ignore = "requires: static-ability continuous-effect generation (PT boost for all white)"]
fn cr_611_3b_ex1_static_all_white_get_1_1_generates_continuous_effect() {}

/// CR 611.3c – Same static ability off/on battlefield applies/removes effect.
#[test]
#[ignore = "requires: static-effect activation/deactivation on zone change"]
fn cr_611_3c_ex1_static_effect_on_off_battlefield() {}

// ── CR 613 ────────────────────────────────────────────────────────────────────

/// CR 613.4d – P/T with switch (+0/+1, then switch): example 1.
#[test]
#[ignore = "requires: layer-7 P/T effect ordering (PT modification then switch)"]
fn cr_613_4d_ex1_pt_switch_after_modification() {}

/// CR 613.4d – P/T with switch: example 2.
#[test]
#[ignore = "requires: layer-7 P/T effect ordering"]
fn cr_613_4d_ex2_pt_switch_order_matters() {}

/// CR 613.4d – P/T with switch: example 3.
#[test]
#[ignore = "requires: layer-7 P/T effect ordering"]
fn cr_613_4d_ex3_pt_switch_with_prior_modification() {}

/// CR 613.5 – Honor of the Pure adds +1/+1 to white creatures continuously.
#[test]
#[ignore = "requires: continuous-effect PT boost for colored creatures (Honor of the Pure)"]
fn cr_613_5_ex1_honor_of_the_pure_static_pt_boost() {}

/// CR 613.5 – Gray Ogre (2/2) with +1/+1 counter becomes 3/3; then effects apply on top.
#[test]
#[ignore = "requires: counter-based PT and layered continuous-effect PT stacking"]
fn cr_613_5_ex2_gray_ogre_counter_and_effects() {}

/// CR 613.6 – "+1/+1 and becomes red" effect timestamp ordering.
#[test]
#[ignore = "requires: layer system with timestamp ordering for colour and PT"]
fn cr_613_6_ex1_color_and_pt_effect_timestamp_ordering() {}

/// CR 613.6 – Act of Treason control changes alongside PT effects.
#[test]
#[ignore = "requires: control-change effect with PT modification in layer system"]
fn cr_613_6_ex2_act_of_treason_control_change_with_pt() {}

/// CR 613.6 – "All noncreature artifacts become 2/2 artifact creatures."
#[test]
#[ignore = "requires: type-change + PT-setting continuous effect for artifacts"]
fn cr_613_6_ex3_noncreature_artifacts_become_2_2() {}

/// CR 613.6 – Svogthos can become a creature via activated ability.
#[test]
#[ignore = "requires: activated type-change ability (Svogthos, the Restless Tomb)"]
fn cr_613_6_ex4_svogthos_activated_creature_ability() {}

/// CR 613.7a – Rune of Flight grants Equipment it enchants a flying ability.
#[test]
#[ignore = "requires: Aura granting Equipment an ability that the equipped creature gains"]
fn cr_613_7a_ex1_rune_of_flight_grants_equipment_flying() {}

/// CR 613.9 – Two conflicting effects: "can't block" vs "must block".
#[test]
#[ignore = "requires: restriction/requirement conflict resolution in declare-blockers"]
fn cr_613_9_ex1_cant_block_vs_must_block_conflict() {}

/// CR 613.9 – "+1/+1" for white creatures vs "enchanted creature loses all abilities".
#[test]
#[ignore = "requires: layer-based ability-granting vs ability-removing interaction"]
fn cr_613_9_ex2_pt_boost_vs_loses_all_abilities() {}

// ── CR 614 ────────────────────────────────────────────────────────────────────

/// CR 614.4 – Regeneration can be activated in response to damage.
#[test]
#[ignore = "requires: regeneration mechanic and damage-response activation"]
fn cr_614_4_ex1_regeneration_in_response_to_damage() {}

/// CR 614.5 – Two independent "if X would happen, instead Y" replacement effects.
#[test]
#[ignore = "requires: multiple-replacement-effect ordering (CR 616 layer)"]
fn cr_614_5_ex1_two_simultaneous_replacement_effects() {}

/// CR 614.12 – Voice of All: as-enters choice applies as replacement to ETB event.
#[test]
#[ignore = "requires: as-this-enters replacement effect for choice-on-entry"]
fn cr_614_12_ex1_voice_of_all_enter_choice_replacement() {}

/// CR 614.12 – Yixlid Jailer: cards in graveyards lose all abilities.
#[test]
#[ignore = "requires: continuous effect removing abilities from graveyard cards"]
fn cr_614_12_ex2_yixlid_jailer_graveyard_cards_lose_abilities() {}

/// CR 614.12 – Orb of Dreams: permanents enter tapped (replacement for ETB).
#[test]
#[ignore = "requires: enters-tapped replacement effect (Orb of Dreams)"]
fn cr_614_12_ex3_orb_of_dreams_permanents_enter_tapped() {}

/// CR 614.13a – Sutured Ghoul: exile as-enters choice.
#[test]
#[ignore = "requires: as-enters exile-from-hand choice (Sutured Ghoul)"]
fn cr_614_13a_ex1_sutured_ghoul_exile_as_enters() {}

/// CR 614.13b – Jund plane card: triggered ability from plane.
#[test]
#[ignore = "requires: planechase plane card triggered ability"]
fn cr_614_13b_ex1_jund_plane_triggered_ability() {}

/// CR 614.13c – Ashiok, Wicked Manipulator: pay life instead of exile.
#[test]
#[ignore = "requires: pay-life-instead-of-exile replacement effect (Ashiok, Wicked Manipulator)"]
fn cr_614_13c_ex1_ashiok_pay_life_instead_of_exile() {}

// ── CR 615 ────────────────────────────────────────────────────────────────────

/// CR 615.4 – Prevention ability activated in response to pending damage.
#[test]
#[ignore = "requires: damage-prevention mechanic with response timing"]
fn cr_615_4_ex1_prevention_activated_in_response_to_damage() {}

/// CR 615.10 – Daunting Defender: redirects damage to Clerics.
#[test]
#[ignore = "requires: damage-redirection prevention effect (Daunting Defender)"]
fn cr_615_10_ex1_daunting_defender_cleric_damage_redirect() {}

/// CR 615.11 – Wojek Apothecary: prevent next 1 damage, gain life equal to prevented.
#[test]
#[ignore = "requires: prevention-with-life-gain effect (Wojek Apothecary)"]
fn cr_615_11_ex1_wojek_apothecary_prevent_and_gain_life() {}

// ── CR 616 ────────────────────────────────────────────────────────────────────

/// CR 616.1f – Two "if would enter, instead" replacement effects.
#[test]
#[ignore = "requires: multiple ETB replacement effects and controller choice ordering"]
fn cr_616_1f_ex1_two_etb_replacement_effects() {}

/// CR 616.1f – Essence of the Wild: creatures you control enter as copies of itself.
#[test]
#[ignore = "requires: ETB replacement 'enters as copy' (Essence of the Wild)"]
fn cr_616_1f_ex2_essence_of_the_wild_enters_as_copy() {}

/// CR 616.1g – Token that's a copy of Voice of All: controller makes choice for it.
#[test]
#[ignore = "requires: token-copy of as-enters-choice card (Voice of All token)"]
fn cr_616_1g_ex1_token_copy_of_voice_of_all_choice() {}

/// CR 616.2 – "If you would gain life, draw that many cards instead" replacement.
#[test]
#[ignore = "requires: gain-life replacement drawing cards instead"]
fn cr_616_2_ex1_life_gain_replaced_by_card_draw() {}

// ── CR 700 ────────────────────────────────────────────────────────────────────

/// CR 700.1 – Blocked by two creatures is one event (not two).
#[test]
#[ignore = "requires: event-semantics tracking (blocked-by-two = one block event)"]
fn cr_700_1_ex1_blocked_by_two_is_one_event() {}

/// CR 700.3c – Fact or Fiction: opponent separates cards into two piles.
#[test]
#[ignore = "requires: reveal + pile-division player choice effect (Fact or Fiction)"]
fn cr_700_3c_ex1_fact_or_fiction_opponent_separates_piles() {}

/// CR 700.5a – Altar of the Pantheon devotion: colorless artifact with no colored mana cost.
#[test]
#[ignore = "requires: devotion counting mechanic (number of colored mana symbols)"]
fn cr_700_5a_ex1_altar_of_the_pantheon_devotion_colorless() {}

/// CR 700.7 – Effect yields two results simultaneously.
#[test]
#[ignore = "requires: simultaneous multi-part effect resolution"]
fn cr_700_7_ex1_simultaneous_effect_two_results() {}

/// CR 700.14 – Bark-Knuckle Boxer when cast.
#[test]
#[ignore = "requires: Bark-Knuckle Boxer triggered-when-cast ability"]
fn cr_700_14_ex1_bark_knuckle_boxer_when_cast() {}

// ── CR 701 ────────────────────────────────────────────────────────────────────

/// CR 701.12a – Exchange control: one target illegal → neither exchanges.
#[test]
#[ignore = "requires: exchange-control effect (both targets must be legal)"]
fn cr_701_12a_ex1_exchange_control_one_illegal_target() {}

/// CR 701.20c – Telepathy: opponents play with hand revealed.
#[test]
#[ignore = "requires: play-with-revealed-hand continuous effect (Telepathy)"]
fn cr_701_20c_ex1_telepathy_opponents_revealed_hand() {}

/// CR 701.23b – Splinter: exile artifact + search for same name in owner's graveyard.
#[test]
#[ignore = "requires: search-graveyard-by-name effect (Splinter)"]
fn cr_701_23b_ex1_splinter_exile_and_search_graveyard() {}

/// CR 701.23c – Lobotomy: reveal hand, choose card, exile all copies from all zones.
#[test]
#[ignore = "requires: search-all-zones-and-exile effect (Lobotomy)"]
fn cr_701_23c_ex1_lobotomy_exile_all_copies() {}

/// CR 701.23f – Aven Mindcensor restricts search to top 4 cards.
#[test]
#[ignore = "requires: search-restriction replacement effect (Aven Mindcensor)"]
fn cr_701_23f_ex1_aven_mindcensor_restrict_search_to_top_4() {}

/// CR 701.24c – Guile: shuffle into library on going to graveyard (from anywhere).
#[test]
#[ignore = "requires: graveyard-replacement shuffle effect (Guile)"]
fn cr_701_24c_ex1_guile_shuffles_into_library_from_graveyard() {}

/// CR 701.24c – Black Sun's Zenith: shuffle into library on going to graveyard.
#[test]
#[ignore = "requires: graveyard-replacement shuffle effect (Black Sun's Zenith)"]
fn cr_701_24c_ex2_black_suns_zenith_shuffles_into_library() {}

/// CR 701.24d – Loaming Shaman: shuffle target player's graveyard into their library.
#[test]
#[ignore = "requires: triggered shuffle-graveyard-into-library on ETB (Loaming Shaman)"]
fn cr_701_24d_ex1_loaming_shaman_etb_shuffle_graveyard() {}

/// CR 701.24g – Darksteel Colossus + Gravebane Zombie going to graveyard simultaneously.
#[test]
#[ignore = "requires: simultaneous zone-change with different replacement effects"]
fn cr_701_24g_ex1_darksteel_colossus_and_gravebane_zombie_simultaneous() {}

/// CR 701.42c – Midnight Scavengers and copy token: conjure-related.
#[test]
#[ignore = "requires: conjure mechanic (Midnight Scavengers meld conjure)"]
fn cr_701_42c_ex1_midnight_scavengers_conjure() {}

/// CR 701.67b – Spirit Water Revival spell: craft-cost mechanic.
#[test]
#[ignore = "requires: craft mechanic (Spirit Water Revival)"]
fn cr_701_67b_ex1_spirit_water_revival_craft() {}

// ── CR 702 ────────────────────────────────────────────────────────────────────

/// CR 702.1a – Varolz: each creature card in your graveyard has echo equal to its mana value.
#[test]
#[ignore = "requires: echo ability derived from graveyard card's mana value (Varolz)"]
fn cr_702_1a_ex1_varolz_echo_equals_mana_value() {}

/// CR 702.1b – Volcano Hellion echo {X} where X equals its power.
#[test]
#[ignore = "requires: echo X ability keyed on power value (Volcano Hellion)"]
fn cr_702_1b_ex1_volcano_hellion_echo_x_equals_power() {}

/// CR 702.1b – Fire//Ice split card echo totals both halves.
#[test]
#[ignore = "requires: split-card echo cost (Fire//Ice)"]
fn cr_702_1b_ex2_fire_ice_echo_cost() {}

/// CR 702.1c – Concerted Effort grants keywords each upkeep.
#[test]
#[ignore = "requires: upkeep-triggered keyword-granting effect (Concerted Effort)"]
fn cr_702_1c_ex1_concerted_effort_grants_keywords_each_upkeep() {}

/// CR 702.14d – Flying + snow forestwalk: can't be blocked by snow Forest controller even with own snow forestwalk.
#[test]
#[ignore = "requires: snow-landwalk variant and mutual snow-landwalk non-cancellation"]
fn cr_702_14d_ex1_snow_forestwalk_doesnt_cancel_between_players() {}

/// CR 702.15e – Ajani's Pridemate gains +1/+1 on lifelink damage.
#[test]
#[ignore = "requires: lifelink life-gain triggered ability (Ajani's Pridemate)"]
fn cr_702_15e_ex1_ajanis_pridemate_lifelink_trigger() {}

/// CR 702.19b – 2/2 creature blocks two attackers (can block an additional creature).
#[test]
#[ignore = "requires: can-block-additional-creature ability and multi-blocking assignment"]
fn cr_702_19b_ex1_can_block_additional_creature_blocks_two() {}

/// CR 702.19b – 6/6 trample blocked by 2/2 with protection from green: still gets trampled?
#[test]
#[ignore = "requires: trample+protection interaction (protection prevents damage but trample assigns through)"]
fn cr_702_19b_ex2_trample_blocker_protection_from_green() {}

/// CR 702.19c – 3/3 trample on planeswalker with 3 loyalty counters: all combat damage to planeswalker.
#[test]
#[ignore = "requires: planeswalker combat damage assignment and trample interaction"]
fn cr_702_19c_ex1_trample_against_planeswalker() {}

/// CR 702.22h – Band with flying + ground creature: flying restriction applies to whole band.
#[test]
#[ignore = "requires: banding mechanic with evasion interaction"]
fn cr_702_22h_ex1_banding_with_flying_evasion() {}

/// CR 702.24a – Cumulative upkeep {W} or {U} with two age counters: pay 3 mana total.
#[test]
#[ignore = "requires: cumulative-upkeep mechanic with alternative-cost option"]
fn cr_702_24a_ex1_cumulative_upkeep_or_choice() {}

/// CR 702.24a – Cumulative upkeep "sacrifice a creature" with one age counter.
#[test]
#[ignore = "requires: cumulative-upkeep with sacrifice cost"]
fn cr_702_24a_ex2_cumulative_upkeep_sacrifice_creature() {}

/// CR 702.24b – Two instances of "cumulative upkeep—Pay 1 life": pay twice as many counters' life.
#[test]
#[ignore = "requires: multiple cumulative-upkeep instances stacking (CR 702.24b)"]
fn cr_702_24b_ex1_two_cumulative_upkeep_instances_stack() {}

/// CR 702.26b – You control three creatures (one phased out): "cast a spell for each creature" sees two.
#[test]
#[ignore = "requires: phasing mechanic (phased-out permanents don't exist for counting)"]
fn cr_702_26b_ex1_phased_out_creature_not_counted() {}

/// CR 702.26b – "Destroy all creatures you control": phased-out creature survives.
#[test]
#[ignore = "requires: phasing mechanic (phased-out permanents not affected by destroy-all)"]
fn cr_702_26b_ex2_phased_out_creature_survives_destroy_all() {}

/// CR 702.44c – Modular—Sunburst: enters with +1/+1 counters per color spent.
#[test]
#[ignore = "requires: sunburst mechanic and modular ability stacking"]
fn cr_702_44c_ex1_modular_sunburst_enters_with_counters_per_color() {}

/// CR 702.47a – Splice onto Arcane: the spliced card stays in hand after use.
#[test]
#[ignore = "requires: splice-onto-Arcane mechanic"]
fn cr_702_47a_ex1_splice_card_stays_in_hand() {}

/// CR 702.47c – Glacial Ray (splice onto Arcane): adds its text to Kodama's Reach.
#[test]
#[ignore = "requires: splice-onto-Arcane mechanic with text merging"]
fn cr_702_47c_ex1_glacial_ray_splice_adds_damage() {}

/// CR 702.51b – Heartless Summoning reduces creature spells by {2}.
#[test]
#[ignore = "requires: cost-reduction continuous effect (Heartless Summoning)"]
fn cr_702_51b_ex1_heartless_summoning_cost_reduction() {}

/// CR 702.65b – Aura swap: only Aura cards in graveyard; only Aura in hand.
#[test]
#[ignore = "requires: aura-swap mechanic (swap enchanting Aura with one from hand)"]
fn cr_702_65b_ex1_aura_swap_no_legal_target_in_graveyard() {}

/// CR 702.65b – Aura swap: you control but don't own the Aura; swapped to opponent's graveyard.
#[test]
#[ignore = "requires: aura-swap ownership vs control distinction"]
fn cr_702_65b_ex2_aura_swap_controller_doesnt_own() {}

/// CR 702.103d – Aether Storm: creature spells can't be cast; pay 4 life to sacrifice it.
#[test]
#[ignore = "requires: cast-restriction static effect and life-to-remove (Aether Storm)"]
fn cr_702_103d_ex1_aether_storm_creature_spells_cant_be_cast() {}

/// CR 702.103d – Garruk's Horde: may cast creature from top of library.
#[test]
#[ignore = "requires: cast-from-top-of-library permission with ravenous-related creature"]
fn cr_702_103d_ex2_garruks_horde_cast_from_top() {}

/// CR 702.164b – Toxic 2 + Toxic 1 = total Toxic 3 (sum of all instances).
#[test]
fn cr_702_164b_ex1_toxic_stacks_additively() {
    // CR 702.164b: if a creature has multiple instances of toxic, use the sum of all N values.
    let def = {
        let mut d = make_creature_def(1, 1);
        d.rules_text = vec![
            RulesText::Active(Rule::Static(StaticAbility::ToxicN(2))),
            RulesText::Active(Rule::Static(StaticAbility::ToxicN(1))),
        ];
        d
    };
    let perm = PermanentState::new(&def);
    assert_eq!(perm.toxic_n(), Some(3), "Toxic 2 + Toxic 1 = total Toxic 3");
}

/// CR 702.177b – Elvish Refueler exhaust ability costs mana and is once-per-game.
#[test]
#[ignore = "requires: exhaust mechanic (once-per-game activated ability)"]
fn cr_702_177b_ex1_elvish_refueler_exhaust_once_per_game() {}

/// CR 702.184c – Tapestry Warden: each creature you control with ward gets ward on the tapestry.
#[test]
#[ignore = "requires: Tapestry Warden ability granting ward to other permanents"]
fn cr_702_184c_ex1_tapestry_warden_ward_granting() {}

// ── CR 704 ────────────────────────────────────────────────────────────────────

/// CR 704.4 – Maro's power and toughness during mid-resolution: SBAs wait until end of resolution.
#[test]
#[ignore = "requires: dynamic-power-toughness CDA (Maro) and SBA-timing during resolution"]
fn cr_704_4_ex1_maro_toughness_zero_mid_resolution_survives() {
    // Would test: during "Discard your hand, draw 7", Maro briefly hits toughness 0 but is
    // back to 7 by the time SBAs are checked.
}

/// CR 704.7 – Multiple simultaneous loss conditions replaced by Lich's Mirror.
#[test]
#[ignore = "requires: Lich's Mirror game-loss replacement effect and simultaneous SBAs"]
fn cr_704_7_ex1_lichs_mirror_replaces_simultaneous_loss() {}

/// CR 704.8 – Young Wolf with +1/+1 counter has three -1/-1 counters; undying doesn't trigger.
#[test]
#[ignore = "requires: undying mechanic and counter-interaction with SBAs"]
fn cr_704_8_ex1_young_wolf_undying_with_positive_counter() {}

// ── CR 707 ────────────────────────────────────────────────────────────────────

/// CR 707.2 – Chimeric Staff: artifact animates as creature with {X}: ability.
#[test]
#[ignore = "requires: activated type-change to creature with X-based P/T (Chimeric Staff)"]
fn cr_707_2_ex1_chimeric_staff_becomes_creature() {}

/// CR 707.2 – Clone copies face-down Grinning Demon; clone is face-up Demon.
#[test]
#[ignore = "requires: copy-effect engine and face-down permanent handling (morph)"]
fn cr_707_2_ex2_clone_copies_face_down_grinning_demon() {}

/// CR 707.3 – Vesuvan Doppelganger copies specific face.
#[test]
#[ignore = "requires: copy-effect engine with face-choice (Vesuvan Doppelganger)"]
fn cr_707_3_ex1_vesuvan_doppelganger_copy_face() {}

/// CR 707.3 – Tomoya the Revealer (flipped flip card) becomes copy of Nezumi Shortfang.
#[test]
#[ignore = "requires: flip-card support and copy-effect flipped-status preservation"]
fn cr_707_3_ex2_tomoya_copies_nezumi_shortfang() {}

/// CR 707.3 – Face-down Grinning Demon copies Wandering Eye (face-down remains).
#[test]
#[ignore = "requires: copy-effect engine and face-down status handling"]
fn cr_707_3_ex3_face_down_demon_copies_wandering_eye() {}

/// CR 707.3 – Face-down Grinning Demon copies Wall of Rats (face-down remains).
#[test]
#[ignore = "requires: copy-effect engine and face-down status handling"]
fn cr_707_3_ex4_face_down_demon_copies_wall_of_rats() {}

/// CR 707.4 – Unstable Shapeshifter copies whenever another creature enters.
#[test]
#[ignore = "requires: ETB-triggered copy effect (Unstable Shapeshifter)"]
fn cr_707_4_ex1_unstable_shapeshifter_copies_on_enter() {}

/// CR 707.5 – Skyshroud Behemoth fading 2: enters with 2 fade counters; copy gets them too.
#[test]
#[ignore = "requires: fading mechanic and copy-effect counter inheritance"]
fn cr_707_5_ex1_skyshroud_behemoth_fading_copy() {}

/// CR 707.5 – Clone copies Wall of Omens; ETB trigger fires for the Clone.
#[test]
#[ignore = "requires: copy-effect ETB trigger on copy (Clone of Wall of Omens draws a card)"]
fn cr_707_5_ex2_clone_copies_wall_of_omens_etb_triggers() {}

/// CR 707.6 – Clone copies Adaptive Automaton and makes its own creature-type choice.
#[test]
#[ignore = "requires: copy-effect engine with as-enters choice (Adaptive Automaton)"]
fn cr_707_6_ex1_clone_copies_adaptive_automaton_own_choice() {}

/// CR 707.8a – Afflicted Deserter (front face of DFC) as a copy token.
#[test]
#[ignore = "requires: double-faced-card support and token-copy-DFC rules"]
fn cr_707_8a_ex1_afflicted_deserter_dfc_copy_token() {}

/// CR 707.8a – Clone copies non-DFC → token is also non-DFC.
#[test]
#[ignore = "requires: copy-token DFC-status preservation"]
fn cr_707_8a_ex2_clone_not_dfc_token_also_not_dfc() {}

/// CR 707.9a – Quirion Elves enters; Unstable Shapeshifter copies before ETB ability.
#[test]
#[ignore = "requires: copy-effect timing with ETB triggered abilities"]
fn cr_707_9a_ex1_quirion_elves_shapeshifter_copy_timing() {}

/// CR 707.9b – Copy Artifact enchants as an enchantment copy.
#[test]
#[ignore = "requires: copy-as-enchantment ability (Copy Artifact)"]
fn cr_707_9b_ex1_copy_artifact_enters_as_enchantment() {}

/// CR 707.9d – Quicksilver Gargantuan copies target creature with new P/T 7/7.
#[test]
#[ignore = "requires: copy-effect engine with P/T override (Quicksilver Gargantuan)"]
fn cr_707_9d_ex1_quicksilver_gargantuan_7_7_copy() {}

/// CR 707.9d – Glasspool Mimic copies target creature.
#[test]
#[ignore = "requires: copy-effect engine (Glasspool Mimic)"]
fn cr_707_9d_ex2_glasspool_mimic_copies_creature() {}

/// CR 707.9e – Altered Ego copies with additional counters.
#[test]
#[ignore = "requires: copy-effect engine with extra-counter addition (Altered Ego)"]
fn cr_707_9e_ex1_altered_ego_copy_with_counters() {}

/// CR 707.9f – Moritte of the Frost copies as a legendary snow permanent.
#[test]
#[ignore = "requires: copy-effect engine with supertype addition (Moritte of the Frost)"]
fn cr_707_9f_ex1_moritte_copies_as_legendary_snow() {}

/// CR 707.10 – Fork copies Emerald Charm; Fork is on the stack as a copy.
#[test]
#[ignore = "requires: spell-copy effect (Fork)"]
fn cr_707_10_ex1_fork_copies_emerald_charm() {}

/// CR 707.10 – Fling copied: the copy requires its own target; original has its own sacrifice.
#[test]
#[ignore = "requires: spell-copy of additional-cost spell (Fling)"]
fn cr_707_10_ex2_fling_copy_own_sacrifice() {}

/// CR 707.10 – Dawnglow Infusion copied: copy uses original's X and color check.
#[test]
#[ignore = "requires: spell-copy of X-spell with color-check (Dawnglow Infusion)"]
fn cr_707_10_ex3_dawnglow_infusion_copy() {}

/// CR 707.10e – Frontline Heroism: whenever you cast a spell, token copies of the enchanted creature.
#[test]
#[ignore = "requires: cast-triggered token-copy ability (Frontline Heroism)"]
fn cr_707_10e_ex1_frontline_heroism_cast_trigger_token_copy() {}

/// CR 707.11 – Unstable Shapeshifter copies Olivia Voldaren; gains ability that references Shapeshifter.
#[test]
#[ignore = "requires: copy-effect self-reference replacement (Unstable Shapeshifter)"]
fn cr_707_11_ex1_unstable_shapeshifter_copies_olivia_self_reference() {}

// ── CR 709 ────────────────────────────────────────────────────────────────────

/// CR 709.4b – Assault//Battery: mana cost {3}{R}{G}, red and green.
#[test]
#[ignore = "requires: split-card combined mana cost and color"]
fn cr_709_4b_ex1_assault_battery_combined_cost_color() {}

/// CR 709.4b – Fire//Ice: same mana cost as Steam Augury.
#[test]
#[ignore = "requires: split-card combined mana cost comparison"]
fn cr_709_4b_ex2_fire_ice_combined_cost_equals_steam_augury() {}

// ── CR 710 ────────────────────────────────────────────────────────────────────

/// CR 710.2 – Akki Lavarunner flips into a legendary creature.
#[test]
#[ignore = "requires: flip-card mechanic (flip transformation on condition)"]
fn cr_710_2_ex1_akki_lavarunner_flips_to_legendary() {}

// ── CR 712 ────────────────────────────────────────────────────────────────────

/// CR 712.9 – Clone copies Wildblood Pack (back face of Huntmaster DFC).
#[test]
#[ignore = "requires: double-faced-card copy rules (clone of back face)"]
fn cr_712_9_ex1_clone_copies_wildblood_pack_back_face() {}

/// CR 712.9 – Cytoshape causes Kruin Outlaw to copy another creature while transformed.
#[test]
#[ignore = "requires: double-faced-card copy interaction while transformed"]
fn cr_712_9_ex2_cytoshape_transformed_dfc_copy() {}

/// CR 712.13a – Mycosynth Lattice + March of the Machines: artifact DFCs become creatures?
#[test]
#[ignore = "requires: interaction between Mycosynth Lattice, March of the Machines, and DFCs"]
fn cr_712_13a_ex1_mycosynth_lattice_march_machines_dfc() {}

/// CR 712.18 – Village Ironsmith (front face DFC) given +2/+0.
#[test]
#[ignore = "requires: double-faced-card support (effects on front face)"]
fn cr_712_18_ex1_village_ironsmith_front_face_pt_effect() {}

/// CR 712.21 – Chittering Host (melded): dies as one object, two cards go to graveyard.
#[test]
#[ignore = "requires: meld mechanic and meld-unravel on death"]
fn cr_712_21_ex1_chittering_host_melded_dies() {}

/// CR 712.21b – Duplicant enters and exiles imprinted creature.
#[test]
#[ignore = "requires: imprint mechanic (Duplicant ETB exile)"]
fn cr_712_21b_ex1_duplicant_etb_imprint() {}

/// CR 712.21c – Otherworldly Journey exiles creature; returns with +1/+1 counter at end step.
#[test]
#[ignore = "requires: exile-and-return mechanic (Otherworldly Journey delayed return)"]
fn cr_712_21c_ex1_otherworldly_journey_exile_and_return() {}

/// CR 712.21c – False Demise Aura: when enchanted creature dies, return it.
#[test]
#[ignore = "requires: Aura triggered-ability on enchanted-creature-dies (False Demise)"]
fn cr_712_21c_ex2_false_demise_return_enchanted_creature() {}

/// CR 712.21c – Mimic Vat imprints creature that dies.
#[test]
#[ignore = "requires: Mimic Vat imprint-on-death triggered ability"]
fn cr_712_21c_ex3_mimic_vat_imprints_dying_creature() {}

/// CR 712.21d – Leyline of the Void replaces graveyard with exile.
#[test]
#[ignore = "requires: graveyard-replacement exile effect (Leyline of the Void)"]
fn cr_712_21d_ex1_leyline_of_the_void_replace_graveyard() {}

// ── CR 722 ────────────────────────────────────────────────────────────────────

/// CR 722.3c – Croaking Counterpart copies Encouraging Aviator.
#[test]
#[ignore = "requires: transforming-token-copy effect (Croaking Counterpart)"]
fn cr_722_3c_ex1_croaking_counterpart_copies_aviator() {}

// ── CR 723 ────────────────────────────────────────────────────────────────────

/// CR 723.4 – Controller of a player can see that player's hand and face-down cards.
#[test]
#[ignore = "requires: player-control mechanic (Mindslaver) and hidden-information rules"]
fn cr_723_4_ex1_controller_sees_controlled_hand() {}

/// CR 723.5 – Controller decides spells the controlled player casts.
#[test]
#[ignore = "requires: player-control mechanic (spell-casting decisions)"]
fn cr_723_5_ex1_controller_decides_spells_cast() {}

/// CR 723.5 – Controller decides which creatures the controlled player attacks with.
#[test]
#[ignore = "requires: player-control mechanic (attack decisions)"]
fn cr_723_5_ex2_controller_decides_attacks() {}

/// CR 723.5a – Controlled player paying mana: controller decides how much but must use mana source.
#[test]
#[ignore = "requires: player-control mana-payment decision (CR 723.5a)"]
fn cr_723_5a_ex1_controlled_player_mana_payment() {}

/// CR 723.5b – Controlled player may still leave the table for a restroom break.
#[test]
#[ignore = "requires: player-control mechanic (scope of what controller cannot decide)"]
fn cr_723_5b_ex1_controlled_player_still_decides_to_leave() {}

// ── CR 727 ────────────────────────────────────────────────────────────────────

/// CR 727.2 – Living Wish creature card: part of library in restarted game.
#[test]
#[ignore = "requires: restart-game mechanic and outside-the-game card tracking"]
fn cr_727_2_ex1_living_wish_card_in_restarted_game_library() {}

// ── CR 729 ────────────────────────────────────────────────────────────────────

/// CR 729.5 – Card from main game in subgame: returned to main-game library on subgame end.
#[test]
#[ignore = "requires: subgame mechanic"]
fn cr_729_5_ex1_card_from_main_game_in_subgame_returns() {}

// ── CR 732 ────────────────────────────────────────────────────────────────────

/// CR 732.2a – Presence of Gond + Intruder Alarm: shortcut to create 1,000,000 tokens.
#[test]
#[ignore = "requires: game-shortcut handling for infinite loops (CR 732)"]
fn cr_732_2a_ex1_presence_of_gond_intruder_alarm_shortcut() {}

/// CR 732.2b – "Go" shortcut: opponent says "cast a spell during beginning of combat".
#[test]
#[ignore = "requires: game-shortcut proposal and acceptance (CR 732.2b)"]
fn cr_732_2b_ex1_go_shortcut_opponent_intervenes() {}

/// CR 732.3 – Fragmented loop: active player must do something different to break it.
#[test]
#[ignore = "requires: loop-detection mechanic (CR 732.3)"]
fn cr_732_3_ex1_fragmented_loop_active_player_must_break() {}

/// CR 732.5 – Mandatory loop: player not forced to use optional non-loop-breaking ability.
#[test]
#[ignore = "requires: mandatory-loop handling and optional ability determination"]
fn cr_732_5_ex1_mandatory_loop_not_forced_to_use_optional_ability() {}

// ── CR 800 ────────────────────────────────────────────────────────────────────

/// CR 800.4a – Mind Control: if Alex leaves, enchantment leaves, creature returns.
#[test]
#[ignore = "requires: multiplayer player-leaving rules and aura-controller removal"]
fn cr_800_4a_ex1_mind_control_controller_leaves() {}

/// CR 800.4a – Act of Treason: if controller leaves, effect ends and creature reverts.
#[test]
#[ignore = "requires: multiplayer player-leaving rules and control-effect removal"]
fn cr_800_4a_ex2_act_of_treason_controller_leaves() {}

/// CR 800.4a – Bribery: Serra Angel ownership vs control on player leaving.
#[test]
#[ignore = "requires: multiplayer player-leaving rules (owner vs controller distinction)"]
fn cr_800_4a_ex3_bribery_owner_leaves_creature_exiled() {}

/// CR 800.4a – Genesis Chamber tokens: controller-dependent survival on player leaving.
#[test]
#[ignore = "requires: multiplayer player-leaving rules and token controller tracking"]
fn cr_800_4a_ex4_genesis_chamber_tokens_controller_leaves() {}

/// CR 800.4d – Astral Slide delayed trigger: if controller leaves before end step, trigger not put on stack.
#[test]
#[ignore = "requires: multiplayer player-leaving rules and delayed-trigger abandonment"]
fn cr_800_4d_ex1_astral_slide_controller_leaves_before_end_step() {}

// ── CR 801 ────────────────────────────────────────────────────────────────────

/// CR 801.2a – Range of influence 1 means only adjacent players.
#[test]
#[ignore = "requires: multiplayer range-of-influence seating mechanic"]
fn cr_801_2a_ex1_range_1_only_adjacent_players() {}

/// CR 801.2a – Range of influence 2 includes players two seats away.
#[test]
#[ignore = "requires: multiplayer range-of-influence seating mechanic"]
fn cr_801_2a_ex2_range_2_includes_two_seats_away() {}

/// CR 801.2c – Player leaving changes neighbors' range.
#[test]
#[ignore = "requires: multiplayer range-of-influence and player-leaving update"]
fn cr_801_2c_ex1_player_leaving_changes_range() {}

/// CR 801.5a – Cuombajj Witches: opponent picks target within range of both players.
#[test]
#[ignore = "requires: multiplayer range-restricted targeting (Cuombajj Witches)"]
fn cr_801_5a_ex1_cuombajj_witches_opponent_target_within_range() {}

/// CR 801.5b – Spell with opponent-chooses-mode: opponent outside range can still choose.
#[test]
#[ignore = "requires: multiplayer opponent-chooses-mode across range (CR 801.5b)"]
fn cr_801_5b_ex1_opponent_chooses_outside_range() {}

/// CR 801.5c – Emperor game: only nearest opponent separates Fact or Fiction piles.
#[test]
#[ignore = "requires: Emperor format and range-of-influence Fact or Fiction"]
fn cr_801_5c_ex1_emperor_fact_or_fiction_nearest_opponent() {}

/// CR 801.7 – Aura triggers: one fires for blocked-event, other does not (blocked creature out of range).
#[test]
#[ignore = "requires: multiplayer range-restricted event triggers on Auras"]
fn cr_801_7_ex1_aura_triggers_range_restricted() {}

/// CR 801.7a – Carissa controls creature, Alex's Extractor Demon range interaction.
#[test]
#[ignore = "requires: multiplayer range-of-influence triggered ability scope"]
fn cr_801_7a_ex1_extractor_demon_range_restricted_trigger() {}

/// CR 801.10 – Pyroclasm deals damage within range only (six-player range-1 game).
#[test]
#[ignore = "requires: multiplayer range-restricted damage spell (Pyroclasm)"]
fn cr_801_10_ex1_pyroclasm_range_1_six_player() {}

/// CR 801.11 – Coat of Arms range-1 boost considers only in-range creatures.
#[test]
#[ignore = "requires: multiplayer range-restricted static ability (Coat of Arms)"]
fn cr_801_11_ex1_coat_of_arms_range_restricted() {}

/// CR 801.11 – Rob's Coat of Arms boost includes Alex's-left neighbor (out of Rob's own range).
#[test]
#[ignore = "requires: multiplayer range-of-influence asymmetric boost"]
fn cr_801_11_ex2_coat_of_arms_asymmetric_range() {}

/// CR 801.13a – Lava Axe + Captain's Maneuver: damage redirected to out-of-range player clipped.
#[test]
#[ignore = "requires: multiplayer range-restricted damage redirection (Captain's Maneuver)"]
fn cr_801_13a_ex1_lava_axe_redirect_out_of_range() {}

/// CR 801.13b – Prevention effect in range protects against out-of-range damage source.
#[test]
#[ignore = "requires: multiplayer range-restricted prevention effect"]
fn cr_801_13b_ex1_prevention_in_range_vs_out_of_range_source() {}

/// CR 801.13b – Mending Hands from Alex prevents Carissa's Lightning Blast on Rob.
#[test]
#[ignore = "requires: multiplayer range-restricted prevention (cross-player prevention)"]
fn cr_801_13b_ex2_mending_hands_range_prevents_out_of_range_source() {}

/// CR 801.13b – Fog prevents all combat damage within Alex's range.
#[test]
#[ignore = "requires: multiplayer range-restricted combat-damage prevention (Fog)"]
fn cr_801_13b_ex3_fog_prevents_combat_damage_within_range() {}

// ── CR 802 ────────────────────────────────────────────────────────────────────

/// CR 802.2a – Rob attacks different players; mountainwalk depends only on Carissa's lands.
#[test]
#[ignore = "requires: multiplayer simultaneous attacks and per-defender landwalk check"]
fn cr_802_2a_ex1_mountainwalk_depends_on_defending_player() {}

// ── CR 807 ────────────────────────────────────────────────────────────────────

/// CR 807.4a – Grand Melee: 16 players → 4 turn markers.
#[test]
#[ignore = "requires: Grand Melee format support"]
fn cr_807_4a_ex1_grand_melee_turn_markers_sixteen_players() {}

/// CR 807.4j – Extra turn in Grand Melee deferred until next turn-marker receipt.
#[test]
#[ignore = "requires: Grand Melee format support with extra turns"]
fn cr_807_4j_ex1_grand_melee_extra_turn_deferred() {}

// ── CR 809 ────────────────────────────────────────────────────────────────────

/// CR 809.3c – Emperor can't attack any opponents at start of Emperor game.
#[test]
#[ignore = "requires: Emperor format support (attack-range restrictions)"]
fn cr_809_3c_ex1_emperor_cannot_attack_at_start() {}

/// CR 809.6a – Emperor game with two teams of four: seating order.
#[test]
#[ignore = "requires: Emperor format support and team seating arrangement"]
fn cr_809_6a_ex1_emperor_four_player_teams_seating() {}

// ── CR 810 ────────────────────────────────────────────────────────────────────

/// CR 810.8a – 2HG: Transcendence prevents team loss at 0 life.
#[test]
#[ignore = "requires: Two-Headed Giant (2HG) format support"]
fn cr_810_8a_ex1_2hg_transcendence_team_life_zero() {}

/// CR 810.8a – 2HG: player draws from empty library → team loses.
#[test]
#[ignore = "requires: Two-Headed Giant (2HG) format support"]
fn cr_810_8a_ex2_2hg_player_draws_empty_library_team_loses() {}

/// CR 810.8a – 2HG: Platinum Angel prevents team loss.
#[test]
#[ignore = "requires: Two-Headed Giant (2HG) format support"]
fn cr_810_8a_ex3_2hg_platinum_angel_team_cant_lose() {}

/// CR 810.9 – 2HG: Flame Rift deals 4 damage to each player → 8 total per team.
#[test]
#[ignore = "requires: Two-Headed Giant (2HG) format support and team-life damage pooling"]
fn cr_810_9_ex1_2hg_flame_rift_8_per_team() {}

/// CR 810.9a – 2HG: Beacon of Immortality doubles player's life total (team life adjusts).
#[test]
#[ignore = "requires: Two-Headed Giant (2HG) format support and team-life accounting"]
fn cr_810_9a_ex1_2hg_beacon_of_immortality_doubles_life() {}

/// CR 810.9a – 2HG: Test of Endurance checks team life total ≥ 50.
#[test]
#[ignore = "requires: Two-Headed Giant (2HG) format support"]
fn cr_810_9a_ex2_2hg_test_of_endurance_team_life() {}

/// CR 810.9a – 2HG: Lurking Evil halves player's life (team life adjusts).
#[test]
#[ignore = "requires: Two-Headed Giant (2HG) format support"]
fn cr_810_9a_ex3_2hg_lurking_evil_half_life() {}

/// CR 810.9c – 2HG: life-total-becomes effect works relative to player's share.
#[test]
#[ignore = "requires: Two-Headed Giant (2HG) format support"]
fn cr_810_9c_ex1_2hg_life_total_becomes_effect() {}

/// CR 810.9d – 2HG: Repay in Kind equalises team life totals.
#[test]
#[ignore = "requires: Two-Headed Giant (2HG) format support (Repay in Kind)"]
fn cr_810_9d_ex1_2hg_repay_in_kind_equalise_life() {}

// ── CR 811 ────────────────────────────────────────────────────────────────────

/// CR 811.3 – Alternating Teams game: seating order with three teams of three.
#[test]
#[ignore = "requires: Alternating Teams format support"]
fn cr_811_3_ex1_alternating_teams_seating_three_teams() {}

// ── CR 902 ────────────────────────────────────────────────────────────────────

/// CR 902.4 – Vanguard life modifier -3: starting life total is 17.
#[test]
#[ignore = "requires: Vanguard format support (life-modifier starting-life)"]
fn cr_902_4_ex1_vanguard_life_modifier_minus_3_starting_life_17() {}

/// CR 902.5a – Vanguard hand modifier +2: player starts with 9 cards.
#[test]
#[ignore = "requires: Vanguard format support (hand-modifier starting-hand-size)"]
fn cr_902_5a_ex1_vanguard_hand_modifier_plus_2_starting_hand_9() {}

/// CR 902.5b – Vanguard hand modifier -1: maximum hand size is 6.
#[test]
#[ignore = "requires: Vanguard format support (hand-modifier maximum-hand-size)"]
fn cr_902_5b_ex1_vanguard_hand_modifier_minus_1_max_hand_6() {}

// ── CR 903 ────────────────────────────────────────────────────────────────────

/// CR 903.3 – Face-down commander is still a commander.
#[test]
#[ignore = "requires: Commander format support (commander identity across face-down/copy)"]
fn cr_903_3_ex1_face_down_commander_is_still_a_commander() {}

/// CR 903.4 – Bosh, Iron Golem color identity: red (from mana symbol in rules text).
#[test]
#[ignore = "requires: Commander color-identity calculation from rules-text mana symbols"]
fn cr_903_4_ex1_bosh_iron_golem_color_identity_red() {
    // Would test: Bosh's mana cost {8} has no colored symbols, but the ability text
    // "{3}{R}" contributes Red to the color identity.
}

/// CR 903.4d – Civilized Scholar + Homicidal Brute: blue+red color identity from both faces.
#[test]
#[ignore = "requires: Commander color-identity from double-faced card both faces"]
fn cr_903_4d_ex1_civilized_scholar_dfc_color_identity_blue_red() {}

/// CR 903.5c – Wort, Raidmother color identity: red+green from {4}{R/G}{R/G} mana cost.
#[test]
#[ignore = "requires: Commander color-identity calculation including hybrid pips"]
fn cr_903_5c_ex1_wort_raidmother_color_identity_red_green() {
    // Would test: {R/G} hybrid pips contribute both Red and Green to color identity.
}

/// CR 903.5d – Wort deck: only red, green, or both-red-green cards; no Plains/Island/Swamp lands.
#[test]
#[ignore = "requires: Commander deck-legality check against color identity"]
fn cr_903_5d_ex1_wort_deck_lands_must_match_color_identity() {}
