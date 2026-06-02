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

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AbilityAST {
    Static(StaticAbility),
    Triggered(TriggeredAbility),
    Activated(ActivatedAbility),
}
