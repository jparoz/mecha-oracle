pub mod activated;
pub mod casting;
pub mod combat;
pub mod cycling;
pub mod mana;
pub mod stack;
pub mod state_based_actions;
pub mod targeting;
pub mod triggered;
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
    NoManaCheckpoint,
    AbilityIndexOutOfRange,
    InvalidPaymentPlan,
    NotYourPriority,
    WrongNumberOfTargets, // CR 601.2c: wrong number of targets declared
    IllegalTarget,        // CR 601.2c: declared target is not a legal target
}
