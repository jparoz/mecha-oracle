# Protection Quality Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Expand the protection system to support non-colour qualities (`ProtectionQuality` enum), add damage/enchant/equip prevention (D and E of DEBT), implement `HexproofFrom(ProtectionQuality)`, and wire everything end-to-end through targeting, combat, and the parser.

**Architecture:** A new `ProtectionQuality` enum in `types/ability.rs` replaces the colour-only `ProtectionFromColor(ManaColor)` variant and provides the shared `source_matches_quality` predicate used by all enforcement sites. A `has_protection_from` helper in `engine/mod.rs` wraps that predicate for use in combat, stack, and SBA code. Source characteristics (colours, card types, subtypes) are threaded through `DamageStep`, `inject_source_flags`, `is_legal_target`, and `legal_targets` so every path can evaluate protection at runtime.

**Tech Stack:** Rust, `cargo test`, `cargo clippy`

## Global Constraints

- Spec: `docs/superpowers/specs/2026-06-23-protection-quality-design.md`
- All CR references must be verified with `grep '^NNN\\.MM' docs/CR.txt` before adding them to code comments
- Run `cargo test 2>&1 | grep -E "^test result|FAILED|error\["` after every task
- Run `cargo clippy --all-targets 2>&1 | grep -E "^error|^warning"` before final commit of each task
- Commit after every task with `Co-Authored-By: Claude Sonnet 4.6 <noreply@anthropic.com>`

---

## File Map

| File | What changes |
|---|---|
| `src/types/ability.rs` | Add `ProtectionQuality`, `source_matches_quality`; rename `ProtectionFromColor` → `ProtectionFrom(ProtectionQuality)`; add `HexproofFrom(ProtectionQuality)` |
| `src/types/effect.rs` | Add `source_colors`, `source_card_types`, `source_subtypes` to `DamageStep` |
| `src/engine/mod.rs` | Add `has_protection_from` shared helper |
| `src/engine/stack.rs` | Expand `inject_source_flags`; add DealDamage protection check; add Attach + aura-ETB protection check |
| `src/engine/triggered.rs` | Update `inject_source_flags` call site |
| `src/engine/activated.rs` | Update `inject_source_flags` call site |
| `src/engine/targeting.rs` | Expand `is_legal_target`/`legal_targets`; add `HexproofFrom` check |
| `src/engine/combat.rs` | Guard attacker→blocker and blocker→attacker damage; update blocking check |
| `src/engine/state_based_actions.rs` | Pass aura card-types/subtypes to `is_legal_target`; add equipment protection detach check |
| `src/parser/oracle.rs` | Extract `parse_protection_quality`; handle all qualities; add `hexproof from` parsing |
| `src/serve.rs` | Update two `legal_targets` call sites |
| `docs/todo.md` | Delete completed bullets |

---

## Task 1: Foundation types in `types/ability.rs`

Add `ProtectionQuality` enum and `source_matches_quality`, rename `ProtectionFromColor` → `ProtectionFrom(ProtectionQuality)`, add `HexproofFrom(ProtectionQuality)`, and update all existing uses so the codebase compiles cleanly.

**Files:**
- Modify: `src/types/ability.rs`
- Modify: `src/engine/combat.rs` (rename in blocking check + tests)
- Modify: `src/engine/targeting.rs` (rename in targeting check + tests)
- Modify: `src/parser/oracle.rs` (rename in parsing + tests)

**Interfaces:**
- Produces:
  - `pub enum ProtectionQuality { Color(ManaColor), CardType(CardType), CreatureType(String), Everything }`
  - `pub fn source_matches_quality(q: &ProtectionQuality, colors: &[ManaColor], card_types: &[CardType], subtypes: &[String]) -> bool`
  - `KeywordAbility::ProtectionFrom(ProtectionQuality)` replacing `ProtectionFromColor(ManaColor)`
  - `KeywordAbility::HexproofFrom(ProtectionQuality)`

- [ ] **Step 1: Write failing tests**

Add to the `#[cfg(test)]` block at the bottom of `src/types/ability.rs`:

```rust
#[test]
fn protection_from_color_display_name_uses_quality() {
    use crate::types::mana::ManaColor;
    assert_eq!(
        KeywordAbility::ProtectionFrom(ProtectionQuality::Color(ManaColor::Blue)).display_name(),
        "Protection from blue"
    );
}

#[test]
fn protection_from_artifact_display_name() {
    use crate::types::card::CardType;
    assert_eq!(
        KeywordAbility::ProtectionFrom(ProtectionQuality::CardType(CardType::Artifact)).display_name(),
        "Protection from artifacts"
    );
}

#[test]
fn protection_from_everything_display_name() {
    assert_eq!(
        KeywordAbility::ProtectionFrom(ProtectionQuality::Everything).display_name(),
        "Protection from everything"
    );
}

#[test]
fn hexproof_from_color_display_name() {
    use crate::types::mana::ManaColor;
    assert_eq!(
        KeywordAbility::HexproofFrom(ProtectionQuality::Color(ManaColor::Black)).display_name(),
        "Hexproof from black"
    );
}

#[test]
fn source_matches_quality_color() {
    use crate::types::mana::ManaColor;
    let q = ProtectionQuality::Color(ManaColor::Blue);
    assert!(source_matches_quality(&q, &[ManaColor::Blue], &[], &[]));
    assert!(!source_matches_quality(&q, &[ManaColor::Red], &[], &[]));
    assert!(!source_matches_quality(&q, &[], &[], &[]));
}

#[test]
fn source_matches_quality_card_type() {
    use crate::types::card::CardType;
    let q = ProtectionQuality::CardType(CardType::Artifact);
    assert!(source_matches_quality(&q, &[], &[CardType::Artifact], &[]));
    assert!(!source_matches_quality(&q, &[], &[CardType::Creature], &[]));
}

#[test]
fn source_matches_quality_creature_type() {
    let q = ProtectionQuality::CreatureType("Vampire".into());
    assert!(source_matches_quality(&q, &[], &[], &["Vampire".to_string()]));
    assert!(source_matches_quality(&q, &[], &[], &["vampire".to_string()]));
    assert!(!source_matches_quality(&q, &[], &[], &["Zombie".to_string()]));
}

#[test]
fn source_matches_quality_everything() {
    let q = ProtectionQuality::Everything;
    assert!(source_matches_quality(&q, &[], &[], &[]));
}
```

