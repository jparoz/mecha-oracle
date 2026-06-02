/// A continuous effect (e.g. Flying, Trample). Phase 2+ adds keyword variants.
#[derive(Debug, Clone)]
pub struct StaticAbility;

/// The event that fires a triggered ability. Phase 2+ adds condition variants.
#[derive(Debug, Clone)]
pub struct TriggerEvent;

/// An ability that triggers on a game event. Phase 2+ adds trigger + effect fields.
#[derive(Debug, Clone)]
pub struct TriggeredAbility {
    pub trigger: TriggerEvent,
}

/// An ability paid for with a cost. Phase 2+ adds cost + effect fields.
#[derive(Debug, Clone)]
pub struct ActivatedAbility;

#[derive(Debug, Clone)]
pub enum AbilityAST {
    Static(StaticAbility),
    Triggered(TriggeredAbility),
    Activated(ActivatedAbility),
}
