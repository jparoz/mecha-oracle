# Color Tracking + 6 Keywords Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add authoritative color tracking to `CardDefinition` (sourced from Scryfall's `colors` field), then implement Ward, Landwalk, Battle Cry, Fear, Intimidate, and Protection from color.

**Architecture:** Color data flows from Scryfall JSON → `CardDefinition::colors` → engine query points (targeting + combat). Each keyword is a new `StaticAbility` variant. Ward uses the existing stack machinery (Option B): a `WardTrigger` stack object is pushed above the spell; `pay_ward` marks it paid; resolution counters the spell if unpaid.

**Tech Stack:** Rust, `cargo test`, `cargo clippy --all-targets`

---

## File Map

| File | Change |
|---|---|
| `src/types/card.rs` | Add `colors: Vec<ManaColor>` to `CardDefinition` |
| `src/types/ability.rs` | New `StaticAbility` variants; new `LandwalkKind`, `WardCost` enums |
| `src/types/stack.rs` | New `StackPayload::WardTrigger` variant |
| `src/types/mod.rs` | Re-export `LandwalkKind`, `WardCost` |
| `src/types/mana.rs` | `Display` impl for `ManaPip` and `ManaCost` |
| `src/engine/mod.rs` | Add `InsufficientLife` to `EngineError`; declare `pub mod ward` |
| `src/engine/ward.rs` | New file: `pay_ward` function |
| `src/engine/stack.rs` | Handle `WardTrigger` in `resolve_top`; add `counter_spell_on_stack` |
| `src/engine/casting.rs` | Generate Ward triggers after spell pushed; pass `source_colors` |
| `src/engine/activated.rs` | Generate Ward triggers after non-mana ability pushed; pass `source_colors` |
| `src/engine/targeting.rs` | Add `source_colors: &[ManaColor]` param; Protection check |
| `src/engine/combat.rs` | Add Fear/Intimidate/Landwalk/Protection/BattleCry |
| `src/engine/triggered.rs` | Add Battle Cry to `collect_attack_triggers` |
| `src/parser/oracle.rs` | Promote 7 keyword families from unimplemented to parsed |
| `src/cards/scryfall.rs` | Parse `colors` JSON field |
| `src/serve.rs` | Update `legal_targets` call; add `/pay_ward` endpoint |
| `docs/todo.md` | Add Protection from X remaining work notes |

---

## Task 1: `CardDefinition::colors` field + Scryfall parsing

**Files:**
- Modify: `src/types/card.rs`
- Modify: `src/cards/scryfall.rs`

### Step 1: Write the failing test

In `src/cards/scryfall.rs`, the fixture already has `"colors": []` in the Forest entry. Add a test that checks a colored card gets its colors populated. Add this to the `#[cfg(test)]` block in `src/cards/scryfall.rs` (or create a new test file if there isn't one):

```rust
// In src/cards/scryfall.rs — find the existing #[cfg(test)] block or add one
#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::mana::ManaColor;
    use serde_json::json;

    #[test]
    fn scryfall_colors_parsed_for_blue_card() {
        let v = json!({
            "name": "Counterspell",
            "mana_cost": "{U}{U}",
            "type_line": "Instant",
            "oracle_text": "Counter target spell.",
            "colors": ["U"],
            "layout": "normal"
        });
        let ParsedEntry::Card(def) = parse_entry(&v).unwrap() else { panic!() };
        assert_eq!(def.colors, vec![ManaColor::Blue]);
    }

    #[test]
    fn scryfall_colors_empty_for_colorless_card() {
        let v = json!({
            "name": "Forest",
            "type_line": "Basic Land — Forest",
            "oracle_text": "",
            "colors": [],
            "layout": "normal"
        });
        let ParsedEntry::Card(def) = parse_entry(&v).unwrap() else { panic!() };
        assert_eq!(def.colors, vec![]);
    }
}
```

### Step 2: Run test to verify it fails

```bash
cargo test scryfall_colors 2>&1 | grep -E "^test result|FAILED|error\["
```
Expected: compile error — `CardDefinition` has no field `colors`.

### Step 3: Add `colors` to `CardDefinition`

In `src/types/card.rs`, add the `ManaColor` import and new field:

```rust
use super::ability::{OracleSpan, TextAnnotation};
use super::mana::{ManaCost, ManaColor};  // add ManaColor

// ... enums unchanged ...

#[derive(Debug, Clone)]
pub struct CardDefinition {
    pub name: String,
    pub mana_cost: Option<ManaCost>,
    pub type_line: TypeLine,
    pub oracle_text: String,
    pub abilities: Vec<OracleSpan>,
    pub text_annotations: Vec<TextAnnotation>,
    pub power: Option<i32>,
    pub toughness: Option<i32>,
    pub colors: Vec<ManaColor>,  // NEW — CR 105.4 / Scryfall authoritative
}
```

### Step 4: Fix all struct literal compilation errors

Run this to find every file with a broken struct literal:

```bash
cargo build 2>&1 | grep "missing field" | head -30
```

**Every** `CardDefinition { ... }` literal throughout the codebase needs `colors: vec![],` added. There are approximately 70 of them, spread across:
- `src/engine/casting.rs` (many in tests — the `make_instant_def` helper and others)
- `src/engine/combat.rs` (test helpers)
- `src/engine/cycling.rs`
- `src/engine/targeting.rs`
- `src/engine/triggered.rs`
- `src/engine/state_based_actions.rs`
- `src/types/card_object.rs`
- `src/serve.rs` (3 literals)
- `src/cards/scryfall.rs` (the `parse_entry` function — see Step 5)

For every struct literal **except** the one in `parse_entry`, add:

```rust
    toughness: ...,   // existing last field
    colors: vec![],   // ADD THIS LINE
}
```

Use `cargo build 2>&1 | grep "missing field \`colors\`"` to drive the fix — it prints the file and line number for each error.

### Step 5: Populate `colors` in `parse_entry`

In `src/cards/scryfall.rs`, update the `parse_entry` function. Add a helper below `color_from_str`:

```rust
fn color_from_str_no_colorless(s: &str) -> Option<ManaColor> {
    match s {
        "W" => Some(ManaColor::White),
        "U" => Some(ManaColor::Blue),
        "B" => Some(ManaColor::Black),
        "R" => Some(ManaColor::Red),
        "G" => Some(ManaColor::Green),
        _ => None,
    }
}
```

Then in `parse_entry`, before the `let def = CardDefinition {` line, add:

```rust
let colors: Vec<ManaColor> = v["colors"]
    .as_array()
    .map(|arr| {
        arr.iter()
            .filter_map(|c| c.as_str().and_then(color_from_str_no_colorless))
            .collect()
    })
    .unwrap_or_default();
```

And update the `def` construction:

```rust
let def = CardDefinition {
    name,
    mana_cost,
    type_line,
    oracle_text,
    abilities,
    text_annotations,
    power,
    toughness,
    colors,   // ADD
};
```

### Step 6: Run tests to verify they pass

```bash
cargo test 2>&1 | grep -E "^test result|FAILED|error\["
```
Expected: `test result: ok` (all tests pass).

### Step 7: Commit

```bash
git add src/types/card.rs src/cards/scryfall.rs src/engine/ src/serve.rs src/types/
git commit -m "$(cat <<'EOF'
feat: add CardDefinition::colors field; parse Scryfall colors array

CR 105.4: card color is determined by color indicators and mana cost, but
Scryfall's authoritative `colors` field covers all edge cases (color
indicators, oracle-text-granted colors). Populate it at parse time.

Co-Authored-By: Claude Sonnet 4.6 <noreply@anthropic.com>
EOF
)"
```

---

## Task 2: New types + EngineError variant + module declaration

**Files:**
- Modify: `src/types/ability.rs`
- Modify: `src/types/stack.rs`
- Modify: `src/types/mod.rs`
- Modify: `src/types/mana.rs`
- Modify: `src/engine/mod.rs`

### Step 1: Write failing tests

Add these to `src/types/ability.rs` in the existing `#[cfg(test)]` block:

```rust
#[test]
fn new_static_ability_display_names() {
    use crate::types::mana::{ManaCost, ManaPip};
    assert_eq!(StaticAbility::Fear.display_name(), "Fear");
    assert_eq!(StaticAbility::Intimidate.display_name(), "Intimidate");
    assert_eq!(StaticAbility::BattleCry.display_name(), "Battle cry");
    assert_eq!(
        StaticAbility::WardMana(ManaCost { pips: vec![ManaPip::Generic(2)] }).display_name(),
        "Ward {2}"
    );
    assert_eq!(StaticAbility::WardLife(2).display_name(), "Ward—Pay 2 life");
    assert_eq!(
        StaticAbility::Landwalk(LandwalkKind::LandType("Island".to_string())).display_name(),
        "Islandwalk"
    );
    assert_eq!(
        StaticAbility::Landwalk(LandwalkKind::Nonbasic).display_name(),
        "Nonbasic landwalk"
    );
    assert_eq!(
        StaticAbility::ProtectionFromColor(ManaColor::Blue).display_name(),
        "Protection from blue"
    );
}
```

Add to `src/types/mana.rs`:

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mana_pip_display() {
        assert_eq!(ManaPip::Generic(2).to_string(), "{2}");
        assert_eq!(ManaPip::Blue.to_string(), "{U}");
        assert_eq!(ManaPip::Hybrid(ManaColor::White, ManaColor::Blue).to_string(), "{W/U}");
    }

    #[test]
    fn mana_cost_display() {
        let cost = ManaCost { pips: vec![ManaPip::Generic(1), ManaPip::Blue] };
        assert_eq!(cost.to_string(), "{1}{U}");
    }
}
```

### Step 2: Run to verify failures

```bash
cargo test new_static_ability_display_names 2>&1 | grep -E "^test result|FAILED|error\["
cargo test mana_pip_display 2>&1 | grep -E "^test result|FAILED|error\["
```

### Step 3: Add `LandwalkKind` and `WardCost` to `ability.rs`

In `src/types/ability.rs`, add after the imports:

```rust
use super::mana::ManaColor;

