use super::ability::AbilityAST;
use super::mana::ManaCost;

#[derive(Debug, Clone, PartialEq)]
pub enum Supertype {
    Basic,
    Legendary,
    Snow,
    World,
}

#[derive(Debug, Clone, PartialEq)]
pub enum CardType {
    Creature,
    Land,
    Instant,
    Sorcery,
    Artifact,
    Enchantment,
    Planeswalker,
}

#[derive(Debug, Clone)]
pub struct TypeLine {
    pub supertypes: Vec<Supertype>,
    pub card_types: Vec<CardType>,
    pub subtypes: Vec<String>,
}

impl TypeLine {
    pub fn is_creature(&self) -> bool {
        self.card_types.contains(&CardType::Creature)
    }

    pub fn is_land(&self) -> bool {
        self.card_types.contains(&CardType::Land)
    }

    pub fn is_permanent(&self) -> bool {
        self.card_types.iter().any(|t| {
            matches!(
                t,
                CardType::Creature
                    | CardType::Land
                    | CardType::Artifact
                    | CardType::Enchantment
                    | CardType::Planeswalker
            )
        })
    }
}

/// The static Oracle data for a card — shared across all copies.
#[derive(Debug, Clone)]
pub struct CardDefinition {
    pub name: String,
    pub mana_cost: Option<ManaCost>,
    pub type_line: TypeLine,
    pub oracle_text: String,
    pub abilities: Vec<AbilityAST>,
    pub power: Option<i32>,
    pub toughness: Option<i32>,
}

impl CardDefinition {
    pub fn grizzly_bears() -> Self {
        Self {
            name: "Grizzly Bears".into(),
            mana_cost: Some(ManaCost {
                generic: 1,
                green: 1,
                ..Default::default()
            }),
            type_line: TypeLine {
                supertypes: vec![],
                card_types: vec![CardType::Creature],
                subtypes: vec!["Bear".into()],
            },
            oracle_text: String::new(),
            abilities: vec![],
            power: Some(2),
            toughness: Some(2),
        }
    }

    pub fn hill_giant() -> Self {
        Self {
            name: "Hill Giant".into(),
            mana_cost: Some(ManaCost {
                generic: 2,
                green: 1,
                ..Default::default()
            }),
            type_line: TypeLine {
                supertypes: vec![],
                card_types: vec![CardType::Creature],
                subtypes: vec!["Giant".into()],
            },
            oracle_text: String::new(),
            abilities: vec![],
            power: Some(3),
            toughness: Some(3),
        }
    }

    pub fn forest() -> Self {
        Self {
            name: "Forest".into(),
            mana_cost: None,
            type_line: TypeLine {
                supertypes: vec![Supertype::Basic],
                card_types: vec![CardType::Land],
                subtypes: vec!["Forest".into()],
            },
            oracle_text: String::new(),
            abilities: vec![],
            power: None,
            toughness: None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn grizzly_bears_is_creature() {
        let card = CardDefinition::grizzly_bears();
        assert!(card.type_line.is_creature());
        assert!(!card.type_line.is_land());
        assert_eq!(card.power, Some(2));
        assert_eq!(card.toughness, Some(2));
        assert!(card.abilities.is_empty());
    }

    #[test]
    fn forest_is_land_not_creature() {
        let card = CardDefinition::forest();
        assert!(card.type_line.is_land());
        assert!(!card.type_line.is_creature());
        assert!(card.mana_cost.is_none());
    }

    #[test]
    fn grizzly_bears_cmc_is_two() {
        let cost = CardDefinition::grizzly_bears().mana_cost.unwrap();
        assert_eq!(cost.converted_mana_cost(), 2);
    }
}
