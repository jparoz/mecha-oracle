use super::ability::WardCost;
use super::effect::Effect;
use super::ids::{ObjectId, PlayerId};

/// A unique identifier for a stack object (CR 405).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct StackId(pub u64);

/// The content of a stack object, which can be a spell or an ability (CR 405.4).
#[derive(Debug, Clone)]
pub enum StackPayload {
    /// A spell on the stack. The card_id points to an entry in GameState.objects with zone = Zone::Stack.
    Spell { card_id: ObjectId },
    /// A triggered ability on the stack (CR 405.4).
    TriggeredAbility {
        source_id: ObjectId,
        effect: Effect,
        label: String,
    },
    /// An activated ability on the stack (CR 405.4).
    ActivatedAbility {
        source_id: ObjectId,
        effect: Effect,
        label: String,
    },
    /// CR 702.21b: triggered by targeting a permanent with Ward.
    /// Counters the triggering spell/ability if the Ward cost is not paid.
    WardTrigger {
        counters_if_unpaid: StackId,
        cost: WardCost,
        paid: bool,
    },
}

/// An object on the stack (CR 405).
/// The stack is a zone where spells and non-mana abilities wait to resolve.
#[derive(Debug, Clone)]
pub struct StackObject {
    pub id: StackId,
    pub payload: StackPayload,
    pub controller: PlayerId,
    pub targets: Vec<super::effect::EffectTarget>, // declared targets (CR 115.1)
}
