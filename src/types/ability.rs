use super::card::CardType;
use super::counter::CounterKind;
use super::effect::Effect;
use super::ids::{ObjectId, PlayerId};
use super::mana::{ManaColor, ManaCost};
use super::step::Step;

// CR 702.14
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LandwalkKind {
    LandType(String), // e.g. "Island", "Swamp", "Forest", "Mountain", "Plains"
    Nonbasic,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StaticAbility {
    Flying,
    Reach,
    Trample,
    FirstStrike,
    DoubleStrike,
    Vigilance,
    Haste,
    Lifelink,
    Deathtouch,
    Menace,
    Indestructible,
    Defender,
    Shadow,
    Horsemanship,
    Skulk,
    Decayed,
    Flash,
    Exalted,
    Flanking,
    BushidoN(u32),
    Melee,
    Prowess,
    Shroud,                         // CR 702.18
    Hexproof,                       // CR 702.11
    Landwalk(LandwalkKind),         // CR 702.14
    BattleCry,                      // CR 702.91
    Fear,                           // CR 702.36
    Intimidate,                     // CR 702.13
    ProtectionFromColor(ManaColor), // CR 702.16 (partial — blocking + targeting only)
    Wither,                         // CR 702.80
    Infect,                         // CR 702.90
    ToxicN(u32),                    // CR 702.164
    Evolve,                         // CR 702.100
    Training,                       // CR 702.149
    Persist,                        // CR 702.79
    Undying,                        // CR 702.93
}

/// CR 109.5: who "you" refers to in a triggered ability — the controller of the source
/// at the time the ability triggered.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TurnOwner {
    You,
    Opponent,
    Any,
}

/// Open-ended filter describing which objects satisfy a trigger event's subject requirement.
/// All fields are optional; the empty filter (all defaults) matches any object.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct TriggerSubjectFilter {
    /// Some(true) = subject must be the source card itself; Some(false) = must not be self; None = any.
    pub is_self: Option<bool>,
    /// Restrict by controller relative to the trigger source's controller. None = any.
    pub controller: Option<TurnOwner>,
    /// Subject must have at least one of these card types. Empty = no constraint.
    pub card_types: Vec<CardType>,
    /// Subject must have all of these subtypes. Empty = no constraint.
    pub subtypes: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DamageTargetKind {
    Player,
    Creature,
    Any,
}

/// Controls how targets are populated on the StackObject at trigger dispatch time.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub enum TriggerTargetMode {
    /// No targets — effect resolves without targeting (DrawCard, GainLife, Payment).
    #[default]
    None,
    /// Target the source permanent itself (Prowess, Training, Melee, Bushido).
    Source,
    /// Target the object that triggered the event — the subject (Exalted, Flanking).
    Subject,
    /// Target all current attackers except the source (Battle Cry).
    AllOtherAttackers,
}

/// Closed enum of game-state predicates checked at trigger time.
/// Each variant corresponds to a condition that cannot be expressed by TriggerSubjectFilter alone.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TriggerCondition {
    ExactlyOneAttacker,                         // CR 702.83b Exalted
    AttackingAlongsideGreaterPowerCreature,     // CR 702.149a Training
    EnteringCreatureHasGreaterPower,            // CR 702.100a Evolve (power)
    EnteringCreatureHasGreaterToughness,        // CR 702.100a Evolve (toughness)
    EnteringCreatureHasGreaterPowerOrToughness, // CR 702.100a Evolve (either)
    SubjectLacksKeyword(StaticAbility),         // CR 702.25b Flanking
    SubjectLacksCounter(CounterKind),           // CR 702.79 Persist / CR 702.93 Undying
}

