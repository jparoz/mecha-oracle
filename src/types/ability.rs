use super::card::CardType;
use super::effect::Effect;
use super::mana::{ManaColor, ManaCost};

// CR 702.14
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LandwalkKind {
    LandType(String), // e.g. "Island", "Swamp", "Forest", "Mountain", "Plains"
    Nonbasic,
}

// CR 702.21
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WardCost {
    Mana(ManaCost),
    Life(u32),
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
    WardMana(ManaCost),             // CR 702.21 — Ward {cost}
    WardLife(u32),                  // CR 702.21 — Ward—Pay N life
    Landwalk(LandwalkKind),         // CR 702.14
    BattleCry,                      // CR 702.91
    Fear,                           // CR 702.36
    Intimidate,                     // CR 702.13
    ProtectionFromColor(ManaColor), // CR 702.16 (partial — blocking + targeting only)
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TriggerEvent {
    EntersTheBattlefield { subject_is_self: bool },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TriggeredAbility {
    pub trigger: TriggerEvent,
    pub effect: Effect,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ActivatedAbility {
    pub cost: ActivationCost,
    pub target_requirements: Vec<TargetFilter>,
    pub effect: Effect,
}

pub type ActivationCost = Vec<CostComponent>;

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
            Self::WardMana(cost) => format!("Ward {cost}"),
            Self::WardLife(n) => format!("Ward\u{2014}Pay {n} life"),
            Self::Landwalk(LandwalkKind::LandType(t)) => format!("{t}walk"),
            Self::Landwalk(LandwalkKind::Nonbasic) => "Nonbasic landwalk".to_string(),
            Self::BattleCry => "Battle cry".to_string(),
            Self::Fear => "Fear".to_string(),
            Self::Intimidate => "Intimidate".to_string(),
            Self::ProtectionFromColor(c) => {
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

/// Describes what kind of permanent or player can be targeted (CR 115.4).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TargetFilter {
    Creature,
    Player,
    Any, // CR 115.4: creature, player, planeswalker, battle
}

/// A spell ability — the text of an instant or sorcery that takes effect when it resolves.
/// Wraps effect steps and any targeting requirements.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SpellAbility {
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
pub enum Ability {
    Static(StaticAbility),
    Triggered(TriggeredAbility),
    Activated(ActivatedAbility),
    SpellEffect(SpellAbility),
    Cycling(ManaCost),
}

/// A typed span of oracle text.
/// The ordered sequence of spans represents the full oracle text.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum OracleSpan {
    /// A recognised ability the engine can act on.
    Parsed(Ability),
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
    fn oracle_span_variants_are_comparable() {
        let a = OracleSpan::Parsed(Ability::Static(StaticAbility::Flying));
        let b = OracleSpan::Parsed(Ability::Static(StaticAbility::Flying));
        assert_eq!(a, b);

        let c = OracleSpan::Ignored(IgnoredKind::ReminderText, "(reminder)".into());
        let d = OracleSpan::Ignored(IgnoredKind::ReminderText, "(reminder)".into());
        assert_eq!(c, d);

        let e = OracleSpan::Unparsed("When this enters".into());
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
        use crate::types::mana::{ManaColor, ManaCost, ManaPip};
        assert_eq!(StaticAbility::Fear.display_name(), "Fear");
        assert_eq!(StaticAbility::Intimidate.display_name(), "Intimidate");
        assert_eq!(StaticAbility::BattleCry.display_name(), "Battle cry");
        assert_eq!(
            StaticAbility::WardMana(ManaCost {
                pips: vec![ManaPip::Generic(2)]
            })
            .display_name(),
            "Ward {2}"
        );
        assert_eq!(
            StaticAbility::WardLife(2).display_name(),
            "Ward\u{2014}Pay 2 life"
        );
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
}
