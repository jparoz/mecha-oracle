use super::ids::{ObjectId, PlayerId};
use super::mana::ManaPool;

#[derive(Debug, Clone)]
pub enum EffectTarget {
    Player(PlayerId),
    Object(ObjectId),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EffectStep {
    AddMana(ManaPool),
    Mill(u32),
    DrawCard(u32),
    GainLife(u32),
}

pub type Effect = Vec<EffectStep>;
