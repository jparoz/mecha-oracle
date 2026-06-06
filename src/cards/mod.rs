mod downloader;
mod scryfall;

pub use downloader::update_cards;

use crate::types::card::CardDefinition;
use std::collections::HashMap;
use std::path::Path;

pub struct CardDatabase {
    inner: HashMap<String, CardDefinition>,
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
        let mut loaded = 0usize;
        let mut skipped = 0usize;
        let mut unparsed = 0usize;
        for v in &cards {
            match scryfall::parse_card(v) {
                Ok(def) => {
                    if def.has_unparsed() {
                        unparsed += 1;
                    }
                    inner.insert(def.name.to_lowercase(), def);
                    loaded += 1;
                }
                Err(e) => {
                    let name = v["name"].as_str().unwrap_or("<unknown>");
                    tracing::debug!(card = name, error = %e, "skipped card");
                    skipped += 1;
                }
            }
        }
        tracing::info!(loaded, skipped, unparsed, "card database loaded");

        Ok(Self { inner })
    }

    /// Number of loaded cards that contain at least one `OracleSpan::Unparsed` span.
    pub fn unparsed_count(&self) -> usize {
        self.inner.values().filter(|def| def.has_unparsed()).count()
    }

    pub fn get(&self, name: &str) -> Option<&CardDefinition> {
        self.inner.get(&name.to_lowercase())
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
    fn unparsed_count_zero_for_current_fixtures() {
        // All current fixture cards have only keywords or empty oracle text.
        // This will change in Task 8 when a Landfall card is added.
        let db = test_db();
        assert_eq!(db.unparsed_count(), 0);
    }
}