// CR 702.14
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LandwalkKind {
    LandType(String), // e.g. "Island", "Swamp", "Forest", "Mountain", "Plains"
    Nonbasic,
}

// CR 702.21
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WardCost {
    Mana(ManaCost),
    Life(u32),
}
```

### Step 4: Add new `StaticAbility` variants to `ability.rs`

In the `StaticAbility` enum, add after `Hexproof`:

```rust
    WardMana(ManaCost),              // CR 702.21 — Ward {cost}
    WardLife(u32),                   // CR 702.21 — Ward—Pay N life
    Landwalk(LandwalkKind),          // CR 702.14
    BattleCry,                       // CR 702.91
    Fear,                            // CR 702.36
    Intimidate,                      // CR 702.13
    ProtectionFromColor(ManaColor),  // CR 702.16 (partial — blocking + targeting)
```

### Step 5: Add `display_name` arms for new variants

In the `display_name` method, add after the `Hexproof` arm:

```rust
    Self::WardMana(cost) => format!("Ward {cost}"),
    Self::WardLife(n) => format!("Ward\u{2014}Pay {n} life"),
    Self::Landwalk(LandwalkKind::LandType(t)) => format!("{t}walk"),
    Self::Landwalk(LandwalkKind::Nonbasic) => "Nonbasic landwalk".to_string(),
    Self::BattleCry => "Battle cry".to_string(),
    Self::Fear => "Fear".to_string(),
    Self::Intimidate => "Intimidate".to_string(),
    Self::ProtectionFromColor(c) => {
        let color_name = match c {
            ManaColor::White => "white",
            ManaColor::Blue => "blue",
            ManaColor::Black => "black",
            ManaColor::Red => "red",
            ManaColor::Green => "green",
            ManaColor::Colorless => "colorless",
        };
        format!("Protection from {color_name}")
    }
```

### Step 6: Add `Display` for `ManaPip` and `ManaCost` in `mana.rs`

In `src/types/mana.rs`, add after the `ManaColor` Display impl:

```rust
impl std::fmt::Display for ManaPip {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ManaPip::White => write!(f, "{{W}}"),
            ManaPip::Blue => write!(f, "{{U}}"),
            ManaPip::Black => write!(f, "{{B}}"),
            ManaPip::Red => write!(f, "{{R}}"),
            ManaPip::Green => write!(f, "{{G}}"),
            ManaPip::Colorless => write!(f, "{{C}}"),
            ManaPip::Generic(n) => write!(f, "{{{n}}}"),
            ManaPip::X => write!(f, "{{X}}"),
            ManaPip::Snow => write!(f, "{{S}}"),
            ManaPip::Hybrid(a, b) => write!(f, "{{{a}/{b}}}"),
            ManaPip::GenericHybrid(n, c) => write!(f, "{{{n}/{c}}}"),
            ManaPip::ColorlessHybrid(c) => write!(f, "{{C/{c}}}"),
            ManaPip::Phyrexian(c) => write!(f, "{{{c}/P}}"),
            ManaPip::HybridPhyrexian(a, b) => write!(f, "{{{a}/{b}/P}}"),
        }
    }
}

impl std::fmt::Display for ManaCost {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        for pip in &self.pips {
            write!(f, "{pip}")?;
        }
        Ok(())
    }
}
```

### Step 7: Add `WardTrigger` to `StackPayload` in `stack.rs`

In `src/types/stack.rs`, add the import:

```rust
use super::ability::WardCost;
```

And add to the `StackPayload` enum after `ActivatedAbility`:

```rust
    /// CR 702.21b: triggered by targeting a permanent with Ward. Counters the
    /// triggering spell/ability if the Ward cost is not paid before this resolves.
    WardTrigger {
        counters_if_unpaid: StackId,
        cost: WardCost,
        paid: bool,
    },
```

### Step 8: Update `engine/mod.rs`

Add `InsufficientLife` to `EngineError` and declare the `ward` module:

```rust
pub mod activated;
pub mod casting;
pub mod combat;
pub mod cycling;
pub mod mana;
pub mod stack;
pub mod state_based_actions;
pub mod targeting;
pub mod triggered;
pub mod turn;
pub mod ward;  // ADD

#[derive(Debug, Clone, PartialEq)]
pub enum EngineError {
    CardNotFound,
    // ... existing variants ...
    WrongNumberOfTargets,
    IllegalTarget,
    InsufficientLife,  // ADD — CR 702.21: Ward life cost cannot be paid
}
```

### Step 9: Re-export new types from `types/mod.rs`

In `src/types/mod.rs`, update the `ability` re-export line:

```rust
pub use ability::{
    Ability, ActivatedAbility, ActivationCost, CardFilter, CastFilter, CostComponent,
    IgnoredKind, LandwalkKind, OracleSpan, PermanentFilter, SpellAbility, StaticAbility,
    TargetFilter, TriggerEvent, TriggeredAbility, WardCost,
};
```

### Step 10: Run tests

```bash
cargo test 2>&1 | grep -E "^test result|FAILED|error\["
```
Expected: all pass.

### Step 11: Commit

```bash
git add src/types/ src/engine/mod.rs
git commit -m "$(cat <<'EOF'
feat: add LandwalkKind, WardCost, 7 StaticAbility variants, StackPayload::WardTrigger

Defines the type-level scaffolding for Ward, Landwalk, Battle Cry, Fear,
Intimidate, and ProtectionFromColor. Also adds Display for ManaPip/ManaCost
(needed for Ward display names) and InsufficientLife engine error variant.

Co-Authored-By: Claude Sonnet 4.6 <noreply@anthropic.com>
EOF
)"
```

---

## Task 3: Parser — promote 7 keyword families

**Files:**
- Modify: `src/parser/oracle.rs`

### Step 1: Write failing tests

Add to the `#[cfg(test)]` block in `src/parser/oracle.rs` (or wherever the parser tests live — check by running `grep -n "mod tests" src/parser/oracle.rs`):

```rust
#[test]
fn fear_parses_as_static_ability() {
    use crate::types::{Ability, OracleSpan, StaticAbility};
    let (spans, _) = parse_permanent("Fear", "Test");
    assert_eq!(spans, vec![OracleSpan::Parsed(Ability::Static(StaticAbility::Fear))]);
}

#[test]
fn intimidate_parses_as_static_ability() {
    use crate::types::{Ability, OracleSpan, StaticAbility};
    let (spans, _) = parse_permanent("Intimidate", "Test");
    assert_eq!(spans, vec![OracleSpan::Parsed(Ability::Static(StaticAbility::Intimidate))]);
}

#[test]
fn battle_cry_parses_as_static_ability() {
    use crate::types::{Ability, OracleSpan, StaticAbility};
    let (spans, _) = parse_permanent("Battle cry", "Test");
    assert_eq!(spans, vec![OracleSpan::Parsed(Ability::Static(StaticAbility::BattleCry))]);
}

#[test]
fn ward_mana_parses_as_ward_mana() {
    use crate::types::{Ability, OracleSpan, StaticAbility};
    use crate::types::mana::{ManaCost, ManaPip};
    let (spans, _) = parse_permanent("Ward {2}", "Test");
    assert_eq!(
        spans,
        vec![OracleSpan::Parsed(Ability::Static(StaticAbility::WardMana(
            ManaCost { pips: vec![ManaPip::Generic(2)] }
        )))]
    );
}

#[test]
fn ward_life_parses_from_em_dash_paragraph() {
    use crate::types::{Ability, OracleSpan, StaticAbility};
    let (spans, _) = parse_permanent("Ward\u{2014}Pay 2 life.", "Test");
    assert_eq!(spans, vec![OracleSpan::Parsed(Ability::Static(StaticAbility::WardLife(2)))]);
}

#[test]
fn islandwalk_parses_as_landwalk() {
    use crate::types::{Ability, LandwalkKind, OracleSpan, StaticAbility};
    let (spans, _) = parse_permanent("Islandwalk", "Test");
    assert_eq!(
        spans,
        vec![OracleSpan::Parsed(Ability::Static(StaticAbility::Landwalk(
            LandwalkKind::LandType("Island".to_string())
        )))]
    );
}

#[test]
fn swampwalk_parses_as_landwalk() {
    use crate::types::{Ability, LandwalkKind, OracleSpan, StaticAbility};
    let (spans, _) = parse_permanent("Swampwalk", "Test");
    assert_eq!(
        spans,
        vec![OracleSpan::Parsed(Ability::Static(StaticAbility::Landwalk(
            LandwalkKind::LandType("Swamp".to_string())
        )))]
    );
}

#[test]
fn nonbasic_landwalk_parses_as_nonbasic() {
    use crate::types::{Ability, LandwalkKind, OracleSpan, StaticAbility};
    let (spans, _) = parse_permanent("Nonbasic landwalk", "Test");
    assert_eq!(
        spans,
        vec![OracleSpan::Parsed(Ability::Static(StaticAbility::Landwalk(
            LandwalkKind::Nonbasic
        )))]
    );
}

#[test]
fn protection_from_blue_parses_as_protection() {
    use crate::types::{Ability, OracleSpan, StaticAbility};
    use crate::types::mana::ManaColor;
    let (spans, _) = parse_permanent("Protection from blue", "Test");
    assert_eq!(
        spans,
        vec![OracleSpan::Parsed(Ability::Static(StaticAbility::ProtectionFromColor(
            ManaColor::Blue
        )))]
    );
}

#[test]
fn protection_from_artifacts_stays_unimplemented() {
    use crate::types::OracleSpan;
    let (spans, _) = parse_permanent("Protection from artifacts", "Test");
    assert!(matches!(&spans[0], OracleSpan::ParsedUnimplemented(_)));
}
```