- [ ] **Step 2: Run tests — expect compile errors** (types don't exist yet)

```bash
cargo test 2>&1 | grep -E "^error|FAILED|^test result"
```

- [ ] **Step 3: Add `ProtectionQuality` enum and `source_matches_quality` to `ability.rs`**

Insert after the `LandwalkKind` enum (before `KeywordAbility`):

```rust
use super::card::CardType;

// CR 702.16a: the quality that protection applies to.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ProtectionQuality {
    Color(ManaColor),
    CardType(CardType),
    CreatureType(String), // creature subtype, e.g. "Eldrazi", "Vampire"
    Everything,           // CR 702.16j
}

impl ProtectionQuality {
    fn quality_name(&self) -> String {
        match self {
            Self::Color(c) => match c {
                ManaColor::White => "white".to_string(),
                ManaColor::Blue => "blue".to_string(),
                ManaColor::Black => "black".to_string(),
                ManaColor::Red => "red".to_string(),
                ManaColor::Green => "green".to_string(),
                ManaColor::Colorless => "colorless".to_string(),
            },
            Self::CardType(ct) => match ct {
                CardType::Artifact => "artifacts".to_string(),
                CardType::Creature => "creatures".to_string(),
                CardType::Instant => "instants".to_string(),
                CardType::Sorcery => "sorceries".to_string(),
                CardType::Enchantment => "enchantments".to_string(),
                CardType::Land => "lands".to_string(),
                CardType::Planeswalker => "planeswalkers".to_string(),
            },
            Self::CreatureType(s) => s.clone(),
            Self::Everything => "everything".to_string(),
        }
    }
}

// CR 702.16a: returns true if the described source has the given protection quality.
pub fn source_matches_quality(
    quality: &ProtectionQuality,
    source_colors: &[ManaColor],
    source_card_types: &[CardType],
    source_subtypes: &[String],
) -> bool {
    match quality {
        ProtectionQuality::Color(c) => source_colors.contains(c),
        ProtectionQuality::CardType(ct) => source_card_types.contains(ct),
        ProtectionQuality::CreatureType(st) => source_subtypes
            .iter()
            .any(|s| s.eq_ignore_ascii_case(st)),
        ProtectionQuality::Everything => true,
    }
}
```

Note: `use super::card::CardType;` is already present at the top of `ability.rs` — do not duplicate it.

- [ ] **Step 4: Rename `ProtectionFromColor` → `ProtectionFrom` and add `HexproofFrom` in the `KeywordAbility` enum**

Replace:
```rust
    ProtectionFromColor(ManaColor), // CR 702.16 (partial — blocking + targeting only)
```
With:
```rust
    ProtectionFrom(ProtectionQuality), // CR 702.16
    HexproofFrom(ProtectionQuality),   // CR 702.11d
```

- [ ] **Step 5: Update `display_name` in the `impl KeywordAbility` block**

Replace the `Self::ProtectionFromColor(c)` arm:
```rust
            Self::ProtectionFromColor(c) => {
                // CR 105.4: colorless is not a color — ProtectionFromColor(Colorless) should never be constructed
                debug_assert!(
                    *c != ManaColor::Colorless,
                    "ProtectionFromColor: Colorless is not a valid color (CR 105.4)"
                );
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
With:
```rust
            Self::ProtectionFrom(q) => format!("Protection from {}", q.quality_name()),
            Self::HexproofFrom(q) => format!("Hexproof from {}", q.quality_name()),
```

- [ ] **Step 6: Fix compile errors in `combat.rs` — update blocking check**

In `src/engine/combat.rs`, the blocking legality check around line 318–329 uses `SA::ProtectionFromColor(c)`. Replace:
```rust
    // CR 702.16f: Protection — can't be blocked by creatures with the protected quality
    {
        use crate::types::ability::{KeywordAbility as SA, Rule};
        let blocker_colors = &blocker_obj.definition.colors;
        for span in &attacker_obj.definition.rules_text {
            if let crate::types::RulesText::Active(Rule::Static(SA::ProtectionFromColor(c))) = span
                && blocker_colors.contains(c)
            {
                return false;
            }
        }
    }
```
With:
```rust
    // CR 702.16f: Protection — can't be blocked by creatures with the protected quality
    {
        use crate::types::ability::{KeywordAbility as SA, ProtectionQuality, Rule, source_matches_quality};
        let blocker_colors = &blocker_obj.definition.colors;
        let blocker_types = &blocker_obj.definition.type_line.card_types;
        let blocker_subtypes = &blocker_obj.definition.type_line.subtypes;
        for span in &attacker_obj.definition.rules_text {
            if let crate::types::RulesText::Active(Rule::Static(SA::ProtectionFrom(q))) = span {
                if source_matches_quality(q, blocker_colors, blocker_types, blocker_subtypes) {
                    return false;
                }
            }
        }
    }
```

Also update the two blocking tests in `combat.rs` that reference `ProtectionFromColor`:
```rust
// Find and replace both occurrences:
KeywordAbility::ProtectionFromColor(ManaColor::Red)
// → 
KeywordAbility::ProtectionFrom(ProtectionQuality::Color(ManaColor::Red))
```
Add the import `use crate::types::ability::ProtectionQuality;` to the test module at the top of the tests block in `combat.rs`.

- [ ] **Step 7: Fix compile errors in `targeting.rs` — update protection check and tests**

In `src/engine/targeting.rs`, the `is_legal_target` function around line 49–56 uses `ProtectionFromColor`. Replace:
```rust
            // CR 702.16c: protection prevents targeting by sources of protected quality
            for span in &obj.definition.rules_text {
                if let RulesText::Active(Rule::Static(KeywordAbility::ProtectionFromColor(c))) =
                    span
                    && source_colors.contains(c)
                {
                    return false;
                }
            }
```
With:
```rust
            // CR 702.16c: protection prevents targeting by sources of protected quality
            for span in &obj.definition.rules_text {
                if let RulesText::Active(Rule::Static(KeywordAbility::ProtectionFrom(q))) = span {
                    if crate::types::ability::source_matches_quality(
                        q,
                        source_colors,
                        &[],
                        &[],
                    ) {
                        return false;
                    }
                }
            }
```

Note: `source_card_types` and `source_subtypes` are `&[]` here because `is_legal_target` doesn't yet accept them — that expansion happens in Task 4. For now this compiles and correctly handles the colour case.

Update the two tests in `targeting.rs` that reference `ProtectionFromColor`:
```rust
// Both occurrences:
KeywordAbility::ProtectionFromColor(ManaColor::Blue)
// →
KeywordAbility::ProtectionFrom(ProtectionQuality::Color(ManaColor::Blue))
```
Add `use crate::types::ability::ProtectionQuality;` to the test module imports.

- [ ] **Step 8: Fix compile errors in `oracle.rs` — update parsing and tests**

In `src/parser/oracle.rs` around line 533, replace:
```rust
            return RulesText::Active(Rule::Static(KeywordAbility::ProtectionFromColor(c)));
```
With:
```rust
            return RulesText::Active(Rule::Static(KeywordAbility::ProtectionFrom(
                crate::types::ability::ProtectionQuality::Color(c),
            )));
```

Update the test at line ~3009 that references `ProtectionFromColor`:
```rust
// Find:
KeywordAbility::ProtectionFromColor(ManaColor::Blue)
// Replace with:
KeywordAbility::ProtectionFrom(crate::types::ability::ProtectionQuality::Color(ManaColor::Blue))
```

- [ ] **Step 9: Run tests — all should pass**

```bash
cargo test 2>&1 | grep -E "^test result|FAILED|error\["
```
Expected: `test result: ok`

- [ ] **Step 10: Clippy**

```bash
cargo clippy --all-targets 2>&1 | grep -E "^error|^warning"
```
Fix any warnings.

- [ ] **Step 11: Commit**

```bash
git add src/types/ability.rs src/engine/combat.rs src/engine/targeting.rs src/parser/oracle.rs
git commit -m "$(cat <<'EOF'
feat: add ProtectionQuality enum, rename ProtectionFromColor → ProtectionFrom, add HexproofFrom

Co-Authored-By: Claude Sonnet 4.6 <noreply@anthropic.com>
EOF
)"
```

---

## Task 2: `has_protection_from` helper in `engine/mod.rs`

**Files:**
- Modify: `src/engine/mod.rs`

**Interfaces:**
- Consumes: `ProtectionQuality`, `source_matches_quality` from Task 1
- Produces: `pub(crate) fn has_protection_from(target_obj: &CardObject, source_colors: &[ManaColor], source_card_types: &[CardType], source_subtypes: &[String]) -> bool`

- [ ] **Step 1: Write the failing test**

Add to `src/engine/mod.rs` tests block:

```rust
    #[test]
    fn has_protection_from_color_returns_true_for_matching_color() {
        use crate::types::ability::{KeywordAbility, ProtectionQuality, Rule, RulesText};
        use crate::types::mana::ManaColor;

        let mut gs = two_player_state();
        let def = CardDefinition {
            name: "Protected".into(),
            mana_cost: None,
            type_line: TypeLine {
                supertypes: vec![],
                card_types: vec![CardType::Creature],
                subtypes: vec![],
            },
            oracle_text: String::new(),
            rules_text: vec![RulesText::Active(Rule::Static(KeywordAbility::ProtectionFrom(
                ProtectionQuality::Color(ManaColor::Blue),
            )))],
            text_annotations: vec![],
            power: Some(2),
            toughness: Some(2),
            colors: vec![],
        };
        let id = add_permanent(&mut gs, PlayerId(0), def, Zone::Battlefield);
        let obj = gs.objects.get(&id).unwrap();
        assert!(has_protection_from(obj, &[ManaColor::Blue], &[], &[]));
        assert!(!has_protection_from(obj, &[ManaColor::Red], &[], &[]));
    }

    #[test]
    fn has_protection_from_everything_always_true() {
        use crate::types::ability::{KeywordAbility, ProtectionQuality, Rule, RulesText};

        let mut gs = two_player_state();
        let def = CardDefinition {
            name: "Protected".into(),
            mana_cost: None,
            type_line: TypeLine {
                supertypes: vec![],
                card_types: vec![CardType::Creature],
                subtypes: vec![],
            },
            oracle_text: String::new(),
            rules_text: vec![RulesText::Active(Rule::Static(KeywordAbility::ProtectionFrom(
                ProtectionQuality::Everything,
            )))],
            text_annotations: vec![],
            power: Some(2),
            toughness: Some(2),
            colors: vec![],
        };
        let id = add_permanent(&mut gs, PlayerId(0), def, Zone::Battlefield);
        let obj = gs.objects.get(&id).unwrap();
        assert!(has_protection_from(obj, &[], &[], &[]));
    }
