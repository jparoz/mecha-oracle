# Layout Routing Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Route Scryfall cards and tokens into separate maps inside `CardDatabase` so name collisions (e.g. the Llanowar Elves token overwriting the creature) are impossible.

**Architecture:** `parse_entry` in `scryfall.rs` reads the `layout` JSON field and wraps the parsed `CardDefinition` in either `ParsedEntry::Card` or `ParsedEntry::Token`. `CardDatabase` matches on the variant to populate two separate `HashMap<String, CardDefinition>` maps; `get()` searches cards only, `get_token()` searches tokens.

**Tech Stack:** Rust, serde_json

---

## File Map

- Modify: `src/cards/scryfall.rs` — add `ParsedEntry` enum, rename `parse_card` → `parse_entry`
- Modify: `src/cards/mod.rs` — add `tokens` map, update `from_path` loop, add `get_token()`
- Modify: `tests/fixtures/oracle_cards_test.json` — add `"layout": "normal"` to all existing entries, append a token entry for Llanowar Elves

---

### Task 1: Add `ParsedEntry` enum and `parse_entry` to `scryfall.rs`

**Files:**
- Modify: `src/cards/scryfall.rs`

- [ ] **Step 1: Write the failing test**

Add this test inside the existing `#[cfg(test)] mod tests` block at the bottom of `src/cards/scryfall.rs`:

```rust
#[test]
fn parse_entry_routes_token() {
    let v = json!({
        "layout": "token",
        "name": "Llanowar Elves",
        "mana_cost": "",
        "type_line": "Token Creature \u{2014} Elf Druid",
        "oracle_text": ""
    });
    assert!(matches!(parse_entry(&v), Ok(ParsedEntry::Token(_))));
}

#[test]
fn parse_entry_routes_normal_card() {
    let v = json!({
        "layout": "normal",
        "name": "Grizzly Bears",
        "mana_cost": "{1}{G}",
        "type_line": "Creature \u{2014} Bear",
        "oracle_text": "",
        "power": "2",
        "toughness": "2"
    });
    assert!(matches!(parse_entry(&v), Ok(ParsedEntry::Card(_))));
}

#[test]
fn parse_entry_missing_layout_defaults_to_card() {
    let v = json!({
        "name": "Hill Giant",
        "mana_cost": "{3}{R}",
        "type_line": "Creature \u{2014} Giant",
        "oracle_text": "",
        "power": "3",
        "toughness": "3"
    });
    assert!(matches!(parse_entry(&v), Ok(ParsedEntry::Card(_))));
}
```

- [ ] **Step 2: Run to confirm it fails**

```bash
cargo test 2>&1 | grep -E "^test result|FAILED|error\["
```

Expected: compile error — `parse_entry` and `ParsedEntry` not defined yet.

- [ ] **Step 3: Add `ParsedEntry` enum and `parse_entry` function**

At the top of `src/cards/scryfall.rs`, add the enum after the `use` imports:

```rust
pub enum ParsedEntry {
    Card(CardDefinition),
    Token(CardDefinition),
}
```

Then add `parse_entry` immediately after the existing `parse_card` function (keep `parse_card` in place for now — it will be removed in Task 2):

```rust
pub fn parse_entry(v: &Value) -> Result<ParsedEntry, String> {
    let def = parse_card(v)?;
    Ok(match v["layout"].as_str() {
        Some("token") => ParsedEntry::Token(def),
        _ => ParsedEntry::Card(def),
    })
}
```

- [ ] **Step 4: Run tests to confirm they pass**

```bash
cargo test 2>&1 | grep -E "^test result|FAILED|error\["
```

Expected: `test result: ok`

- [ ] **Step 5: Commit**

```bash
git add src/cards/scryfall.rs
git commit -m "feat: add ParsedEntry enum and parse_entry routing by layout field"
```

---

### Task 2: Update `CardDatabase` to use two maps

**Files:**
- Modify: `src/cards/mod.rs`

- [ ] **Step 1: Write the failing tests**

Add these tests inside the existing `#[cfg(test)] mod tests` block in `src/cards/mod.rs`:

```rust
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
```

- [ ] **Step 2: Run to confirm they fail**

```bash
cargo test 2>&1 | grep -E "^test result|FAILED|error\["
```

Expected: compile error — `get_token` not defined, and the fixture doesn't have a token entry yet.

- [ ] **Step 3: Add token entry to the test fixture**

Open `tests/fixtures/oracle_cards_test.json`. Add `"layout": "normal"` to every existing entry, and append a new token entry at the end of the JSON array. The finished file should look like this (only the changed/added parts shown for clarity — apply these edits to the real file):

Every existing card object gets `"layout": "normal"` added, for example:
```json
{
  "object": "card",
  "layout": "normal",
  "name": "Forest",
  ...
}
```

Append at the end of the array (before the closing `]`):
```json
{
  "object": "token",
  "layout": "token",
  "name": "Llanowar Elves",
  "mana_cost": "",
  "type_line": "Token Creature — Elf Druid",
  "oracle_text": "",
  "power": "1",
  "toughness": "1"
}
```