### Step 2: Run to verify failures

```bash
cargo test --test-opts "" 2>&1 | grep -E "fear_parses|intimidate_parses|battle_cry|ward_mana|ward_life|islandwalk|swampwalk|nonbasic_landwalk|protection_from" | head -20
```

Expected: each prints `FAILED`.

### Step 3: Implement parser changes in `match_keyword`

In `src/parser/oracle.rs`, inside `match_keyword`, **before** the `is_cr702_keyword` check, add these cases after the `BushidoN` and `Cycling` blocks:

```rust
// Fear (CR 702.36)
if s == "fear" {
    return OracleSpan::Parsed(Ability::Static(StaticAbility::Fear));
}

// Intimidate (CR 702.13)
if s == "intimidate" {
    return OracleSpan::Parsed(Ability::Static(StaticAbility::Intimidate));
}

// Battle Cry (CR 702.91)
if s == "battle cry" {
    return OracleSpan::Parsed(Ability::Static(StaticAbility::BattleCry));
}

// Ward {cost} (CR 702.21) — mana cost form e.g. "Ward {2}"
if let Some(rest) = s.strip_prefix("ward ") {
    if let Some(cost) = try_parse_mana_cost(kw["ward ".len()..].trim()) {
        return OracleSpan::Parsed(Ability::Static(StaticAbility::WardMana(cost)));
    }
}

// Protection from [color] (CR 702.16)
if let Some(quality) = s.strip_prefix("protection from ") {
    let color = match quality.trim_end_matches('.') {
        "white" => Some(ManaColor::White),
        "blue"  => Some(ManaColor::Blue),
        "black" => Some(ManaColor::Black),
        "red"   => Some(ManaColor::Red),
        "green" => Some(ManaColor::Green),
        _ => None,
    };
    if let Some(c) = color {
        return OracleSpan::Parsed(Ability::Static(StaticAbility::ProtectionFromColor(c)));
    }
    // Non-color protections remain ParsedUnimplemented
    return ParsedUnimplemented(kw.to_string());
}

// Landwalk (CR 702.14): ends with "walk", prefix identifies the land type
if s.ends_with("walk") {
    let prefix = &s[..s.len() - "walk".len()];
    let kind = if prefix == "nonbasic land" || prefix == "non-basic land" {
        LandwalkKind::Nonbasic
    } else {
        let type_name = match prefix.trim_end() {
            "island" => "Island",
            "swamp"  => "Swamp",
            "forest" => "Forest",
            "mountain" => "Mountain",
            "plains" => "Plains",
            other => {
                // Title-case the prefix for unknown land types
                let mut c = other.chars();
                return OracleSpan::Parsed(Ability::Static(StaticAbility::Landwalk(
                    LandwalkKind::LandType(
                        c.next().map(|ch| ch.to_uppercase().collect::<String>())
                            .unwrap_or_default()
                            + c.as_str()
                    )
                )));
            }
        };
        LandwalkKind::LandType(type_name.to_string())
    };
    return OracleSpan::Parsed(Ability::Static(StaticAbility::Landwalk(kind)));
}
```

Also add `LandwalkKind` to the imports at the top of the file:

```rust
use crate::types::{
    Ability, IgnoredKind, LandwalkKind, OracleSpan,
    ability::{ActivatedAbility, StaticAbility},
};
```

Remove `"fear"`, `"intimidate"`, and `"battle cry"` from the `matches!(s, ...)` list in `is_cr702_keyword`. Remove `s.starts_with("ward")`, `s.starts_with("protection from")`, and `kw_part.ends_with("walk")` from `is_cr702_keyword` (they are now handled above).

### Step 4: Handle Ward em-dash form in `parse_permanent`

In `parse_permanent`, the em-dash handler dispatches on `match_keyword(left)`. For `"Ward—Pay 2 life"`, `left = "Ward"` which now returns `OracleSpan::Parsed(...)` — but wait, we don't want that path; "ward" with no cost is not a valid keyword. We need to intercept **before** the `match match_keyword(left)` call.

Find the em-dash handler in `parse_permanent` (around line 911). Before the `match match_keyword(left)` block, add:

```rust
// Ward em-dash life cost: "Ward—Pay N life." (CR 702.21)
let left_lower = left.to_lowercase();
if left_lower == "ward" {
    // Parse "Pay N life" from right side
    let right_lower = right.to_lowercase();
    let life_str = right_lower
        .strip_prefix("pay ")
        .and_then(|s| s.strip_suffix(" life").or_else(|| s.strip_suffix(" life.")));
    if let Some(n) = life_str.and_then(|s| parse_number_word(s.trim())) {
        let para_start = subslice_offset(text, paragraph);
        spans.push(OracleSpan::Parsed(Ability::Static(StaticAbility::WardLife(n))));
        // No annotation needed for fully-parsed spans
        continue;
    }
    // Unrecognized Ward—... form: fall through to normal em-dash handling
}
```

This block goes **before** the `match match_keyword(left) {` line.

Also ensure `StaticAbility` and `Ability` are in scope (they already are via existing imports).

### Step 5: Run tests

```bash
cargo test 2>&1 | grep -E "^test result|FAILED|error\["
```
Expected: all pass.

### Step 6: Commit

```bash
git add src/parser/oracle.rs
git commit -m "$(cat <<'EOF'
feat: parse Fear, Intimidate, BattleCry, Ward, Landwalk, ProtectionFromColor

Promotes 7 keyword families from ParsedUnimplemented to fully-parsed
StaticAbility variants. Ward em-dash life form handled before the general
em-dash dispatch in parse_permanent. Protection from non-color qualities
remain ParsedUnimplemented per spec.

Co-Authored-By: Claude Sonnet 4.6 <noreply@anthropic.com>
EOF
)"
```

---

## Task 4: Targeting — `source_colors` parameter + Protection check

**Files:**
- Modify: `src/engine/targeting.rs`
- Modify: `src/engine/casting.rs`
- Modify: `src/engine/activated.rs`
- Modify: `src/serve.rs`

### Step 1: Write failing tests

Add to `src/engine/targeting.rs` tests:

```rust
#[test]
fn protection_from_blue_blocks_blue_spell() {
    use crate::types::mana::ManaColor;
    let mut gs = two_player_state();
    let id = place_creature(
        &mut gs,
        PlayerId(1),
        vec![OracleSpan::Parsed(Ability::Static(
            StaticAbility::ProtectionFromColor(ManaColor::Blue)
        ))],
    );
    let target = EffectTarget::Object { id };
    // Blue spell (source_colors = [Blue]) cannot target the protected creature
    assert!(!is_legal_target(
        &gs,
        &target,
        TargetFilter::Creature,
        PlayerId(0),
        &[ManaColor::Blue],
    ));
}

#[test]
fn protection_from_blue_allows_red_spell() {
    use crate::types::mana::ManaColor;
    let mut gs = two_player_state();
    let id = place_creature(
        &mut gs,
        PlayerId(1),
        vec![OracleSpan::Parsed(Ability::Static(
            StaticAbility::ProtectionFromColor(ManaColor::Blue)
        ))],
    );
    let target = EffectTarget::Object { id };
    // Red spell cannot be blocked by Protection from Blue
    assert!(is_legal_target(
        &gs,
        &target,
        TargetFilter::Creature,
        PlayerId(0),
        &[ManaColor::Red],
    ));
}
```

Note: all existing test calls to `is_legal_target` and `legal_targets` will need `&[]` added as the last argument after this change. Compile errors will guide you.

### Step 2: Run to verify failures

```bash
cargo test protection_from_blue 2>&1 | grep -E "^test result|FAILED|error\["
```

### Step 3: Update `is_legal_target` signature and add Protection check

In `src/engine/targeting.rs`, update the function signature and add the Protection check:

```rust
use crate::types::mana::ManaColor;

// CR 115.4 + CR 702.16c
pub fn is_legal_target(
    state: &GameState,
    target: &EffectTarget,
    filter: TargetFilter,
    caster: PlayerId,
    source_colors: &[ManaColor],  // NEW
) -> bool {
    match target {
        EffectTarget::Object { id } => {
            let obj = match state.objects.get(id) {
                Some(o) => o,
                None => return false,
            };
            if obj.zone != Zone::Battlefield {
                return false;
            }
            let passes_filter = match filter {
                TargetFilter::Creature => obj.is_creature(),
                TargetFilter::Player => false,
                TargetFilter::Any => obj.is_creature(),
            };
            if !passes_filter {
                return false;
            }
            if obj.has_keyword(StaticAbility::Shroud) {
                return false;
            }
            if obj.has_keyword(StaticAbility::Hexproof) && obj.controller != caster {
                return false;
            }
            // CR 702.16c: protection prevents targeting by sources with the protected quality
            for span in &obj.definition.abilities {
                if let OracleSpan::Parsed(Ability::Static(StaticAbility::ProtectionFromColor(c)))
                    = span
                {
                    if source_colors.contains(c) {
                        return false;
                    }
                }
            }
            true
        }
        EffectTarget::Player { id } => {
            let player = match state.get_player(*id) {
                Some(p) => p,
                None => return false,
            };
            if player.has_lost {
                return false;
            }
            matches!(filter, TargetFilter::Player | TargetFilter::Any)
        }
    }
}
```

Also add the missing imports at the top of the function if needed:
```rust
use crate::types::ability::Ability;
use crate::types::OracleSpan;
```

### Step 4: Update `legal_targets` signature