```

- [ ] **Step 2: Run tests — expect compile error** (`has_protection_from` not defined)

```bash
cargo test 2>&1 | grep -E "^error|FAILED|^test result"
```

- [ ] **Step 3: Implement `has_protection_from` in `engine/mod.rs`**

Add after the `continuous_pt_bonus` function (before `#[cfg(test)]`):

```rust
// CR 702.16c/d/e: returns true if target_obj has ProtectionFrom any quality
// satisfied by the given source characteristics.
pub(crate) fn has_protection_from(
    target_obj: &crate::types::CardObject,
    source_colors: &[crate::types::mana::ManaColor],
    source_card_types: &[crate::types::card::CardType],
    source_subtypes: &[String],
) -> bool {
    use crate::types::RulesText;
    use crate::types::ability::{KeywordAbility, Rule, source_matches_quality};
    target_obj.definition.rules_text.iter().any(|span| {
        if let RulesText::Active(Rule::Static(KeywordAbility::ProtectionFrom(q))) = span {
            source_matches_quality(q, source_colors, source_card_types, source_subtypes)
        } else {
            false
        }
    })
}
```

- [ ] **Step 4: Run tests**

```bash
cargo test 2>&1 | grep -E "^test result|FAILED|error\["
```
Expected: `test result: ok`

- [ ] **Step 5: Commit**

```bash
git add src/engine/mod.rs
git commit -m "$(cat <<'EOF'
feat: add has_protection_from helper to engine/mod.rs

Co-Authored-By: Claude Sonnet 4.6 <noreply@anthropic.com>
EOF
)"
```

---

## Task 3: `DamageStep` expansion + `inject_source_flags`

Snapshot source characteristics into `DamageStep` at stack-push time so they are available at resolution for protection checks.

**Files:**
- Modify: `src/types/effect.rs`
- Modify: `src/engine/stack.rs`
- Modify: `src/engine/triggered.rs`
- Modify: `src/engine/activated.rs`

**Interfaces:**
- Consumes: `ProtectionQuality`, `CardType` from Task 1
- Produces: `DamageStep` with `source_colors: Vec<ManaColor>`, `source_card_types: Vec<CardType>`, `source_subtypes: Vec<String>`; updated `inject_source_flags(effect, rules_text, colors, card_types, subtypes) -> Effect`

- [ ] **Step 1: Add three fields to `DamageStep` in `src/types/effect.rs`**

Replace the existing `DamageStep` struct:
```rust
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct DamageStep {
    pub amount: u32,
    pub lifelink: bool,
    pub deathtouch: bool,
    pub wither: bool,
    pub infect: bool,
}
```
With:
```rust
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct DamageStep {
    pub amount: u32,
    pub lifelink: bool,
    pub deathtouch: bool,
    pub wither: bool,
    pub infect: bool,
    // CR 702.16e: source characteristics snapshotted at stack-push time (LKI).
    pub source_colors: Vec<crate::types::mana::ManaColor>,
    pub source_card_types: Vec<crate::types::card::CardType>,
    pub source_subtypes: Vec<String>,
}
```

- [ ] **Step 2: Update `inject_source_flags` in `src/engine/stack.rs`**

Replace the signature and body of `inject_source_flags`:

```rust
pub(crate) fn inject_source_flags(
    effect: crate::types::effect::Effect,
    source_rules_text: &[crate::types::RulesText],
    source_colors: &[crate::types::mana::ManaColor],
    source_card_types: &[crate::types::card::CardType],
    source_subtypes: &[String],
) -> crate::types::effect::Effect {
    use crate::types::ability::KeywordAbility;
    use crate::types::effect::{DamageStep, EffectStep};

    effect
        .into_iter()
        .map(|step| match step {
            EffectStep::DealDamage(s) => EffectStep::DealDamage(DamageStep {
                lifelink: has_damage_kw(source_rules_text, &KeywordAbility::Lifelink),
                deathtouch: has_damage_kw(source_rules_text, &KeywordAbility::Deathtouch),
                wither: has_damage_kw(source_rules_text, &KeywordAbility::Wither),
                infect: has_damage_kw(source_rules_text, &KeywordAbility::Infect),
                source_colors: source_colors.to_vec(),
                source_card_types: source_card_types.to_vec(),
                source_subtypes: source_subtypes.to_vec(),
                ..s
            }),
            other => other,
        })
        .collect()
}
```

- [ ] **Step 3: Update the `inject_source_flags` call site in `stack.rs` (spell resolution)**

Find the block around line 513–518 that reads:
```rust
                let spell_rules_text: Vec<crate::types::RulesText> = state
                    .objects
                    .get(&card_id)
                    .map(|o| o.definition.rules_text.clone())
                    .unwrap_or_default();
                let steps = inject_source_flags(steps, &spell_rules_text);
```
Replace with:
```rust
                let (spell_rules_text, spell_colors, spell_card_types, spell_subtypes) = state
                    .objects
                    .get(&card_id)
                    .map(|o| (
                        o.definition.rules_text.clone(),
                        o.definition.colors.clone(),
                        o.definition.type_line.card_types.clone(),
                        o.definition.type_line.subtypes.clone(),
                    ))
                    .unwrap_or_default();
                let steps = inject_source_flags(
                    steps,
                    &spell_rules_text,
                    &spell_colors,
                    &spell_card_types,
                    &spell_subtypes,
                );
```

- [ ] **Step 4: Update `inject_source_flags` call site in `src/engine/triggered.rs`**

Find the block around line 294–298 that reads:
```rust
        let (controller, rules_text) = match state.objects.get(&source_id) {
            Some(o) => (o.controller, o.definition.rules_text.clone()),
            None => continue,
        };
```
Replace with:
```rust
        let (controller, rules_text, source_colors, source_card_types, source_subtypes) =
            match state.objects.get(&source_id) {
                Some(o) => (
                    o.controller,
                    o.definition.rules_text.clone(),
                    o.definition.colors.clone(),
                    o.definition.type_line.card_types.clone(),
                    o.definition.type_line.subtypes.clone(),
                ),
                None => continue,
            };
```

Find line ~518:
```rust
            let effect = inject_source_flags(triggered_clone.effect, &rules_text);
```
Replace with:
```rust
            let effect = inject_source_flags(
                triggered_clone.effect,
                &rules_text,
                &source_colors,
                &source_card_types,
                &source_subtypes,
            );
```

- [ ] **Step 5: Update `inject_source_flags` call site in `src/engine/activated.rs`**

Find the block around line 186–205. Replace the `source_rules_text` extraction and the `inject_source_flags` call:

```rust
        let (source_rules_text, source_colors, source_card_types, source_subtypes) = state
            .battlefield
            .get(&object_id)
            .and_then(|_| state.objects.get(&object_id))
            .map(|o| (
                o.definition.rules_text.clone(),
                o.definition.colors.clone(),
                o.definition.type_line.card_types.clone(),
                o.definition.type_line.subtypes.clone(),
            ))
            .unwrap_or_default();
```

