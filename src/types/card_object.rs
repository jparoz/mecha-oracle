use super::ability::{AbilityAST, OracleSpan, StaticAbility};
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
    /// True if this creature has been dealt damage by a source with deathtouch
    /// since the last time state-based actions were checked (CR 704.5h).
    pub damaged_by_deathtouch: bool,
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
            damaged_by_deathtouch: false,
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

    /// Returns true if this object has the given static keyword ability in its parsed AST.
    pub fn has_keyword(&self, kw: StaticAbility) -> bool {
        self.definition
            .abilities
            .iter()
            .any(|span| matches!(span, OracleSpan::Parsed(AbilityAST::Static(k)) if *k == kw))
    }

    pub fn can_attack(&self) -> bool {
        self.is_creature()
            && self.zone == Zone::Battlefield
            && !self.tapped
            && !self.has_keyword(StaticAbility::Defender)
            && (!self.summoning_sick || self.has_keyword(StaticAbility::Haste))
    }

    pub fn can_block(&self) -> bool {
        self.is_creature()
            && self.zone == Zone::Battlefield
            && !self.tapped
            && !self.has_keyword(StaticAbility::Decayed)
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
    fn has_keyword_returns_true_for_matching_ability() {
        use crate::types::{AbilityAST, OracleSpan, ability::StaticAbility};
        let mut def = grizzly_bears();
        def.abilities = vec![OracleSpan::Parsed(AbilityAST::Static(
            StaticAbility::Flying,
        ))];
        let obj = CardObject::new(ObjectId(1), def, PlayerId(0), Zone::Battlefield);
        assert!(obj.has_keyword(StaticAbility::Flying));
        assert!(!obj.has_keyword(StaticAbility::Trample));
    }

    #[test]
    fn summoning_sick_creature_with_haste_can_attack() {
        use crate::types::{AbilityAST, OracleSpan, ability::StaticAbility};
        let mut def = grizzly_bears();
        def.abilities = vec![OracleSpan::Parsed(AbilityAST::Static(StaticAbility::Haste))];
        let mut obj = CardObject::new(ObjectId(1), def, PlayerId(0), Zone::Battlefield);
        obj.summoning_sick = true;
        assert!(obj.can_attack()); // haste bypasses summoning sickness
    }

    #[test]
    fn damaged_by_deathtouch_initialises_false() {
        let obj = CardObject::new(ObjectId(1), grizzly_bears(), PlayerId(0), Zone::Battlefield);
        assert!(!obj.damaged_by_deathtouch);
    }
}