```rust
pub fn legal_targets(
    state: &GameState,
    filter: TargetFilter,
    caster: PlayerId,
    source_colors: &[ManaColor],  // NEW
) -> Vec<EffectTarget> {
    let mut result = Vec::new();
    if matches!(filter, TargetFilter::Creature | TargetFilter::Any) {
        for &id in state.battlefield.keys() {
            let t = EffectTarget::Object { id };
            if is_legal_target(state, &t, filter, caster, source_colors) {
                result.push(t);
            }
        }
    }
    if matches!(filter, TargetFilter::Player | TargetFilter::Any) {
        for player in &state.players {
            let t = EffectTarget::Player { id: player.id };
            if is_legal_target(state, &t, filter, caster, source_colors) {
                result.push(t);
            }
        }
    }
    result
}
```

### Step 5: Fix all call sites

**In `src/engine/targeting.rs` tests** — every `is_legal_target(...)` and `legal_targets(...)` call needs `&[]` appended as the last argument.

**In `src/engine/casting.rs`** (around line 125):
```rust
if !is_legal_target(&state, target, *filter, player_id, &[]) {
```

Wait — for cast_spell we actually want to pass the spell's colors. Find the block that calls `is_legal_target`:

```rust
// Before the loop, extract the spell's colors:
let spell_colors: Vec<crate::types::mana::ManaColor> = state
    .objects
    .get(&object_id)
    .map(|o| o.definition.colors.clone())
    .unwrap_or_default();
// Then in the loop:
if !is_legal_target(&state, target, *filter, player_id, &spell_colors) {
```

**In `src/engine/activated.rs`** (around line 61):
```rust
// Before the loop, extract the ability source's colors:
let source_colors: Vec<crate::types::mana::ManaColor> = state
    .objects
    .get(&object_id)
    .map(|o| o.definition.colors.clone())
    .unwrap_or_default();
// Then in the loop:
if !is_legal_target(&state, target, *filter, activating_player, &source_colors) {
```

**In `src/serve.rs`** (around line 450):
```rust
// Extract the spell's colors just before the legal_targets call:
let spell_colors = obj.definition.colors.clone();
// ...
for target in legal_targets(state, *filter, pid, &spell_colors) {
```

