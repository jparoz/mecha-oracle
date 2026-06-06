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
}

/// The event that fires a triggered ability. Phase 2+ adds condition variants.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TriggerEvent;

/// An ability that triggers on a game event. Phase 2+ adds trigger + effect fields.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TriggeredAbility {
    pub trigger: TriggerEvent,
}

/// An ability paid for with a cost. Phase 2+ adds cost + effect fields.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ActivatedAbility;

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
}
