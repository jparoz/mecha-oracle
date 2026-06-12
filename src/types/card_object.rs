use super::ability::{Ability, OracleSpan, StaticAbility};
use super::card::CardDefinition;
use super::ids::{ObjectId, PlayerId};
use super::zone::Zone;

/// CR 305.6: lands with basic land subtypes (Forest, Island, Mountain, Plains, Swamp)
/// get intrinsic mana abilities.
fn inject_intrinsic_abilities(definition: &mut CardDefinition) {
    use super::ability::{ActivatedAbility, CostComponent};
    use super::effect::EffectStep;
    use super::mana::ManaPool;

    // CR 305.6: each basic land subtype grants a {T}: Add {X} mana ability.
    for subtype in &definition.type_line.subtypes {
        let pool = match subtype.as_str() {
            "Forest" => ManaPool {
                green: 1,
                ..Default::default()
            },
            "Island" => ManaPool {
                blue: 1,
                ..Default::default()
            },
            "Mountain" => ManaPool {
                red: 1,
                ..Default::default()
            },
            "Plains" => ManaPool {
                white: 1,
                ..Default::default()
            },
            "Swamp" => ManaPool {
                black: 1,
                ..Default::default()
            },
            _ => continue,
        };
        definition
            .abilities
            .push(OracleSpan::Parsed(Ability::Activated(ActivatedAbility {
                cost: vec![CostComponent::Tap],
                target_requirements: vec![],
                effect: vec![EffectStep::AddMana(pool)],
            })));
    }
}

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
    pub fn new(id: ObjectId, mut definition: CardDefinition, owner: PlayerId, zone: Zone) -> Self {
        inject_intrinsic_abilities(&mut definition);
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

    #[test]
    fn basic_land_gets_intrinsic_mana_ability() {
        use crate::cards::test_helpers::test_db;
        use crate::types::ability::{Ability, CostComponent, OracleSpan};
        use crate::types::effect::EffectStep;

        let db = test_db();
        let forest_def = db.get("Forest").unwrap().clone();
        let obj = CardObject::new(ObjectId(1), forest_def, PlayerId(0), Zone::Hand);

        let mana_abilities: Vec<_> = obj
            .definition
            .abilities
            .iter()
            .filter_map(|span| match span {
                OracleSpan::Parsed(Ability::Activated(a)) => Some(a),
                _ => None,
            })
            .collect();

        assert_eq!(
            mana_abilities.len(),
            1,
            "Forest should have exactly one activated ability"
        );
        assert!(
            mana_abilities[0].cost.contains(&CostComponent::Tap),
            "cost should contain {{T}}"
        );
        assert!(
            matches!(&mana_abilities[0].effect[0], EffectStep::AddMana(p) if p.green == 1),
            "effect should add one green mana"
        );
    }

    #[test]
    fn mountain_gets_intrinsic_tap_for_red_mana() {
        use crate::cards::test_helpers::test_db;
        use crate::types::ability::{Ability, OracleSpan};
        use crate::types::effect::EffectStep;

        let db = test_db();
        let mountain_def = db.get("Mountain").unwrap().clone();
        let obj = CardObject::new(ObjectId(1), mountain_def, PlayerId(0), Zone::Hand);

        let mana_abilities: Vec<_> = obj
            .definition
            .abilities
            .iter()
            .filter_map(|span| match span {
                OracleSpan::Parsed(Ability::Activated(a)) => Some(a),
                _ => None,
            })
            .collect();

        assert_eq!(mana_abilities.len(), 1);
        assert!(matches!(&mana_abilities[0].effect[0], EffectStep::AddMana(p) if p.red == 1),);
    }

    #[test]
    fn non_land_gets_no_intrinsic_ability() {
        use crate::cards::test_helpers::test_db;
        use crate::types::ability::{Ability, OracleSpan};

        let db = test_db();
        let obj = CardObject::new(
            ObjectId(1),
            db.get("Grizzly Bears").unwrap().clone(),
            PlayerId(0),
            Zone::Hand,
        );

        let mana_abilities = obj
            .definition
            .abilities
            .iter()
            .filter(|span| matches!(span, OracleSpan::Parsed(Ability::Activated(_))))
            .count();

        assert_eq!(mana_abilities, 0);
    }
}
