use super::ability::OracleSpan;
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
    pub abilities: Vec<OracleSpan>,
    pub power: Option<i32>,
    pub toughness: Option<i32>,
}

impl CardDefinition {
    pub fn has_unparsed(&self) -> bool {
        self.abilities
            .iter()
            .any(|s| matches!(s, OracleSpan::Unparsed(_)))
    }
}
