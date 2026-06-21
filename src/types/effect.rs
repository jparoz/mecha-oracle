use super::ability::Cost;
use super::counter::CounterKind;
use super::ids::{ObjectId, PlayerId};
use super::mana::ManaPool;
use super::permanent::PTDelta;
use super::stack::StackId;
use super::zone::{Zone, ZoneOwner};

/// A declared target on the stack (CR 115.1).
/// Struct variants for clean Serde round-tripping via the API.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum EffectTarget {
    Player { id: PlayerId },
    Object { id: ObjectId },
    StackObject { id: StackId },
}

/// CR 702.15b, 702.2b, 702.80a, 702.90b/c — source keyword flags snapshotted at
/// stack-push time. All flags default to false; the parser always produces flag-less
/// steps and inject_source_flags fills them in at runtime.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct DamageStep {
    pub amount: u32,
    pub lifelink: bool,
    pub deathtouch: bool,
    pub wither: bool,
    pub infect: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EffectStep {
    AddMana(ManaPool),
    Mill(u32),
    DrawCard(u32),
    GainLife(u32),
    BoostPermanentPT(PTDelta),
    /// CR 122.6: put one or more counters of a given kind onto the target.
    AddCounter {
        kind: CounterKind,
        count: u32,
    },
    /// Move a card object between zones (CR 400.7).
    /// `from` is the expected current zone; step is a no-op if the object is not there.
    /// `to_player` determines who controls a permanent entering the battlefield, or whose
    /// hand/library/graveyard receives the card for player-specific destination zones.
    MoveZone {
        from: Zone,
        to: Zone,
        to_player: ZoneOwner,
    },
    DealDamage(DamageStep),
    CounterSpell, // CR 701.5: counter the target spell on the stack
    /// CR 118.12: inline cost-payment obligation raised during resolution.
    /// Pauses effect resolution; `pay_pending_cost`/`decline_pending_cost` resume it.
    Payment {
        cost: Cost,
        on_paid: Effect,
        on_declined: Effect,
    },
    Unimplemented(String), // parsed but not yet executable; skipped at resolution
}

pub type Effect = Vec<EffectStep>;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn payment_step_construction() {
        use crate::types::ability::CostComponent;
        use crate::types::mana::{ManaCost, ManaPip};
        let step = EffectStep::Payment {
            cost: vec![CostComponent::Mana(ManaCost {
                pips: vec![ManaPip::Generic(3)],
            })],
            on_paid: vec![],
            on_declined: vec![EffectStep::CounterSpell],
        };
        assert!(matches!(step, EffectStep::Payment { .. }));
    }

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

    #[test]
    fn move_zone_step_construction() {
        use crate::types::zone::{Zone, ZoneOwner};
        let step = EffectStep::MoveZone {
            from: Zone::Graveyard,
            to: Zone::Battlefield,
            to_player: ZoneOwner::CardOwner,
        };
        assert!(matches!(step, EffectStep::MoveZone { .. }));
    }
}
