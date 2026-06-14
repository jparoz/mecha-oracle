use super::ids::{ObjectId, PlayerId};
use super::mana::ManaPool;
use super::permanent::PTDelta;
use super::stack::StackId;

/// A declared target on the stack (CR 115.1).
/// Struct variants for clean Serde round-tripping via the API.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum EffectTarget {
    Player { id: PlayerId },
    Object { id: ObjectId },
    StackObject { id: StackId },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EffectStep {
    AddMana(ManaPool),
    Mill(u32),
    DrawCard(u32),
    GainLife(u32),
    BoostPermanentPT(PTDelta),
    DealDamage(u32),
    CounterSpell,          // CR 701.5: counter the target spell on the stack
    Unimplemented(String), // parsed but not yet executable; skipped at resolution
}

pub type Effect = Vec<EffectStep>;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn effect_target_object_serializes_and_deserializes() {
        let t = EffectTarget::Object { id: ObjectId(42) };
        let json = serde_json::to_string(&t).unwrap();
        assert_eq!(json, r#"{"kind":"object","id":42}"#);
        let round_trip: EffectTarget = serde_json::from_str(&json).unwrap();
        assert_eq!(round_trip, t);
    }

    #[test]
    fn effect_target_player_serializes_and_deserializes() {
        let t = EffectTarget::Player { id: PlayerId(1) };
        let json = serde_json::to_string(&t).unwrap();
        assert_eq!(json, r#"{"kind":"player","id":1}"#);
        let round_trip: EffectTarget = serde_json::from_str(&json).unwrap();
        assert_eq!(round_trip, t);
    }

    #[test]
    fn effect_target_stack_object_serializes_and_deserializes() {
        use crate::types::stack::StackId;
        let t = EffectTarget::StackObject { id: StackId(7) };
        let json = serde_json::to_string(&t).unwrap();
        assert_eq!(json, r#"{"kind":"stack_object","id":7}"#);
        let round_trip: EffectTarget = serde_json::from_str(&json).unwrap();
        assert_eq!(round_trip, t);
    }
}
