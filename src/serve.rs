use axum::{
    Json, Router,
    extract::State,
    response::{Html, IntoResponse},
    routing::{get, post},
};
use mecha_oracle::cards::CardDatabase;
use mecha_oracle::engine::activated::{activate_ability, can_pay_cost};
use mecha_oracle::engine::casting::{cast_spell, play_land};
use mecha_oracle::engine::combat::{declare_attackers, declare_blockers};
use mecha_oracle::engine::cycling::cycle_card;
use mecha_oracle::engine::mana::{
    can_pay_mana, greedy_payment_plan, reset_mana, tap_land_for_mana,
};
use mecha_oracle::engine::stack::pass_priority;
use mecha_oracle::engine::targeting::legal_targets;
use mecha_oracle::engine::turn::{advance_step, apply_step_start, draw_card, skip_to_first_main};
use mecha_oracle::types::ability::{
    Ability, ActivatedAbility, CostComponent, OracleSpan, StaticAbility, TriggeredAbility,
};
use mecha_oracle::types::effect::{EffectStep, EffectTarget};
use mecha_oracle::types::stack::StackPayload;
use mecha_oracle::types::{CardObject, GameState, ObjectId, Player, PlayerId, Step, Zone};
use serde::{Deserialize, Serialize};
use std::sync::{Arc, Mutex};

// ── Config ──────────────────────────────────────────────────────────────────

type DeckConfig = Vec<Vec<String>>;

fn load_config(path: &str) -> Result<DeckConfig, String> {
    let text = std::fs::read_to_string(path).map_err(|e| format!("Cannot read {path}: {e}"))?;
    serde_json::from_str(&text).map_err(|e| format!("Invalid JSON in {path}: {e}"))
}

fn build_game_state(
    config: DeckConfig,
    db: &CardDatabase,
    shuffle: bool,
) -> Result<GameState, String> {
    if config.len() != 2 {
        return Err(format!(
            "Config must have exactly 2 decklists, got {}",
            config.len()
        ));
    }

    let players = vec![
        Player::new(PlayerId(0), "Player 1"),
        Player::new(PlayerId(1), "Player 2"),
    ];
    let mut gs = GameState::new(players);

    let base_seed = if shuffle {
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .subsec_nanos() as u64
    } else {
        0
    };

    for (player_idx, names) in config.iter().enumerate() {
        let pid = PlayerId(player_idx as u8);

        for name in names {
            let def = db
                .get(name)
                .ok_or_else(|| format!("Unknown card: {name:?}"))?
                .clone();
            let id = gs.alloc_id();
            let obj = CardObject::new(id, def, pid, Zone::Library);
            gs.add_object(obj);
            gs.libraries.get_mut(&pid).unwrap().push(id);
        }

        if shuffle {
            let lib = gs.libraries.get_mut(&pid).unwrap();
            let n = lib.len();
            let mut rng = base_seed.wrapping_add(player_idx as u64 * 6364136223846793005);
            for i in (1..n).rev() {
                rng = rng
                    .wrapping_mul(6364136223846793005)
                    .wrapping_add(1442695040888963407);
                let j = (rng >> 33) as usize % (i + 1);
                lib.swap(i, j);
            }
        }
    }

    for _ in 0..7 {
        for pid in [PlayerId(0), PlayerId(1)] {
            if !gs.libraries[&pid].is_empty() {
                gs = draw_card(gs, pid);
            }
        }
    }

    gs = skip_to_first_main(gs);

    Ok(gs)
}

// ── View model ──────────────────────────────────────────────────────────────

#[derive(Serialize)]
struct ManaPoolView {
    w: u32,
    u: u32,
    b: u32,
    r: u32,
    g: u32,
    c: u32,
}

#[derive(Serialize)]
#[serde(rename_all = "snake_case")]
enum SpanKind {
    Parsed,
    Ignored,
    Unparsed,
    ParsedUnimplemented,
}

#[derive(Serialize)]
struct OracleSpanView {
    kind: SpanKind,
    text: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    ignored_kind: Option<mecha_oracle::types::IgnoredKind>,
}

#[derive(Serialize)]
struct StackItemView {
    id: u64,
    kind: String,
    label: String,
    controller: PlayerId,
    card: Option<CardView>,
}

#[derive(Serialize)]
struct ActionItemView {
    label: String,
    can_pay_cost: bool,
    #[serde(flatten)]
    kind: ActionItemKind,
}

#[derive(Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
enum ActionItemKind {
    /// Pre-built JSON payload posted verbatim to /action
    Server { action: serde_json::Value },
    /// Toggle this creature in/out of the client-side attacker-staging list
    ToggleAttacker { object_id: u64 },
    /// Assign this creature as a blocker for the given attacker (client-side staging)
    AssignBlocker { blocker_id: u64, attacker_id: u64 },
}

#[derive(Serialize)]
struct CardView {
    id: ObjectId,
    name: String,
    type_line: String,
    oracle_text: Vec<OracleSpanView>,
    mana_cost: Option<String>,
    power: Option<i32>,
    toughness: Option<i32>,
    tapped: bool,
    summoning_sick: bool,
    damage_marked: u32,
    is_attacking: bool,
    is_blocking: bool,
    actions: Vec<ActionItemView>,
}

#[derive(Serialize)]
struct PlayerView {
    life: i32,
    mana_pool: ManaPoolView,
    hand: Vec<CardView>,
    lands: Vec<CardView>,
    creatures: Vec<CardView>,
    library_count: usize,
    graveyard: Vec<CardView>,
}

#[derive(Serialize)]
struct GameView {
    turn: u32,
    step: String,
    active_player: PlayerId,
    priority_player: PlayerId,
    lands_played_this_turn: u32,
    game_over: bool,
    winner: Option<PlayerId>,
    p1: PlayerView,
    p2: PlayerView,
    can_reset_mana: bool,
    attackers_declared: bool,
    blockers_declared: bool,
    stack: Vec<StackItemView>,
    consecutive_passes: u32,
}

fn format_mana_cost(cost: &mecha_oracle::types::mana::ManaCost) -> String {
    use mecha_oracle::types::mana::ManaPip;
    cost.pips
        .iter()
        .map(|pip| match pip {
            ManaPip::White => "W".to_string(),
            ManaPip::Blue => "U".to_string(),
            ManaPip::Black => "B".to_string(),
            ManaPip::Red => "R".to_string(),
            ManaPip::Green => "G".to_string(),
            ManaPip::Colorless => "C".to_string(),
            ManaPip::Generic(n) => n.to_string(),
            ManaPip::X => "X".to_string(),
            ManaPip::Hybrid(c1, c2) => format!("{c1}/{c2}"),
            ManaPip::GenericHybrid(n, c) => format!("{n}/{c}"),
            ManaPip::ColorlessHybrid(c) => format!("C/{c}"),
            ManaPip::Phyrexian(c) => format!("{c}/P"),
            ManaPip::HybridPhyrexian(c1, c2) => format!("{c1}/{c2}/P"),
            ManaPip::Snow => "S".to_string(),
        })
        .collect::<Vec<_>>()
        .join("")
}

