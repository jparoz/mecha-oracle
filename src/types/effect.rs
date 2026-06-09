use super::ids::{ObjectId, PlayerId};
use super::mana::ManaPool;

/// Retained for the future targeting system (stack project).
#[derive(Debug, Clone, PartialEq, Eq)]
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
    Unimplemented(String), // parsed but not yet executable; skipped at resolution
}

pub type Effect = Vec<EffectStep>;
