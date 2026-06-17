use axum::{
    Json, Router,
    extract::State,
    response::{Html, IntoResponse},
    routing::{get, post},
};
use mecha_oracle::cards::CardDatabase;
use mecha_oracle::engine::activated::activate_ability;
use mecha_oracle::engine::casting::{cast_spell, play_land};
use mecha_oracle::engine::combat::{can_block_attacker, declare_attackers, declare_blockers};
use mecha_oracle::engine::costs::{
    can_pay_cost_components, decline_pending_cost, pay_pending_cost,
};
use mecha_oracle::engine::cycling::cycle_card;
use mecha_oracle::engine::mana::{reset_mana, tap_land_for_mana};
use mecha_oracle::engine::stack::pass_priority;
use mecha_oracle::engine::targeting::legal_targets;
use mecha_oracle::engine::turn::{advance_step, apply_step_start, draw_card, skip_to_first_main};
use mecha_oracle::types::ability::{
    Ability, ActivatedAbility, AnnotationKind, CostComponent, OracleSpan, StaticAbility,
    TextAnnotation,
};
use mecha_oracle::types::effect::{EffectStep, EffectTarget};
use mecha_oracle::types::stack::StackPayload;
use mecha_oracle::types::{
    CardObject, CounterKind, GameState, ObjectId, Player, PlayerId, Step, Zone,
};
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
struct TextAnnotationView {
    start: usize,
    end: usize,
    kind: AnnotationKind,
}

/// Converts a UTF-8 byte offset to a Unicode codepoint offset.
/// MTG oracle text is BMP-only, so codepoint offsets equal JS string char indices.
fn byte_to_char(s: &str, byte_offset: usize) -> usize {
    s[..byte_offset].chars().count()
}

fn annotation_views(oracle_text: &str, anns: &[TextAnnotation]) -> Vec<TextAnnotationView> {
    anns.iter()
        .map(|a| TextAnnotationView {
            start: byte_to_char(oracle_text, a.start),
            end: byte_to_char(oracle_text, a.end),
            kind: a.kind.clone(),
        })
        .collect()
}

#[derive(Serialize)]
struct StackItemView {
    id: u64,
    kind: String,
    label: String,
    controller: PlayerId,
    card: Option<CardView>,
    #[serde(skip_serializing_if = "Option::is_none")]
    cost_label: Option<String>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    targets: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    source_name: Option<String>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    source_colors: Vec<String>,
}

#[derive(Serialize)]
struct ActionItemView {
    label: String,
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
    oracle_text: String,
    text_annotations: Vec<TextAnnotationView>,
    mana_cost: Option<String>,
    power: Option<i32>,
    toughness: Option<i32>,
    colors: Vec<String>,
    tapped: bool,
    summoning_sick: bool,
    damage_marked: u32,
    is_attacking: bool,
    is_blocking: bool,
    actions: Vec<ActionItemView>,
    counters: Vec<CounterView>,
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
    poison_counters: u32,
}

#[derive(Serialize)]
struct CounterView {
    label: String,
    kind: String,
    count: u32,
    sublabel: Option<String>,
}

fn counter_to_view(kind: &CounterKind, count: u32) -> CounterView {
    match kind {
        CounterKind::PtModifier { power, toughness } => {
            let label = format!("{:+}/{:+}", power, toughness);
            let kind_str = if *power >= 0 && *toughness >= 0 && (*power > 0 || *toughness > 0) {
                "plus"
            } else if *power <= 0 && *toughness <= 0 && (*power < 0 || *toughness < 0) {
                "minus"
            } else {
                "mixed"
            };
            let net_p = *power * count as i32;
            let net_t = *toughness * count as i32;
            CounterView {
                label,
                kind: kind_str.to_string(),
                count,
                sublabel: Some(format!("{:+}/{:+} to P/T", net_p, net_t)),
            }
        }
        CounterKind::Poison => CounterView {
            label: "Poison".to_string(),
            kind: "poison".to_string(),
            count,
            sublabel: None,
        },
        CounterKind::Named(name) => {
            let label = {
                let mut chars = name.chars();
                match chars.next() {
                    None => String::new(),
                    Some(first) => first.to_uppercase().collect::<String>() + chars.as_str(),
                }
            };
            CounterView {
                label,
                kind: "named".to_string(),
                count,
                sublabel: None,
            }
        }
    }
}

#[derive(Serialize)]
struct PendingPaymentView {
    paying_player: u64,
    cost_label: String,
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
    pending_payment: Option<PendingPaymentView>,
}

