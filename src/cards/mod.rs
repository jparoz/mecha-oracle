mod downloader;
mod scryfall;

pub use downloader::update_cards;
use scryfall::ParsedEntry;

use crate::types::card::CardDefinition;
use std::collections::HashMap;
use std::path::Path;

pub struct CardDatabase {
    inner: HashMap<String, CardDefinition>,
    tokens: HashMap<String, CardDefinition>,
}

impl CardDatabase {
    /// Load from the platform user data directory.
    pub fn open() -> Result<Self, String> {
        let dirs = directories::ProjectDirs::from("", "", "mecha-oracle")
            .ok_or("Cannot determine user data directory")?;
        let path = dirs.data_dir().join("oracle_cards.json");
        Self::from_path(&path)
    }

    /// Load from an arbitrary path (useful for tests and custom installs).
    pub fn from_path(path: &Path) -> Result<Self, String> {
        let content = std::fs::read_to_string(path)
            .map_err(|e| format!("Cannot read {}: {e}", path.display()))?;
        let cards: Vec<serde_json::Value> = serde_json::from_str(&content)
            .map_err(|e| format!("Invalid JSON in {}: {e}", path.display()))?;

        let mut inner = HashMap::new();
        let mut tokens = HashMap::new();
        let mut skipped = 0usize;
        let mut partially_parsed = 0usize;
        let mut fully_parsed = 0usize;
        let mut art_cards = 0usize;
        let mut un_cards = 0usize;
        for v in &cards {
            match scryfall::parse_entry(v) {
                Ok(ParsedEntry::Card(def)) => {
                    if def.has_unparsed() {
                        partially_parsed += 1;
                    } else {
                        fully_parsed += 1;
                    }
                    if let Some(existing) = inner.insert(def.name.to_lowercase(), def) {
                        tracing::warn!(card = ?existing, "overwrote");
                    }
                }
                Ok(ParsedEntry::Token(def)) => {
                    tokens.insert(def.name.to_lowercase(), def);
                }
                Ok(ParsedEntry::ArtCard) => {
                    art_cards += 1;
                }
                Ok(ParsedEntry::UnCard) => {
                    un_cards += 1;
                }
                Err(e) => {
                    let name = v["name"].as_str().unwrap_or("<unknown>");
                    tracing::debug!(card = name, error = %e, "skipped card");
                    skipped += 1;
                }
            }
        }
        let card_count = inner.len();
        let token_count = tokens.len();
        tracing::info!(
            cards = card_count,
            partially_parsed,
            fully_parsed,
            tokens = token_count,
            art_cards,
            un_cards,
            skipped,
            "card database loaded"
        );

        Ok(Self { inner, tokens })
    }

    /// Number of loaded cards that contain at least one `OracleSpan::Unparsed` span.
    pub fn unparsed_count(&self) -> usize {
        self.inner.values().filter(|def| def.has_unparsed()).count()
    }

    pub fn get(&self, name: &str) -> Option<&CardDefinition> {
        self.inner.get(&name.to_lowercase())
    }

    pub fn get_token(&self, name: &str) -> Option<&CardDefinition> {
        self.tokens.get(&name.to_lowercase())
    }
}

#[cfg(test)]
pub mod test_helpers {
    use super::CardDatabase;
    use std::path::Path;

    pub fn test_db() -> CardDatabase {
        CardDatabase::from_path(Path::new("tests/fixtures/oracle_cards_test.json")).unwrap()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use test_helpers::test_db;

    #[test]
    fn landfall_card_loads_successfully() {
        let db = test_db();
        assert!(db.get("Grazing Gladehart").is_some());
    }

    #[test]
    fn landfall_card_has_unparsed_span() {
        let db = test_db();
        let card = db.get("Grazing Gladehart").unwrap();
        assert!(card.has_unparsed());
    }

    #[test]
    fn landfall_card_span_structure() {
        use crate::types::{IgnoredKind, OracleSpan};
        let db = test_db();
        let card = db.get("Grazing Gladehart").unwrap();
        assert_eq!(card.abilities.len(), 2);
        assert!(matches!(
            &card.abilities[0],
            OracleSpan::Ignored(IgnoredKind::AbilityWord, _)
        ));
        assert!(matches!(&card.abilities[1], OracleSpan::Unparsed(_)));
    }

    #[test]
    fn keyword_only_card_has_no_unparsed_spans() {
        use crate::types::OracleSpan;
        let db = test_db();
        let card = db.get("Serra Angel").unwrap();
        assert!(!card.has_unparsed());
        assert!(
            card.abilities
                .iter()
                .all(|s| matches!(s, OracleSpan::Parsed(_)))
        );
    }

    #[test]
    fn unparsed_count_reflects_landfall_card() {
        // Llanowar Elves' "{T}: Add {G}." is now parsed as Activated,
        // so only Grazing Gladehart (Landfall card) remains unparsed.
        let db = test_db();
        assert_eq!(db.unparsed_count(), 1);
    }

    #[test]
    fn llanowar_elves_loads_with_activated_ability() {
        use crate::types::{Ability, OracleSpan};
        let db = test_db();
        let card = db
            .get("Llanowar Elves")
            .expect("Llanowar Elves not in fixture");
        assert!(
            card.abilities
                .iter()
                .any(|s| { matches!(s, OracleSpan::Parsed(Ability::Activated(_))) })
        );
    }

    #[test]
    fn token_does_not_overwrite_card() {
        let db = test_db();
        let card = db.get("Llanowar Elves").expect("creature not found");
        // The creature has a mana cost; the token does not
        assert!(card.mana_cost.is_some());
    }

    #[test]
    fn get_token_returns_token() {
        let db = test_db();
        let token = db.get_token("Llanowar Elves").expect("token not found");
        assert!(token.mana_cost.is_none());
    }
}
