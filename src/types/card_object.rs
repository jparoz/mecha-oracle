use super::card::CardDefinition;
use super::ids::{ObjectId, PlayerId};
use super::zone::Zone;

/// A card object in the game — a unique instance distinct from its definition.
/// Multiple copies of "Grizzly Bears" each have their own ObjectId.
#[derive(Debug, Clone)]
pub struct CardObject {
    pub id: ObjectId,
    pub definition: CardDefinition,
    /// Current P/T, which may diverge from printed P/T once effects exist.
    pub current_power: Option<i32>,
    pub current_toughness: Option<i32>,
    pub damage_marked: u32,
    pub controller: PlayerId,
    pub owner: PlayerId,
    pub zone: Zone,
    pub tapped: bool,
    /// True until the controller's next untap step (CR 302.6).
    pub summoning_sick: bool,
}

impl CardObject {
    pub fn new(id: ObjectId, definition: CardDefinition, owner: PlayerId, zone: Zone) -> Self {
        let power = definition.power;
        let toughness = definition.toughness;
        Self {
            id,
            definition,
            current_power: power,
            current_toughness: toughness,
            damage_marked: 0,
            controller: owner,
            owner,
            zone,
            tapped: false,
            summoning_sick: true,
        }
    }

    pub fn is_creature(&self) -> bool {
        self.definition.type_line.is_creature()
    }
    pub fn is_land(&self) -> bool {
        self.definition.type_line.is_land()
    }

    pub fn effective_power(&self) -> Option<i32> {
        self.current_power
    }
    pub fn effective_toughness(&self) -> Option<i32> {
        self.current_toughness
    }

    /// Engine checks abilities from the AST before applying base rules.
    /// Phase 1: always returns false — no abilities parsed yet.
    /// Phase 2+: match ability nodes in self.definition.abilities.
    pub fn has_ability(&self, _query: &str) -> bool {
        false
    }

    pub fn can_attack(&self) -> bool {
        self.is_creature() && self.zone == Zone::Battlefield && !self.tapped && !self.summoning_sick
    }

    pub fn can_block(&self) -> bool {
        self.is_creature() && self.zone == Zone::Battlefield && !self.tapped
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
    fn new_creature_enters_summoning_sick() {
        let obj = CardObject::new(ObjectId(1), grizzly_bears(), PlayerId(0), Zone::Battlefield);
        assert!(obj.summoning_sick);
        assert!(!obj.can_attack());
    }

    #[test]
    fn creature_can_attack_after_sickness_cleared() {
        let mut obj = CardObject::new(ObjectId(1), grizzly_bears(), PlayerId(0), Zone::Battlefield);
        obj.summoning_sick = false;
        assert!(obj.can_attack());
    }

    #[test]
    fn tapped_creature_cannot_attack_or_block() {
        let mut obj = CardObject::new(ObjectId(1), grizzly_bears(), PlayerId(0), Zone::Battlefield);
        obj.summoning_sick = false;
        obj.tapped = true;
        assert!(!obj.can_attack());
        assert!(!obj.can_block());
    }

    #[test]
    fn has_ability_always_false_in_phase_1() {
        let obj = CardObject::new(ObjectId(1), grizzly_bears(), PlayerId(0), Zone::Battlefield);
        assert!(!obj.has_ability("flying"));
    }
}
