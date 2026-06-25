/// A single position in the turn sequence. Each variant maps to exactly one valid
/// (phase, step) combination, so invalid combinations are unrepresentable.
///
/// Moved here from game_state.rs so that ability.rs (which game_state.rs imports via Cost)
/// can reference Step without creating a circular import.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Step {
    // Beginning phase
    Untap,
    Upkeep,
    Draw,
    // Main phases — two separate steps instead of a shared `Main`
    PreCombatMain,
    PostCombatMain,
    // Combat phase
    BeginningOfCombat,
    DeclareAttackers,
    DeclareBlockers,
    CombatDamage,
    EndOfCombat,
    // Ending phase
    End,
    Cleanup,
}

impl Step {
    /// Returns the phase that contains this step (CR 500.1).
    pub fn phase(self) -> Phase {
        match self {
            Step::Untap | Step::Upkeep | Step::Draw => Phase::Beginning,
            Step::PreCombatMain => Phase::PreCombatMain,
            Step::BeginningOfCombat
            | Step::DeclareAttackers
            | Step::DeclareBlockers
            | Step::CombatDamage
            | Step::EndOfCombat => Phase::Combat,
            Step::PostCombatMain => Phase::PostCombatMain,
            Step::End | Step::Cleanup => Phase::Ending,
        }
    }
}

/// One of the five phases of a turn (CR 500.1). Derived from `Step::phase()`; not stored directly.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Phase {
    Beginning,
    PreCombatMain,
    Combat,
    PostCombatMain,
    Ending,
}