fn format_type_line(tl: &mecha_oracle::types::card::TypeLine) -> String {
    use mecha_oracle::types::card::{CardType, Supertype};
    let mut parts: Vec<&str> = Vec::new();
    for st in &tl.supertypes {
        parts.push(match st {
            Supertype::Basic => "Basic",
            Supertype::Legendary => "Legendary",
            Supertype::Snow => "Snow",
            Supertype::World => "World",
        });
    }
    for ct in &tl.card_types {
        parts.push(match ct {
            CardType::Creature => "Creature",
            CardType::Land => "Land",
            CardType::Instant => "Instant",
            CardType::Sorcery => "Sorcery",
            CardType::Artifact => "Artifact",
            CardType::Enchantment => "Enchantment",
            CardType::Planeswalker => "Planeswalker",
        });
    }
    let main = parts.join(" ");
    if tl.subtypes.is_empty() {
        main
    } else {
        format!("{} — {}", main, tl.subtypes.join(" "))
    }
}

fn format_mana_cost_braced(cost: &mecha_oracle::types::mana::ManaCost) -> String {
    use mecha_oracle::types::mana::ManaPip;
    cost.pips
        .iter()
        .map(|pip| match pip {
            ManaPip::White => "{W}".to_string(),
            ManaPip::Blue => "{U}".to_string(),
            ManaPip::Black => "{B}".to_string(),
            ManaPip::Red => "{R}".to_string(),
            ManaPip::Green => "{G}".to_string(),
            ManaPip::Colorless => "{C}".to_string(),
            ManaPip::Generic(n) => format!("{{{n}}}"),
            ManaPip::X => "{X}".to_string(),
            ManaPip::Hybrid(c1, c2) => format!("{{{c1}/{c2}}}"),
            ManaPip::GenericHybrid(n, c) => format!("{{{n}/{c}}}"),
            ManaPip::ColorlessHybrid(c) => format!("{{C/{c}}}"),
            ManaPip::Phyrexian(c) => format!("{{{c}/P}}"),
            ManaPip::HybridPhyrexian(c1, c2) => format!("{{{c1}/{c2}/P}}"),
            ManaPip::Snow => "{S}".to_string(),
        })
        .collect::<String>()
}

fn format_mana_pool(pool: &mecha_oracle::types::mana::ManaPool) -> String {
    let mut s = String::new();
    for _ in 0..pool.white {
        s.push_str("{W}");
    }
    for _ in 0..pool.blue {
        s.push_str("{U}");
    }
    for _ in 0..pool.black {
        s.push_str("{B}");
    }
    for _ in 0..pool.red {
        s.push_str("{R}");
    }
    for _ in 0..pool.green {
        s.push_str("{G}");
    }
    for _ in 0..pool.colorless {
        s.push_str("{C}");
    }
    s
}

fn format_activated_ability(ability: &ActivatedAbility) -> String {
    let cost_parts: Vec<String> = ability
        .cost
        .iter()
        .map(|c| match c {
            CostComponent::Tap => "{T}".to_string(),
            CostComponent::Mana(m) => format_mana_cost_braced(m),
            CostComponent::PayLife(n) => format!("Pay {n} life"),
            CostComponent::Sacrifice(n, _) => format!("Sacrifice {n}"),
            CostComponent::Discard(n, _) => format!("Discard {n}"),
            CostComponent::Unimplemented(s) => s.clone(),
        })
        .collect();
    let effect_parts: Vec<String> = ability
        .effect
        .iter()
        .map(|e| match e {
            EffectStep::AddMana(pool) => format!("Add {}", format_mana_pool(pool)),
            EffectStep::Mill(n) => format!("Mill {n}"),
            EffectStep::DrawCard(n) => {
                if *n == 1 {
                    "Draw a card".to_string()
                } else {
                    format!("Draw {n} cards")
                }
            }
            EffectStep::GainLife(n) => format!("Gain {n} life"),
            EffectStep::BoostPermanentPT(delta) => {
                format!("Boost by {}/{}", delta.power, delta.toughness)
            }
            EffectStep::DealDamage(n) => format!("Deal {n} damage"),
            EffectStep::Unimplemented(s) => s.clone(),
        })
        .collect();
    format!("{}: {}", cost_parts.join(", "), effect_parts.join(". "))
}

fn format_spell_effect(effect: &[EffectStep]) -> String {
    effect
        .iter()
        .map(|step| match step {
            EffectStep::DrawCard(1) => "Draw a card".to_string(),
            EffectStep::DrawCard(n) => format!("Draw {n} cards"),
            EffectStep::GainLife(n) => format!("Gain {n} life"),
            EffectStep::Mill(n) => format!("Mill {n}"),
            EffectStep::AddMana(pool) => format!("Add {}", format_mana_pool(pool)),
            EffectStep::BoostPermanentPT(delta) => {
                format!("Boost by {}/{}", delta.power, delta.toughness)
            }
            EffectStep::DealDamage(n) => format!("Deal {n} damage"),
            EffectStep::Unimplemented(s) => s.clone(),
        })
        .collect::<Vec<_>>()
        .join(", then ")
}

fn format_triggered_ability(t: &TriggeredAbility) -> String {
    use mecha_oracle::types::ability::TriggerEvent;
    let trigger_str = match &t.trigger {
        TriggerEvent::EntersTheBattlefield { .. } => "When this enters",
    };
    let effect_parts: Vec<String> = t
        .effect
        .iter()
        .map(|e| match e {
            EffectStep::DrawCard(1) => "draw a card".to_string(),
            EffectStep::DrawCard(n) => format!("draw {n} cards"),
            EffectStep::GainLife(n) => format!("you gain {n} life"),
            EffectStep::AddMana(pool) => format!("add {}", format_mana_pool(pool)),
            EffectStep::Mill(n) => format!("mill {n}"),
            EffectStep::BoostPermanentPT(delta) => {
                format!("boost by {}/{}", delta.power, delta.toughness)
            }
            EffectStep::DealDamage(n) => format!("deal {n} damage"),
            EffectStep::Unimplemented(s) => s.to_string(),
        })
        .collect();
    format!("{}, {}.", trigger_str, effect_parts.join(". "))
}

fn can_cast_structural(state: &GameState, pid: PlayerId, obj: &CardObject) -> bool {
    use mecha_oracle::types::card::CardType;
    if obj.zone != Zone::Hand {
        return false;
    }
    if state.priority_player != pid {
        return false;
    }
    if obj.definition.mana_cost.is_none() {
        return false;
    }
    let is_instant_speed = obj
        .definition
        .type_line
        .card_types
        .contains(&CardType::Instant)
        || obj.has_keyword(StaticAbility::Flash);
    if is_instant_speed {
        return true;
    }
    state.active_player == pid
        && matches!(state.step(), Step::PreCombatMain | Step::PostCombatMain)
        && state.stack.is_empty()
}

fn compute_actions(state: &GameState, pid: PlayerId, obj: &CardObject) -> Vec<ActionItemView> {
    match obj.zone {
        Zone::Hand => compute_hand_actions(state, pid, obj),
        Zone::Battlefield => compute_battlefield_actions(state, pid, obj),
        _ => vec![],
    }
}

