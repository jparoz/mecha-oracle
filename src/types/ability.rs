use super::effect::Effect;
use super::mana::ManaCost;

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
    pub fn display_name(&self) -> &'static str {
        match self {
            Self::Flying => "Flying",
            Self::Reach => "Reach",
            Self::Trample => "Trample",
            Self::FirstStrike => "First strike",
            Self::DoubleStrike => "Double strike",
            Self::Vigilance => "Vigilance",
            Self::Haste => "Haste",
            Self::Lifelink => "Lifelink",
            Self::Deathtouch => "Deathtouch",
            Self::Menace => "Menace",
            Self::Indestructible => "Indestructible",
            Self::Defender => "Defender",
            Self::Shadow => "Shadow",
            Self::Horsemanship => "Horsemanship",
            Self::Skulk => "Skulk",
            Self::Decayed => "Decayed",
        }
    }
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

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AbilityAST {
    Static(StaticAbility),
    Triggered(TriggeredAbility),
    Activated(ActivatedAbility),
}

/// A typed span of oracle text.
/// The ordered sequence of spans represents the full oracle text.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum OracleSpan {
    /// A recognised ability the engine can act on.
    Parsed(AbilityAST),
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
        let a = OracleSpan::Parsed(AbilityAST::Static(StaticAbility::Flying));
        let b = OracleSpan::Parsed(AbilityAST::Static(StaticAbility::Flying));
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
}
