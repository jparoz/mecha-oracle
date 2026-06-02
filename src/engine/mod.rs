pub mod casting;
pub mod combat;
pub mod mana;
pub mod state_based_actions;
pub mod turn;

#[derive(Debug, Clone, PartialEq)]
pub enum EngineError {
    CardNotFound,
    CardNotInHand,
    CardNotOnBattlefield,
    AlreadyTapped,
    InsufficientMana,
    CannotCastNow,
    LandLimitReached,
    NotALand,
    NotACreature,
    NotYourCard,
    SummoningSick,
    CreatureTapped,
    InvalidBlocker,            // blocker can't legally block this attacker
    MenaceRequiresTwoBlockers, // menace attacker has exactly one blocker
}
