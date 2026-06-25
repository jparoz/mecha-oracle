use super::card::CardType;
use super::counter::CounterKind;
use super::effect::Effect;
use super::ids::{ObjectId, PlayerId};
use super::mana::{ManaColor, ManaCost};
use super::permanent::PTDelta;
use super::step::Step;

/// The subtype or category of land that triggers a Landwalk ability (CR 702.14).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LandwalkKind {
    LandType(String), // e.g. "Island", "Swamp", "Forest", "Mountain", "Plains"
    Nonbasic,
}

/// The quality that a Protection or HexproofFrom ability applies to (CR 702.16a).
/// `source_matches_quality` tests whether a source satisfies this quality.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ProtectionQuality {
    Color(ManaColor),
    CardType(CardType),
    CreatureType(String), // creature subtype, e.g. "Eldrazi", "Vampire"
    Everything,           // CR 702.16j
}

impl ProtectionQuality {
    fn quality_name(&self) -> String {
        match self {
            Self::Color(c) => match c {
                ManaColor::White => "white".to_string(),
                ManaColor::Blue => "blue".to_string(),
                ManaColor::Black => "black".to_string(),
                ManaColor::Red => "red".to_string(),
                ManaColor::Green => "green".to_string(),
                ManaColor::Colorless => "colorless".to_string(),
            },
            Self::CardType(ct) => match ct {
                CardType::Artifact => "artifacts".to_string(),
                CardType::Creature => "creatures".to_string(),
                CardType::Instant => "instants".to_string(),
                CardType::Sorcery => "sorceries".to_string(),
                CardType::Enchantment => "enchantments".to_string(),
                CardType::Land => "lands".to_string(),
                CardType::Planeswalker => "planeswalkers".to_string(),
            },
            Self::CreatureType(s) => s.clone(),
            Self::Everything => "everything".to_string(),
        }
    }
}

/// Returns true if a source with the given characteristics matches `quality` (CR 702.16a).
/// Used by targeting (`is_legal_target`) and combat (`can_block_attacker`, damage prevention).
pub fn source_matches_quality(
    quality: &ProtectionQuality,
    source_colors: &[ManaColor],
    source_card_types: &[CardType],
    source_subtypes: &[String],
) -> bool {
    match quality {
        ProtectionQuality::Color(c) => source_colors.contains(c),
        ProtectionQuality::CardType(ct) => source_card_types.contains(ct),
        ProtectionQuality::CreatureType(st) => {
            source_subtypes.iter().any(|s| s.eq_ignore_ascii_case(st))
        }
        ProtectionQuality::Everything => true,
    }
}

/// A keyword ability (CR 702) that a card has as a static rule.
/// Parameterised variants carry the N value (Bushido N) or quality (ProtectionFrom).
/// All variants are stored as `RulesText::Active(Rule::Static(kw))` in `CardDefinition.rules_text`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum KeywordAbility {
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
    Shroud,                            // CR 702.18
    Hexproof,                          // CR 702.11
    Landwalk(LandwalkKind),            // CR 702.14
    BattleCry,                         // CR 702.91
    Fear,                              // CR 702.36
    Intimidate,                        // CR 702.13
    ProtectionFrom(ProtectionQuality), // CR 702.16
    HexproofFrom(ProtectionQuality),   // CR 702.11d
    Wither,                            // CR 702.80
    Infect,                            // CR 702.90
    ToxicN(u32),                       // CR 702.164
    Evolve,                            // CR 702.100
    Training,                          // CR 702.149
    Persist,                           // CR 702.79
    Undying,                           // CR 702.93
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
    SubjectLacksKeyword(KeywordAbility),        // CR 702.25b Flanking
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

/// A pattern stored in `TriggeredAbility.trigger` that is matched against runtime `GameEvent`s.
/// Each variant mirrors the corresponding `GameEvent` variant but carries filter parameters
/// instead of concrete ids. The engine's `triggered.rs` module does the matching.
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

/// A triggered ability (CR 113.3b) as stored in `RulesText::Active(Rule::Triggered(...))`.
/// The engine collects matching abilities when a `GameEvent` fires and pushes them onto the stack.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TriggeredAbility {
    pub trigger: TriggerEvent,
    pub condition: Option<TriggerCondition>,
    pub target_mode: TriggerTargetMode,
    pub effect: Effect,
}

/// An activated ability (CR 113.3a) stored in `RulesText::Active(Rule::Activated(...))`.
/// Players activate these via `engine::activated::activate_ability`. Mana abilities
/// (those whose effect contains `AddMana`) bypass the stack per CR 605.3.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ActivatedAbility {
    pub cost: Cost,
    pub target_requirements: Vec<TargetFilter>,
    pub effect: Effect,
}