fn compute_hand_actions(state: &GameState, pid: PlayerId, obj: &CardObject) -> Vec<ActionItemView> {
    let mut actions = Vec::new();

    // Play land (no mana cost — always can_pay_cost: true when structurally valid)
    if obj.definition.type_line.is_land() {
        let can_play = state.active_player == pid
            && state.priority_player == pid
            && state.lands_played_this_turn == 0
            && matches!(state.step(), Step::PreCombatMain | Step::PostCombatMain)
            && state.stack.is_empty();
        if can_play {
            actions.push(ActionItemView {
                label: "Play land".to_string(),
                can_pay_cost: true,
                kind: ActionItemKind::Server {
                    action: serde_json::json!({
                        "type": "play_land",
                        "object_id": obj.id.0
                    }),
                },
            });
        }
        return actions;
    }

    // Cast spell
    if let Some(cost) = &obj.definition.mana_cost
        && can_cast_structural(state, pid, obj)
    {
        let player = state.get_player(pid).unwrap();
        let mana_ok = greedy_payment_plan(cost, &player.mana_pool, player.life).is_some();

        // Collect target requirements from all SpellEffect abilities
        let target_filters: Vec<_> = obj
            .definition
            .abilities
            .iter()
            .filter_map(|span| match span {
                OracleSpan::Parsed(Ability::SpellEffect(sa))
                    if !sa.target_requirements.is_empty() =>
                {
                    Some(sa.target_requirements.as_slice())
                }
                _ => None,
            })
            .flatten()
            .copied()
            .collect();

        if target_filters.is_empty() {
            // Untargeted spell
            actions.push(ActionItemView {
                label: format!("Cast {}", obj.definition.name),
                can_pay_cost: mana_ok,
                kind: ActionItemKind::Server {
                    action: serde_json::json!({
                        "type": "cast_spell",
                        "object_id": obj.id.0
                    }),
                },
            });
        } else {
            // Targeted spell: one action per legal target
            let mut seen = std::collections::HashSet::new();
            for filter in &target_filters {
                for target in legal_targets(state, *filter, pid) {
                    let key = match &target {
                        EffectTarget::Object { id } => format!("o{}", id.0),
                        EffectTarget::Player { id } => format!("p{}", id.0),
                    };
                    if !seen.insert(key) {
                        continue;
                    }
                    let target_name = match &target {
                        EffectTarget::Object { id } => state
                            .objects
                            .get(id)
                            .map(|o| o.definition.name.clone())
                            .unwrap_or_default(),
                        EffectTarget::Player { id } => state
                            .get_player(*id)
                            .map(|p| p.name.clone())
                            .unwrap_or_default(),
                    };
                    let target_val = serde_json::to_value(&target).unwrap();
                    actions.push(ActionItemView {
                        label: format!("Cast {} → {}", obj.definition.name, target_name),
                        can_pay_cost: mana_ok,
                        kind: ActionItemKind::Server {
                            action: serde_json::json!({
                                "type": "cast_spell",
                                "object_id": obj.id.0,
                                "targets": [target_val]
                            }),
                        },
                    });
                }
            }
            // If no legal targets were found, no action is emitted (structural failure).
        }
    }

    // Cycling
    for span in &obj.definition.abilities {
        if let OracleSpan::Parsed(Ability::Cycling(cost)) = span
            && state.priority_player == pid
        {
            let player = state.get_player(pid).unwrap();
            let mana_ok = can_pay_mana(cost, &player.mana_pool, player.life);
            actions.push(ActionItemView {
                label: format!("Cycle ({})", format_mana_cost(cost)),
                can_pay_cost: mana_ok,
                kind: ActionItemKind::Server {
                    action: serde_json::json!({
                        "type": "cycle_card",
                        "object_id": obj.id.0
                    }),
                },
            });
        }
    }

    actions
}

fn compute_battlefield_actions(
    state: &GameState,
    pid: PlayerId,
    obj: &CardObject,
) -> Vec<ActionItemView> {
    let mut actions = Vec::new();

    // Attacker toggle (no cost — can_pay_cost always true)
    if state.step() == Step::DeclareAttackers && pid == state.active_player {
        let cmt = state.controllers_most_recent_turn(pid);
        let can_atk = state
            .battlefield
            .get(&obj.id)
            .map(|p| p.can_attack(cmt))
            .unwrap_or(false);
        if can_atk {
            actions.push(ActionItemView {
                label: "Declare as attacker".to_string(),
                can_pay_cost: true,
                kind: ActionItemKind::ToggleAttacker {
                    object_id: obj.id.0,
                },
            });
        }
    }

    // Blocker assignment (no cost — can_pay_cost always true)
    if state.step() == Step::DeclareBlockers && pid != state.active_player {
        let can_blk = state
            .battlefield
            .get(&obj.id)
            .map(|p| p.can_block())
            .unwrap_or(false);
        if can_blk {
            for &atk_id in &state.combat.attackers {
                let atk_name = state
                    .objects
                    .get(&atk_id)
                    .map(|o| o.definition.name.as_str())
                    .unwrap_or("Unknown");
                actions.push(ActionItemView {
                    label: format!("Block {atk_name}"),
                    can_pay_cost: true,
                    kind: ActionItemKind::AssignBlocker {
                        blocker_id: obj.id.0,
                        attacker_id: atk_id.0,
                    },
                });
            }
        }
    }

    // Activated abilities
    // CR 117.1b: non-mana activated abilities require priority; mana abilities do not.
    let abilities: Vec<_> = obj
        .definition
        .abilities
        .iter()
        .filter_map(|span| match span {
            OracleSpan::Parsed(Ability::Activated(a)) => Some(a),
            _ => None,
        })
        .enumerate()
        .collect();

    for (i, ability) in &abilities {
        let produces_mana = ability
            .effect
            .iter()
            .any(|e| matches!(e, EffectStep::AddMana(_)));
        // Mana abilities don't need priority; non-mana abilities do (CR 117.1b)
        let structural_ok = produces_mana || state.priority_player == pid;
        if structural_ok {
            let cost_ok = can_pay_cost(state, obj.id, ability, pid);
            actions.push(ActionItemView {
                label: format_activated_ability(ability),
                can_pay_cost: cost_ok,
                kind: ActionItemKind::Server {
                    action: serde_json::json!({
                        "type": "activate_ability",
                        "object_id": obj.id.0,
                        "ability_index": i
                    }),
                },
            });
        }
    }

    actions
}