And update the `inject_source_flags` call within the `StackObject` construction to:
```rust
                effect: crate::engine::stack::inject_source_flags(
                    ability.effect.clone(),
                    &source_rules_text,
                    &source_colors,
                    &source_card_types,
                    &source_subtypes,
                ),
```

- [ ] **Step 6: Fix `inject_source_flags` unit tests in `stack.rs`**

The unit tests call `inject_source_flags(effect, &rules_text)` — they need the three new `&[]` args. Find all test call sites (search for `inject_source_flags(effect` in the test module) and add `&[], &[], &[]`:

```rust
// Before:
let result = inject_source_flags(effect, &rules_text);
// After:
let result = inject_source_flags(effect, &rules_text, &[], &[], &[]);

// Before:
let result = inject_source_flags(effect, &[]);
// After:
let result = inject_source_flags(effect, &[], &[], &[], &[]);
```

- [ ] **Step 7: Run tests**

```bash
cargo test 2>&1 | grep -E "^test result|FAILED|error\["
```
Expected: `test result: ok`

- [ ] **Step 8: Commit**

```bash
git add src/types/effect.rs src/engine/stack.rs src/engine/triggered.rs src/engine/activated.rs
git commit -m "$(cat <<'EOF'
feat: snapshot source characteristics in DamageStep via expanded inject_source_flags

Co-Authored-By: Claude Sonnet 4.6 <noreply@anthropic.com>
EOF
)"
```

---

## Task 4: Targeting expansion and caller updates

Expand `is_legal_target` / `legal_targets` to accept source card types and subtypes, add the `HexproofFrom` check, and update all callers.

**Files:**
- Modify: `src/engine/targeting.rs`
- Modify: `src/serve.rs`
- Modify: `src/engine/state_based_actions.rs`

**Interfaces:**
- Consumes: `ProtectionQuality`, `source_matches_quality` from Task 1
- Produces:
  - `pub fn is_legal_target(state, target, filter, caster, source_colors: &[ManaColor], source_card_types: &[CardType], source_subtypes: &[String]) -> bool`
  - `pub fn legal_targets(state, filter, caster, source_colors, source_card_types, source_subtypes) -> Vec<EffectTarget>`

- [ ] **Step 1: Write failing tests in `targeting.rs`**

Add to the test module in `src/engine/targeting.rs`:

```rust
    #[test]
    fn hexproof_from_blue_blocks_blue_spell_from_opponent() {
        use crate::types::ability::ProtectionQuality;
        use crate::types::mana::ManaColor;
        let mut gs = two_player_state();
        let id = place_creature(
            &mut gs,
            PlayerId(1),
            vec![RulesText::Active(Rule::Static(KeywordAbility::HexproofFrom(
                ProtectionQuality::Color(ManaColor::Blue),
            )))],
        );
        let target = EffectTarget::Object { id };
        // Blue spell from opponent — blocked
        assert!(!is_legal_target(
            &gs, &target, &TargetFilter::Creature,
            PlayerId(0), &[ManaColor::Blue], &[], &[],
        ));
    }

    #[test]
    fn hexproof_from_blue_allows_red_spell() {
        use crate::types::ability::ProtectionQuality;
        use crate::types::mana::ManaColor;
        let mut gs = two_player_state();
        let id = place_creature(
            &mut gs,
            PlayerId(1),
            vec![RulesText::Active(Rule::Static(KeywordAbility::HexproofFrom(
                ProtectionQuality::Color(ManaColor::Blue),
            )))],
        );
        let target = EffectTarget::Object { id };
        assert!(is_legal_target(
            &gs, &target, &TargetFilter::Creature,
            PlayerId(0), &[ManaColor::Red], &[], &[],
        ));
    }

    #[test]
    fn hexproof_from_blue_allows_controller_targeting() {
        use crate::types::ability::ProtectionQuality;
        use crate::types::mana::ManaColor;
        let mut gs = two_player_state();
        let id = place_creature(
            &mut gs,
            PlayerId(1),
            vec![RulesText::Active(Rule::Static(KeywordAbility::HexproofFrom(
                ProtectionQuality::Color(ManaColor::Blue),
            )))],
        );
        let target = EffectTarget::Object { id };
        assert!(is_legal_target(
            &gs, &target, &TargetFilter::Creature,
            PlayerId(1), &[ManaColor::Blue], &[], &[],
        ));
    }

    #[test]
    fn protection_from_artifact_blocks_artifact_source() {
        use crate::types::ability::ProtectionQuality;
        use crate::types::card::CardType;
        let mut gs = two_player_state();
        let id = place_creature(
            &mut gs,
            PlayerId(1),
            vec![RulesText::Active(Rule::Static(KeywordAbility::ProtectionFrom(
                ProtectionQuality::CardType(CardType::Artifact),
            )))],
        );
        let target = EffectTarget::Object { id };
        assert!(!is_legal_target(
            &gs, &target, &TargetFilter::Creature,
            PlayerId(0), &[], &[CardType::Artifact], &[],
        ));
        // Non-artifact source is fine
        assert!(is_legal_target(
            &gs, &target, &TargetFilter::Creature,
            PlayerId(0), &[], &[CardType::Creature], &[],
        ));
    }
```

- [ ] **Step 2: Run tests — expect compile errors** (wrong argument count)

```bash
cargo test 2>&1 | grep -E "^error|FAILED|^test result"
```

- [ ] **Step 3: Update `is_legal_target` and `legal_targets` signatures and logic**

In `src/engine/targeting.rs`, replace the full `is_legal_target` function signature and the `ProtectionFrom` check:

```rust
pub fn is_legal_target(
    state: &GameState,
    target: &EffectTarget,
    filter: &TargetFilter,
    caster: PlayerId,
    source_colors: &[ManaColor],
    source_card_types: &[crate::types::card::CardType],
    source_subtypes: &[String],
) -> bool {
```

Inside the `EffectTarget::Object` arm, replace the old `ProtectionFromColor` block with:
```rust
            // CR 702.11d: HexproofFrom prevents targeting by opponents from sources of the quality
            for span in &obj.definition.rules_text {
                if let RulesText::Active(Rule::Static(KeywordAbility::HexproofFrom(q))) = span {
                    if obj.controller != caster
                        && crate::types::ability::source_matches_quality(
                            q,
                            source_colors,
                            source_card_types,
                            source_subtypes,
                        )
                    {
                        return false;
                    }
                }
            }
            // CR 702.16c: protection prevents targeting by sources of protected quality
            for span in &obj.definition.rules_text {
                if let RulesText::Active(Rule::Static(KeywordAbility::ProtectionFrom(q))) = span {
                    if crate::types::ability::source_matches_quality(
                        q,
                        source_colors,
                        source_card_types,
                        source_subtypes,
                    ) {
                        return false;
                    }
                }
            }
```

Update `legal_targets` signature:
```rust
pub fn legal_targets(
    state: &GameState,
    filter: &TargetFilter,
    caster: PlayerId,
    source_colors: &[ManaColor],
    source_card_types: &[crate::types::card::CardType],
    source_subtypes: &[String],
) -> Vec<EffectTarget> {
```

Update the three `is_legal_target` calls inside `legal_targets` to pass `source_card_types, source_subtypes`.

- [ ] **Step 4: Update all existing `is_legal_target` / `legal_targets` call sites in `targeting.rs` tests**

Every test call to `is_legal_target` needs `&[], &[]` appended. For example:
```rust
// Before:
assert!(is_legal_target(&gs, &target, &TargetFilter::Creature, PlayerId(0), &[]));
// After:
assert!(is_legal_target(&gs, &target, &TargetFilter::Creature, PlayerId(0), &[], &[], &[]));
```
Search for `is_legal_target(` and `legal_targets(` in the test module and update all occurrences.

- [ ] **Step 5: Update `serve.rs` call sites**

In `src/serve.rs`, find the two `legal_targets` calls:

Around line 569–572:
```rust
            let spell_colors = obj.definition.colors.clone();
            // ...
            for target in legal_targets(state, filter, pid, &spell_colors) {
```
Replace with:
```rust
            let spell_colors = obj.definition.colors.clone();
            let spell_card_types = obj.definition.type_line.card_types.clone();
            let spell_subtypes = obj.definition.type_line.subtypes.clone();
            // ...
            for target in legal_targets(state, filter, pid, &spell_colors, &spell_card_types, &spell_subtypes) {
```

