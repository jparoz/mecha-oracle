use super::ability::{Ability, OracleSpan, StaticAbility};
use super::card::CardDefinition;
use super::ids::{ObjectId, PlayerId};
use super::zone::Zone;

/// A card object in the game — a unique instance distinct from its definition.
/// Multiple copies of "Grizzly Bears" each have their own ObjectId.
/// Battlefield-specific state (tapped, summoning sickness, damage, P/T modifiers) lives
/// on PermanentState, which exists only for cards currently on the battlefield.
#[derive(Debug, Clone)]
pub struct CardObject {
    pub id: ObjectId,
    pub definition: CardDefinition,
    pub controller: PlayerId,
    pub owner: PlayerId,
    pub zone: Zone,
}

impl CardObject {
    pub fn new(id: ObjectId, definition: CardDefinition, owner: PlayerId, zone: Zone) -> Self {
        Self {
            id,
            definition,
            controller: owner,
            owner,
            zone,
        }
    }

    pub fn is_creature(&self) -> bool {
        self.definition.type_line.is_creature()
    }

    pub fn is_land(&self) -> bool {
        self.definition.type_line.is_land()
    }

    pub fn has_keyword(&self, kw: StaticAbility) -> bool {
        self.definition
            .abilities
            .iter()
            .any(|span| matches!(span, OracleSpan::Parsed(Ability::Static(k)) if *k == kw))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cards::test_helpers::test_db;

    fn grizzly_bears() -> super::super::card::CardDefinition {
        test_db().get("Grizzly Bears").unwrap().clone()
    }

    #[test]
    fn has_keyword_returns_true_for_matching_ability() {
        use crate::types::{Ability, OracleSpan, ability::StaticAbility};
        let mut def = grizzly_bears();
        def.abilities = vec![OracleSpan::Parsed(Ability::Static(StaticAbility::Flying))];
        let obj = CardObject::new(ObjectId(1), def, PlayerId(0), Zone::Battlefield);
        assert!(obj.has_keyword(StaticAbility::Flying));
        assert!(!obj.has_keyword(StaticAbility::Trample));
    }

    #[test]
    fn is_creature_and_is_land_are_correct() {
        let obj = CardObject::new(ObjectId(1), grizzly_bears(), PlayerId(0), Zone::Hand);
        assert!(obj.is_creature());
        assert!(!obj.is_land());
    }
}