The `obj` variable is already in scope at this point (it's the card being cast).

### Step 6: Run tests

```bash
cargo test 2>&1 | grep -E "^test result|FAILED|error\["
```
Expected: all pass.

### Step 7: Commit

```bash
git add src/engine/targeting.rs src/engine/casting.rs src/engine/activated.rs src/serve.rs
git commit -m "$(cat <<'EOF'
feat: add source_colors to targeting; enforce ProtectionFromColor (T in DEBT)

CR 702.16c: protection prevents targeting by sources of the protected quality.
is_legal_target and legal_targets now receive source_colors so Protection from
color can gate targeting at declaration time. Cast and activate call sites
pass the spell/ability source's CardDefinition::colors.

Co-Authored-By: Claude Sonnet 4.6 <noreply@anthropic.com>
EOF
)"
```

---

## Task 5: Combat blocking — Fear, Intimidate, Landwalk, Protection

**Files:**
- Modify: `src/engine/combat.rs`

### Step 1: Write failing tests

Add to the tests in `src/engine/combat.rs` (find the `#[cfg(test)]` block). You'll need a helper that places a creature with specific abilities and optionally specific colors.

Add a helper first:

```rust
fn place_creature_with_colors(
    state: &mut GameState,
    owner: PlayerId,
    abilities: Vec<OracleSpan>,
    colors: Vec<ManaColor>,
) -> ObjectId {
    use crate::types::card::{CardDefinition, CardType, TypeLine};
    let def = CardDefinition {
        name: "Test Creature".into(),
        mana_cost: None,
        type_line: TypeLine {
            supertypes: vec![],
            card_types: vec![CardType::Creature],
            subtypes: vec![],
        },
        oracle_text: String::new(),
        abilities,
        text_annotations: vec![],
        power: Some(2),
        toughness: Some(2),
        colors,
    };
    let id = state.alloc_id();
    let obj = CardObject::new(id, def, owner, Zone::Battlefield);
    state.battlefield.insert(id, PermanentState::new(&obj.definition));
    state.add_object(obj);
    id
}
```

Then the tests:

```rust
#[test]
fn fear_blocks_non_artifact_non_black_creature() {
    use crate::types::ability::StaticAbility;
    use crate::types::mana::ManaColor;
    let mut gs = make_two_player_state();
    let attacker = place_creature_with_colors(
        &mut gs, PlayerId(0),
        vec![OracleSpan::Parsed(Ability::Static(StaticAbility::Fear))],
        vec![ManaColor::Black],
    );
    gs.combat.attackers = vec![attacker];
    // Green blocker — not artifact, not black
    let blocker = place_creature_with_colors(&mut gs, PlayerId(1), vec![], vec![ManaColor::Green]);
    assert!(!can_block_attacker(&gs, blocker, attacker));
}

#[test]
fn fear_allows_black_creature_to_block() {
    use crate::types::ability::StaticAbility;
    use crate::types::mana::ManaColor;
    let mut gs = make_two_player_state();
    let attacker = place_creature_with_colors(
        &mut gs, PlayerId(0),
        vec![OracleSpan::Parsed(Ability::Static(StaticAbility::Fear))],
        vec![],
    );
    gs.combat.attackers = vec![attacker];
    let blocker = place_creature_with_colors(&mut gs, PlayerId(1), vec![], vec![ManaColor::Black]);
    assert!(can_block_attacker(&gs, blocker, attacker));
}

#[test]
fn intimidate_blocks_different_color_non_artifact() {
    use crate::types::ability::StaticAbility;
    use crate::types::mana::ManaColor;
    let mut gs = make_two_player_state();
    let attacker = place_creature_with_colors(
        &mut gs, PlayerId(0),
        vec![OracleSpan::Parsed(Ability::Static(StaticAbility::Intimidate))],
        vec![ManaColor::Red],
    );
    gs.combat.attackers = vec![attacker];
    // Blue blocker shares no color with Red attacker
    let blocker = place_creature_with_colors(&mut gs, PlayerId(1), vec![], vec![ManaColor::Blue]);
    assert!(!can_block_attacker(&gs, blocker, attacker));
}

#[test]
fn intimidate_allows_same_color_blocker() {
    use crate::types::ability::StaticAbility;
    use crate::types::mana::ManaColor;
    let mut gs = make_two_player_state();
    let attacker = place_creature_with_colors(
        &mut gs, PlayerId(0),
        vec![OracleSpan::Parsed(Ability::Static(StaticAbility::Intimidate))],
        vec![ManaColor::Red],
    );
    gs.combat.attackers = vec![attacker];
    // Red/Green blocker shares Red with attacker
    let blocker = place_creature_with_colors(
        &mut gs, PlayerId(1), vec![], vec![ManaColor::Red, ManaColor::Green]
    );
    assert!(can_block_attacker(&gs, blocker, attacker));
}

#[test]
fn islandwalk_unblockable_when_defender_controls_island() {
    use crate::types::ability::StaticAbility;
    use crate::types::{LandwalkKind, card::{CardType, TypeLine}};
    let mut gs = make_two_player_state();
    let attacker = place_creature_with_colors(
        &mut gs, PlayerId(0),
        vec![OracleSpan::Parsed(Ability::Static(StaticAbility::Landwalk(
            LandwalkKind::LandType("Island".to_string())
        )))],
        vec![],
    );
    gs.combat.attackers = vec![attacker];
    // Place an Island under PlayerId(1)'s control
    let island_def = CardDefinition {
        name: "Island".into(),
        mana_cost: None,
        type_line: TypeLine {
            supertypes: vec![crate::types::card::Supertype::Basic],
            card_types: vec![CardType::Land],
            subtypes: vec!["Island".to_string()],
        },
        oracle_text: String::new(),
        abilities: vec![],
        text_annotations: vec![],
        power: None,
        toughness: None,
        colors: vec![],
    };
    let land_id = gs.alloc_id();
    let obj = CardObject::new(land_id, island_def, PlayerId(1), Zone::Battlefield);
    gs.battlefield.insert(land_id, PermanentState::new(&obj.definition));
    gs.add_object(obj);

    let blocker = place_creature_with_colors(&mut gs, PlayerId(1), vec![], vec![]);
    assert!(!can_block_attacker(&gs, blocker, attacker));
}

#[test]
fn protection_from_red_blocks_red_blocker() {
    use crate::types::ability::StaticAbility;
    use crate::types::mana::ManaColor;
    let mut gs = make_two_player_state();
    // Attacker has Protection from Red
    let attacker = place_creature_with_colors(
        &mut gs, PlayerId(0),
        vec![OracleSpan::Parsed(Ability::Static(
            StaticAbility::ProtectionFromColor(ManaColor::Red)
        ))],
        vec![],
    );
    gs.combat.attackers = vec![attacker];
    // Red blocker tries to block
    let blocker = place_creature_with_colors(&mut gs, PlayerId(1), vec![], vec![ManaColor::Red]);
    assert!(!can_block_attacker(&gs, blocker, attacker));
}
```

### Step 2: Run to verify failures

```bash
cargo test fear_blocks 2>&1 | grep -E "^test result|FAILED|error\["
```

### Step 3: Add a `make_two_player_state` helper if not present

Check if combat.rs tests have such a helper already — if not, add:

```rust
fn make_two_player_state() -> GameState {
    use crate::types::Player;
    GameState::new(vec![
        Player::new(PlayerId(0), "Alice"),
        Player::new(PlayerId(1), "Bob"),
    ])
}
```

### Step 4: Implement evasion checks in `can_block_attacker`

In `src/engine/combat.rs`, find the `can_block_attacker` function (around line 160). After the existing `true` at the end, add these checks **before** the final `true` return:

```rust
    // CR 702.36b: Fear — can't be blocked except by artifact or black creatures
    if attacker_obj.has_keyword(StaticAbility::Fear) {
        let blocker_is_artifact = blocker_obj
            .definition
            .type_line
            .card_types
            .contains(&crate::types::card::CardType::Artifact);
        let blocker_is_black = blocker_obj
            .definition
            .colors
            .contains(&crate::types::mana::ManaColor::Black);
        if !blocker_is_artifact && !blocker_is_black {
            return false;
        }
    }

    // CR 702.13b: Intimidate — can't be blocked except by artifact or same-color
    if attacker_obj.has_keyword(StaticAbility::Intimidate) {
        let blocker_is_artifact = blocker_obj
            .definition
            .type_line
            .card_types
            .contains(&crate::types::card::CardType::Artifact);
        let attacker_colors = &attacker_obj.definition.colors;
        let blocker_colors = &blocker_obj.definition.colors;
        let shares_color = attacker_colors.iter().any(|c| blocker_colors.contains(c));
        if !blocker_is_artifact && !shares_color {
            return false;
        }
    }

    // CR 702.14b: Landwalk — can't be blocked if defending player controls matching land
    {
        use crate::types::{LandwalkKind, StaticAbility};
        let defending_player = state.opponent_of(state.active_player);
        for span in &attacker_obj.definition.abilities {
            if let crate::types::OracleSpan::Parsed(crate::types::Ability::Static(
                StaticAbility::Landwalk(kind)
            )) = span {
                let defender_has_land = state.battlefield.iter().any(|(&land_id, _)| {
                    let land_obj = match state.objects.get(&land_id) {
                        Some(o) => o,
                        None => return false,
                    };
                    if land_obj.controller != defending_player {
                        return false;
                    }
                    if !land_obj.definition.type_line.is_land() {
                        return false;
                    }
                    match kind {
                        LandwalkKind::LandType(t) => {
                            land_obj.definition.type_line.subtypes.contains(t)
                        }
                        LandwalkKind::Nonbasic => !land_obj
                            .definition
                            .type_line
                            .supertypes
                            .contains(&crate::types::card::Supertype::Basic),
                    }
                });
                if defender_has_land {
                    return false;
                }
            }
        }
    }

    // CR 702.16d: Protection — can't be blocked by sources of protected quality
    {
        let blocker_colors = &blocker_obj.definition.colors;
        for span in &attacker_obj.definition.abilities {
            if let crate::types::OracleSpan::Parsed(crate::types::Ability::Static(
                StaticAbility::ProtectionFromColor(c)
            )) = span
            {
                if blocker_colors.contains(c) {
                    return false;
                }
            }
        }
    }

    true
```

Also add `use crate::types::mana::ManaColor;` near the top of `can_block_attacker` if needed.

### Step 5: Run tests

```bash
cargo test 2>&1 | grep -E "^test result|FAILED|error\["
```

### Step 6: Commit

```bash
git add src/engine/combat.rs
git commit -m "$(cat <<'EOF'
feat: enforce Fear, Intimidate, Landwalk, Protection blocking in can_block_attacker

CR 702.36b Fear: only artifact or black creatures may block.
CR 702.13b Intimidate: only artifact or same-color creatures may block.
CR 702.14b Landwalk: unblockable if defending player controls matching land.
CR 702.16d Protection: can't be blocked by creatures with protected quality.

Co-Authored-By: Claude Sonnet 4.6 <noreply@anthropic.com>
EOF
)"
```

---

## Task 6: Battle Cry in `collect_attack_triggers`

**Files:**
- Modify: `src/engine/triggered.rs`

### Step 1: Write failing test

Add to `src/engine/triggered.rs` tests:

```rust
#[test]
fn battle_cry_boosts_each_other_attacker_not_self() {
    use crate::types::ability::{Ability, StaticAbility};
    use crate::types::card::{CardDefinition, CardType, TypeLine};
    use crate::types::mana::{ManaColor, ManaCost};
    use crate::types::{CardObject, GameState, OracleSpan, PermanentState, Player, PlayerId, Zone};

    let mut gs = GameState::new(vec![
        Player::new(PlayerId(0), "Alice"),
        Player::new(PlayerId(1), "Bob"),
    ]);
    gs.step = crate::types::Step::DeclareAttackers;
    gs.active_player = PlayerId(0);

    fn make_creature(
        gs: &mut GameState,
        owner: PlayerId,
        abilities: Vec<OracleSpan>,
    ) -> ObjectId {
        let def = CardDefinition {
            name: "Creature".into(),
            mana_cost: None,
            type_line: TypeLine {
                supertypes: vec![],
                card_types: vec![CardType::Creature],
                subtypes: vec![],
            },
            oracle_text: String::new(),
            abilities,
            text_annotations: vec![],
            power: Some(2),
            toughness: Some(2),
            colors: vec![],
        };
        let id = gs.alloc_id();
        let obj = CardObject::new(id, def, owner, Zone::Battlefield);
        let mut perm = PermanentState::new(&obj.definition);
        perm.controller_since_turn = 0;
        gs.battlefield.insert(id, perm);
        gs.add_object(obj);
        id
    }

    let battle_cry_id = make_creature(
        &mut gs,
        PlayerId(0),
        vec![OracleSpan::Parsed(Ability::Static(StaticAbility::BattleCry))],
    );
    let ally1 = make_creature(&mut gs, PlayerId(0), vec![]);
    let ally2 = make_creature(&mut gs, PlayerId(0), vec![]);

    gs.combat.attackers = vec![battle_cry_id, ally1, ally2];

    let triggers = collect_attack_triggers(&mut gs);

    // Battle Cry source boosts ally1 and ally2 (not itself)
    let battle_cry_triggers: Vec<_> = triggers
        .iter()
        .filter(|t| {
            matches!(
                &t.payload,
                crate::types::StackPayload::TriggeredAbility { source_id, label, .. }
                if *source_id == battle_cry_id && label == "Battle Cry"
            )
        })
        .collect();
    assert_eq!(battle_cry_triggers.len(), 2, "should boost 2 other attackers");

    // Confirm neither trigger targets the battle cry creature itself
    for t in &battle_cry_triggers {
        if let crate::types::StackPayload::TriggeredAbility { .. } = &t.payload {
            assert_ne!(t.targets, vec![crate::types::effect::EffectTarget::Object { id: battle_cry_id }]);
        }
    }
}
```

### Step 2: Run to verify failure

```bash
cargo test battle_cry_boosts 2>&1 | grep -E "^test result|FAILED|error\["
```

### Step 3: Add Battle Cry to `collect_attack_triggers`

In `src/engine/triggered.rs`, inside `collect_attack_triggers`, add this block after the Melee section (before the final `result`):

```rust
    // Battle Cry (CR 702.91b): when this attacks, each OTHER attacking creature gets +1/+0.
    let battle_cry_attackers: Vec<ObjectId> = attackers
        .iter()
        .filter(|&&id| {
            state
                .battlefield
                .get(&id)
                .map(|p| p.has_keyword(StaticAbility::BattleCry))
                .unwrap_or(false)
        })
        .copied()
        .collect();
    for source_id in battle_cry_attackers {
        for &other_id in attackers.iter().filter(|&&id| id != source_id) {
            let sid = state.alloc_stack_id();
            use crate::types::effect::EffectTarget;
            result.push(StackObject {
                id: sid,
                payload: StackPayload::TriggeredAbility {
                    source_id,
                    effect: vec![EffectStep::BoostPermanentPT(PTDelta {
                        power: 1,
                        toughness: 0,
                    })],
                    label: "Battle Cry".into(),
                },
                controller: attacking_player,
                targets: vec![EffectTarget::Object { id: other_id }],
            });
        }
    }
```

### Step 4: Run tests

```bash
cargo test 2>&1 | grep -E "^test result|FAILED|error\["
```

### Step 5: Commit

```bash
git add src/engine/triggered.rs
git commit -m "$(cat <<'EOF'
feat: implement Battle Cry (CR 702.91b) in collect_attack_triggers

When a Battle Cry creature attacks, each OTHER attacking creature gets +1/+0
until EOT. Generates one BoostPermanentPT trigger per other attacker.

Co-Authored-By: Claude Sonnet 4.6 <noreply@anthropic.com>
EOF
)"
```

---

## Task 7: Ward trigger generation in `cast_spell` and `activate_ability`

**Files:**
- Modify: `src/engine/casting.rs`
- Modify: `src/engine/activated.rs`

### Step 1: Write failing test

Add to `src/engine/casting.rs` tests:

```rust
#[test]
fn cast_spell_targeting_warded_creature_pushes_ward_trigger() {
    use crate::types::ability::{Ability, SpellAbility, StaticAbility, TargetFilter};
    use crate::types::card::{CardDefinition, CardType, TypeLine};
    use crate::types::effect::EffectTarget;
    use crate::types::mana::{ManaColor, ManaCost, ManaPip};
    use crate::types::stack::StackPayload;
    use crate::types::{CardObject, OracleSpan, PermanentState, WardCost};

    let mut gs = make_state();
    gs.get_player_mut(PlayerId(0)).unwrap().mana_pool.green += 1;

    // Place a Ward {2} creature under opponent's control
    let warded_def = CardDefinition {
        name: "Warded Creature".into(),
        mana_cost: None,
        type_line: TypeLine {
            supertypes: vec![],
            card_types: vec![CardType::Creature],
            subtypes: vec![],
        },
        oracle_text: String::new(),
        abilities: vec![OracleSpan::Parsed(Ability::Static(StaticAbility::WardMana(
            ManaCost { pips: vec![ManaPip::Generic(2)] },
        )))],
        text_annotations: vec![],
        power: Some(2),
        toughness: Some(2),
        colors: vec![],
    };
    let warded_id = gs.alloc_id();
    let warded_obj = CardObject::new(warded_id, warded_def, PlayerId(1), Zone::Battlefield);
    gs.battlefield.insert(warded_id, PermanentState::new(&warded_obj.definition));
    gs.add_object(warded_obj);

    // Spell targeting the warded creature
    let spell_def = CardDefinition {
        name: "Giant Growth".into(),
        mana_cost: Some(ManaCost { pips: vec![ManaPip::Green] }),
        type_line: TypeLine {
            supertypes: vec![],
            card_types: vec![CardType::Instant],
            subtypes: vec![],
        },
        oracle_text: "Target creature gets +3/+3 until end of turn.".into(),
        abilities: vec![OracleSpan::Parsed(Ability::SpellEffect(SpellAbility {
            target_requirements: vec![TargetFilter::Creature],
            steps: vec![],
        }))],
        text_annotations: vec![],
        power: None,
        toughness: None,
        colors: vec![ManaColor::Green],
    };
    let spell_id = put_in_hand(&mut gs, PlayerId(0), spell_def);

    let gs = cast_spell(
        gs,
        PlayerId(0),
        spell_id,
        vec![EffectTarget::Object { id: warded_id }],
    )
    .unwrap();

    // Stack: [spell, ward_trigger] — ward trigger is on top (last pushed)
    assert_eq!(gs.stack.len(), 2);
    let top_id = *gs.stack.last().unwrap();
    let top = &gs.stack_objects[&top_id];
    assert!(matches!(
        &top.payload,
        StackPayload::WardTrigger { cost: WardCost::Mana(_), paid: false, .. }
    ));
}
```

### Step 2: Run to verify failure

```bash
cargo test cast_spell_targeting_warded 2>&1 | grep -E "^test result|FAILED|error\["
```

### Step 3: Implement Ward trigger generation in `cast_spell`

In `src/engine/casting.rs`, after the existing cast triggers are pushed (after the `for t in cast_triggers` loop, before the final `Ok(state)`), add:

```rust
    // CR 702.21b: Ward — when this permanent becomes the target of a spell or ability
    // an opponent controls, that player must pay the Ward cost or the spell is countered.
    {
        use crate::types::ability::{Ability, StaticAbility};
        use crate::types::stack::{StackObject, StackPayload};
        use crate::types::WardCost;
        let spell_stack_id = stack_id; // already captured above
        let spell_obj = state.stack_objects.get(&spell_stack_id).unwrap();
        let targets = spell_obj.targets.clone();
        let caster = spell_obj.controller;

        let mut ward_triggers = Vec::new();
        for target in &targets {
            if let crate::types::effect::EffectTarget::Object { id: target_id } = target {
                let target_obj = match state.objects.get(target_id) {
                    Some(o) => o,
                    None => continue,
                };
                // Ward only fires when an opponent's permanent is targeted
                if target_obj.controller == caster {
                    continue;
                }
                for span in &target_obj.definition.abilities {
                    let ward_cost = match span {
                        OracleSpan::Parsed(Ability::Static(StaticAbility::WardMana(cost))) => {
                            Some(WardCost::Mana(cost.clone()))
                        }
                        OracleSpan::Parsed(Ability::Static(StaticAbility::WardLife(n))) => {
                            Some(WardCost::Life(*n))
                        }
                        _ => None,
                    };
                    if let Some(cost) = ward_cost {
                        let ward_id = state.alloc_stack_id();
                        ward_triggers.push(StackObject {
                            id: ward_id,
                            payload: StackPayload::WardTrigger {
                                counters_if_unpaid: spell_stack_id,
                                cost,
                                paid: false,
                            },
                            controller: caster, // spell controller must pay
                            targets: vec![],
                        });
                    }
                }
            }
        }
        for t in ward_triggers {
            let id = t.id;
            state.stack.push(id);
            state.stack_objects.insert(id, t);
        }
    }
```

You also need to import `OracleSpan` and `Ability` at the top of `casting.rs` if not already present:

```rust
use crate::types::{GameState, ObjectId, OracleSpan, PermanentState, PlayerId, Step, Zone};
use crate::types::ability::Ability;
```

### Step 4: Implement Ward trigger generation in `activate_ability`

In `src/engine/activated.rs`, after the ability's stack object is pushed (find the section where `ActivatedAbility` is pushed to the stack), add the same Ward trigger generation block. The source of the ability is `object_id`, and the caster is `activating_player`. The targets are `declared_targets`.

Look for where the ActivatedAbility StackObject is pushed (search for `StackPayload::ActivatedAbility`), and after that push, add:

```rust
    // CR 702.21b: Ward triggers for non-mana abilities
    if !produces_mana {
        use crate::types::ability::{Ability, StaticAbility};
        use crate::types::stack::{StackObject, StackPayload as SP};
        use crate::types::WardCost;
        use crate::types::OracleSpan;
        let mut ward_triggers = Vec::new();
        for target in &declared_targets {
            if let crate::types::effect::EffectTarget::Object { id: target_id } = target {
                let target_obj = match state.objects.get(target_id) {
                    Some(o) => o,
                    None => continue,
                };
                if target_obj.controller == activating_player {
                    continue;
                }
                for span in &target_obj.definition.abilities {
                    let ward_cost = match span {
                        OracleSpan::Parsed(Ability::Static(StaticAbility::WardMana(cost))) => {
                            Some(WardCost::Mana(cost.clone()))
                        }
                        OracleSpan::Parsed(Ability::Static(StaticAbility::WardLife(n))) => {
                            Some(WardCost::Life(*n))
                        }
                        _ => None,
                    };
                    if let Some(cost) = ward_cost {
                        let ward_id = state.alloc_stack_id();
                        ward_triggers.push(StackObject {
                            id: ward_id,
                            payload: SP::WardTrigger {
                                counters_if_unpaid: ability_stack_id,
                                cost,
                                paid: false,
                            },
                            controller: activating_player,
                            targets: vec![],
                        });
                    }
                }
            }
        }
        for t in ward_triggers {
            let id = t.id;
            state.stack.push(id);
            state.stack_objects.insert(id, t);
        }
    }
```

Note: You'll need to capture `ability_stack_id` when creating the ability's StackObject — look for where `alloc_stack_id()` is called for the ability and save it as `ability_stack_id`.

### Step 5: Run tests

```bash
cargo test 2>&1 | grep -E "^test result|FAILED|error\["
```

### Step 6: Commit

```bash
git add src/engine/casting.rs src/engine/activated.rs
git commit -m "$(cat <<'EOF'
feat: generate WardTrigger stack objects when targeting warded permanents

CR 702.21b: when an opponent's spell or ability targets a permanent with Ward,
a WardTrigger is pushed above the spell on the stack. The spell's controller
must pay the Ward cost before passing priority, or the spell is countered.

Co-Authored-By: Claude Sonnet 4.6 <noreply@anthropic.com>
EOF
)"
```

---

## Task 8: Ward payment — `engine/ward.rs`

**Files:**
- Create: `src/engine/ward.rs`

### Step 1: Write failing test

Create `src/engine/ward.rs` with only the test module first:

```rust
use super::EngineError;
use crate::types::stack::StackId;
use crate::types::{GameState, PlayerId};

pub fn pay_ward(
    _state: GameState,
    _player_id: PlayerId,
    _trigger_id: StackId,
) -> Result<GameState, EngineError> {
    todo!()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::engine::casting::cast_spell;
    use crate::types::ability::{Ability, SpellAbility, StaticAbility, TargetFilter};
    use crate::types::card::{CardDefinition, CardType, TypeLine};
    use crate::types::effect::EffectTarget;
    use crate::types::mana::{ManaCost, ManaPip};
    use crate::types::stack::StackPayload;
    use crate::types::{CardObject, OracleSpan, PermanentState, Player, PlayerId, Step, WardCost, Zone};

    fn setup_ward_scenario() -> (GameState, crate::types::ObjectId, crate::types::stack::StackId) {
        let mut gs = GameState::new(vec![
            Player::new(PlayerId(0), "Alice"),
            Player::new(PlayerId(1), "Bob"),
        ]);
        gs.step = Step::PreCombatMain;
        gs.get_player_mut(PlayerId(0)).unwrap().mana_pool.green += 1;

        // Warded creature under player 1
        let warded_def = CardDefinition {
            name: "Warded".into(),
            mana_cost: None,
            type_line: TypeLine {
                supertypes: vec![],
                card_types: vec![CardType::Creature],
                subtypes: vec![],
            },
            oracle_text: String::new(),
            abilities: vec![OracleSpan::Parsed(Ability::Static(StaticAbility::WardMana(
                ManaCost { pips: vec![ManaPip::Generic(2)] },
            )))],
            text_annotations: vec![],
            power: Some(2),
            toughness: Some(2),
            colors: vec![],
        };
        let warded_id = gs.alloc_id();
        let wo = CardObject::new(warded_id, warded_def, PlayerId(1), Zone::Battlefield);
        gs.battlefield.insert(warded_id, PermanentState::new(&wo.definition));
        gs.add_object(wo);

        // Targeted spell from player 0
        let spell_def = CardDefinition {
            name: "Bolt".into(),
            mana_cost: Some(ManaCost { pips: vec![ManaPip::Green] }),
            type_line: TypeLine {
                supertypes: vec![],
                card_types: vec![CardType::Instant],
                subtypes: vec![],
            },
            oracle_text: "Target creature gets +3/+3 until end of turn.".into(),
            abilities: vec![OracleSpan::Parsed(Ability::SpellEffect(SpellAbility {
                target_requirements: vec![TargetFilter::Creature],
                steps: vec![],
            }))],
            text_annotations: vec![],
            power: None,
            toughness: None,
            colors: vec![],
        };
        let spell_id = gs.alloc_id();
        let so = CardObject::new(spell_id, spell_def, PlayerId(0), Zone::Hand);
        gs.hands.get_mut(&PlayerId(0)).unwrap().push(spell_id);
        gs.add_object(so);

        let gs = cast_spell(
            gs,
            PlayerId(0),
            spell_id,
            vec![EffectTarget::Object { id: warded_id }],
        )
        .unwrap();

        // Find the WardTrigger stack ID
        let ward_id = *gs.stack.last().unwrap();
        (gs, warded_id, ward_id)
    }

    #[test]
    fn pay_ward_mana_marks_trigger_paid() {
        let (mut gs, _, ward_id) = setup_ward_scenario();
        // Give player 0 the 2 generic mana for Ward {2}
        gs.get_player_mut(PlayerId(0)).unwrap().mana_pool.colorless += 2;

        let gs = pay_ward(gs, PlayerId(0), ward_id).unwrap();

        let top = &gs.stack_objects[&ward_id];
        assert!(matches!(
            &top.payload,
            StackPayload::WardTrigger { paid: true, .. }
        ));
    }

    #[test]
    fn pay_ward_insufficient_mana_returns_error() {
        let (gs, _, ward_id) = setup_ward_scenario();
        // Player 0 has no mana left (spent it on the spell)
        let result = pay_ward(gs, PlayerId(0), ward_id);
        assert!(matches!(result, Err(EngineError::InsufficientMana)));
    }
}
```

### Step 2: Run to verify the todo!() panic

```bash
cargo test pay_ward 2>&1 | grep -E "^test result|FAILED|error\["
```

### Step 3: Implement `pay_ward`

Replace the `todo!()` stub with the full implementation:

```rust
use super::{EngineError, mana::{greedy_payment_plan, pay_mana_cost}};
use crate::types::stack::{StackId, StackPayload};
use crate::types::{GameState, PlayerId, WardCost};

/// CR 702.21b: pay the Ward cost on a WardTrigger stack object.
/// Validates the trigger is on top of the stack, the payer controls the spell
/// being countered, and the payment is sufficient. Marks `paid = true`.
pub fn pay_ward(
    mut state: GameState,
    player_id: PlayerId,
    trigger_id: StackId,
) -> Result<GameState, EngineError> {
    // Validate trigger exists and is a WardTrigger
    let (counters_if_unpaid, cost) = {
        let obj = state
            .stack_objects
            .get(&trigger_id)
            .ok_or(EngineError::CardNotFound)?;
        match &obj.payload {
            StackPayload::WardTrigger { counters_if_unpaid, cost, .. } => {
                (*counters_if_unpaid, cost.clone())
            }
            _ => return Err(EngineError::NotYourPriority),
        }
    };

    // Validate: player_id must control the spell being countered (the payer)
    let spell_controller = state
        .stack_objects
        .get(&counters_if_unpaid)
        .map(|o| o.controller)
        .ok_or(EngineError::CardNotFound)?;
    if spell_controller != player_id {
        return Err(EngineError::NotYourPriority);
    }

    // Pay the Ward cost
    match &cost {
        WardCost::Mana(mana_cost) => {
            let player = state
                .get_player(player_id)
                .ok_or(EngineError::CardNotFound)?;
            let plan = greedy_payment_plan(mana_cost, &player.mana_pool, player.life)
                .ok_or(EngineError::InsufficientMana)?;
            state = pay_mana_cost(state, player_id, mana_cost, &plan)?;
        }
        WardCost::Life(n) => {
            let player = state
                .get_player_mut(player_id)
                .ok_or(EngineError::CardNotFound)?;
            if player.life < *n as i32 {
                return Err(EngineError::InsufficientLife);
            }
            player.life -= *n as i32;
        }
    }

    // Mark the trigger as paid
    if let Some(obj) = state.stack_objects.get_mut(&trigger_id) {
        if let StackPayload::WardTrigger { ref mut paid, .. } = obj.payload {
            *paid = true;
        }
    }

    Ok(state)
}
```

### Step 4: Run tests

```bash
cargo test 2>&1 | grep -E "^test result|FAILED|error\["
```

### Step 5: Commit

```bash
git add src/engine/ward.rs src/engine/mod.rs
git commit -m "$(cat <<'EOF'
feat: implement pay_ward — Ward cost payment for mana and life

CR 702.21b: the controller of a spell targeting a Ward permanent must pay
the Ward cost (mana or life) before passing priority. Marking paid=true
allows the spell to survive when the WardTrigger resolves.

Co-Authored-By: Claude Sonnet 4.6 <noreply@anthropic.com>
EOF
)"
```

---

## Task 9: Ward resolution in `resolve_top` + `counter_spell_on_stack`

**Files:**
- Modify: `src/engine/stack.rs`

### Step 1: Write failing tests

Add to `src/engine/stack.rs` tests:

```rust
#[test]
fn unpaid_ward_trigger_counters_spell() {
    // Set up: spell on stack, WardTrigger on top (unpaid)
    // After passing priority twice, WardTrigger resolves and counters the spell.
    use crate::engine::casting::cast_spell;
    use crate::engine::ward::pay_ward;
    use crate::types::ability::{Ability, SpellAbility, StaticAbility, TargetFilter};
    use crate::types::card::{CardDefinition, CardType, TypeLine};
    use crate::types::effect::EffectTarget;
    use crate::types::mana::{ManaCost, ManaPip};
    use crate::types::{CardObject, OracleSpan, PermanentState, Player, PlayerId, Step, Zone};

    let mut gs = GameState::new(vec![
        Player::new(PlayerId(0), "Alice"),
        Player::new(PlayerId(1), "Bob"),
    ]);
    gs.step = Step::PreCombatMain;
    gs.get_player_mut(PlayerId(0)).unwrap().mana_pool.green += 1;

    // Warded creature under player 1
    let warded_def = CardDefinition {
        name: "Warded".into(),
        mana_cost: None,
        type_line: TypeLine { supertypes: vec![], card_types: vec![CardType::Creature], subtypes: vec![] },
        oracle_text: String::new(),
        abilities: vec![OracleSpan::Parsed(Ability::Static(StaticAbility::WardMana(
            ManaCost { pips: vec![ManaPip::Generic(2)] },
        )))],
        text_annotations: vec![],
        power: Some(2),
        toughness: Some(2),
        colors: vec![],
    };
    let warded_id = gs.alloc_id();
    let wo = CardObject::new(warded_id, warded_def, PlayerId(1), Zone::Battlefield);
    gs.battlefield.insert(warded_id, PermanentState::new(&wo.definition));
    gs.add_object(wo);

    // Targeted spell
    let spell_def = CardDefinition {
        name: "Shatter".into(),
        mana_cost: Some(ManaCost { pips: vec![ManaPip::Green] }),
        type_line: TypeLine { supertypes: vec![], card_types: vec![CardType::Instant], subtypes: vec![] },
        oracle_text: "Target creature gets +3/+3.".into(),
        abilities: vec![OracleSpan::Parsed(Ability::SpellEffect(SpellAbility {
            target_requirements: vec![TargetFilter::Creature],
            steps: vec![],
        }))],
        text_annotations: vec![],
        power: None,
        toughness: None,
        colors: vec![],
    };
    let spell_id = gs.alloc_id();
    let so = CardObject::new(spell_id, spell_def, PlayerId(0), Zone::Hand);
    gs.hands.get_mut(&PlayerId(0)).unwrap().push(spell_id);
    gs.add_object(so);

    let gs = cast_spell(gs, PlayerId(0), spell_id, vec![EffectTarget::Object { id: warded_id }]).unwrap();
    // Stack: [spell, ward_trigger]; ward_trigger is on top
    assert_eq!(gs.stack.len(), 2);

    // Both players pass without paying Ward
    let gs = pass_priority(gs, PlayerId(0)).unwrap();
    let gs = pass_priority(gs, PlayerId(1)).unwrap();

    // WardTrigger resolves: spell should be countered (removed from stack)
    // and spell card should be in graveyard
    assert_eq!(gs.stack.len(), 0, "both spell and trigger should be gone");
    let spell_zone = gs.objects[&spell_id].zone;
    assert_eq!(spell_zone, Zone::Graveyard);
}

#[test]
fn paid_ward_trigger_does_not_counter_spell() {
    use crate::engine::casting::cast_spell;
    use crate::engine::ward::pay_ward;
    use crate::types::ability::{Ability, SpellAbility, StaticAbility, TargetFilter};
    use crate::types::card::{CardDefinition, CardType, TypeLine};
    use crate::types::effect::EffectTarget;
    use crate::types::mana::{ManaCost, ManaPip};
    use crate::types::{CardObject, OracleSpan, PermanentState, Player, PlayerId, Step, Zone};

    let mut gs = GameState::new(vec![
        Player::new(PlayerId(0), "Alice"),
        Player::new(PlayerId(1), "Bob"),
    ]);
    gs.step = Step::PreCombatMain;
    gs.get_player_mut(PlayerId(0)).unwrap().mana_pool.green += 1;

    // Same setup as above (warded creature + spell)
    let warded_def = CardDefinition {
        name: "Warded".into(),
        mana_cost: None,
        type_line: TypeLine { supertypes: vec![], card_types: vec![CardType::Creature], subtypes: vec![] },
        oracle_text: String::new(),
        abilities: vec![OracleSpan::Parsed(Ability::Static(StaticAbility::WardMana(
            ManaCost { pips: vec![ManaPip::Generic(2)] },
        )))],
        text_annotations: vec![],
        power: Some(2),
        toughness: Some(2),
        colors: vec![],
    };
    let warded_id = gs.alloc_id();
    let wo = CardObject::new(warded_id, warded_def, PlayerId(1), Zone::Battlefield);
    gs.battlefield.insert(warded_id, PermanentState::new(&wo.definition));
    gs.add_object(wo);

    let spell_def = CardDefinition {
        name: "Shatter".into(),
        mana_cost: Some(ManaCost { pips: vec![ManaPip::Green] }),
        type_line: TypeLine { supertypes: vec![], card_types: vec![CardType::Instant], subtypes: vec![] },
        oracle_text: "Target creature gets +3/+3.".into(),
        abilities: vec![OracleSpan::Parsed(Ability::SpellEffect(SpellAbility {
            target_requirements: vec![TargetFilter::Creature],
            steps: vec![],
        }))],
        text_annotations: vec![],
        power: None,
        toughness: None,
        colors: vec![],
    };
    let spell_id = gs.alloc_id();
    let so = CardObject::new(spell_id, spell_def, PlayerId(0), Zone::Hand);
    gs.hands.get_mut(&PlayerId(0)).unwrap().push(spell_id);
    gs.add_object(so);

    let gs = cast_spell(gs, PlayerId(0), spell_id, vec![EffectTarget::Object { id: warded_id }]).unwrap();
    let ward_id = *gs.stack.last().unwrap();

    // Player pays Ward {2}
    gs.get_player_mut(PlayerId(0)).unwrap().mana_pool.colorless += 2;
    // (need to make state mutable to add mana — but pay_ward takes ownership)
    // Restructure: get a mut reference before calling pay_ward
    let gs = {
        let mut gs = gs;
        gs.get_player_mut(PlayerId(0)).unwrap().mana_pool.colorless += 2;
        gs
    };
    let gs = pay_ward(gs, PlayerId(0), ward_id).unwrap();

    // Both players pass
    let gs = pass_priority(gs, PlayerId(0)).unwrap();
    let gs = pass_priority(gs, PlayerId(1)).unwrap();

    // Ward trigger resolved (paid) — spell should still be on stack
    assert_eq!(gs.stack.len(), 1, "spell survives when Ward is paid");
    assert_eq!(gs.objects[&spell_id].zone, Zone::Stack);
}
```

### Step 2: Run to verify failures

```bash
cargo test unpaid_ward_trigger 2>&1 | grep -E "^test result|FAILED|error\["
cargo test paid_ward_trigger 2>&1 | grep -E "^test result|FAILED|error\["
```

### Step 3: Add `counter_spell_on_stack` helper and `WardTrigger` case to `resolve_top`

In `src/engine/stack.rs`, find `resolve_top` (look for `fn resolve_top`). Add this helper function after the `execute_effect_steps` function:

```rust
/// Remove a spell from the stack and move its card to the graveyard.
/// Used by Ward resolution when the Ward cost is not paid (CR 702.21c).
fn counter_spell_on_stack(state: &mut GameState, spell_stack_id: StackId) {
    // Remove the spell from the stack vec
    state.stack.retain(|&id| id != spell_stack_id);
    // Get the card ObjectId from the spell's payload
    if let Some(spell_obj) = state.stack_objects.remove(&spell_stack_id) {
        if let StackPayload::Spell { card_id } = spell_obj.payload {
            // Move card to graveyard of the spell's controller
            if let Some(obj) = state.objects.get_mut(&card_id) {
                obj.zone = Zone::Graveyard;
            }
            if let Some(gy) = state.graveyards.get_mut(&spell_obj.controller) {
                gy.push(card_id);
            }
        }
    }
}
```

Then in `resolve_top`, find the match on `StackPayload` (or the top-of-stack resolution logic) and add a `WardTrigger` arm. Look for where `StackPayload::Spell`, `StackPayload::TriggeredAbility`, and `StackPayload::ActivatedAbility` are handled. Add:

```rust
StackPayload::WardTrigger { counters_if_unpaid, paid, .. } => {
    if !paid {
        counter_spell_on_stack(&mut state, counters_if_unpaid);
    }
    // WardTrigger itself was already popped from the stack at the top of resolve_top
}
```

**Note:** You need to find exactly where in `resolve_top` the payload is matched. The function pops the top stack object and then dispatches on its payload. The `WardTrigger` arm just calls `counter_spell_on_stack` if unpaid; the trigger itself is already handled by the pop.

Also add `Zone` to the imports in `stack.rs` if not already present:
```rust
use crate::types::{GameState, PermanentState, PlayerId, Zone};
```

### Step 4: Run tests

```bash
cargo test 2>&1 | grep -E "^test result|FAILED|error\["
```

### Step 5: Commit

```bash
git add src/engine/stack.rs
git commit -m "$(cat <<'EOF'
feat: resolve WardTrigger in resolve_top; add counter_spell_on_stack

CR 702.21c: if the Ward cost is not paid, the triggering spell or ability
is countered. counter_spell_on_stack removes the spell from the stack and
moves its card to the graveyard.

Co-Authored-By: Claude Sonnet 4.6 <noreply@anthropic.com>
EOF
)"
```

---

## Task 10: `serve.rs` `/pay_ward` endpoint + `docs/todo.md` notes + clippy

**Files:**
- Modify: `src/serve.rs`
- Modify: `docs/todo.md`

### Step 1: Add `/pay_ward` endpoint to serve.rs

Find where other action endpoints are handled in `serve.rs` (search for `"cast_spell"` or `"pass_priority"` in the match arm that dispatches actions). Add a `pay_ward` case:

```rust
"pay_ward" => {
    use mecha_oracle::engine::ward::pay_ward;
    let trigger_id = body["trigger_id"]
        .as_u64()
        .ok_or("missing trigger_id")
        .map(|id| mecha_oracle::types::stack::StackId(id))?;
    let new_state = pay_ward(state.clone(), pid, trigger_id)
        .map_err(|e| format!("{e:?}"))?;
    // ... persist new_state and return updated game view
}
```

The exact integration depends on how other endpoints are structured. Mirror the pattern used for `"cast_spell"` or `"activate_ability"`.

### Step 2: Add Protection remaining work to `docs/todo.md`

In `docs/todo.md`, replace the existing Protection from X bullet under `## 🎨 Color-tracking block` with the following (the main bullet can say it's partially implemented):

```markdown
- **Protection from X** (702.16): partial implementation — blocking (B in DEBT) and targeting
  (T in DEBT) are done via `ProtectionFromColor(ManaColor)`. Remaining work:
  - **Damage prevention (D in DEBT)**: prevent all damage from sources with protected quality
    — requires a "protection check" in combat damage path and `DealDamage` effect step.
  - **Enchant/Equip prevention (E in DEBT)**: can't be enchanted or equipped by things with
    protected quality — requires aura attachment rules (future work).
  - **Protection from non-color qualities**: protection from artifacts, from instants, from a
    creature type, from a card name — needs a richer `ProtectionQuality` enum beyond `ManaColor`.
  - **Protection from everything** (CR 702.16e): shorthand for all qualities — needs
    `StaticAbility::ProtectionFromAll`.
  - **Hexproof from color** (CR 702.11e, e.g. "hexproof from black") — related but separate;
    currently `ParsedUnimplemented`.
```

Also remove `Ward`, `Landwalk`, `Battle Cry`, `Fear`, and `Intimidate` from the `## ✅ Unblocked` and `## 🎨 Color-tracking block` sections, since they are now implemented.

### Step 3: Run clippy and fix any issues

```bash
cargo clippy --fix --all-targets 2>&1 | head -50
cargo clippy --all-targets 2>&1 | grep -E "^error|^warning\[" | head -30
```

Fix any remaining warnings. Common ones to expect:
- Unused imports (if some spans/arms are now unreachable after removing from `is_cr702_keyword`)
- `match` arms with unreachable patterns

### Step 4: Run full test suite

```bash
cargo test 2>&1 | grep -E "^test result|FAILED|error\["
```
Expected: all pass, no failures.

### Step 5: Commit

```bash
git add src/serve.rs docs/todo.md
git commit -m "$(cat <<'EOF'
feat: add /pay_ward endpoint; document remaining Protection from X work

Adds the server endpoint for paying Ward costs. Updates docs/todo.md to
reflect the 6 newly implemented keywords and documents remaining DEBT legs
for Protection from X (damage, enchant, non-color qualities, from everything).

Co-Authored-By: Claude Sonnet 4.6 <noreply@anthropic.com>
EOF
)"
```

---

## Self-Review

**Spec coverage check:**
- ✅ CardDefinition::colors + Scryfall parsing (Task 1)
- ✅ LandwalkKind, WardCost, 7 StaticAbility variants, ManaPip/ManaCost Display (Task 2)
- ✅ Parser promotes Fear, Intimidate, BattleCry, WardMana, WardLife, Landwalk, ProtectionFromColor (Task 3)
- ✅ Targeting source_colors + Protection check + all call sites (Task 4)
- ✅ Fear/Intimidate/Landwalk/Protection in can_block_attacker (Task 5)
- ✅ Battle Cry in collect_attack_triggers (Task 6)
- ✅ Ward trigger generation in cast_spell + activate_ability (Task 7)
- ✅ pay_ward implementation (Task 8)
- ✅ WardTrigger resolution + counter_spell_on_stack (Task 9)
- ✅ /pay_ward endpoint + docs/todo.md Protection remaining work notes (Task 10)

**Type consistency check:**
- `LandwalkKind` defined in Task 2, used in Tasks 3 and 5 ✅
- `WardCost` defined in Task 2, used in Tasks 7, 8, 9 ✅
- `WardTrigger` defined in Task 2, generated in Task 7, paid in Task 8, resolved in Task 9 ✅
- `source_colors: &[ManaColor]` signature added in Task 4, call sites updated in Task 4 ✅
- `colors: vec![]` in all struct literals (Task 1), `colors: vec![...]` in test helpers (Tasks 5-9) ✅

**Placeholder check:** No TBDs or TODOs in code blocks. All function signatures match usage.