Around line 610–617 (aura enchant targets):
```rust
            let spell_colors = obj.definition.colors.clone();
            // ...
            for target in legal_targets(state, &enchant_filter, pid, &spell_colors) {
```
Replace with:
```rust
            let spell_colors = obj.definition.colors.clone();
            let spell_card_types = obj.definition.type_line.card_types.clone();
            let spell_subtypes = obj.definition.type_line.subtypes.clone();
            // ...
            for target in legal_targets(state, &enchant_filter, pid, &spell_colors, &spell_card_types, &spell_subtypes) {
```

- [ ] **Step 6: Update `state_based_actions.rs` call site**

In `src/engine/state_based_actions.rs`, find the `is_legal_target` call around line 109–116:
```rust
                    let colors = state
                        .objects
                        .get(&id)
                        .map(|o| o.definition.colors.clone())
                        .unwrap_or_default();
                    !crate::engine::targeting::is_legal_target(
                        state, &target, &enchant, controller, &colors,
                    )
```
Replace with:
```rust
                    let (colors, card_types, subtypes) = state
                        .objects
                        .get(&id)
                        .map(|o| (
                            o.definition.colors.clone(),
                            o.definition.type_line.card_types.clone(),
                            o.definition.type_line.subtypes.clone(),
                        ))
                        .unwrap_or_default();
                    !crate::engine::targeting::is_legal_target(
                        state, &target, &enchant, controller, &colors, &card_types, &subtypes,
                    )
```

- [ ] **Step 7: Run tests**

```bash
cargo test 2>&1 | grep -E "^test result|FAILED|error\["
```
Expected: `test result: ok`

- [ ] **Step 8: Commit**

```bash
git add src/engine/targeting.rs src/serve.rs src/engine/state_based_actions.rs
git commit -m "$(cat <<'EOF'
feat: expand is_legal_target with card_types/subtypes; add HexproofFrom targeting check

Co-Authored-By: Claude Sonnet 4.6 <noreply@anthropic.com>
EOF
)"
```

---

## Task 5: Parser expansion

Extract `parse_protection_quality`, handle all qualities for `protection from`, and implement `hexproof from [quality]`.

**Files:**
- Modify: `src/parser/oracle.rs`

**Interfaces:**
- Consumes: `ProtectionQuality`, `HexproofFrom` from Task 1
- Produces: `parse_protection_quality(s: &str) -> Option<ProtectionQuality>` (module-private)

- [ ] **Step 1: Write failing parser tests**

Add to the test module in `src/parser/oracle.rs`:

```rust
    #[test]
    fn protection_from_everything_parses() {
        let (spans, _) = parse_permanent("Protection from everything", "Test");
        assert_eq!(
            spans,
            vec![active(KeywordAbility::ProtectionFrom(
                crate::types::ability::ProtectionQuality::Everything
            ))]
        );
    }

    #[test]
    fn protection_from_artifacts_parses() {
        let (spans, _) = parse_permanent("Protection from artifacts", "Test");
        assert_eq!(
            spans,
            vec![active(KeywordAbility::ProtectionFrom(
                crate::types::ability::ProtectionQuality::CardType(
                    crate::types::card::CardType::Artifact
                )
            ))]
        );
    }

    #[test]
    fn protection_from_vampire_creatures_parses() {
        let (spans, _) = parse_permanent("Protection from vampire creatures", "Test");
        assert_eq!(
            spans,
            vec![active(KeywordAbility::ProtectionFrom(
                crate::types::ability::ProtectionQuality::CreatureType("Vampire".into())
            ))]
        );
    }

    #[test]
    fn hexproof_from_black_parses() {
        let (spans, _) = parse_permanent("Hexproof from black", "Test");
        assert_eq!(
            spans,
            vec![active(KeywordAbility::HexproofFrom(
                crate::types::ability::ProtectionQuality::Color(ManaColor::Black)
            ))]
        );
    }

    #[test]
    fn hexproof_from_artifacts_parses() {
        let (spans, _) = parse_permanent("Hexproof from artifacts", "Test");
        assert_eq!(
            spans,
            vec![active(KeywordAbility::HexproofFrom(
                crate::types::ability::ProtectionQuality::CardType(
                    crate::types::card::CardType::Artifact
                )
            ))]
        );
    }
```

- [ ] **Step 2: Run tests — expect FAILED** (unrecognised qualities still return `ParsedUnimplemented`)

```bash
cargo test 2>&1 | grep -E "^test result|FAILED|error\["
```

- [ ] **Step 3: Add `parse_protection_quality` helper function**

Insert as a module-level private function near the top of `src/parser/oracle.rs` (before the main `parse_keyword` function, or grouped with helpers):

```rust
fn parse_protection_quality(s: &str) -> Option<crate::types::ability::ProtectionQuality> {
    use crate::types::ability::ProtectionQuality;
    use crate::types::card::CardType;
    use crate::types::mana::ManaColor;
    match s {
        "white" => Some(ProtectionQuality::Color(ManaColor::White)),
        "blue" => Some(ProtectionQuality::Color(ManaColor::Blue)),
        "black" => Some(ProtectionQuality::Color(ManaColor::Black)),
        "red" => Some(ProtectionQuality::Color(ManaColor::Red)),
        "green" => Some(ProtectionQuality::Color(ManaColor::Green)),
        "everything" => Some(ProtectionQuality::Everything),
        "artifacts" | "artifact" => Some(ProtectionQuality::CardType(CardType::Artifact)),
        "creatures" | "creature" => Some(ProtectionQuality::CardType(CardType::Creature)),
        "instants" | "instant" => Some(ProtectionQuality::CardType(CardType::Instant)),
        "enchantments" | "enchantment" => Some(ProtectionQuality::CardType(CardType::Enchantment)),
        "sorceries" | "sorcery" => Some(ProtectionQuality::CardType(CardType::Sorcery)),
        "lands" | "land" => Some(ProtectionQuality::CardType(CardType::Land)),
        "planeswalkers" | "planeswalker" => Some(ProtectionQuality::CardType(CardType::Planeswalker)),
        other => {
            // "[subtype] creatures" / "[subtype] creature" → CreatureType
            let subtype = other
                .strip_suffix(" creatures")
                .or_else(|| other.strip_suffix(" creature"));
            if let Some(sub) = subtype {
                if !sub.is_empty() {
                    let mut chars = sub.chars();
                    let title = chars
                        .next()
                        .map(|c| c.to_uppercase().collect::<String>())
                        .unwrap_or_default()
                        + chars.as_str();
                    return Some(ProtectionQuality::CreatureType(title));
                }
            }
            None
        }
    }
}
```

- [ ] **Step 4: Refactor the `protection from` block in `parse_keyword`**

Replace the existing `protection from` block (around line 522–537):
```rust
    // Protection from [color] (CR 702.16)
    if let Some(quality) = s.strip_prefix("protection from ") {
        let color = match quality.trim_end_matches('.') {
            "white" => Some(ManaColor::White),
            // ... (existing color matching)
        };
        if let Some(c) = color {
            return RulesText::Active(Rule::Static(KeywordAbility::ProtectionFrom(
                crate::types::ability::ProtectionQuality::Color(c),
            )));
        }
        // Non-color protections remain ParsedUnimplemented
        return ParsedUnimplemented(kw.to_string());
    }
```
With:
```rust
    // Protection from [quality] (CR 702.16)
    if let Some(q_str) = s.strip_prefix("protection from ") {
        let quality_str = q_str.trim_end_matches('.');
        if let Some(q) = parse_protection_quality(quality_str) {
            return RulesText::Active(Rule::Static(KeywordAbility::ProtectionFrom(q)));
        }
        return ParsedUnimplemented(kw.to_string());
    }

    // Hexproof from [quality] (CR 702.11d)
    if let Some(q_str) = s.strip_prefix("hexproof from ") {
        let quality_str = q_str.trim_end_matches('.');
        if let Some(q) = parse_protection_quality(quality_str) {
            return RulesText::Active(Rule::Static(KeywordAbility::HexproofFrom(q)));
        }
        return ParsedUnimplemented(kw.to_string());
    }
```