/// The cost of an activated ability or spell (CR 117). A list of cost components that
/// must all be paid simultaneously (CR 601.2g). Used in `ActivatedAbility.cost` and `PendingPayment`.
pub type Cost = Vec<CostComponent>;

// EffectStep and Effect are defined in effect.rs and re-exported via types/mod.rs.

/// One component of a cost (CR 117.1). All components in a `Cost` vec must be paid.
///
/// `Tap`: the engine marks the permanent tapped before calling `pay_cost_components`.
/// `Sacrifice`/`Discard`: parsed but not yet executed in `pay_cost_components` — handled by callers.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CostComponent {
    Tap,
    Mana(ManaCost),
    PayLife(u32),
    Sacrifice(u32, PermanentFilter),
    Discard(u32, CardFilter),
    Unimplemented(String),
}

/// Controller constraint for continuous-effect and permanent-filter subject filters (CR 611.3b).
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub enum ControllerFilter {
    #[default]
    Any,
    You,
    Opponent,
}

// CR 611.3b: describes which permanents on the battlefield a continuous effect applies to.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PermanentFilter {
    pub controller: ControllerFilter,
    pub card_types: Vec<CardType>,
    pub subtypes: Vec<String>,
    pub colors: Vec<ManaColor>,
    /// If non-empty, only match permanents whose ObjectId is in this list.
    /// Used for effects targeting a specific object. Empty = no ID constraint.
    pub object_ids: Vec<ObjectId>,
}

impl Default for PermanentFilter {
    fn default() -> Self {
        Self {
            controller: ControllerFilter::Any,
            card_types: vec![],
            subtypes: vec![],
            colors: vec![],
            object_ids: vec![],
        }
    }
}

/// A continuous effect that modifies P/T of matching permanents (CR 611.1, CR 611.3b).
/// Sources: `Rule::Continuous` (global anthems), `Rule::Aura.grants`, `Rule::Equip.grants`.
/// Evaluated on-the-fly by `engine::continuous_pt_bonus`; not stored in `GameState`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ContinuousEffect {
    pub subject_filter: PermanentFilter,
    /// P/T modification applied to each matching permanent. None reserved for future non-PT effects.
    pub pt_modification: Option<PTDelta>,
}

/// Placeholder filter for card-in-hand discard costs. Currently has no fields — future expansion
/// may add card-type or property constraints to match "discard a land", etc.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CardFilter;

/// Records how a spell was cast — stored on `StackObject` and read at resolution time.
// Standard: paid normal mana cost.
// Kicked: paid mana cost + Kicker cost (702.33a).
// Multikicked(n): paid mana cost + n × Multikicker cost (702.33c); n ≥ 1.
// Dashed: paid Dash alternative cost instead of mana cost (702.109a).
// Evoked: paid Evoke alternative cost instead of mana cost (702.74a).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CastMode {
    #[default]
    Standard,
    Kicked,
    Multikicked(u32),
    Dashed,
    Evoked,
}

impl KeywordAbility {
    /// Returns the canonical display name of this keyword ability (e.g. "First strike", "Bushido 2").
    /// Used in the serve.rs UI to label ability chips and tooltips.
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
            Self::ProtectionFrom(q) => format!("Protection from {}", q.quality_name()),
            Self::HexproofFrom(q) => format!("Hexproof from {}", q.quality_name()),
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
    /// Returns a filter that matches any spell (e.g. Extort — no restriction).
    pub fn any() -> Self {
        Self::default()
    }

    /// Returns a filter that matches only noncreature spells (e.g. Prowess).
    pub fn noncreature() -> Self {
        Self {
            excluded_card_types: vec![CardType::Creature],
        }
    }

    /// Returns true if a spell with the given card types satisfies this filter.
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
    /// Returns a filter that matches any spell on the stack.
    pub fn any() -> Self {
        Self::default()
    }

    /// Returns a filter that matches only noncreature spells.
    pub fn noncreature() -> Self {
        Self {
            excluded_types: vec![CardType::Creature],
            ..Default::default()
        }
    }

    /// Returns a filter that matches only creature spells.
    pub fn creature() -> Self {
        Self {
            included_types: vec![CardType::Creature],
            ..Default::default()
        }
    }

    /// Returns a filter that matches instants and sorceries.
    pub fn instant_or_sorcery() -> Self {
        Self {
            included_types: vec![CardType::Instant, CardType::Sorcery],
            ..Default::default()
        }
    }