fn build_player_view(state: &GameState, pid: PlayerId) -> PlayerView {
    use mecha_oracle::types::ObjectId;
    use std::collections::HashSet;
    let player = state.get_player(pid).unwrap();
    let all_blockers: HashSet<ObjectId> = state
        .combat
        .blocking_map
        .values()
        .flatten()
        .copied()
        .collect();

    let to_card_view = |obj: &mecha_oracle::types::CardObject| {
        let perm = state.battlefield.get(&obj.id);
        CardView {
            id: obj.id,
            name: obj.definition.name.clone(),
            type_line: format_type_line(&obj.definition.type_line),
            oracle_text: {
                obj.definition
                    .abilities
                    .iter()
                    .map(|span| match span {
                        OracleSpan::Parsed(Ability::Static(kw)) => OracleSpanView {
                            kind: SpanKind::Parsed,
                            text: kw.display_name(),
                            ignored_kind: None,
                        },
                        OracleSpan::Parsed(Ability::Activated(a)) => OracleSpanView {
                            kind: SpanKind::Parsed,
                            text: format_activated_ability(a),
                            ignored_kind: None,
                        },
                        OracleSpan::Parsed(Ability::Triggered(t)) => OracleSpanView {
                            kind: SpanKind::Parsed,
                            text: format_triggered_ability(t),
                            ignored_kind: None,
                        },
                        OracleSpan::Parsed(Ability::SpellEffect(spell_ability)) => OracleSpanView {
                            kind: SpanKind::Parsed,
                            text: format_spell_effect(&spell_ability.steps),
                            ignored_kind: None,
                        },
                        OracleSpan::Parsed(Ability::Cycling(cost)) => OracleSpanView {
                            kind: SpanKind::Parsed,
                            text: format!("Cycling {}", format_mana_cost(cost)),
                            ignored_kind: None,
                        },
                        OracleSpan::Ignored(kind, t) => OracleSpanView {
                            kind: SpanKind::Ignored,
                            text: t.clone(),
                            ignored_kind: Some(kind.clone()),
                        },
                        OracleSpan::ParsedUnimplemented(t) => OracleSpanView {
                            kind: SpanKind::ParsedUnimplemented,
                            text: t.clone(),
                            ignored_kind: None,
                        },
                        OracleSpan::Unparsed(t) => OracleSpanView {
                            kind: SpanKind::Unparsed,
                            text: t.clone(),
                            ignored_kind: None,
                        },
                    })
                    .collect()
            },
            mana_cost: obj.definition.mana_cost.as_ref().map(format_mana_cost),
            power: perm.and_then(|p| p.effective_power()),
            toughness: perm.and_then(|p| p.effective_toughness()),
            tapped: perm.map(|p| p.tapped).unwrap_or(false),
            summoning_sick: perm
                .map(|p| p.summoning_sick(state.controllers_most_recent_turn(pid)))
                .unwrap_or(false),
            damage_marked: perm.map(|p| p.damage_marked).unwrap_or(0),
            is_attacking: state.combat.attackers.contains(&obj.id),
            is_blocking: all_blockers.contains(&obj.id),
            actions: compute_actions(state, pid, obj),
        }
    };

    let bf_objects: Vec<_> = state
        .battlefield
        .keys()
        .filter_map(|id| state.objects.get(id))
        .filter(|obj| obj.controller == pid)
        .collect();

    PlayerView {
        life: player.life,
        mana_pool: ManaPoolView {
            w: player.mana_pool.white,
            u: player.mana_pool.blue,
            b: player.mana_pool.black,
            r: player.mana_pool.red,
            g: player.mana_pool.green,
            c: player.mana_pool.colorless,
        },
        hand: state.hands[&pid]
            .iter()
            .filter_map(|id| state.objects.get(id))
            .map(to_card_view)
            .collect(),
        lands: bf_objects
            .iter()
            .filter(|obj| obj.is_land() && !obj.is_creature())
            .map(|obj| to_card_view(obj))
            .collect(),
        creatures: bf_objects
            .iter()
            .filter(|obj| obj.is_creature())
            .map(|obj| to_card_view(obj))
            .collect(),
        library_count: state.libraries[&pid].len(),
        graveyard: state.graveyards[&pid]
            .iter()
            .filter_map(|id| state.objects.get(id))
            .map(to_card_view)
            .collect(),
    }
}

fn build_game_view(state: &GameState) -> GameView {
    let stack: Vec<StackItemView> = state
        .stack
        .iter()
        .map(|&sid| {
            let obj = &state.stack_objects[&sid];
            match &obj.payload {
                StackPayload::Spell { card_id } => {
                    let card = state.objects.get(card_id);
                    StackItemView {
                        id: sid.0,
                        kind: "spell".into(),
                        label: card.map(|c| c.definition.name.clone()).unwrap_or_default(),
                        controller: obj.controller,
                        card: card.map(|c| CardView {
                            id: c.id,
                            name: c.definition.name.clone(),
                            type_line: format_type_line(&c.definition.type_line),
                            oracle_text: vec![],
                            mana_cost: c.definition.mana_cost.as_ref().map(format_mana_cost),
                            power: c.definition.power,
                            toughness: c.definition.toughness,
                            tapped: false,
                            summoning_sick: false,
                            damage_marked: 0,
                            is_attacking: false,
                            is_blocking: false,
                            actions: vec![],
                        }),
                    }
                }
                StackPayload::TriggeredAbility { label, .. } => StackItemView {
                    id: sid.0,
                    kind: "triggered_ability".into(),
                    label: label.clone(),
                    controller: obj.controller,
                    card: None,
                },
                StackPayload::ActivatedAbility { label, .. } => StackItemView {
                    id: sid.0,
                    kind: "activated_ability".into(),
                    label: label.clone(),
                    controller: obj.controller,
                    card: None,
                },
            }
        })
        .collect();

    GameView {
        turn: state.turn_number,
        step: format!("{:?}", state.step()),
        active_player: state.active_player,
        priority_player: state.priority_player,
        lands_played_this_turn: state.lands_played_this_turn,
        game_over: state.is_game_over(),
        winner: state.winner(),
        p1: build_player_view(state, PlayerId(0)),
        p2: build_player_view(state, PlayerId(1)),
        can_reset_mana: state.mana_checkpoint.is_some(),
        attackers_declared: state.combat.attackers_declared,
        blockers_declared: state.combat.blockers_declared,
        stack,
        consecutive_passes: state.consecutive_passes,
    }
}

// ── Actions ──────────────────────────────────────────────────────────────────

#[derive(Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
enum ActionRequest {
    TapLand {
        object_id: u64,
    },
    PlayLand {
        object_id: u64,
    },
    CastSpell {
        object_id: u64,
        #[serde(default)]
        targets: Vec<mecha_oracle::types::effect::EffectTarget>,
    },
    DeclareAttackers {
        attacker_ids: Vec<u64>,
    },
    DeclareBlockers {
        blocks: Vec<[u64; 2]>,
    },
    AdvanceStep,
    PassPriority {
        player_id: u8,
    },
    ResetMana,
    ActivateAbility {
        object_id: u64,
        ability_index: usize,
        #[serde(default)]
        x_value: Option<u32>,
        #[serde(default)]
        payment_plan: Option<mecha_oracle::types::mana::PaymentPlan>,
        #[serde(default)]
        targets: Vec<mecha_oracle::types::effect::EffectTarget>,
    },
    CycleCard {
        object_id: u64,
    },
}

#[derive(Serialize)]
struct ActionResponse {
    ok: bool,
    state: GameView,
    #[serde(skip_serializing_if = "Option::is_none")]
    error: Option<String>,
}

// After pass_priority has already advanced the step, apply step-start actions and
// auto-advance through Untap/Cleanup steps (CR 502, 514).
fn apply_step_start_loop(mut state: GameState) -> GameState {
    loop {
        state = apply_step_start(state);
        if !matches!(state.step(), Step::Untap | Step::Cleanup) || state.is_game_over() {
            break;
        }
        state = advance_step(state);
    }
    state
}