Note: the `hexproof from` block must appear BEFORE the `is_likely_keyword` fallthrough that currently sends `hexproof from ...` to `ParsedUnimplemented`. Since the new handler returns early, the fallthrough never fires for recognised qualities.

- [ ] **Step 5: Update the existing `protection_from_artifacts_stays_unimplemented` test**

This test currently asserts `ParsedUnimplemented` for "Protection from artifacts". After the change it will parse correctly. Update it:
```rust
    #[test]
    fn protection_from_artifacts_parses_as_protection() {
        let (spans, _) = parse_permanent("Protection from artifacts", "");
        assert_eq!(
            spans,
            vec![active(KeywordAbility::ProtectionFrom(
                crate::types::ability::ProtectionQuality::CardType(
                    crate::types::card::CardType::Artifact
                )
            ))]
        );
    }
```

- [ ] **Step 6: Run tests**

```bash
cargo test 2>&1 | grep -E "^test result|FAILED|error\["
```
Expected: `test result: ok`

- [ ] **Step 7: Commit**

```bash
git add src/parser/oracle.rs
git commit -m "$(cat <<'EOF'
feat: expand protection/hexproof-from parsing to cover all ProtectionQuality variants

Co-Authored-By: Claude Sonnet 4.6 <noreply@anthropic.com>
EOF
)"
```

---

## Task 6: Combat damage prevention (D in DEBT, combat path)

**Files:**
- Modify: `src/engine/combat.rs`

**Interfaces:**
- Consumes: `has_protection_from` from Task 2

- [ ] **Step 1: Write failing tests**

Add to the `#[cfg(test)]` block in `src/engine/combat.rs`. Use the existing helpers: `make_combat_state()` returns a two-player `GameState` at `Step::DeclareAttackers`; `place_creature_with_colors(state, owner, rules_text: Vec<RulesText>, colors: Vec<ManaColor>)` places a 2/2 creature.

```rust
    #[test]
    fn protection_from_red_blocker_takes_no_damage_from_red_attacker() {
        // Blocker has protection from red; attacker is red.
        // CR 702.16e: the red attacker's damage to the protected blocker is prevented.
        use crate::types::ability::{KeywordAbility, ProtectionQuality, Rule};
        use crate::types::RulesText;

        let mut gs = make_combat_state();
        let attacker = place_creature_with_colors(&mut gs, PlayerId(0), vec![], vec![ManaColor::Red]);
        let blocker = place_creature_with_colors(
            &mut gs,
            PlayerId(1),
            vec![RulesText::Active(Rule::Static(KeywordAbility::ProtectionFrom(
                ProtectionQuality::Color(ManaColor::Red),
            )))],
            vec![],
        );
        gs.combat.attackers = vec![attacker];
        gs.combat.blocking_map = [(attacker, vec![blocker])].into();
        gs.step = Step::CombatDamage;
        let gs = deal_combat_damage(gs);
        assert!(gs.battlefield.contains_key(&blocker), "blocker should survive");
        assert_eq!(gs.battlefield[&blocker].damage_marked, 0);
    }

    #[test]
    fn protection_from_red_attacker_takes_no_damage_from_red_blocker() {
        // Attacker has protection from red; blocker is red.
        // CR 702.16e: the red blocker's damage to the protected attacker is prevented.
        use crate::types::ability::{KeywordAbility, ProtectionQuality, Rule};
        use crate::types::RulesText;

        let mut gs = make_combat_state();
        let attacker = place_creature_with_colors(
            &mut gs,
            PlayerId(0),
            vec![RulesText::Active(Rule::Static(KeywordAbility::ProtectionFrom(
                ProtectionQuality::Color(ManaColor::Red),
            )))],
            vec![],
        );
        let blocker = place_creature_with_colors(&mut gs, PlayerId(1), vec![], vec![ManaColor::Red]);
        gs.combat.attackers = vec![attacker];
        gs.combat.blocking_map = [(attacker, vec![blocker])].into();
        gs.step = Step::CombatDamage;
        let gs = deal_combat_damage(gs);
        assert!(gs.battlefield.contains_key(&attacker), "attacker should survive");
        assert_eq!(gs.battlefield[&attacker].damage_marked, 0);
    }

    #[test]
    fn protection_from_everything_prevents_all_combat_damage() {
        use crate::types::ability::{KeywordAbility, ProtectionQuality, Rule};
        use crate::types::RulesText;

        let mut gs = make_combat_state();
        let attacker = place_creature_with_colors(&mut gs, PlayerId(0), vec![], vec![ManaColor::Red]);
        let blocker = place_creature_with_colors(
            &mut gs,
            PlayerId(1),
            vec![RulesText::Active(Rule::Static(KeywordAbility::ProtectionFrom(
                ProtectionQuality::Everything,
            )))],
            vec![],
        );
        gs.combat.attackers = vec![attacker];
        gs.combat.blocking_map = [(attacker, vec![blocker])].into();
        gs.step = Step::CombatDamage;
        let gs = deal_combat_damage(gs);
        assert!(gs.battlefield.contains_key(&blocker));
        assert_eq!(gs.battlefield[&blocker].damage_marked, 0);
    }
```