    /// Returns true if a spell with the given types, mana value, and colors satisfies all constraints.
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

/// The master enum of all classifiable rules text a card can have.
/// Each variant maps to a specific CR section and governs how the engine handles the card.
/// Stored as `RulesText::Active(Rule::...)` in `CardDefinition.rules_text`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Rule {
    Static(KeywordAbility),
    Triggered(TriggeredAbility),
    Activated(ActivatedAbility),
    SpellAbility(SpellAbility),
    Cycling(ManaCost),
    Continuous(ContinuousEffect), // CR 611.3b
    // CR 303.4: an Aura enchants the object matching `enchant`.
    // `enchant` is the target requirement at cast time and for SBA legality checks.
    // `grants` is applied to the attached permanent while on the battlefield.
    Aura {
        enchant: TargetFilter,
        grants: ContinuousEffect,
    },
    // CR 301.5: an Equipment with an Equip activated ability.
    // `cost` is paid at sorcery speed to attach/re-attach.
    // `grants` is applied to the equipped creature.
    Equip {
        cost: Cost,
        grants: ContinuousEffect,
    },
    // (702.33a) Optional additional cost; pays mana_cost + additional_cost.
    Kicker {
        additional_cost: ManaCost,
    },
    // (702.33c) Repeatable additional cost; pays mana_cost + n × additional_cost, n ≥ 1.
    Multikicker {
        additional_cost: ManaCost,
    },
    // (702.109a) Alternative cost that replaces mana_cost; grants Haste; returns to hand at end step.
    Dash {
        alternative_cost: ManaCost,
    },
    // (702.74a) Alternative cost that replaces mana_cost; ETB trigger sacrifices the permanent.
    Evoke {
        alternative_cost: ManaCost,
    },
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
        let a = RulesText::Active(Rule::Static(KeywordAbility::Flying));
        let b = RulesText::Active(Rule::Static(KeywordAbility::Flying));
        assert_eq!(a, b);

        let c = RulesText::Ignored(IgnoredKind::ReminderText, "(reminder)".into());
        let d = RulesText::Ignored(IgnoredKind::ReminderText, "(reminder)".into());
        assert_eq!(c, d);

