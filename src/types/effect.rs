use super::ids::{ObjectId, PlayerId};
use super::zone::Zone;

#[derive(Debug, Clone)]
pub enum EffectTarget {
    Player(PlayerId),
    Object(ObjectId),
}

#[derive(Debug, Clone)]
pub enum Effect {
    DealDamage  { target: EffectTarget, amount: u32 },
    DestroyPermanent { target: ObjectId },
    DrawCard    { player: PlayerId },
    GainLife    { player: PlayerId, amount: u32 },
    MoveToZone  { object: ObjectId, to: Zone },
}