/// CR 202.2b — lands have no mana cost and so are colorless by definition, but their
/// *display* color (for the UI only) is derived from basic land subtypes and any
/// colored mana symbols printed in their rules text, e.g. "Swamp Forest" → [Black,
/// Green]. Non-land cards with no printed colors stay colorless (CR 105.2 remains
/// authoritative for everything else, e.g. protection-from-color targeting at
/// `legal_targets`).
fn display_colors(
    def: &mecha_oracle::types::card::CardDefinition,
) -> Vec<mecha_oracle::types::mana::ManaColor> {
    use mecha_oracle::types::mana::ManaColor;
    if !def.colors.is_empty() {
        return def.colors.clone();
    }
    if !def.type_line.is_land() {
        return vec![];
    }
    let mut colors: Vec<ManaColor> = Vec::new();
    let mut push = |c: ManaColor| {
        if !colors.contains(&c) {
            colors.push(c);
        }
    };
    for subtype in &def.type_line.subtypes {
        match subtype.as_str() {
            "Plains" => push(ManaColor::White),
            "Island" => push(ManaColor::Blue),
            "Swamp" => push(ManaColor::Black),
            "Mountain" => push(ManaColor::Red),
            "Forest" => push(ManaColor::Green),
            _ => {}
        }
    }
    for (needle, color) in [
        ("{W}", ManaColor::White),
        ("{U}", ManaColor::Blue),
        ("{B}", ManaColor::Black),
        ("{R}", ManaColor::Red),
        ("{G}", ManaColor::Green),
    ] {
        if def.oracle_text.contains(needle) {
            push(color);
        }
    }
    colors
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
            EffectStep::AddCounter { kind, count } => format!("Add {count} {kind:?} counter(s)"),
            EffectStep::DealDamage(n) => format!("Deal {n} damage"),
            EffectStep::CounterSpell => "Counter target spell".to_string(),
            EffectStep::Payment { .. } => "Pay cost".to_string(),
            EffectStep::Unimplemented(s) => s.clone(),
        })
        .collect();
    format!("{}: {}", cost_parts.join(", "), effect_parts.join(". "))
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

    let is_land = obj.definition.type_line.is_land();

    // Play land (no mana cost — always structurally valid when conditions met)
    if is_land {
        let can_play = state.active_player == pid
            && state.priority_player == pid
            && state.lands_played_this_turn == 0
            && matches!(state.step(), Step::PreCombatMain | Step::PostCombatMain)
            && state.stack.is_empty();
        if can_play {
            actions.push(ActionItemView {
                label: "Play land".to_string(),
                kind: ActionItemKind::Server {
                    action: serde_json::json!({
                        "type": "play_land",
                        "object_id": obj.id.0
                    }),
                },
            });
        }
    }

    // Cast spell (lands cannot be cast)
    if !is_land && obj.definition.mana_cost.is_some() && can_cast_structural(state, pid, obj) {
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
            .cloned()
            .collect();

        let cost_label = obj
            .definition
            .mana_cost
            .as_ref()
            .map(format_mana_cost_braced)
            .unwrap_or_default();

        if target_filters.is_empty() {
            // Untargeted spell
            actions.push(ActionItemView {
                label: format!("Cast {}", obj.definition.name),
                kind: ActionItemKind::Server {
                    action: serde_json::json!({
                        "type": "cast_spell",
                        "object_id": obj.id.0,
                        "cost_label": cost_label
                    }),
                },
            });
        } else {
            // Targeted spell: one action per legal target
            let spell_colors = obj.definition.colors.clone();
            let mut seen = std::collections::HashSet::new();
            for filter in &target_filters {
                for target in legal_targets(state, filter, pid, &spell_colors) {
                    let key = match &target {
                        EffectTarget::Object { id } => format!("o{}", id.0),
                        EffectTarget::Player { id } => format!("p{}", id.0),
                        EffectTarget::StackObject { id } => format!("s{}", id.0),
                    };
                    if !seen.insert(key) {
                        continue;
                    }
                    let target_name = target_display_name(state, &target);
                    let target_val = serde_json::to_value(&target).unwrap();
                    actions.push(ActionItemView {
                        label: format!("Cast {} → {}", obj.definition.name, target_name),
                        kind: ActionItemKind::Server {
                            action: serde_json::json!({
                                "type": "cast_spell",
                                "object_id": obj.id.0,
                                "targets": [target_val],
                                "cost_label": cost_label
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
            actions.push(ActionItemView {
                label: format!("Cycle ({})", format_mana_cost_braced(cost)),
                kind: ActionItemKind::Server {
                    action: serde_json::json!({
                        "type": "cycle_card",
                        "object_id": obj.id.0,
                        "cost_label": format_mana_cost_braced(cost)
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

    // Attacker toggle (no cost)
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
                kind: ActionItemKind::ToggleAttacker {
                    object_id: obj.id.0,
                },
            });
        }
    }

    // Blocker assignment (no cost)
    if state.step() == Step::DeclareBlockers && pid != state.active_player {
        for &atk_id in &state.combat.attackers {
            // can_block_attacker delegates to perm.can_block() (tapped, Decayed) internally
            if !can_block_attacker(state, obj.id, atk_id) {
                continue;
            }
            let atk_name = state
                .objects
                .get(&atk_id)
                .map(|o| o.definition.name.as_str())
                .unwrap_or("Unknown");
            actions.push(ActionItemView {
                label: format!("Block {atk_name}"),
                kind: ActionItemKind::AssignBlocker {
                    blocker_id: obj.id.0,
                    attacker_id: atk_id.0,
                },
            });
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
            if !can_pay_cost_components(state, pid, Some(obj.id), &ability.cost) {
                continue;
            }
            actions.push(ActionItemView {
                label: format_activated_ability(ability),
                kind: ActionItemKind::Server {
                    action: serde_json::json!({
                        "type": "activate_ability",
                        "object_id": obj.id.0,
                        "ability_index": i,
                        "mana_ability": produces_mana,
                        "cost_label": format_ability_cost_label(&ability.cost)
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
            oracle_text: obj.definition.oracle_text.clone(),
            text_annotations: annotation_views(
                &obj.definition.oracle_text,
                &obj.definition.text_annotations,
            ),
            mana_cost: obj
                .definition
                .mana_cost
                .as_ref()
                .map(format_mana_cost_braced),
            power: perm.and_then(|p| p.effective_power()),
            toughness: perm.and_then(|p| p.effective_toughness()),
            colors: display_colors(&obj.definition)
                .iter()
                .map(|c| c.to_string())
                .collect(),
            tapped: perm.map(|p| p.tapped).unwrap_or(false),
            summoning_sick: perm
                .map(|p| p.summoning_sick(state.controllers_most_recent_turn(pid)))
                .unwrap_or(false),
            damage_marked: perm.map(|p| p.damage_marked).unwrap_or(0),
            is_attacking: state.combat.attackers.contains(&obj.id),
            is_blocking: all_blockers.contains(&obj.id),
            actions: compute_actions(state, pid, obj),
            counters: perm
                .map(|p| {
                    p.counters
                        .iter()
                        .map(|(kind, &count)| counter_to_view(kind, count))
                        .collect()
                })
                .unwrap_or_default(),
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
        poison_counters: player.counter_count(&CounterKind::Poison),
    }
}

fn format_ability_cost_label(cost: &[CostComponent]) -> String {
    cost.iter()
        .map(|c| match c {
            CostComponent::Tap => "{T}".to_string(),
            CostComponent::Mana(m) => format_mana_cost_braced(m),
            CostComponent::PayLife(n) => format!("Pay {n} life"),
            CostComponent::Sacrifice(n, _) => format!("Sacrifice {n}"),
            CostComponent::Discard(n, _) => format!("Discard {n}"),
            CostComponent::Unimplemented(s) => s.clone(),
        })
        .collect::<Vec<_>>()
        .join(", ")
}

fn target_display_name(state: &GameState, target: &EffectTarget) -> String {
    match target {
        EffectTarget::Object { id } => state
            .objects
            .get(id)
            .map(|o| o.definition.name.clone())
            .unwrap_or_default(),
        EffectTarget::Player { id } => state
            .get_player(*id)
            .map(|p| p.name.clone())
            .unwrap_or_default(),
        EffectTarget::StackObject { id } => state
            .stack_objects
            .get(id)
            .and_then(|obj| match &obj.payload {
                StackPayload::Spell { card_id } => state
                    .objects
                    .get(card_id)
                    .map(|c| c.definition.name.clone()),
                _ => None,
            })
            .unwrap_or_default(),
    }
}

fn build_game_view(state: &GameState) -> GameView {
    let stack: Vec<StackItemView> = state
        .stack
        .iter()
        .map(|&sid| {
            let obj = &state.stack_objects[&sid];
            let targets: Vec<String> = obj
                .targets
                .iter()
                .map(|t| target_display_name(state, t))
                .collect();
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
                            oracle_text: c.definition.oracle_text.clone(),
                            text_annotations: annotation_views(
                                &c.definition.oracle_text,
                                &c.definition.text_annotations,
                            ),
                            mana_cost: c.definition.mana_cost.as_ref().map(format_mana_cost_braced),
                            power: c.definition.power,
                            toughness: c.definition.toughness,
                            colors: display_colors(&c.definition)
                                .iter()
                                .map(|c| c.to_string())
                                .collect(),
                            tapped: false,
                            summoning_sick: false,
                            damage_marked: 0,
                            is_attacking: false,
                            is_blocking: false,
                            actions: vec![],
                            counters: vec![],
                        }),
                        cost_label: None,
                        targets,
                        source_name: None,
                        source_colors: vec![],
                    }
                }
                StackPayload::TriggeredAbility {
                    label, source_id, ..
                } => StackItemView {
                    id: sid.0,
                    kind: "triggered_ability".into(),
                    label: label.clone(),
                    controller: obj.controller,
                    card: None,
                    cost_label: None,
                    targets,
                    source_name: state
                        .objects
                        .get(source_id)
                        .map(|o| o.definition.name.clone()),
                    source_colors: state
                        .objects
                        .get(source_id)
                        .map(|o| display_colors(&o.definition))
                        .unwrap_or_default()
                        .iter()
                        .map(|c| c.to_string())
                        .collect(),
                },
                StackPayload::ActivatedAbility {
                    label, source_id, ..
                } => StackItemView {
                    id: sid.0,
                    kind: "activated_ability".into(),
                    label: label.clone(),
                    controller: obj.controller,
                    card: None,
                    cost_label: None,
                    targets,
                    source_name: state
                        .objects
                        .get(source_id)
                        .map(|o| o.definition.name.clone()),
                    source_colors: state
                        .objects
                        .get(source_id)
                        .map(|o| display_colors(&o.definition))
                        .unwrap_or_default()
                        .iter()
                        .map(|c| c.to_string())
                        .collect(),
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
        pending_payment: state.pending_payment.as_ref().map(|pp| PendingPaymentView {
            paying_player: pp.paying_player.0 as u64,
            cost_label: format_ability_cost_label(&pp.cost),
        }),
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
        #[serde(default)]
        x_value: Option<u32>,
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
        targets: Vec<mecha_oracle::types::effect::EffectTarget>,
    },
    CycleCard {
        object_id: u64,
    },
    /// CR 118.12: pay the current inline cost obligation.
    PayPendingCost,
    /// CR 118.12: decline the current inline cost obligation (spell will be countered).
    DeclinePendingCost,
}

#[derive(Serialize)]
struct ActionResponse {
    ok: bool,
    state: GameView,
    #[serde(skip_serializing_if = "Option::is_none")]
    error: Option<String>,
}

fn has_valid_attackers(state: &GameState) -> bool {
    let cmt = state.controllers_most_recent_turn(state.active_player);
    state.battlefield.iter().any(|(&id, perm)| {
        state
            .objects
            .get(&id)
            .map(|o| o.controller == state.active_player)
            .unwrap_or(false)
            && perm.can_attack(cmt)
    })
}

// O(A×B): attackers × battlefield; acceptable for current board sizes.
fn has_valid_blockers(state: &GameState) -> bool {
    let defender = state.opponent_of(state.active_player);
    state.combat.attackers.iter().any(|&atk_id| {
        state.battlefield.keys().any(|&blk_id| {
            state
                .objects
                .get(&blk_id)
                .map(|o| o.controller == defender)
                .unwrap_or(false)
                && can_block_attacker(state, blk_id, atk_id)
        })
    })
}

// After pass_priority has already advanced the step, apply step-start actions and
// auto-advance through Untap/Cleanup steps (CR 502, 514). When no valid options
// exist for DA/DB, auto-declare the empty set (no UI shown) but still give players
// priority at that step (CR 506.1 — DB and CD skipped by advance_step when attackers=0).
fn apply_step_start_loop(mut state: GameState) -> GameState {
    loop {
        state = apply_step_start(state);
        if state.is_game_over() {
            break;
        }
        let step = state.step();
        if step == Step::DeclareAttackers && !has_valid_attackers(&state) {
            let active = state.active_player;
            state = declare_attackers(state, active, &[])
                .expect("auto-declare empty attackers cannot fail");
            break; // players still get priority at DA per CR 506.1
        } else if step == Step::DeclareBlockers && !has_valid_blockers(&state) {
            let defender = state.opponent_of(state.active_player);
            state = declare_blockers(state, defender, &[])
                .expect("auto-declare empty blockers cannot fail");
            break; // players still get priority at DB
        } else if !matches!(step, Step::Untap | Step::Cleanup) {
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
        ActionRequest::CastSpell {
            object_id,
            targets,
            x_value,
        } => {
            let player = state.priority_player;
            cast_spell(state, player, ObjectId(object_id), targets, x_value)
                .map_err(|e| format!("{e:?}"))
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
            targets,
        } => {
            let player = state.priority_player;
            activate_ability(
                state,
                ObjectId(object_id),
                ability_index,
                player,
                x_value,
                targets,
            )
            .map_err(|e| format!("{e:?}"))
        }
        ActionRequest::CycleCard { object_id } => {
            let player = state.priority_player;
            cycle_card(state, ObjectId(object_id), player).map_err(|e| format!("{e:?}"))
        }
        ActionRequest::PayPendingCost => {
            let player = state.priority_player;
            pay_pending_cost(state, player).map_err(|e| format!("{e:?}"))
        }
        ActionRequest::DeclinePendingCost => {
            decline_pending_cost(state).map_err(|e| format!("{e:?}"))
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
        .any(|a| matches!(a.kind, ActionItemKind::Server { .. }))
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
        use mecha_oracle::types::{CardObject, Zone};
        let config = vec![
            (0..10).map(|_| "Forest".to_string()).collect(),
            (0..10).map(|_| "Forest".to_string()).collect(),
        ];
        let db = test_db();
        let mut gs = build_game_state(config, &db, false).unwrap();
        // Add P0 creature to prevent DA auto-skip
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
        use mecha_oracle::types::{CardObject, Zone};
        let config = vec![
            (0..10).map(|_| "Forest".to_string()).collect(),
            (0..10).map(|_| "Forest".to_string()).collect(),
        ];
        let db = test_db();
        let mut gs = build_game_state(config, &db, false).unwrap();
        // Add an untapped creature for P0 so DA is not auto-skipped
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

        for _ in 0..4 {
            gs = dispatch_action(gs, ActionRequest::AdvanceStep).unwrap();
        }
        assert_eq!(gs.step(), Step::DeclareAttackers);
        assert!(!gs.combat.attackers_declared);

        assert!(dispatch_action(gs, ActionRequest::AdvanceStep).is_err());
    }

    #[test]
    fn advance_step_blocked_before_blockers_declared() {
        use mecha_oracle::types::{CardObject, Zone};
        let config = vec![
            (0..10).map(|_| "Forest".to_string()).collect(),
            (0..10).map(|_| "Forest".to_string()).collect(),
        ];
        let db = test_db();
        let mut gs = build_game_state(config, &db, false).unwrap();
        // P0 attacker — prevents DA auto-skip and makes DB non-trivially skippable
        let p0_id = {
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
            id
        };
        // P1 blocker — makes has_valid_blockers return true, preventing DB auto-skip
        {
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
        }
        for _ in 0..4 {
            gs = dispatch_action(gs, ActionRequest::AdvanceStep).unwrap();
        }
        assert_eq!(gs.step(), Step::DeclareAttackers);
        gs = dispatch_action(
            gs,
            ActionRequest::DeclareAttackers {
                attacker_ids: vec![p0_id.0],
            },
        )
        .unwrap();
        for _ in 0..2 {
            gs = dispatch_action(gs, ActionRequest::AdvanceStep).unwrap();
        }
        assert_eq!(gs.step(), Step::DeclareBlockers);
        assert!(!gs.combat.blockers_declared);

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
        // PC (2) + BOC (2) → DA/DB auto-skipped → CD (2) → EOC (2) → PC2 (2) → End
        for _ in 0..10 {
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
            text_annotations: vec![],
            power: None,
            toughness: None,
            colors: vec![],
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

    #[test]
    fn autoskips_declare_attackers_when_no_valid_attackers() {
        // All-Forest deck: no creatures → attackers auto-declared empty; players get
        // priority at DA; advance_step then skips DB+CD per CR 506.1.
        let config = vec![
            (0..10).map(|_| "Forest".to_string()).collect(),
            (0..10).map(|_| "Forest".to_string()).collect(),
        ];
        let db = test_db();
        let mut gs = build_game_state(config, &db, false).unwrap();
        // 2 passes → BOC; 2 more passes → DA (auto-declared, priority window)
        for _ in 0..4 {
            gs = dispatch_action(gs, ActionRequest::AdvanceStep).unwrap();
        }
        assert_eq!(
            gs.step(),
            Step::DeclareAttackers,
            "should stop at DA (auto-declared) to give players priority per CR 506.1"
        );
        assert!(gs.combat.attackers_declared);
        assert!(!gs.combat.blockers_declared);
        // 2 more passes → advance_step skips DB+CD → EndOfCombat
        for _ in 0..2 {
            gs = dispatch_action(gs, ActionRequest::AdvanceStep).unwrap();
        }
        assert_eq!(
            gs.step(),
            Step::EndOfCombat,
            "should skip to EndOfCombat per CR 506.1 when no attackers declared"
        );
    }

    #[test]
    fn no_autoskip_declare_attackers_when_valid_attacker_exists() {
        use mecha_oracle::types::{CardObject, Zone};
        let db = test_db();
        let config = vec![
            (0..10).map(|_| "Forest".to_string()).collect(),
            (0..10).map(|_| "Forest".to_string()).collect(),
        ];
        let mut gs = build_game_state(config, &db, false).unwrap();
        // Add an untapped, non-sick creature for P0
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

        for _ in 0..4 {
            gs = dispatch_action(gs, ActionRequest::AdvanceStep).unwrap();
        }
        assert_eq!(
            gs.step(),
            Step::DeclareAttackers,
            "should stop at DA when a valid attacker exists"
        );
        assert!(!gs.combat.attackers_declared);
    }

    #[test]
    fn autoskips_declare_blockers_when_no_valid_blocker_for_any_attacker() {
        // P0 has a flying attacker; P1 has only a ground creature — no valid blockers.
        use mecha_oracle::types::ability::StaticAbility;
        use mecha_oracle::types::card::{CardDefinition, CardType, TypeLine};
        use mecha_oracle::types::{Ability, CardObject, OracleSpan, Zone};
        let db = test_db();
        let config = vec![
            (0..10).map(|_| "Forest".to_string()).collect(),
            (0..10).map(|_| "Forest".to_string()).collect(),
        ];
        let mut gs = build_game_state(config, &db, false).unwrap();

        let flying_id = {
            let id = gs.alloc_id();
            let def = CardDefinition {
                name: "Flying Attacker".into(),
                mana_cost: None,
                type_line: TypeLine {
                    supertypes: vec![],
                    card_types: vec![CardType::Creature],
                    subtypes: vec![],
                },
                oracle_text: String::new(),
                abilities: vec![OracleSpan::Parsed(Ability::Static(StaticAbility::Flying))],
                text_annotations: vec![],
                power: Some(2),
                toughness: Some(2),
                colors: vec![],
            };
            let obj = CardObject::new(id, def, PlayerId(0), Zone::Battlefield);
            let mut perm = PermanentState::new(&obj.definition);
            perm.controller_since_turn = 0;
            gs.battlefield.insert(id, perm);
            gs.add_object(obj);
            id
        };
        {
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
        }

        for _ in 0..4 {
            gs = dispatch_action(gs, ActionRequest::AdvanceStep).unwrap();
        }
        assert_eq!(gs.step(), Step::DeclareAttackers);
        gs = dispatch_action(
            gs,
            ActionRequest::DeclareAttackers {
                attacker_ids: vec![flying_id.0],
            },
        )
        .unwrap();
        // 2 passes → transition to DB; auto-declare empty blockers; priority at DB
        for _ in 0..2 {
            gs = dispatch_action(gs, ActionRequest::AdvanceStep).unwrap();
        }
        assert_eq!(
            gs.step(),
            Step::DeclareBlockers,
            "should stop at DB (auto-declared) to give players priority"
        );
        assert!(gs.combat.blockers_declared);
        // 2 more passes → CombatDamage
        for _ in 0..2 {
            gs = dispatch_action(gs, ActionRequest::AdvanceStep).unwrap();
        }
        assert_eq!(
            gs.step(),
            Step::CombatDamage,
            "should advance to CombatDamage after DB priority window"
        );
    }

    #[test]
    fn no_autoskip_declare_blockers_when_valid_blocker_exists() {
        use mecha_oracle::types::{CardObject, Zone};
        let db = test_db();
        let config = vec![
            (0..10).map(|_| "Forest".to_string()).collect(),
            (0..10).map(|_| "Forest".to_string()).collect(),
        ];
        let mut gs = build_game_state(config, &db, false).unwrap();

        let p0_id = {
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
            id
        };
        {
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
        }

        for _ in 0..4 {
            gs = dispatch_action(gs, ActionRequest::AdvanceStep).unwrap();
        }
        assert_eq!(gs.step(), Step::DeclareAttackers);
        gs = dispatch_action(
            gs,
            ActionRequest::DeclareAttackers {
                attacker_ids: vec![p0_id.0],
            },
        )
        .unwrap();
        for _ in 0..2 {
            gs = dispatch_action(gs, ActionRequest::AdvanceStep).unwrap();
        }
        assert_eq!(
            gs.step(),
            Step::DeclareBlockers,
            "should stop at DB when P1 has a valid blocker"
        );
        assert!(!gs.combat.blockers_declared);
    }

    #[test]
    fn blocker_ui_only_shows_valid_pairings() {
        use mecha_oracle::types::ability::StaticAbility;
        use mecha_oracle::types::card::{CardDefinition, CardType, TypeLine};
        use mecha_oracle::types::{Ability, CardObject, OracleSpan, Zone};

        let db = test_db();
        let config = vec![
            (0..10).map(|_| "Forest".to_string()).collect(),
            (0..10).map(|_| "Forest".to_string()).collect(),
        ];
        let mut gs = build_game_state(config, &db, false).unwrap();

        // P0: a flying attacker
        let flying_atk = {
            let id = gs.alloc_id();
            let def = CardDefinition {
                name: "Flying Attacker".into(),
                mana_cost: None,
                type_line: TypeLine {
                    supertypes: vec![],
                    card_types: vec![CardType::Creature],
                    subtypes: vec![],
                },
                oracle_text: String::new(),
                abilities: vec![OracleSpan::Parsed(Ability::Static(StaticAbility::Flying))],
                text_annotations: vec![],
                power: Some(2),
                toughness: Some(2),
                colors: vec![],
            };
            let obj = CardObject::new(id, def, PlayerId(0), Zone::Battlefield);
            let mut perm = PermanentState::new(&obj.definition);
            perm.controller_since_turn = 0;
            gs.battlefield.insert(id, perm);
            gs.add_object(obj);
            id
        };
        // P0: a ground attacker
        let ground_atk = {
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
            id
        };
        // P1: a ground blocker (can block ground_atk but not flying_atk)
        let ground_blk = {
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
            id
        };

        // Navigate to DeclareAttackers and declare both P0 creatures
        for _ in 0..4 {
            gs = dispatch_action(gs, ActionRequest::AdvanceStep).unwrap();
        }
        assert_eq!(gs.step(), Step::DeclareAttackers);
        gs = dispatch_action(
            gs,
            ActionRequest::DeclareAttackers {
                attacker_ids: vec![flying_atk.0, ground_atk.0],
            },
        )
        .unwrap();
        for _ in 0..2 {
            gs = dispatch_action(gs, ActionRequest::AdvanceStep).unwrap();
        }
        assert_eq!(gs.step(), Step::DeclareBlockers);

        let view = build_game_view(&gs);
        let blk_card = view
            .p2
            .creatures
            .iter()
            .find(|c| c.id == ground_blk)
            .unwrap();

        let blocker_targets: Vec<u64> = blk_card
            .actions
            .iter()
            .filter_map(|a| {
                if let ActionItemKind::AssignBlocker { attacker_id, .. } = a.kind {
                    Some(attacker_id)
                } else {
                    None
                }
            })
            .collect();

        assert!(
            blocker_targets.contains(&ground_atk.0),
            "ground blocker should be offered as blocker for ground attacker"
        );
        assert!(
            !blocker_targets.contains(&flying_atk.0),
            "ground blocker must not be offered as blocker for flying attacker"
        );
    }

    #[test]
    fn target_display_name_resolves_each_target_kind() {
        use mecha_oracle::types::stack::{StackObject, StackPayload};

        let config = vec![
            (0..10).map(|_| "Forest".to_string()).collect(),
            (0..10).map(|_| "Forest".to_string()).collect(),
        ];
        let db = test_db();
        let mut gs = build_game_state(config, &db, false).unwrap();

        // Object target: a creature on the battlefield
        let creature_id = gs.alloc_id();
        let creature = CardObject::new(
            creature_id,
            db.get("Grizzly Bears").unwrap().clone(),
            PlayerId(0),
            Zone::Battlefield,
        );
        let perm = PermanentState::new(&creature.definition);
        gs.battlefield.insert(creature_id, perm);
        gs.add_object(creature);

        // StackObject target: a spell already on the stack
        let spell_card_id = gs.alloc_id();
        let spell_card = CardObject::new(
            spell_card_id,
            db.get("Lightning Bolt").unwrap().clone(),
            PlayerId(1),
            Zone::Stack,
        );
        gs.add_object(spell_card);
        let spell_stack_id = gs.alloc_stack_id();
        gs.stack.push(spell_stack_id);
        gs.stack_objects.insert(
            spell_stack_id,
            StackObject {
                id: spell_stack_id,
                payload: StackPayload::Spell {
                    card_id: spell_card_id,
                },
                controller: PlayerId(1),
                targets: vec![],
                x_value: None,
            },
        );

        assert_eq!(
            target_display_name(&gs, &EffectTarget::Object { id: creature_id }),
            "Grizzly Bears"
        );
        assert_eq!(
            target_display_name(&gs, &EffectTarget::Player { id: PlayerId(1) }),
            "Player 2"
        );
        assert_eq!(
            target_display_name(&gs, &EffectTarget::StackObject { id: spell_stack_id }),
            "Lightning Bolt"
        );
    }

    #[test]
    fn stack_item_view_includes_targets_and_source_name_for_ability() {
        use mecha_oracle::types::effect::EffectStep;
        use mecha_oracle::types::stack::{StackObject, StackPayload};

        let config = vec![
            (0..10).map(|_| "Forest".to_string()).collect(),
            (0..10).map(|_| "Forest".to_string()).collect(),
        ];
        let db = test_db();
        let mut gs = build_game_state(config, &db, false).unwrap();

        let source_id = gs.alloc_id();
        let source = CardObject::new(
            source_id,
            db.get("Grizzly Bears").unwrap().clone(),
            PlayerId(0),
            Zone::Battlefield,
        );
        let perm = PermanentState::new(&source.definition);
        gs.battlefield.insert(source_id, perm);
        gs.add_object(source);

        let stack_id = gs.alloc_stack_id();
        let stack_obj = StackObject {
            id: stack_id,
            payload: StackPayload::ActivatedAbility {
                source_id,
                effect: vec![EffectStep::DealDamage(2)],
                label: "Grizzly Bears: activated ability".into(),
            },
            controller: PlayerId(0),
            targets: vec![EffectTarget::Player { id: PlayerId(1) }],
            x_value: None,
        };
        gs.stack.push(stack_id);
        gs.stack_objects.insert(stack_id, stack_obj);

        let view = build_game_view(&gs);
        assert_eq!(view.stack.len(), 1);
        let item = &view.stack[0];
        assert_eq!(item.targets, vec!["Player 2".to_string()]);
        assert_eq!(item.source_name, Some("Grizzly Bears".to_string()));
    }

    #[test]
    fn stack_item_view_includes_source_colors_for_ability() {
        use mecha_oracle::types::effect::EffectStep;
        use mecha_oracle::types::stack::{StackObject, StackPayload};

        let config = vec![
            (0..10).map(|_| "Forest".to_string()).collect(),
            (0..10).map(|_| "Forest".to_string()).collect(),
        ];
        let db = test_db();
        let mut gs = build_game_state(config, &db, false).unwrap();

        let source_id = gs.alloc_id();
        let source = CardObject::new(
            source_id,
            db.get("Grizzly Bears").unwrap().clone(),
            PlayerId(0),
            Zone::Battlefield,
        );
        let perm = PermanentState::new(&source.definition);
        gs.battlefield.insert(source_id, perm);
        gs.add_object(source);

        let stack_id = gs.alloc_stack_id();
        let stack_obj = StackObject {
            id: stack_id,
            payload: StackPayload::ActivatedAbility {
                source_id,
                effect: vec![EffectStep::DealDamage(2)],
                label: "Grizzly Bears: activated ability".into(),
            },
            controller: PlayerId(0),
            targets: vec![],
            x_value: None,
        };
        gs.stack.push(stack_id);
        gs.stack_objects.insert(stack_id, stack_obj);

        let view = build_game_view(&gs);
        let item = &view.stack[0];
        assert_eq!(item.source_colors, vec!["G".to_string()]);
    }

    #[test]
    fn stack_item_view_includes_source_name_for_triggered_ability() {
        use mecha_oracle::types::effect::EffectStep;
        use mecha_oracle::types::stack::{StackObject, StackPayload};

        let config = vec![
            (0..10).map(|_| "Forest".to_string()).collect(),
            (0..10).map(|_| "Forest".to_string()).collect(),
        ];
        let db = test_db();
        let mut gs = build_game_state(config, &db, false).unwrap();

        let source_id = gs.alloc_id();
        let source = CardObject::new(
            source_id,
            db.get("Serra Angel").unwrap().clone(),
            PlayerId(0),
            Zone::Battlefield,
        );
        let perm = PermanentState::new(&source.definition);
        gs.battlefield.insert(source_id, perm);
        gs.add_object(source);

        let stack_id = gs.alloc_stack_id();
        let stack_obj = StackObject {
            id: stack_id,
            payload: StackPayload::TriggeredAbility {
                source_id,
                effect: vec![EffectStep::DealDamage(1)],
                label: "Prowess".into(),
            },
            controller: PlayerId(0),
            targets: vec![],
            x_value: None,
        };
        gs.stack.push(stack_id);
        gs.stack_objects.insert(stack_id, stack_obj);

        let view = build_game_view(&gs);
        let item = &view.stack[0];
        assert!(item.targets.is_empty());
        assert_eq!(item.source_name, Some("Serra Angel".to_string()));
    }

    #[test]
    fn stack_item_view_spell_has_no_source_name() {
        use mecha_oracle::types::stack::{StackObject, StackPayload};

        let config = vec![
            (0..10).map(|_| "Forest".to_string()).collect(),
            (0..10).map(|_| "Forest".to_string()).collect(),
        ];
        let db = test_db();
        let mut gs = build_game_state(config, &db, false).unwrap();

        let card_id = gs.alloc_id();
        let card = CardObject::new(
            card_id,
            db.get("Lightning Bolt").unwrap().clone(),
            PlayerId(0),
            Zone::Stack,
        );
        gs.add_object(card);
        let stack_id = gs.alloc_stack_id();
        let stack_obj = StackObject {
            id: stack_id,
            payload: StackPayload::Spell { card_id },
            controller: PlayerId(0),
            targets: vec![],
            x_value: None,
        };
        gs.stack.push(stack_id);
        gs.stack_objects.insert(stack_id, stack_obj);

        let view = build_game_view(&gs);
        let item = &view.stack[0];
        assert!(item.targets.is_empty());
        assert_eq!(item.source_name, None);
    }

    #[test]
    fn display_colors_uses_printed_colors_when_present() {
        use mecha_oracle::types::card::{CardDefinition, CardType, TypeLine};
        use mecha_oracle::types::mana::ManaColor;
        let def = CardDefinition {
            name: "Test Creature".into(),
            mana_cost: None,
            type_line: TypeLine {
                supertypes: vec![],
                card_types: vec![CardType::Creature],
                subtypes: vec![],
            },
            oracle_text: String::new(),
            abilities: vec![],
            text_annotations: vec![],
            power: None,
            toughness: None,
            colors: vec![ManaColor::Blue],
        };
        assert_eq!(display_colors(&def), vec![ManaColor::Blue]);
    }

    #[test]
    fn display_colors_colorless_nonland_is_empty() {
        use mecha_oracle::types::card::{CardDefinition, CardType, TypeLine};
        let def = CardDefinition {
            name: "Test Artifact".into(),
            mana_cost: None,
            type_line: TypeLine {
                supertypes: vec![],
                card_types: vec![CardType::Artifact],
                subtypes: vec![],
            },
            oracle_text: "Tap: Add {C}.".into(),
            abilities: vec![],
            text_annotations: vec![],
            power: None,
            toughness: None,
            colors: vec![],
        };
        assert_eq!(display_colors(&def), vec![]);
    }

    #[test]
    fn display_colors_land_from_single_basic_subtype() {
        use mecha_oracle::types::card::{CardDefinition, CardType, Supertype, TypeLine};
        use mecha_oracle::types::mana::ManaColor;
        let def = CardDefinition {
            name: "Test Plains".into(),
            mana_cost: None,
            type_line: TypeLine {
                supertypes: vec![Supertype::Basic],
                card_types: vec![CardType::Land],
                subtypes: vec!["Plains".into()],
            },
            oracle_text: "({T}: Add {W}.)".into(),
            abilities: vec![],
            text_annotations: vec![],
            power: None,
            toughness: None,
            colors: vec![],
        };
        assert_eq!(display_colors(&def), vec![ManaColor::White]);
    }

    #[test]
    fn display_colors_land_unions_dual_subtypes_in_wubrg_order() {
        use mecha_oracle::types::card::{CardDefinition, CardType, Supertype, TypeLine};
        use mecha_oracle::types::mana::ManaColor;
        let def = CardDefinition {
            name: "Test Swamp Forest".into(),
            mana_cost: None,
            type_line: TypeLine {
                supertypes: vec![Supertype::Basic],
                card_types: vec![CardType::Land],
                subtypes: vec!["Swamp".into(), "Forest".into()],
            },
            oracle_text: String::new(),
            abilities: vec![],
            text_annotations: vec![],
            power: None,
            toughness: None,
            colors: vec![],
        };
        assert_eq!(
            display_colors(&def),
            vec![ManaColor::Black, ManaColor::Green]
        );
    }

    #[test]
    fn display_colors_land_from_oracle_text_mana_symbol() {
        use mecha_oracle::types::card::{CardDefinition, CardType, TypeLine};
        use mecha_oracle::types::mana::ManaColor;
        let def = CardDefinition {
            name: "Test Utility Land".into(),
            mana_cost: None,
            type_line: TypeLine {
                supertypes: vec![],
                card_types: vec![CardType::Land],
                subtypes: vec!["Gate".into()],
            },
            oracle_text: "{T}: Add {U}.".into(),
            abilities: vec![],
            text_annotations: vec![],
            power: None,
            toughness: None,
            colors: vec![],
        };
        assert_eq!(display_colors(&def), vec![ManaColor::Blue]);
    }

    #[test]
    fn display_colors_land_dedupes_subtype_and_text_match() {
        use mecha_oracle::types::card::{CardDefinition, CardType, Supertype, TypeLine};
        use mecha_oracle::types::mana::ManaColor;
        let def = CardDefinition {
            name: "Test Island".into(),
            mana_cost: None,
            type_line: TypeLine {
                supertypes: vec![Supertype::Basic],
                card_types: vec![CardType::Land],
                subtypes: vec!["Island".into()],
            },
            oracle_text: "({T}: Add {U}.)".into(),
            abilities: vec![],
            text_annotations: vec![],
            power: None,
            toughness: None,
            colors: vec![],
        };
        assert_eq!(display_colors(&def), vec![ManaColor::Blue]);
    }

    #[test]
    fn display_colors_land_with_no_recognized_color_is_empty() {
        use mecha_oracle::types::card::{CardDefinition, CardType, TypeLine};
        let def = CardDefinition {
            name: "Test Wastes".into(),
            mana_cost: None,
            type_line: TypeLine {
                supertypes: vec![],
                card_types: vec![CardType::Land],
                subtypes: vec![],
            },
            oracle_text: "({T}: Add {C}.)".into(),
            abilities: vec![],
            text_annotations: vec![],
            power: None,
            toughness: None,
            colors: vec![],
        };
        assert_eq!(display_colors(&def), vec![]);
    }

    #[test]
    fn build_game_view_land_colors_derived_from_subtype() {
        let config = vec![
            (0..10).map(|_| "Forest".to_string()).collect(),
            (0..10).map(|_| "Forest".to_string()).collect(),
        ];
        let db = test_db();
        let gs = build_game_state(config, &db, false).unwrap();
        let view = build_game_view(&gs);
        let forest = view.p1.hand.iter().find(|c| c.name == "Forest").unwrap();
        assert_eq!(forest.colors, vec!["G".to_string()]);
    }

    #[test]
    fn build_game_view_mana_cost_is_braced() {
        use mecha_oracle::types::{CardObject, Zone};
        let config = vec![
            (0..10).map(|_| "Forest".to_string()).collect(),
            (0..10).map(|_| "Forest".to_string()).collect(),
        ];
        let db = test_db();
        let mut gs = build_game_state(config, &db, false).unwrap();
        let id = gs.alloc_id();
        let obj = CardObject::new(
            id,
            db.get("Grizzly Bears").unwrap().clone(),
            PlayerId(0),
            Zone::Hand,
        );
        gs.hands.get_mut(&PlayerId(0)).unwrap().push(id);
        gs.add_object(obj);

        let view = build_game_view(&gs);
        let bears = view
            .p1
            .hand
            .iter()
            .find(|c| c.name == "Grizzly Bears")
            .unwrap();
        assert_eq!(bears.mana_cost, Some("{1}{G}".to_string()));
    }

    #[test]
    fn cycle_action_label_is_braced() {
        use mecha_oracle::types::card::{CardDefinition, CardType, TypeLine};
        use mecha_oracle::types::mana::{ManaColor, ManaCost, ManaPip};
        use mecha_oracle::types::{Ability, CardObject, OracleSpan, Zone};

        let config = vec![
            (0..10).map(|_| "Forest".to_string()).collect(),
            (0..10).map(|_| "Forest".to_string()).collect(),
        ];
        let db = test_db();
        let mut gs = build_game_state(config, &db, false).unwrap();
        let def = CardDefinition {
            name: "Test Cycler".into(),
            mana_cost: Some(ManaCost {
                pips: vec![ManaPip::Generic(1), ManaPip::Blue],
            }),
            type_line: TypeLine {
                supertypes: vec![],
                card_types: vec![CardType::Creature],
                subtypes: vec![],
            },
            oracle_text: "Cycling {2}".into(),
            abilities: vec![OracleSpan::Parsed(Ability::Cycling(ManaCost {
                pips: vec![ManaPip::Generic(2)],
            }))],
            text_annotations: vec![],
            power: Some(1),
            toughness: Some(1),
            colors: vec![ManaColor::Blue],
        };
        let id = gs.alloc_id();
        let obj = CardObject::new(id, def, PlayerId(0), Zone::Hand);
        gs.hands.get_mut(&PlayerId(0)).unwrap().push(id);
        gs.add_object(obj);

        let view = build_game_view(&gs);
        let card = view
            .p1
            .hand
            .iter()
            .find(|c| c.name == "Test Cycler")
            .unwrap();
        let cycle_action = card
            .actions
            .iter()
            .find(|a| a.label.starts_with("Cycle"))
            .expect("expected a Cycle action");
        assert_eq!(cycle_action.label, "Cycle ({2})");
    }

    #[test]
    fn counter_to_view_plus_modifier() {
        let v = counter_to_view(
            &CounterKind::PtModifier {
                power: 1,
                toughness: 1,
            },
            3,
        );
        assert_eq!(v.label, "+1/+1");
        assert_eq!(v.kind, "plus");
        assert_eq!(v.count, 3);
        assert_eq!(v.sublabel.as_deref(), Some("+3/+3 to P/T"));
    }

    #[test]
    fn counter_to_view_minus_modifier() {
        let v = counter_to_view(
            &CounterKind::PtModifier {
                power: -1,
                toughness: -1,
            },
            2,
        );
        assert_eq!(v.label, "-1/-1");
        assert_eq!(v.kind, "minus");
        assert_eq!(v.count, 2);
        assert_eq!(v.sublabel.as_deref(), Some("-2/-2 to P/T"));
    }

    #[test]
    fn counter_to_view_mixed_modifier() {
        let v = counter_to_view(
            &CounterKind::PtModifier {
                power: 2,
                toughness: -1,
            },
            3,
        );
        assert_eq!(v.label, "+2/-1");
        assert_eq!(v.kind, "mixed");
        assert_eq!(v.sublabel.as_deref(), Some("+6/-3 to P/T"));
    }

    #[test]
    fn counter_to_view_poison() {
        let v = counter_to_view(&CounterKind::Poison, 5);
        assert_eq!(v.label, "Poison");
        assert_eq!(v.kind, "poison");
        assert_eq!(v.count, 5);
        assert!(v.sublabel.is_none());
    }

    #[test]
    fn counter_to_view_named_capitalizes_first_letter() {
        let v = counter_to_view(&CounterKind::Named("charge".to_string()), 4);
        assert_eq!(v.label, "Charge");
        assert_eq!(v.kind, "named");
        assert!(v.sublabel.is_none());
    }

    #[test]
    fn counter_to_view_named_already_capitalized() {
        let v = counter_to_view(&CounterKind::Named("Time".to_string()), 1);
        assert_eq!(v.label, "Time");
        assert_eq!(v.kind, "named");
        assert!(v.sublabel.is_none());
    }

    #[test]
    fn card_view_includes_counters_for_permanent() {
        use mecha_oracle::types::{CardObject, CounterKind, Zone};
        let db = test_db();
        let config = vec![
            (0..10).map(|_| "Forest".to_string()).collect(),
            (0..10).map(|_| "Forest".to_string()).collect(),
        ];
        let mut gs = build_game_state(config, &db, false).unwrap();

        let id = gs.alloc_id();
        let obj = CardObject::new(
            id,
            db.get("Grizzly Bears").unwrap().clone(),
            PlayerId(0),
            Zone::Battlefield,
        );
        let mut perm = PermanentState::new(&obj.definition);
        perm.add_counters(
            CounterKind::PtModifier {
                power: 1,
                toughness: 1,
            },
            3,
        );
        perm.add_counters(CounterKind::Named("charge".to_string()), 2);
        gs.battlefield.insert(id, perm);
        gs.add_object(obj);

        let view = build_game_view(&gs);
        let card = view.p1.creatures.iter().find(|c| c.id == id).unwrap();
        assert_eq!(card.counters.len(), 2);

        let plus = card.counters.iter().find(|c| c.kind == "plus").unwrap();
        assert_eq!(plus.label, "+1/+1");
        assert_eq!(plus.count, 3);
        assert_eq!(plus.sublabel.as_deref(), Some("+3/+3 to P/T"));

        let named = card.counters.iter().find(|c| c.kind == "named").unwrap();
        assert_eq!(named.label, "Charge");
        assert_eq!(named.count, 2);
    }

    #[test]
    fn card_view_empty_counters_when_none() {
        use mecha_oracle::types::{CardObject, Zone};
        let db = test_db();
        let config = vec![
            (0..10).map(|_| "Forest".to_string()).collect(),
            (0..10).map(|_| "Forest".to_string()).collect(),
        ];
        let mut gs = build_game_state(config, &db, false).unwrap();

        let id = gs.alloc_id();
        let obj = CardObject::new(
            id,
            db.get("Grizzly Bears").unwrap().clone(),
            PlayerId(0),
            Zone::Battlefield,
        );
        let perm = PermanentState::new(&obj.definition);
        gs.battlefield.insert(id, perm);
        gs.add_object(obj);

        let view = build_game_view(&gs);
        let card = view.p1.creatures.iter().find(|c| c.id == id).unwrap();
        assert!(card.counters.is_empty());
    }

    #[test]
    fn player_view_includes_poison_counters() {
        use mecha_oracle::types::CounterKind;
        let db = test_db();
        let config = vec![
            (0..10).map(|_| "Forest".to_string()).collect(),
            (0..10).map(|_| "Forest".to_string()).collect(),
        ];
        let mut gs = build_game_state(config, &db, false).unwrap();
        gs.get_player_mut(PlayerId(0))
            .unwrap()
            .add_counters(CounterKind::Poison, 4);

        let view = build_game_view(&gs);
        assert_eq!(view.p1.poison_counters, 4);
        assert_eq!(view.p2.poison_counters, 0);
    }
}