        let e = RulesText::Unparsed("When this enters".into());
        assert_ne!(a, e);
    }

    #[test]
    fn display_name_canonical_casing() {
        assert_eq!(KeywordAbility::Flying.display_name(), "Flying");
        assert_eq!(KeywordAbility::FirstStrike.display_name(), "First strike");
        assert_eq!(KeywordAbility::DoubleStrike.display_name(), "Double strike");
        assert_eq!(
            KeywordAbility::Indestructible.display_name(),
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
        assert_eq!(KeywordAbility::Exalted.display_name(), "Exalted");
        assert_eq!(KeywordAbility::Flanking.display_name(), "Flanking");
        assert_eq!(KeywordAbility::BushidoN(2).display_name(), "Bushido 2");
        assert_eq!(KeywordAbility::Melee.display_name(), "Melee");
        assert_eq!(KeywordAbility::Prowess.display_name(), "Prowess");
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
        assert_eq!(KeywordAbility::Fear.display_name(), "Fear");
        assert_eq!(KeywordAbility::Intimidate.display_name(), "Intimidate");
        assert_eq!(KeywordAbility::BattleCry.display_name(), "Battle cry");
        assert_eq!(
            KeywordAbility::Landwalk(LandwalkKind::LandType("Island".to_string())).display_name(),
            "Islandwalk"
        );
        assert_eq!(
            KeywordAbility::Landwalk(LandwalkKind::Nonbasic).display_name(),
            "Nonbasic landwalk"
        );
    }

    #[test]
    fn protection_from_color_display_name_uses_quality() {
        use crate::types::mana::ManaColor;
        assert_eq!(
            KeywordAbility::ProtectionFrom(ProtectionQuality::Color(ManaColor::Blue))
                .display_name(),
            "Protection from blue"
        );
    }

    #[test]
    fn protection_from_artifact_display_name() {
        use crate::types::card::CardType;
        assert_eq!(
            KeywordAbility::ProtectionFrom(ProtectionQuality::CardType(CardType::Artifact))
                .display_name(),
            "Protection from artifacts"
        );
    }

    #[test]
    fn protection_from_everything_display_name() {
        assert_eq!(
            KeywordAbility::ProtectionFrom(ProtectionQuality::Everything).display_name(),
            "Protection from everything"
        );
    }

    #[test]
    fn hexproof_from_color_display_name() {
        use crate::types::mana::ManaColor;
        assert_eq!(
            KeywordAbility::HexproofFrom(ProtectionQuality::Color(ManaColor::Black)).display_name(),
            "Hexproof from black"
        );
    }

    #[test]
    fn source_matches_quality_color() {
        use crate::types::mana::ManaColor;
        let q = ProtectionQuality::Color(ManaColor::Blue);
        assert!(source_matches_quality(&q, &[ManaColor::Blue], &[], &[]));
        assert!(!source_matches_quality(&q, &[ManaColor::Red], &[], &[]));
        assert!(!source_matches_quality(&q, &[], &[], &[]));
    }

    #[test]
    fn source_matches_quality_card_type() {
        use crate::types::card::CardType;
        let q = ProtectionQuality::CardType(CardType::Artifact);
        assert!(source_matches_quality(&q, &[], &[CardType::Artifact], &[]));
        assert!(!source_matches_quality(&q, &[], &[CardType::Creature], &[]));
    }

    #[test]
    fn source_matches_quality_creature_type() {
        let q = ProtectionQuality::CreatureType("Vampire".into());
        assert!(source_matches_quality(
            &q,
            &[],
            &[],
            &["Vampire".to_string()]
        ));
        assert!(source_matches_quality(
            &q,
            &[],
            &[],
            &["vampire".to_string()]
        ));
        assert!(!source_matches_quality(
            &q,
            &[],
            &[],
            &["Zombie".to_string()]
        ));
    }

    #[test]
    fn source_matches_quality_everything() {
        let q = ProtectionQuality::Everything;
        assert!(source_matches_quality(&q, &[], &[], &[]));
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
        assert_eq!(KeywordAbility::Wither.display_name(), "Wither");
        assert_eq!(KeywordAbility::Infect.display_name(), "Infect");
        assert_eq!(KeywordAbility::ToxicN(2).display_name(), "Toxic 2");
        assert_eq!(KeywordAbility::Evolve.display_name(), "Evolve");
        assert_eq!(KeywordAbility::Training.display_name(), "Training");
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
        assert_eq!(KeywordAbility::Persist.display_name(), "Persist");
        assert_eq!(KeywordAbility::Undying.display_name(), "Undying");
    }

    #[test]
    fn subject_lacks_counter_construction() {
        let cond = TriggerCondition::SubjectLacksCounter(crate::types::CounterKind::PtModifier {
            power: -1,
            toughness: -1,
        });
        assert!(matches!(cond, TriggerCondition::SubjectLacksCounter(_)));
    }

    #[test]
    fn controller_filter_default_is_any() {
        use super::{ControllerFilter, PermanentFilter};
        let f = PermanentFilter::default();
        assert!(matches!(f.controller, ControllerFilter::Any));
        assert!(f.card_types.is_empty());
        assert!(f.subtypes.is_empty());
        assert!(f.colors.is_empty());
    }

    #[test]
    fn permanent_filter_default_has_empty_object_ids() {
        let f = PermanentFilter::default();
        assert!(f.object_ids.is_empty());
    }

    #[test]
    fn rule_aura_construction() {
        use crate::types::permanent::PTDelta;
        let rule = Rule::Aura {
            enchant: TargetFilter::Creature,
            grants: ContinuousEffect {
                subject_filter: PermanentFilter::default(),
                pt_modification: Some(PTDelta {
                    power: 2,
                    toughness: 1,
                }),
            },
        };
        assert!(matches!(rule, Rule::Aura { .. }));
    }

    #[test]
    fn rule_equip_construction() {
        use crate::types::mana::{ManaCost, ManaPip};
        use crate::types::permanent::PTDelta;
        let rule = Rule::Equip {
            cost: vec![CostComponent::Mana(ManaCost {
                pips: vec![ManaPip::Generic(1)],
            })],
            grants: ContinuousEffect {
                subject_filter: PermanentFilter::default(),
                pt_modification: Some(PTDelta {
                    power: 2,
                    toughness: 0,
                }),
            },
        };
        assert!(matches!(rule, Rule::Equip { .. }));
    }

    #[test]
    fn continuous_effect_roundtrips_through_rule() {
        use super::{ContinuousEffect, ControllerFilter, PermanentFilter, Rule, RulesText};
        use crate::types::{card::CardType, permanent::PTDelta};
        let effect = ContinuousEffect {
            subject_filter: PermanentFilter {
                controller: ControllerFilter::You,
                card_types: vec![CardType::Creature],
                ..Default::default()
            },
            pt_modification: Some(PTDelta {
                power: 1,
                toughness: 1,
            }),
        };
        let span = RulesText::Active(Rule::Continuous(effect));
        assert!(matches!(span, RulesText::Active(Rule::Continuous(_))));
    }
}