- [ ] **Step 2: Run tests — expect FAILED** (protection doesn't prevent damage yet)

```bash
cargo test engine::combat 2>&1 | grep -E "^test result|FAILED|error\["
```

- [ ] **Step 3: Snapshot attacker's characteristics alongside existing destructuring**

In `deal_combat_damage`, find the destructuring block (around line 401–430). Add `atk_colors`, `atk_card_types`, `atk_subtypes` to the tuple:

```rust
        let (
            atk_power,
            has_trample,
            has_deathtouch,
            has_lifelink,
            has_wither,
            has_infect,
            atk_controller,
            atk_colors,
            atk_card_types,
            atk_subtypes,
        ) = {
            let obj = match state.objects.get(&attacker_id) {
                Some(o) => o,
                None => continue,
            };
            let atk_cont = super::continuous_pt_bonus(&state, attacker_id);
            let power = state
                .battlefield
                .get(&attacker_id)
                .and_then(|p| p.effective_power(atk_cont.power))
                .map(|p| p.max(0) as u32)
                .unwrap_or(0);
            (
                power,
                obj.has_keyword(KeywordAbility::Trample),
                obj.has_keyword(KeywordAbility::Deathtouch),
                obj.has_keyword(KeywordAbility::Lifelink),
                obj.has_keyword(KeywordAbility::Wither),
                obj.has_keyword(KeywordAbility::Infect),
                obj.controller,
                obj.definition.colors.clone(),
                obj.definition.type_line.card_types.clone(),
                obj.definition.type_line.subtypes.clone(),
            )
        };
```

- [ ] **Step 4: Guard attacker → blocker damage with protection check**

In the blocker-iteration loop, find where damage is accumulated into `damage_to_objects[blocker_id]` (around lines 475–480). Wrap both the wither and normal paths:

```rust
                // CR 702.16e: protection prevents damage from sources of the protected quality.
                if let Some(blocker_obj) = state.objects.get(&blocker_id) {
                    if super::has_protection_from(blocker_obj, &atk_colors, &atk_card_types, &atk_subtypes) {
                        remaining -= assign;
                        // damage is prevented — don't record it
                        continue;
                    }
                }
                if has_wither || has_infect {
                    *wither_to_objects.entry(blocker_id).or_insert(0) += assign;
                } else {
                    *damage_to_objects.entry(blocker_id).or_insert(0) += assign;
                }
```

Also wrap the "last blocker gets remaining damage" block similarly (around lines 496–505).

- [ ] **Step 5: Guard blocker → attacker damage with protection check**

In the blocker-deals-damage loop (around lines 528–565), add a snapshot of blocker's characteristics and a protection check before accumulating into `damage_to_objects[attacker_id]`:

```rust
            // Snapshot blocker characteristics for protection checks.
            let (blk_colors, blk_card_types, blk_subtypes) = {
                let obj = match state.objects.get(&blocker_id) {
                    Some(o) => o,
                    None => continue,
                };
                (
                    obj.definition.colors.clone(),
                    obj.definition.type_line.card_types.clone(),
                    obj.definition.type_line.subtypes.clone(),
                )
            };
```

Then before `*damage_to_objects.entry(attacker_id).or_insert(0) += blk_power`:
```rust
            // CR 702.16e: protection prevents damage.
            if blk_power > 0 {
                if let Some(atk_obj) = state.objects.get(&attacker_id) {
                    if super::has_protection_from(atk_obj, &blk_colors, &blk_card_types, &blk_subtypes) {
                        continue; // damage prevented
                    }
                }
                if blk_wither || blk_infect {
                    *wither_to_objects.entry(attacker_id).or_insert(0) += blk_power;
                } else {
                    *damage_to_objects.entry(attacker_id).or_insert(0) += blk_power;
                }
                // ... rest of deathtouch / lifelink unchanged
            }
```

- [ ] **Step 6: Run tests**

```bash
cargo test 2>&1 | grep -E "^test result|FAILED|error\["
```
Expected: `test result: ok`

- [ ] **Step 7: Commit**

```bash
git add src/engine/combat.rs
git commit -m "$(cat <<'EOF'
feat: CR 702.16e — protection prevents combat damage (D in DEBT, combat path)

Co-Authored-By: Claude Sonnet 4.6 <noreply@anthropic.com>
EOF
)"
```

---

## Task 7: Stack `DealDamage` protection prevention

**Files:**
- Modify: `src/engine/stack.rs`

**Interfaces:**
- Consumes: `has_protection_from` from Task 2; `DamageStep.source_*` fields from Task 3

- [ ] **Step 1: Write failing test**

Add to the test module in `src/engine/stack.rs`:

```rust
    #[test]
    fn deal_damage_to_protected_creature_is_prevented() {
        // Creature has protection from blue; source is blue → damage prevented (CR 702.16e).
        use crate::types::ability::{KeywordAbility, ProtectionQuality, Rule, RulesText};
        use crate::types::card::{CardDefinition, CardType, TypeLine};
        use crate::types::effect::DamageStep;
        use crate::types::mana::ManaColor;

        let mut gs = GameState::new(vec![
            Player::new(PlayerId(0), "Alice"),
            Player::new(PlayerId(1), "Bob"),
        ]);
        let creature_def = CardDefinition {
            name: "Protected".into(),
            mana_cost: None,
            type_line: TypeLine {
                supertypes: vec![],
                card_types: vec![CardType::Creature],
                subtypes: vec![],
            },
            oracle_text: String::new(),
            rules_text: vec![RulesText::Active(Rule::Static(KeywordAbility::ProtectionFrom(
                ProtectionQuality::Color(ManaColor::Blue),
            )))],
            text_annotations: vec![],
            power: Some(2),
            toughness: Some(2),
            colors: vec![],
        };
        let target_id = gs.alloc_id();
        let obj = crate::types::CardObject::new(target_id, creature_def, PlayerId(1), Zone::Battlefield);
        gs.battlefield.insert(target_id, PermanentState::new(&obj.definition));
        gs.add_object(obj);

        let step = EffectStep::DealDamage(DamageStep {
            amount: 3,
            source_colors: vec![ManaColor::Blue],  // blue source
            ..DamageStep::default()
        });
        let targets = vec![crate::types::effect::EffectTarget::Object { id: target_id }];
        let gs = execute_effect_steps(gs, PlayerId(0), &[step], &targets, None);

        // Damage was prevented — creature still at 0 damage_marked
        assert_eq!(gs.battlefield[&target_id].damage_marked, 0);
    }
```

- [ ] **Step 2: Run test — expect FAILED**

```bash
cargo test stack::tests::deal_damage_to_protected_creature_is_prevented 2>&1 | grep -E "FAILED|PASSED|^test result"
```

- [ ] **Step 3: Add protection check in `execute_effect_steps`**

In `src/engine/stack.rs`, find the `EffectStep::DealDamage(s)` arm in `execute_effect_steps` (around line 177). Before the `match targets.first()`, add:

```rust
            EffectStep::DealDamage(s) => {
                let amount = s.amount;
                // CR 702.16e: if the target creature has protection from the source's quality,
                // damage is prevented.
                if let Some(crate::types::effect::EffectTarget::Object { id }) = targets.first() {
                    if let Some(obj) = state.objects.get(id) {
                        if crate::engine::has_protection_from(
                            obj,
                            &s.source_colors,
                            &s.source_card_types,
                            &s.source_subtypes,
                        ) {
                            // Damage prevented — skip this step entirely.
                            continue;  // `continue` the outer `for (i, step)` loop
                        }
                    }
                }
                match targets.first() {
                    // ... rest unchanged
```

Note: the body of `execute_effect_steps` is a `for (i, step)` loop — `continue` skips to the next step.

- [ ] **Step 4: Run tests**

```bash
cargo test 2>&1 | grep -E "^test result|FAILED|error\["
```
Expected: `test result: ok`

- [ ] **Step 5: Commit**

```bash
git add src/engine/stack.rs
git commit -m "$(cat <<'EOF'
feat: CR 702.16e — protection prevents DealDamage effect step (D in DEBT, stack path)

Co-Authored-By: Claude Sonnet 4.6 <noreply@anthropic.com>
EOF
)"
```

---

## Task 8: E in DEBT — enchant/equip prevention

**Files:**
- Modify: `src/engine/state_based_actions.rs`
- Modify: `src/engine/stack.rs`

**Interfaces:**
- Consumes: `has_protection_from` from Task 2; `is_legal_target` expanded in Task 4

- [ ] **Step 1: Write failing tests**

Add to `src/engine/state_based_actions.rs` test module. Use the existing `make_state()` and `add_creature_to_battlefield(state, owner, def)` helpers. Use `make_aura(TargetFilter::Creature)` for the aura base and override `.colors` for blue. Use `make_equipment()` for the artifact equipment.

```rust
    #[test]
    fn aura_with_protected_quality_goes_to_graveyard_via_sba() {
        // Blue aura attached to a creature with protection from blue → SBA sends aura to graveyard.
        // CR 704.5m / 702.16c.
        use crate::types::ability::{KeywordAbility, ProtectionQuality, Rule, RulesText, TargetFilter};
        use crate::types::mana::ManaColor;
        use crate::types::{CardDefinition, CardType, TypeLine};

        let mut gs = make_state();

        // Creature: 2/2, protection from blue
        let creature_id = add_creature_to_battlefield(&mut gs, PlayerId(1), CardDefinition {
            name: "Protected Creature".into(),
            mana_cost: None,
            type_line: TypeLine {
                supertypes: vec![],
                card_types: vec![CardType::Creature],
                subtypes: vec![],
            },
            oracle_text: String::new(),
            rules_text: vec![RulesText::Active(Rule::Static(KeywordAbility::ProtectionFrom(
                ProtectionQuality::Color(ManaColor::Blue),
            )))],
            text_annotations: vec![],
            power: Some(2),
            toughness: Some(2),
            colors: vec![],
        });

        // Blue aura (use make_aura, then set colors to blue)
        let mut aura_def = make_aura(TargetFilter::Creature);
        aura_def.colors = vec![ManaColor::Blue];
        let aura_id = add_creature_to_battlefield(&mut gs, PlayerId(0), aura_def);
        gs.battlefield.get_mut(&aura_id).unwrap().attached_to = Some(creature_id);

        let (gs, _) = check_and_apply_sbas(gs);
        assert!(!gs.battlefield.contains_key(&aura_id), "aura should be gone");
    }

    #[test]
    fn equipment_on_protected_creature_detaches_via_sba() {
        // Artifact equipment attached to creature with protection from artifacts → SBA detaches.
        // CR 704.5n / 702.16d.
        use crate::types::ability::{KeywordAbility, ProtectionQuality, Rule, RulesText};
        use crate::types::{CardDefinition, CardType, TypeLine};

        let mut gs = make_state();

        // Creature: 2/2, protection from artifacts
        let creature_id = add_creature_to_battlefield(&mut gs, PlayerId(1), CardDefinition {
            name: "Protected Creature".into(),
            mana_cost: None,
            type_line: TypeLine {
                supertypes: vec![],
                card_types: vec![CardType::Creature],
                subtypes: vec![],
            },
            oracle_text: String::new(),
            rules_text: vec![RulesText::Active(Rule::Static(KeywordAbility::ProtectionFrom(
                ProtectionQuality::CardType(CardType::Artifact),
            )))],
            text_annotations: vec![],
            power: Some(2),
            toughness: Some(2),
            colors: vec![],
        });

        // Artifact equipment (use make_equipment helper)
        let equip_def = make_equipment();
        let equip_id = add_creature_to_battlefield(&mut gs, PlayerId(0), equip_def);
        gs.battlefield.get_mut(&equip_id).unwrap().attached_to = Some(creature_id);

        let (gs, _) = check_and_apply_sbas(gs);
        assert!(gs.battlefield.contains_key(&equip_id), "equipment stays on battlefield");
        assert_eq!(gs.battlefield[&equip_id].attached_to, None, "equipment should be detached");
    }
```

- [ ] **Step 2: Run tests — expect FAILED**

```bash
cargo test state_based_actions 2>&1 | grep -E "^test result|FAILED|error\["
```

- [ ] **Step 3: Add equipment protection check to `state_based_actions.rs`**

Find the `DetachEquipment` SBA block (around line 125–143). After the existing `if !host_on_battlefield || !host_is_creature` branch, add:

```rust
        if has_equip && let Some(host_id) = perm.attached_to {
            let host_is_creature = state.objects.get(&host_id).map(|o| o.is_creature()).unwrap_or(false);
            let host_on_battlefield = state.battlefield.contains_key(&host_id);
            if !host_on_battlefield || !host_is_creature {
                sbas.push(Sba::DetachEquipment(id));
            } else {
                // CR 702.16d: equipment whose quality matches the host's protection becomes unattached.
                let (equip_colors, equip_types, equip_subtypes) = state
                    .objects
                    .get(&id)
                    .map(|o| (
                        o.definition.colors.clone(),
                        o.definition.type_line.card_types.clone(),
                        o.definition.type_line.subtypes.clone(),
                    ))
                    .unwrap_or_default();
                if let Some(host_obj) = state.objects.get(&host_id) {
                    if crate::engine::has_protection_from(
                        host_obj,
                        &equip_colors,
                        &equip_types,
                        &equip_subtypes,
                    ) {
                        sbas.push(Sba::DetachEquipment(id));
                    }
                }
            }
        }
```

- [ ] **Step 4: Add aura ETB attachment protection check in `stack.rs`**

Find the aura attachment block in `resolve_top` (around line 481–492):
```rust
                // CR 303.4: An Aura enters the battlefield attached to the target declared at cast time.
                if is_aura
                    && let Some(crate::types::effect::EffectTarget::Object { id: host_id }) =
                        targets.first()
                {
                    let host_id = *host_id;
                    if state.battlefield.contains_key(&host_id)
                        && let Some(perm) = state.battlefield.get_mut(&card_id)
                    {
                        perm.attached_to = Some(host_id);
                    }
                }
```
Replace with:
```rust
                // CR 303.4 / 702.16c: attach aura only if host doesn't have protection from it.
                // If protected, the aura stays on battlefield unattached; SBA 704.5m removes it.
                if is_aura {
                    if let Some(crate::types::effect::EffectTarget::Object { id: host_id }) =
                        targets.first()
                    {
                        let host_id = *host_id;
                        if state.battlefield.contains_key(&host_id) {
                            let (aura_colors, aura_types, aura_subtypes) = state
                                .objects
                                .get(&card_id)
                                .map(|o| (
                                    o.definition.colors.clone(),
                                    o.definition.type_line.card_types.clone(),
                                    o.definition.type_line.subtypes.clone(),
                                ))
                                .unwrap_or_default();
                            let host_protected = state
                                .objects
                                .get(&host_id)
                                .map(|o| crate::engine::has_protection_from(
                                    o,
                                    &aura_colors,
                                    &aura_types,
                                    &aura_subtypes,
                                ))
                                .unwrap_or(false);
                            if !host_protected {
                                if let Some(perm) = state.battlefield.get_mut(&card_id) {
                                    perm.attached_to = Some(host_id);
                                }
                            }
                        }
                    }
                }
```

- [ ] **Step 5: Add equipment `Attach` protection check in `stack.rs`**

Find `EffectStep::Attach { source_id }` (around line 362–374). Replace:
```rust
            EffectStep::Attach { source_id } => {
                if let Some(EffectTarget::Object { id: target_id }) = targets.first() {
                    let target_id = *target_id;
                    if state.battlefield.contains_key(source_id)
                        && state.battlefield.contains_key(&target_id)
                        && let Some(perm) = state.battlefield.get_mut(source_id)
                    {
                        perm.attached_to = Some(target_id);
                    }
                }
            }
```
With:
```rust
            // CR 702.6a: attach the equipment (source_id) to the first target.
            // CR 702.16d: skip if target has protection from the equipment's quality.
            EffectStep::Attach { source_id } => {
                if let Some(EffectTarget::Object { id: target_id }) = targets.first() {
                    let target_id = *target_id;
                    if state.battlefield.contains_key(source_id)
                        && state.battlefield.contains_key(&target_id)
                    {
                        let (equip_colors, equip_types, equip_subtypes) = state
                            .objects
                            .get(source_id)
                            .map(|o| (
                                o.definition.colors.clone(),
                                o.definition.type_line.card_types.clone(),
                                o.definition.type_line.subtypes.clone(),
                            ))
                            .unwrap_or_default();
                        let protected = state
                            .objects
                            .get(&target_id)
                            .map(|o| crate::engine::has_protection_from(
                                o,
                                &equip_colors,
                                &equip_types,
                                &equip_subtypes,
                            ))
                            .unwrap_or(false);
                        if !protected {
                            if let Some(perm) = state.battlefield.get_mut(source_id) {
                                perm.attached_to = Some(target_id);
                            }
                        }
                    }
                }
            }
```

- [ ] **Step 6: Run all tests**

```bash
cargo test 2>&1 | grep -E "^test result|FAILED|error\["
```
Expected: `test result: ok`

- [ ] **Step 7: Clippy**

```bash
cargo clippy --all-targets 2>&1 | grep -E "^error|^warning"
```
Fix any warnings.

- [ ] **Step 8: Commit**

```bash
git add src/engine/state_based_actions.rs src/engine/stack.rs
git commit -m "$(cat <<'EOF'
feat: CR 702.16c/d — protection prevents enchant/equip attachment (E in DEBT)

Co-Authored-By: Claude Sonnet 4.6 <noreply@anthropic.com>
EOF
)"
```

---

## Task 9: Update `docs/todo.md`

- [ ] **Step 1: Delete completed bullets**

In `docs/todo.md`, delete these lines from the "Unblocked" section under "Protection from X — partial":
- The "Damage prevention (D in DEBT)" bullet and its description
- The "Enchant/Equip prevention (E in DEBT)" bullet and its description  
- The "Protection from non-color qualities" bullet and its description
- The "Protection from everything" bullet and its description
- The "Hexproof from color" bullet and its description

If all sub-bullets are gone, also delete the "### Protection from X — partial" heading and the surrounding blank lines. Update the summary note at the top of the Unblocked section to reflect that everything is done.

- [ ] **Step 2: Run tests one final time**

```bash
cargo test 2>&1 | grep -E "^test result|FAILED|error\["
cargo clippy --all-targets 2>&1 | grep -E "^error|^warning"
```

- [ ] **Step 3: Commit**

```bash
git add docs/todo.md
git commit -m "$(cat <<'EOF'
chore: mark protection DEBT + HexproofFrom items complete in todo.md

Co-Authored-By: Claude Sonnet 4.6 <noreply@anthropic.com>
EOF
)"
```