- [ ] **Step 4: Update `CardDatabase` in `src/cards/mod.rs`**

Replace the `CardDatabase` struct, its `from_path` method, and add `get_token`. The full updated `src/cards/mod.rs` content (non-test portion) is:

```rust
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
        let mut unparsed = 0usize;
        for v in &cards {
            match scryfall::parse_entry(v) {
                Ok(ParsedEntry::Card(def)) => {
                    if def.has_unparsed() {
                        unparsed += 1;
                    }
                    inner.insert(def.name.to_lowercase(), def);
                }
                Ok(ParsedEntry::Token(def)) => {
                    tokens.insert(def.name.to_lowercase(), def);
                }
                Err(e) => {
                    let name = v["name"].as_str().unwrap_or("<unknown>");
                    tracing::debug!(card = name, error = %e, "skipped card");
                    skipped += 1;
                }
            }
        }
        let loaded = inner.len();
        let token_count = tokens.len();
        tracing::info!(loaded, token_count, skipped, unparsed, "card database loaded");

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
```

- [ ] **Step 5: Run tests to confirm they pass**

```bash
cargo test 2>&1 | grep -E "^test result|FAILED|error\["
```

Expected: `test result: ok`

- [ ] **Step 6: Commit**

```bash
git add src/cards/mod.rs tests/fixtures/oracle_cards_test.json
git commit -m "feat: separate card and token maps in CardDatabase; add get_token()"
```

---

### Task 3: Remove `parse_card` and clean up

**Files:**
- Modify: `src/cards/scryfall.rs`

- [ ] **Step 1: Run tests before touching anything**

```bash
cargo test 2>&1 | grep -E "^test result|FAILED|error\["
```

Expected: `test result: ok` — baseline confirmed.

- [ ] **Step 2: Inline `parse_card` into `parse_entry` and delete it**

In `src/cards/scryfall.rs`, replace the two functions `parse_card` and `parse_entry` with a single `parse_entry` that does everything inline:

```rust
pub fn parse_entry(v: &Value) -> Result<ParsedEntry, String> {
    let name = v["name"].as_str().ok_or("missing name")?.to_string();

    let _span = tracing::debug_span!("parsing", card = name).entered();

    let mana_cost = match v["mana_cost"].as_str() {
        Some(s) if !s.is_empty() => Some(parse_mana_cost(s)),
        _ => None,
    };

    let type_line = v["type_line"]
        .as_str()
        .ok_or("missing type_line")
        .map(parse_type_line)?;

    let oracle_text = v["oracle_text"].as_str().unwrap_or("").to_string();

    let abilities = parse_oracle_text(&oracle_text);

    let power = v["power"].as_str().and_then(|s| s.parse::<i32>().ok());
    let toughness = v["toughness"].as_str().and_then(|s| s.parse::<i32>().ok());

    let def = CardDefinition {
        name,
        mana_cost,
        type_line,
        oracle_text,
        abilities,
        power,
        toughness,
    };

    Ok(match v["layout"].as_str() {
        Some("token") => ParsedEntry::Token(def),
        _ => ParsedEntry::Card(def),
    })
}
```

Remove the old `parse_card` function entirely.

Also update the `#[cfg(test)]` block: any test that called `parse_card` should now call `parse_entry` and unwrap the inner `CardDefinition`. For example, the existing `parse_grizzly_bears` test becomes:

```rust
#[test]
fn parse_grizzly_bears() {
    let v = json!({
        "name": "Grizzly Bears",
        "mana_cost": "{1}{G}",
        "type_line": "Creature \u{2014} Bear",
        "oracle_text": "",
        "power": "2",
        "toughness": "2"
    });
    let ParsedEntry::Card(card) = parse_entry(&v).unwrap() else { panic!("expected Card") };
    assert_eq!(card.name, "Grizzly Bears");
    assert_eq!(card.power, Some(2));
    assert_eq!(card.toughness, Some(2));
    let cost = card.mana_cost.unwrap();
    assert_eq!(cost.mana_value(), 2);
    assert!(cost.pips.contains(&ManaPip::Generic(1)));
    assert!(cost.pips.contains(&ManaPip::Green));
    assert!(card.type_line.is_creature());
    assert_eq!(card.type_line.subtypes, vec!["Bear"]);
}
```

Apply the same pattern to all other tests in the `scryfall.rs` test block that called `parse_card`: replace `parse_card(&v).unwrap()` with `let ParsedEntry::Card(card) = parse_entry(&v).unwrap() else { panic!("expected Card") };` and rename the local variable to `card` where needed.

- [ ] **Step 3: Run tests to confirm they still pass**

```bash
cargo test 2>&1 | grep -E "^test result|FAILED|error\["
```

Expected: `test result: ok`

- [ ] **Step 4: Commit**

```bash
git add src/cards/scryfall.rs
git commit -m "refactor: inline parse_card into parse_entry; remove parse_card"
```
