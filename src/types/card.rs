use super::ability::{RulesText, TextAnnotation};
use super::mana::{ManaColor, ManaCost};

/// Card supertypes as defined in CR 205.4 (Basic, Legendary, Snow, World).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Supertype {
    Basic,
    Legendary,
    Snow,
    World,
}

/// The card types that appear on the type line (CR 205.2).
/// Planeswalkers and Battles are present but many engine paths do not yet handle them.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CardType {
    Creature,
    Land,
    Instant,
    Sorcery,
    Artifact,
    Enchantment,
    Planeswalker,
}

/// The parsed type line of a card (CR 205): supertypes, card types, and subtypes (CR 205.1–205.3).
/// Multi-type cards (e.g. "Artifact Creature — Golem") will have multiple entries in `card_types`.
#[derive(Debug, Clone)]
pub struct TypeLine {
    pub supertypes: Vec<Supertype>,
    pub card_types: Vec<CardType>,
    pub subtypes: Vec<String>,
}

impl TypeLine {
    /// Returns true if the card has the Creature card type (CR 302.1).
    pub fn is_creature(&self) -> bool {
        self.card_types.contains(&CardType::Creature)
    }

    /// Returns true if the card has the Land card type (CR 305.1).
    pub fn is_land(&self) -> bool {
        self.card_types.contains(&CardType::Land)
    }

    /// Returns true if the card is a permanent type (CR 110.1): Creature, Land, Artifact,
    /// Enchantment, or Planeswalker. Instants and Sorceries are not permanents.
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
    pub rules_text: Vec<RulesText>,
    pub text_annotations: Vec<TextAnnotation>,
    pub power: Option<i32>,
    pub toughness: Option<i32>,
    pub colors: Vec<ManaColor>, // CR 105.2 — authoritative card color from Scryfall
}

impl CardDefinition {
    /// Returns true if the oracle text contains at least one `RulesText::Unparsed` entry —
    /// i.e. the parser encountered text it could not classify. Used at load time to count
    /// partially-parsed cards in the card database.
    pub fn has_unparsed(&self) -> bool {
        self.rules_text
            .iter()
            .any(|s| matches!(s, RulesText::Unparsed(_)))
    }
}
