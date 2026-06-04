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
        for v in &cards {
            match scryfall::parse_card(v) {
                Ok(def) => {
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
        tracing::info!(loaded, skipped, "card database loaded");

        Ok(Self { inner })
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