fn dispatch_action(state: GameState, action: ActionRequest) -> Result<GameState, String> {
    match action {
        ActionRequest::TapLand { object_id } => {
            tap_land_for_mana(state, ObjectId(object_id)).map_err(|e| format!("{e:?}"))
        }
        ActionRequest::PlayLand { object_id } => {
            let player = state.priority_player;
            play_land(state, player, ObjectId(object_id)).map_err(|e| format!("{e:?}"))
        }
        ActionRequest::CastSpell { object_id, targets } => {
            let player = state.priority_player;
            cast_spell(state, player, ObjectId(object_id), targets).map_err(|e| format!("{e:?}"))
        }
        ActionRequest::DeclareAttackers { attacker_ids } => {
            let ids: Vec<ObjectId> = attacker_ids.iter().map(|&id| ObjectId(id)).collect();
            let active = state.active_player;
            declare_attackers(state, active, &ids).map_err(|e| format!("{e:?}"))
        }
        ActionRequest::DeclareBlockers { blocks } => {
            let pairs: Vec<(ObjectId, ObjectId)> = blocks
                .iter()
                .map(|[b, a]| (ObjectId(*b), ObjectId(*a)))
                .collect();
            let defender = state.opponent_of(state.active_player);
            declare_blockers(state, defender, &pairs).map_err(|e| format!("{e:?}"))
        }
        ActionRequest::AdvanceStep => {
            if state.is_game_over() {
                return Err("game is over".to_string());
            }
            if state.step() == Step::DeclareAttackers && !state.combat.attackers_declared {
                return Err("must declare attackers before passing priority".to_string());
            }
            if state.step() == Step::DeclareBlockers && !state.combat.blockers_declared {
                return Err("must declare blockers before passing priority".to_string());
            }
            let before = state.step();
            let player = state.priority_player;
            pass_priority(state, player)
                .map(|s| {
                    if s.step() != before {
                        apply_step_start_loop(s)
                    } else {
                        s
                    }
                })
                .map_err(|e| format!("{e:?}"))
        }
        ActionRequest::PassPriority { player_id } => {
            if state.step() == Step::DeclareAttackers && !state.combat.attackers_declared {
                return Err("must declare attackers before passing priority".to_string());
            }
            if state.step() == Step::DeclareBlockers && !state.combat.blockers_declared {
                return Err("must declare blockers before passing priority".to_string());
            }
            let before = state.step();
            pass_priority(state, PlayerId(player_id))
                .map(|s| {
                    if s.step() != before {
                        apply_step_start_loop(s)
                    } else {
                        s
                    }
                })
                .map_err(|e| format!("{e:?}"))
        }
        ActionRequest::ResetMana => reset_mana(state).map_err(|e| format!("{e:?}")),
        ActionRequest::ActivateAbility {
            object_id,
            ability_index,
            x_value,
            payment_plan,
            targets,
        } => {
            let player = state.priority_player;
            activate_ability(
                state,
                ObjectId(object_id),
                ability_index,
                player,
                x_value,
                payment_plan,
                targets,
            )
            .map_err(|e| format!("{e:?}"))
        }
        ActionRequest::CycleCard { object_id } => {
            let player = state.priority_player;
            cycle_card(state, ObjectId(object_id), player, None).map_err(|e| format!("{e:?}"))
        }
    }
}

// ── Game init ────────────────────────────────────────────────────────────────

fn init_game(path: &str, shuffle: bool) -> Result<GameState, String> {
    let db = CardDatabase::open().map_err(|e| format!("Card database error: {e}"))?;
    let config = load_config(path)?;
    build_game_state(config, &db, shuffle)
}

const INDEX_HTML: &str = include_str!("serve.html");
const STYLE_CSS: &str = include_str!("serve.css");
const APP_JS: &str = include_str!("serve.js");

// ── App state ────────────────────────────────────────────────────────────────

#[derive(Clone)]
struct AppState {
    game: Arc<Mutex<GameState>>,
}

// ── Handlers ─────────────────────────────────────────────────────────────────

async fn index_handler() -> impl IntoResponse {
    Html(INDEX_HTML)
}

async fn css_handler() -> impl IntoResponse {
    ([(axum::http::header::CONTENT_TYPE, "text/css")], STYLE_CSS)
}

async fn js_handler() -> impl IntoResponse {
    (
        [(axum::http::header::CONTENT_TYPE, "application/javascript")],
        APP_JS,
    )
}

async fn state_handler(State(app): State<AppState>) -> Json<GameView> {
    let gs = app.game.lock().unwrap();
    Json(build_game_view(&gs))
}

async fn action_handler(
    State(app): State<AppState>,
    Json(req): Json<ActionRequest>,
) -> Json<ActionResponse> {
    let mut gs = app.game.lock().unwrap();
    let current = gs.clone();
    match dispatch_action(current, req) {
        Ok(new_state) => {
            *gs = new_state;
            Json(ActionResponse {
                ok: true,
                state: build_game_view(&gs),
                error: None,
            })
        }
        Err(e) => Json(ActionResponse {
            ok: false,
            state: build_game_view(&gs),
            error: Some(e),
        }),
    }
}

// ── Entry point ───────────────────────────────────────────────────────────────

pub async fn run(shuffle: bool, deck_path: &str) {
    let gs = init_game(deck_path, shuffle).unwrap_or_else(|e| {
        eprintln!("Error: {e}");
        std::process::exit(1);
    });

    let app_state = AppState {
        game: Arc::new(Mutex::new(gs)),
    };

    let router = Router::new()
        .route("/", get(index_handler))
        .route("/static/app.css", get(css_handler))
        .route("/static/app.js", get(js_handler))
        .route("/state", get(state_handler))
        .route("/action", post(action_handler))
        .with_state(app_state);

    let listener = tokio::net::TcpListener::bind("0.0.0.0:3000")
        .await
        .unwrap_or_else(|e| {
            eprintln!("Cannot bind to port 3000: {e}");
            std::process::exit(1);
        });
    println!("Mecha-Oracle UI running at http://localhost:3000");
    axum::serve(listener, router).await.unwrap();
}

// ── Tests ────────────────────────────────────────────────────────────────────

#[cfg(test)]
fn has_payable_server_action(card: &CardView) -> bool {
    card.actions
        .iter()
        .any(|a| a.can_pay_cost && matches!(a.kind, ActionItemKind::Server { .. }))
}

#[cfg(test)]
fn has_toggle_attacker(card: &CardView) -> bool {
    card.actions
        .iter()
        .any(|a| matches!(a.kind, ActionItemKind::ToggleAttacker { .. }))
}

#[cfg(test)]
fn has_assign_blocker(card: &CardView) -> bool {
    card.actions
        .iter()
        .any(|a| matches!(a.kind, ActionItemKind::AssignBlocker { .. }))
}

#[cfg(test)]
mod tests {
    use super::*;
    use mecha_oracle::types::{PermanentState, Step};
    use std::path::Path;

    fn test_db() -> CardDatabase {
        CardDatabase::from_path(Path::new("tests/fixtures/oracle_cards_test.json")).unwrap()
    }

    #[test]
    fn build_game_state_deals_7_cards_each() {
        let config = vec![
            vec![
                "Forest".into(),
                "Forest".into(),
                "Forest".into(),
                "Forest".into(),
                "Grizzly Bears".into(),
                "Grizzly Bears".into(),
                "Grizzly Bears".into(),
                "Grizzly Bears".into(),
                "Forest".into(),
                "Forest".into(),
            ],
            vec![
                "Forest".into(),
                "Forest".into(),
                "Forest".into(),
                "Forest".into(),
                "Grizzly Bears".into(),
                "Grizzly Bears".into(),
                "Grizzly Bears".into(),
                "Grizzly Bears".into(),
                "Forest".into(),
                "Forest".into(),
            ],
        ];
        let db = test_db();
        let gs = build_game_state(config, &db, false).unwrap();
        assert_eq!(gs.hands[&PlayerId(0)].len(), 7);
        assert_eq!(gs.hands[&PlayerId(1)].len(), 7);
        assert_eq!(gs.libraries[&PlayerId(0)].len(), 3);
        assert_eq!(gs.libraries[&PlayerId(1)].len(), 3);
    }