/// Concrete runtime event data fired by the engine at each trigger point.
/// Distinct from TriggerEvent (which carries filter patterns); GameEvent carries IDs and values.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum GameEvent {
    EntersTheBattlefield {
        subject_id: ObjectId,
    },
    Dies {
        subject_id: ObjectId,
    },
    LeavesBattlefield {
        subject_id: ObjectId,
    },
    Attacks {
        subject_id: ObjectId,
    },
    Blocks {
        subject_id: ObjectId,
    },
    BecomesBlocked {
        subject_id: ObjectId,
    },
    DealsCombatDamage {
        subject_id: ObjectId,
        to: DamageTargetKind,
    },
    SpellCast {
        caster: PlayerId,
        spell_id: ObjectId,
    },
    // CR 603.2b: fired at the beginning of each phase/step so that "at the beginning of upkeep"
    // triggers can be collected.
    PhaseStep {
        step: Step,
        active_player: PlayerId,
    },
    DrawsCard {
        player: PlayerId,
    },
    TargetedBy {
        target_id: ObjectId,
        acting_player: PlayerId,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TriggerEvent {
    // Zone changes
    EntersTheBattlefield {
        subject: TriggerSubjectFilter,
    },
    Dies {
        subject: TriggerSubjectFilter,
    },
    LeavesBattlefield {
        subject: TriggerSubjectFilter,
    },

    // Combat
    Attacks {
        subject: TriggerSubjectFilter,
    },
    Blocks {
        subject: TriggerSubjectFilter,
    },
    BecomesBlocked {
        subject: TriggerSubjectFilter,
    },
    DealsCombatDamage {
        subject: TriggerSubjectFilter,
        to: DamageTargetKind,
    },

    // Cast
    SpellCast {
        caster: TurnOwner,
        filter: SpellFilter,
    },

    // Phase/step
    // CR 603.2b: "at the beginning of [step]" triggers.
    PhaseStep {
        step: Step,
        whose_turn: TurnOwner,
    },

    // Draw
    DrawsCard {
        who: TurnOwner,
    },

    // Targeting (CR 702.21: Ward)
    TargetedBy {
        controller: TurnOwner,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TriggeredAbility {
    pub trigger: TriggerEvent,
    pub condition: Option<TriggerCondition>,
    pub target_mode: TriggerTargetMode,
    pub effect: Effect,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ActivatedAbility {
    pub cost: Cost,
    pub target_requirements: Vec<TargetFilter>,
    pub effect: Effect,
}

pub type Cost = Vec<CostComponent>;

// EffectStep and Effect are defined in effect.rs and re-exported via types/mod.rs.

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CostComponent {
    Tap,
    Mana(ManaCost),
    PayLife(u32),
    Sacrifice(u32, PermanentFilter),
    Discard(u32, CardFilter),
    Unimplemented(String),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PermanentFilter;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CardFilter;

impl StaticAbility {
    pub fn display_name(&self) -> String {
        match self {
            Self::Flying => "Flying".to_string(),
            Self::Reach => "Reach".to_string(),
            Self::Trample => "Trample".to_string(),
            Self::FirstStrike => "First strike".to_string(),
            Self::DoubleStrike => "Double strike".to_string(),
            Self::Vigilance => "Vigilance".to_string(),
            Self::Haste => "Haste".to_string(),
            Self::Lifelink => "Lifelink".to_string(),
            Self::Deathtouch => "Deathtouch".to_string(),
            Self::Menace => "Menace".to_string(),
            Self::Indestructible => "Indestructible".to_string(),
            Self::Defender => "Defender".to_string(),
            Self::Shadow => "Shadow".to_string(),
            Self::Horsemanship => "Horsemanship".to_string(),
            Self::Skulk => "Skulk".to_string(),
            Self::Decayed => "Decayed".to_string(),
            Self::Flash => "Flash".to_string(),
            Self::Exalted => "Exalted".to_string(),
            Self::Flanking => "Flanking".to_string(),
            Self::BushidoN(n) => format!("Bushido {n}"),
            Self::Melee => "Melee".to_string(),
            Self::Prowess => "Prowess".to_string(),
            Self::Shroud => "Shroud".to_string(),
            Self::Hexproof => "Hexproof".to_string(),
            Self::Landwalk(LandwalkKind::LandType(t)) => format!("{t}walk"),
            Self::Landwalk(LandwalkKind::Nonbasic) => "Nonbasic landwalk".to_string(),
            Self::BattleCry => "Battle cry".to_string(),
            Self::Fear => "Fear".to_string(),
            Self::Intimidate => "Intimidate".to_string(),
            Self::ProtectionFromColor(c) => {
                // CR 105.4: colorless is not a color — ProtectionFromColor(Colorless) should never be constructed
                debug_assert!(
                    *c != ManaColor::Colorless,
                    "ProtectionFromColor: Colorless is not a valid color (CR 105.4)"
                );
                let color_name = match c {
                    ManaColor::White => "white",
                    ManaColor::Blue => "blue",
                    ManaColor::Black => "black",
                    ManaColor::Red => "red",
                    ManaColor::Green => "green",
                    ManaColor::Colorless => "colorless",
                };
                format!("Protection from {color_name}")
            }
            Self::Wither => "Wither".to_string(),
            Self::Infect => "Infect".to_string(),
            Self::ToxicN(n) => format!("Toxic {n}"),
            Self::Evolve => "Evolve".to_string(),
            Self::Training => "Training".to_string(),
            Self::Persist => "Persist".to_string(),
            Self::Undying => "Undying".to_string(),
        }
    }
}

/// Describes which cast spells activate "whenever you cast a spell" triggers.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct CastFilter {
    /// The spell must not have any of these card types for the trigger to fire.
    pub excluded_card_types: Vec<CardType>,
}

impl CastFilter {
    /// Matches any spell (e.g. Extort — no restriction).
    pub fn any() -> Self {
        Self::default()
    }

    /// Matches only noncreature spells (e.g. Prowess).
    pub fn noncreature() -> Self {
        Self {
            excluded_card_types: vec![CardType::Creature],
        }
    }

    pub fn matches(&self, card_types: &[CardType]) -> bool {
        self.excluded_card_types
            .iter()
            .all(|t| !card_types.contains(t))
    }
}

/// Describes which spells on the stack can be targeted (CR 115.4).
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct SpellFilter {
    /// Spell must have at least one of these types; empty = no constraint.
    pub included_types: Vec<CardType>,
    /// Spell must have none of these types.
    pub excluded_types: Vec<CardType>,
    /// CR 202.3: spell MV must be ≥ this; None = no constraint.
    pub min_mana_value: Option<u32>,
    /// Spell MV must be ≤ this; None = no constraint.
    pub max_mana_value: Option<u32>,
    /// Spell must share ≥1 color with this list; empty = no constraint.
    pub any_of_colors: Vec<ManaColor>,
}

impl SpellFilter {
    pub fn any() -> Self {
        Self::default()
    }

    pub fn noncreature() -> Self {
        Self {
            excluded_types: vec![CardType::Creature],
            ..Default::default()
        }
    }

    pub fn creature() -> Self {
        Self {
            included_types: vec![CardType::Creature],
            ..Default::default()
        }
    }

    pub fn instant_or_sorcery() -> Self {
        Self {
            included_types: vec![CardType::Instant, CardType::Sorcery],
            ..Default::default()
        }
    }

    pub fn matches(&self, card_types: &[CardType], mana_value: u32, colors: &[ManaColor]) -> bool {
        let included_ok = self.included_types.is_empty()
            || self.included_types.iter().any(|t| card_types.contains(t));
        let excluded_ok = self.excluded_types.iter().all(|t| !card_types.contains(t));
        let min_ok = self.min_mana_value.is_none_or(|n| mana_value >= n);
        let max_ok = self.max_mana_value.is_none_or(|n| mana_value <= n);
        let color_ok =
            self.any_of_colors.is_empty() || self.any_of_colors.iter().any(|c| colors.contains(c));
        included_ok && excluded_ok && min_ok && max_ok && color_ok
    }
}

/// Describes what kind of permanent, player, or spell can be targeted (CR 115.4).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TargetFilter {
    Creature,
    Player,
    Any,                // CR 115.4: creature, player, planeswalker, battle
    Spell(SpellFilter), // CR 115.4: a spell on the stack
}

/// The resolving text of an instant or sorcery (CR 113.3a).
/// Wraps effect steps and any targeting requirements.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SpellEffect {
    pub target_requirements: Vec<TargetFilter>, // empty for untargeted spells
    pub steps: Effect,
}

/// Classifies oracle text that has no rules effect and is rendered in italics.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "snake_case")]
pub enum IgnoredKind {
    /// Parenthetical reminder text, e.g. "(This creature can't block.)".
    ReminderText,
    /// Ability words (CR 207.2c) and flavour words (CR 207.2d) that precede an em-dash,
    /// e.g. "Landfall — " or "Cumulative upkeep— ".
    AbilityWord,
}

/// Describes the visual style to apply to an annotated range of oracle text in the UI.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "snake_case")]
pub enum AnnotationKind {
    ReminderText,
    AbilityWord,
    ParsedUnimplemented,
    Unparsed,
}

/// A styled byte-range annotation over a `CardDefinition`'s `oracle_text` field.
/// `start` and `end` are byte offsets (UTF-8) into `oracle_text`, exclusive of `end`.
/// Annotations produced by the parser are non-overlapping and in source order.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TextAnnotation {
    pub start: usize,
    pub end: usize,
    pub kind: AnnotationKind,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Rule {
    Static(StaticAbility),
    Triggered(TriggeredAbility),
    Activated(ActivatedAbility),
    SpellEffect(SpellEffect),
    Cycling(ManaCost),
}

/// A classified entry in a card's rules text (CR 207.1).
/// The ordered sequence of entries represents the full oracle text.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RulesText {
    /// A rule the engine actively enforces.
    Active(Rule),
    /// Non-rules text — displayed in italics in the UI.
    Ignored(IgnoredKind, String),
    /// Text the parser could not interpret — displayed red+underline in the UI.
    Unparsed(String),
    /// A CR 702 keyword the parser recognises by name but the engine does not yet enforce.
    /// Displayed cyan+underline in the UI.
    ParsedUnimplemented(String),
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rules_text_variants_are_comparable() {
        let a = RulesText::Active(Rule::Static(StaticAbility::Flying));
        let b = RulesText::Active(Rule::Static(StaticAbility::Flying));
        assert_eq!(a, b);

        let c = RulesText::Ignored(IgnoredKind::ReminderText, "(reminder)".into());
        let d = RulesText::Ignored(IgnoredKind::ReminderText, "(reminder)".into());
        assert_eq!(c, d);

        let e = RulesText::Unparsed("When this enters".into());
        assert_ne!(a, e);
    }

    #[test]
    fn display_name_canonical_casing() {
        assert_eq!(StaticAbility::Flying.display_name(), "Flying");
        assert_eq!(StaticAbility::FirstStrike.display_name(), "First strike");
        assert_eq!(StaticAbility::DoubleStrike.display_name(), "Double strike");
        assert_eq!(
            StaticAbility::Indestructible.display_name(),
            "Indestructible"
        );
    }

    #[test]
    fn activated_ability_construction() {
        use super::super::mana::ManaPool;
        use crate::types::effect::EffectStep;
        let ability = ActivatedAbility {
            cost: vec![CostComponent::Tap],
            target_requirements: vec![],
            effect: vec![EffectStep::AddMana(ManaPool {
                green: 1,
                ..Default::default()
            })],
        };
        assert_eq!(ability.cost.len(), 1);
        assert_eq!(ability.effect.len(), 1);
        assert!(matches!(ability.cost[0], CostComponent::Tap));
    }

    #[test]
    fn cost_component_unimplemented_round_trips() {
        let c = CostComponent::Unimplemented("Sacrifice a creature".to_string());
        assert!(matches!(c, CostComponent::Unimplemented(_)));
    }

    #[test]
    fn display_name_new_keywords() {
        assert_eq!(StaticAbility::Exalted.display_name(), "Exalted");
        assert_eq!(StaticAbility::Flanking.display_name(), "Flanking");
        assert_eq!(StaticAbility::BushidoN(2).display_name(), "Bushido 2");
        assert_eq!(StaticAbility::Melee.display_name(), "Melee");
        assert_eq!(StaticAbility::Prowess.display_name(), "Prowess");
    }

    #[test]
    fn annotation_kind_serialises_to_snake_case() {
        assert_eq!(
            serde_json::to_string(&AnnotationKind::ReminderText).unwrap(),
            r#""reminder_text""#
        );
        assert_eq!(
            serde_json::to_string(&AnnotationKind::ParsedUnimplemented).unwrap(),
            r#""parsed_unimplemented""#
        );
    }

    #[test]
    fn text_annotation_construction() {
        let ann = TextAnnotation {
            start: 3,
            end: 10,
            kind: AnnotationKind::Unparsed,
        };
        assert_eq!(ann.start, 3);
        assert_eq!(ann.end, 10);
        assert_eq!(ann.kind, AnnotationKind::Unparsed);
    }

    #[test]
    fn new_static_ability_display_names() {
        use crate::types::mana::ManaColor;
        assert_eq!(StaticAbility::Fear.display_name(), "Fear");
        assert_eq!(StaticAbility::Intimidate.display_name(), "Intimidate");
        assert_eq!(StaticAbility::BattleCry.display_name(), "Battle cry");
        assert_eq!(
            StaticAbility::Landwalk(LandwalkKind::LandType("Island".to_string())).display_name(),
            "Islandwalk"
        );
        assert_eq!(
            StaticAbility::Landwalk(LandwalkKind::Nonbasic).display_name(),
            "Nonbasic landwalk"
        );
        assert_eq!(
            StaticAbility::ProtectionFromColor(ManaColor::Blue).display_name(),
            "Protection from blue"
        );
    }

    #[test]
    fn spell_filter_any_matches_all_types() {
        let f = SpellFilter::any();
        assert!(f.matches(&[CardType::Creature], 0, &[]));
        assert!(f.matches(&[CardType::Instant], 0, &[]));
        assert!(f.matches(&[CardType::Sorcery], 0, &[]));
        assert!(f.matches(&[], 0, &[]));
    }

    #[test]
    fn spell_filter_noncreature_excludes_creature_spells() {
        let f = SpellFilter::noncreature();
        assert!(!f.matches(&[CardType::Creature], 0, &[]));
        assert!(f.matches(&[CardType::Instant], 0, &[]));
        assert!(f.matches(&[CardType::Sorcery], 0, &[]));
        assert!(!f.matches(&[CardType::Creature, CardType::Artifact], 0, &[]));
    }

    #[test]
    fn spell_filter_creature_includes_creature_only() {
        let f = SpellFilter::creature();
        assert!(f.matches(&[CardType::Creature], 0, &[]));
        assert!(!f.matches(&[CardType::Instant], 0, &[]));
        assert!(!f.matches(&[CardType::Sorcery], 0, &[]));
    }

    #[test]
    fn spell_filter_instant_or_sorcery_matches_either() {
        let f = SpellFilter::instant_or_sorcery();
        assert!(f.matches(&[CardType::Instant], 0, &[]));
        assert!(f.matches(&[CardType::Sorcery], 0, &[]));
        assert!(!f.matches(&[CardType::Creature], 0, &[]));
        assert!(!f.matches(&[], 0, &[]));
    }

    #[test]
    fn spell_filter_min_mana_value_accepts_at_or_above() {
        let f = SpellFilter {
            min_mana_value: Some(4),
            ..SpellFilter::default()
        };
        assert!(f.matches(&[], 4, &[]));
        assert!(f.matches(&[], 5, &[]));
        assert!(!f.matches(&[], 3, &[]));
    }

    #[test]
    fn spell_filter_max_mana_value_accepts_at_or_below() {
        let f = SpellFilter {
            max_mana_value: Some(2),
            ..SpellFilter::default()
        };
        assert!(f.matches(&[], 0, &[]));
        assert!(f.matches(&[], 2, &[]));
        assert!(!f.matches(&[], 3, &[]));
    }

    #[test]
    fn spell_filter_any_of_colors_must_match_at_least_one() {
        use crate::types::mana::ManaColor;
        let f = SpellFilter {
            any_of_colors: vec![ManaColor::Red, ManaColor::Green],
            ..SpellFilter::default()
        };
        assert!(f.matches(&[], 0, &[ManaColor::Red]));
        assert!(f.matches(&[], 0, &[ManaColor::Green]));
        assert!(!f.matches(&[], 0, &[ManaColor::Blue]));
        assert!(!f.matches(&[], 0, &[]));
    }

    #[test]
    fn spell_filter_combined_mv_and_color() {
        use crate::types::mana::ManaColor;
        let f = SpellFilter {
            any_of_colors: vec![ManaColor::Blue],
            min_mana_value: Some(3),
            ..SpellFilter::default()
        };
        assert!(f.matches(&[], 3, &[ManaColor::Blue]));
        assert!(!f.matches(&[], 2, &[ManaColor::Blue])); // MV too low
        assert!(!f.matches(&[], 3, &[ManaColor::Red])); // wrong color
    }

    #[test]
    fn cost_type_alias_is_vec_cost_component() {
        let c: Cost = vec![CostComponent::Tap];
        assert_eq!(c.len(), 1);
    }

    #[test]
    fn display_name_counter_keywords() {
        assert_eq!(StaticAbility::Wither.display_name(), "Wither");
        assert_eq!(StaticAbility::Infect.display_name(), "Infect");
        assert_eq!(StaticAbility::ToxicN(2).display_name(), "Toxic 2");
        assert_eq!(StaticAbility::Evolve.display_name(), "Evolve");
        assert_eq!(StaticAbility::Training.display_name(), "Training");
    }

    #[test]
    fn trigger_subject_filter_is_self_matches_same_id() {
        let filter = TriggerSubjectFilter {
            is_self: Some(true),
            ..Default::default()
        };
        // is_self matching is a structural check only — tested via subject_filter_matches in triggered.rs
        assert_eq!(filter.is_self, Some(true));
        assert!(filter.card_types.is_empty());
    }

    #[test]
    fn trigger_subject_filter_default_is_empty() {
        let filter = TriggerSubjectFilter::default();
        assert!(filter.is_self.is_none());
        assert!(filter.controller.is_none());
        assert!(filter.card_types.is_empty());
        assert!(filter.subtypes.is_empty());
    }

    #[test]
    fn trigger_target_mode_default_is_none() {
        assert_eq!(TriggerTargetMode::default(), TriggerTargetMode::None);
    }

    #[test]
    fn display_name_persist_undying() {
        assert_eq!(StaticAbility::Persist.display_name(), "Persist");
        assert_eq!(StaticAbility::Undying.display_name(), "Undying");
    }

    #[test]
    fn subject_lacks_counter_construction() {
        let cond = TriggerCondition::SubjectLacksCounter(crate::types::CounterKind::PtModifier {
            power: -1,
            toughness: -1,
        });
        assert!(matches!(cond, TriggerCondition::SubjectLacksCounter(_)));
    }
}