    #[test]
    fn build_game_state_unknown_card_returns_error() {
        let config = vec![vec!["NoSuchCard".into()], vec!["Forest".into()]];
        let db = test_db();
        let result = build_game_state(config, &db, false);
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("Unknown card"));
    }

    #[test]
    fn build_game_state_requires_two_decklists() {
        let config = vec![vec!["Forest".into()]];
        let db = test_db();
        let result = build_game_state(config, &db, false);
        assert!(result.is_err());
    }

    #[test]
    fn build_game_state_starts_at_pre_combat_main() {
        let config = vec![
            (0..10).map(|_| "Forest".to_string()).collect(),
            (0..10).map(|_| "Forest".to_string()).collect(),
        ];
        let db = test_db();
        let gs = build_game_state(config, &db, false).unwrap();
        assert_eq!(gs.step(), Step::PreCombatMain);
    }

    #[test]
    fn build_game_view_initial_life_and_step() {
        let config = vec![
            (0..10).map(|_| "Forest".to_string()).collect(),
            (0..10).map(|_| "Forest".to_string()).collect(),
        ];
        let db = test_db();
        let gs = build_game_state(config, &db, false).unwrap();
        let view = build_game_view(&gs);
        assert_eq!(view.p1.life, 20);
        assert_eq!(view.p2.life, 20);
        assert_eq!(view.active_player, PlayerId(0));
        assert_eq!(view.priority_player, PlayerId(0));
        assert_eq!(view.step, "PreCombatMain");
        assert_eq!(view.turn, 1);
        assert!(!view.attackers_declared);
        assert!(!view.blockers_declared);
    }

    #[test]
    fn game_view_includes_combat_declared_flags() {
        let config = vec![
            (0..10).map(|_| "Forest".to_string()).collect(),
            (0..10).map(|_| "Forest".to_string()).collect(),
        ];
        let db = test_db();
        let mut gs = build_game_state(config, &db, false).unwrap();
        // Navigate to DeclareAttackers: 4 passes (2 per step × 2 steps)
        for _ in 0..4 {
            gs = dispatch_action(gs, ActionRequest::AdvanceStep).unwrap();
        }
        assert!(!build_game_view(&gs).attackers_declared);

        gs = dispatch_action(
            gs,
            ActionRequest::DeclareAttackers {
                attacker_ids: vec![],
            },
        )
        .unwrap();
        assert!(build_game_view(&gs).attackers_declared);
    }

    #[test]
    fn build_game_view_hand_counts() {
        let config = vec![
            (0..10).map(|_| "Forest".to_string()).collect(),
            (0..10).map(|_| "Forest".to_string()).collect(),
        ];
        let db = test_db();
        let gs = build_game_state(config, &db, false).unwrap();
        let view = build_game_view(&gs);
        assert_eq!(view.p1.hand.len(), 7);
        assert_eq!(view.p2.hand.len(), 7);
        assert_eq!(view.p1.library_count, 3);
        assert_eq!(view.p2.library_count, 3);
    }

    #[test]
    fn format_mana_cost_green_green() {
        use mecha_oracle::types::mana::{ManaCost, ManaPip};
        let cost = ManaCost {
            pips: vec![ManaPip::Green, ManaPip::Green],
        };
        assert_eq!(format_mana_cost(&cost), "GG");
    }

    #[test]
    fn format_mana_cost_generic_and_color() {
        use mecha_oracle::types::mana::{ManaCost, ManaPip};
        let cost = ManaCost {
            pips: vec![ManaPip::Generic(3), ManaPip::Green],
        };
        assert_eq!(format_mana_cost(&cost), "3G");
    }

    #[test]
    fn format_type_line_with_subtype() {
        let db = test_db();
        let forest = db.get("Forest").unwrap();
        let result = format_type_line(&forest.type_line);
        assert_eq!(result, "Basic Land — Forest");
    }

    #[test]
    fn ap_passing_priority_shifts_to_nap_without_advancing_step() {
        let config = vec![
            (0..10).map(|_| "Forest".to_string()).collect(),
            (0..10).map(|_| "Forest".to_string()).collect(),
        ];
        let db = test_db();
        let gs = build_game_state(config, &db, false).unwrap();
        assert_eq!(gs.step(), Step::PreCombatMain);
        assert_eq!(gs.priority_player, PlayerId(0));

        let gs = dispatch_action(gs, ActionRequest::AdvanceStep).unwrap();

        assert_eq!(gs.step(), Step::PreCombatMain); // step did NOT advance
        assert_eq!(gs.priority_player, PlayerId(1)); // priority shifted to NAP
    }

    #[test]
    fn nap_passing_priority_advances_step() {
        let config = vec![
            (0..10).map(|_| "Forest".to_string()).collect(),
            (0..10).map(|_| "Forest".to_string()).collect(),
        ];
        let db = test_db();
        let gs = build_game_state(config, &db, false).unwrap();

        let gs = dispatch_action(gs, ActionRequest::AdvanceStep).unwrap(); // AP passes
        let gs = dispatch_action(gs, ActionRequest::AdvanceStep).unwrap(); // NAP passes → advances

        assert_eq!(gs.step(), Step::BeginningOfCombat);
        assert_eq!(gs.priority_player, PlayerId(0)); // resets to AP
    }

    #[test]
    fn advance_step_blocked_before_attackers_declared() {
        let config = vec![
            (0..10).map(|_| "Forest".to_string()).collect(),
            (0..10).map(|_| "Forest".to_string()).collect(),
        ];
        let db = test_db();
        let mut gs = build_game_state(config, &db, false).unwrap();
        // Navigate to DeclareAttackers (4 passes: PC×2, BOC×2)
        for _ in 0..4 {
            gs = dispatch_action(gs, ActionRequest::AdvanceStep).unwrap();
        }
        assert_eq!(gs.step(), Step::DeclareAttackers);
        assert!(!gs.combat.attackers_declared);

        // AdvanceStep must be rejected before attackers are declared
        assert!(dispatch_action(gs, ActionRequest::AdvanceStep).is_err());
    }

    #[test]
    fn advance_step_blocked_before_blockers_declared() {
        let config = vec![
            (0..10).map(|_| "Forest".to_string()).collect(),
            (0..10).map(|_| "Forest".to_string()).collect(),
        ];
        let db = test_db();
        let mut gs = build_game_state(config, &db, false).unwrap();
        // Navigate to DeclareAttackers (4 passes), declare, then advance to DeclareBlockers (2 passes)
        for _ in 0..4 {
            gs = dispatch_action(gs, ActionRequest::AdvanceStep).unwrap();
        }
        gs = dispatch_action(
            gs,
            ActionRequest::DeclareAttackers {
                attacker_ids: vec![],
            },
        )
        .unwrap();
        for _ in 0..2 {
            gs = dispatch_action(gs, ActionRequest::AdvanceStep).unwrap();
        }
        assert_eq!(gs.step(), Step::DeclareBlockers);
        assert!(!gs.combat.blockers_declared);

        // AdvanceStep must be rejected before blockers are declared
        assert!(dispatch_action(gs, ActionRequest::AdvanceStep).is_err());
    }

    #[test]
    fn advance_step_rejected_when_game_is_over() {
        let config = vec![
            (0..10).map(|_| "Forest".to_string()).collect(),
            (0..10).map(|_| "Forest".to_string()).collect(),
        ];
        let db = test_db();
        let mut gs = build_game_state(config, &db, false).unwrap();
        gs.game_over = true;
        assert!(dispatch_action(gs, ActionRequest::AdvanceStep).is_err());
    }

    #[test]
    fn advancing_from_end_step_auto_advances_to_next_upkeep() {
        use mecha_oracle::types::Step;
        let config = vec![
            (0..10).map(|_| "Forest".to_string()).collect(),
            (0..10).map(|_| "Forest".to_string()).collect(),
        ];
        let db = test_db();
        let mut gs = build_game_state(config, &db, false).unwrap();
        // PC (2) + BOC (2) → DeclareAttackers
        for _ in 0..4 {
            gs = dispatch_action(gs, ActionRequest::AdvanceStep).unwrap();
        }
        // Turn-based action: declare (empty) before priority opens
        gs = dispatch_action(
            gs,
            ActionRequest::DeclareAttackers {
                attacker_ids: vec![],
            },
        )
        .unwrap();
        // DA (2) → DeclareBlockers
        for _ in 0..2 {
            gs = dispatch_action(gs, ActionRequest::AdvanceStep).unwrap();
        }
        // Turn-based action: declare (empty) before priority opens
        gs = dispatch_action(gs, ActionRequest::DeclareBlockers { blocks: vec![] }).unwrap();
        // DB (2) → CD (auto-resolves) → CD (2) → EOC (2) → PC2 (2) → End
        for _ in 0..8 {
            gs = dispatch_action(gs, ActionRequest::AdvanceStep).unwrap();
        }
        assert_eq!(gs.step(), Step::End);
        assert_eq!(gs.active_player, PlayerId(0));

        // Two more passes → Cleanup (auto) → Untap (auto) → Upkeep for P1
        let gs = dispatch_action(gs, ActionRequest::AdvanceStep).unwrap();
        assert_eq!(gs.step(), Step::End); // still End after first pass
        let gs = dispatch_action(gs, ActionRequest::AdvanceStep).unwrap();
        assert_eq!(gs.step(), Step::Upkeep);
        assert_eq!(gs.active_player, PlayerId(1));
    }

    #[test]
    fn reset_mana_action_untaps_land_and_restores_pool() {
        use mecha_oracle::engine::casting::play_land;
        let db = test_db();
        let config = vec![
            vec![
                "Forest".into(),
                "Forest".into(),
                "Forest".into(),
                "Forest".into(),
                "Grizzly Bears".into(),
                "Grizzly Bears".into(),
                "Grizzly Bears".into(),
                "Grizzly Bears".into(),
                "Forest".into(),
                "Forest".into(),
            ],
            (0..10).map(|_| "Forest".to_string()).collect(),
        ];
        let mut gs = build_game_state(config, &db, false).unwrap();

        // Play a land from hand so we have one untapped land on the battlefield.
        let land_id = gs.hands[&PlayerId(0)]
            .iter()
            .find(|id| gs.objects[*id].is_land())
            .copied()
            .unwrap();
        gs = play_land(gs, PlayerId(0), land_id).unwrap();

        // Tap it for mana via the action dispatcher.
        gs = dispatch_action(
            gs,
            ActionRequest::TapLand {
                object_id: land_id.0,
            },
        )
        .unwrap();
        assert!(gs.battlefield[&land_id].tapped);
        assert_eq!(gs.get_player(PlayerId(0)).unwrap().mana_pool.green, 1);
        assert!(gs.mana_checkpoint.is_some());
        let view = build_game_view(&gs);
        assert!(view.can_reset_mana);

        // Reset mana.
        gs = dispatch_action(gs, ActionRequest::ResetMana).unwrap();

        assert!(!gs.battlefield[&land_id].tapped, "land untapped");
        assert!(
            gs.get_player(PlayerId(0)).unwrap().mana_pool.is_empty(),
            "pool empty"
        );
        assert!(gs.mana_checkpoint.is_none());
        let view = build_game_view(&gs);
        assert!(!view.can_reset_mana);
    }

    #[test]
    fn dispatch_tap_land_adds_mana_to_pool() {
        use mecha_oracle::engine::casting::play_land;
        let config = vec![
            vec![
                "Forest".into(),
                "Forest".into(),
                "Forest".into(),
                "Forest".into(),
                "Grizzly Bears".into(),
                "Grizzly Bears".into(),
                "Grizzly Bears".into(),
                "Grizzly Bears".into(),
                "Forest".into(),
                "Forest".into(),
            ],
            (0..10).map(|_| "Forest".to_string()).collect(),
        ];
        let db = test_db();
        let mut gs = build_game_state(config, &db, false).unwrap();

        let land_id = gs.hands[&PlayerId(0)]
            .iter()
            .find(|id| gs.objects[*id].is_land())
            .copied()
            .unwrap();
        gs = play_land(gs, PlayerId(0), land_id).unwrap();

        let tap_result = dispatch_action(
            gs,
            ActionRequest::TapLand {
                object_id: land_id.0,
            },
        );
        assert!(tap_result.is_ok());
        let gs2 = tap_result.unwrap();
        assert_eq!(gs2.get_player(PlayerId(0)).unwrap().mana_pool.green, 1);
    }

    #[test]
    fn dryad_arbor_appears_in_creatures_not_lands() {
        // A Land Creature is displayed in the creatures row; the tap-for-mana action
        // is available there via activated abilities. It must NOT appear in the lands row
        // to avoid a duplicate entry that confuses the UI layout.
        use mecha_oracle::engine::casting::play_land;
        let db = test_db();
        let config = vec![
            vec![
                "Dryad Arbor".into(),
                "Forest".into(),
                "Forest".into(),
                "Forest".into(),
                "Forest".into(),
                "Forest".into(),
                "Forest".into(),
                "Forest".into(),
                "Forest".into(),
                "Forest".into(),
            ],
            (0..10).map(|_| "Forest".to_string()).collect(),
        ];
        let mut gs = build_game_state(config, &db, false).unwrap();

        let arbor_id = gs.hands[&PlayerId(0)]
            .iter()
            .find(|id| gs.objects[*id].definition.name == "Dryad Arbor")
            .copied()
            .unwrap();
        gs = play_land(gs, PlayerId(0), arbor_id).unwrap();

        let view = build_game_view(&gs);
        let p1 = &view.p1;
        assert!(
            p1.creatures.iter().any(|c| c.name == "Dryad Arbor"),
            "Dryad Arbor must appear in creatures"
        );
        assert!(
            !p1.lands.iter().any(|c| c.name == "Dryad Arbor"),
            "Dryad Arbor must not appear in lands (it shows in creatures instead)"
        );
    }

    #[test]
    fn can_attack_true_only_for_active_player_at_declare_attackers() {
        use mecha_oracle::types::{CardObject, Step, Zone};
        let db = test_db();
        let config = vec![
            (0..10).map(|_| "Forest".to_string()).collect(),
            (0..10).map(|_| "Forest".to_string()).collect(),
        ];
        let mut gs = build_game_state(config, &db, false).unwrap();

        let p1_id = {
            let id = gs.alloc_id();
            let obj = CardObject::new(
                id,
                db.get("Grizzly Bears").unwrap().clone(),
                PlayerId(0),
                Zone::Battlefield,
            );
            let mut perm = PermanentState::new(&obj.definition);
            perm.controller_since_turn = 0;
            gs.battlefield.insert(id, perm);
            gs.add_object(obj);
            id.0
        };
        let p2_id = {
            let id = gs.alloc_id();
            let obj = CardObject::new(
                id,
                db.get("Grizzly Bears").unwrap().clone(),
                PlayerId(1),
                Zone::Battlefield,
            );
            let mut perm = PermanentState::new(&obj.definition);
            perm.controller_since_turn = 0;
            gs.battlefield.insert(id, perm);
            gs.add_object(obj);
            id.0
        };

        for _ in 0..4 {
            gs = dispatch_action(gs, ActionRequest::AdvanceStep).unwrap();
        }
        assert_eq!(gs.step(), Step::DeclareAttackers);

        let view = build_game_view(&gs);
        let p1_c = view
            .p1
            .creatures
            .iter()
            .find(|c| c.id == ObjectId(p1_id))
            .unwrap();
        let p2_c = view
            .p2
            .creatures
            .iter()
            .find(|c| c.id == ObjectId(p2_id))
            .unwrap();

        assert!(
            has_toggle_attacker(p1_c),
            "active player's creature should have toggle_attacker action"
        );
        assert!(
            !has_toggle_attacker(p2_c),
            "defending player's creature should not have toggle_attacker action"
        );
        assert!(
            !has_assign_blocker(p1_c),
            "assign_blocker is false outside DeclareBlockers"
        );
        assert!(
            !has_assign_blocker(p2_c),
            "assign_blocker is false outside DeclareBlockers"
        );
    }

    #[test]
    fn can_cast_true_for_instant_in_hand_with_mana_and_priority() {
        use mecha_oracle::types::card::{CardDefinition, CardType, TypeLine};
        use mecha_oracle::types::effect::EffectStep;
        use mecha_oracle::types::mana::{ManaCost, ManaPip};
        use mecha_oracle::types::{Ability, OracleSpan};
        use mecha_oracle::types::{CardObject, Zone};

        let config = vec![
            (0..10).map(|_| "Forest".to_string()).collect(),
            (0..10).map(|_| "Forest".to_string()).collect(),
        ];
        let db = test_db();
        let mut gs = build_game_state(config, &db, false).unwrap();
        use mecha_oracle::types::ability::SpellAbility;
        let def = CardDefinition {
            name: "Cheap Instant".into(),
            mana_cost: Some(ManaCost {
                pips: vec![ManaPip::Generic(1)],
            }),
            type_line: TypeLine {
                supertypes: vec![],
                card_types: vec![CardType::Instant],
                subtypes: vec![],
            },
            oracle_text: "Draw a card.".into(),
            abilities: vec![OracleSpan::Parsed(Ability::SpellEffect(SpellAbility {
                target_requirements: vec![],
                steps: vec![EffectStep::DrawCard(1)],
            }))],
            power: None,
            toughness: None,
        };
        let id = gs.alloc_id();
        let obj = CardObject::new(id, def, mecha_oracle::types::PlayerId(0), Zone::Hand);
        gs.hands
            .get_mut(&mecha_oracle::types::PlayerId(0))
            .unwrap()
            .push(id);
        gs.add_object(obj);
        gs.get_player_mut(mecha_oracle::types::PlayerId(0))
            .unwrap()
            .mana_pool
            .colorless = 1;

        let view = build_game_view(&gs);
        let card = view
            .p1
            .hand
            .iter()
            .find(|c| c.name == "Cheap Instant")
            .unwrap();
        assert!(
            has_payable_server_action(card),
            "instant with mana in hand with priority should have a payable server action"
        );
    }

    #[test]
    fn can_cast_false_for_creature_when_not_active_player() {
        use mecha_oracle::types::{CardObject, Zone};
        let db = test_db();
        let config = vec![
            (0..10).map(|_| "Forest".to_string()).collect(),
            (0..10).map(|_| "Forest".to_string()).collect(),
        ];
        let mut gs = build_game_state(config, &db, false).unwrap();
        gs.active_player = mecha_oracle::types::PlayerId(1);
        gs.priority_player = mecha_oracle::types::PlayerId(0);

        let id = gs.alloc_id();
        let obj = CardObject::new(
            id,
            db.get("Grizzly Bears").unwrap().clone(),
            mecha_oracle::types::PlayerId(0),
            Zone::Hand,
        );
        gs.hands
            .get_mut(&mecha_oracle::types::PlayerId(0))
            .unwrap()
            .push(id);
        gs.add_object(obj);
        gs.get_player_mut(mecha_oracle::types::PlayerId(0))
            .unwrap()
            .mana_pool
            .green = 2;

        let view = build_game_view(&gs);
        let card = view
            .p1
            .hand
            .iter()
            .find(|c| c.name == "Grizzly Bears")
            .unwrap();
        assert!(
            card.actions.is_empty(),
            "creature cannot be cast when player is not active player — should have no actions"
        );
    }

    #[test]
    fn can_block_true_only_for_defending_player_at_declare_blockers() {
        use mecha_oracle::types::{CardObject, Step, Zone};
        let db = test_db();
        let config = vec![
            (0..10).map(|_| "Forest".to_string()).collect(),
            (0..10).map(|_| "Forest".to_string()).collect(),
        ];
        let mut gs = build_game_state(config, &db, false).unwrap();

        let p1_id = {
            let id = gs.alloc_id();
            let obj = CardObject::new(
                id,
                db.get("Grizzly Bears").unwrap().clone(),
                PlayerId(0),
                Zone::Battlefield,
            );
            let mut perm = PermanentState::new(&obj.definition);
            perm.controller_since_turn = 0;
            gs.battlefield.insert(id, perm);
            gs.add_object(obj);
            id.0
        };
        let p2_id = {
            let id = gs.alloc_id();
            let obj = CardObject::new(
                id,
                db.get("Grizzly Bears").unwrap().clone(),
                PlayerId(1),
                Zone::Battlefield,
            );
            let mut perm = PermanentState::new(&obj.definition);
            perm.controller_since_turn = 0;
            gs.battlefield.insert(id, perm);
            gs.add_object(obj);
            id.0
        };

        for _ in 0..4 {
            gs = dispatch_action(gs, ActionRequest::AdvanceStep).unwrap();
        }
        let gs = dispatch_action(
            gs,
            ActionRequest::DeclareAttackers {
                attacker_ids: vec![p1_id],
            },
        )
        .unwrap();
        let gs = dispatch_action(gs, ActionRequest::AdvanceStep).unwrap();
        let gs = dispatch_action(gs, ActionRequest::AdvanceStep).unwrap();
        assert_eq!(gs.step(), Step::DeclareBlockers);

        let view = build_game_view(&gs);
        let p1_c = view
            .p1
            .creatures
            .iter()
            .find(|c| c.id == ObjectId(p1_id))
            .unwrap();
        let p2_c = view
            .p2
            .creatures
            .iter()
            .find(|c| c.id == ObjectId(p2_id))
            .unwrap();

        assert!(
            !has_assign_blocker(p1_c),
            "active player's creature should not have assign_blocker action"
        );
        assert!(
            has_assign_blocker(p2_c),
            "defending player's creature should have assign_blocker action"
        );
        assert!(
            !has_toggle_attacker(p1_c),
            "declared attacker (tapped) should not have toggle_attacker action"
        );
    }
}
