# Type Renames Design

**Date:** 2026-06-19
**Status:** Approved

## Motivation

Several core types have names that are either misleading, inconsistent with their usage, or carry unwanted connotations from the parsing phase. This document specifies a set of targeted renames to improve clarity and internal consistency.

## Renames

### 1. `enum Ability` → `enum Rule`

`Ability` holds variants like `SpellEffect` and `Cycling` that are not "abilities" in the CR sense (CR 113). The name `Rule` is intentionally broader: every variant represents a piece of card logic the engine knows about.

CR reference: CR 113 defines abilities; CR 207.1 uses "rules text" to refer collectively to the text defining a card's behaviour.

### 2. `struct SpellAbility` → `struct SpellEffect`

The struct named `SpellAbility` is the payload of the `Rule::SpellEffect` variant. Renaming the struct to match the variant eliminates the name mismatch. The result (`Rule::SpellEffect(SpellEffect { ... })`) follows the same convention as other variants.

### 3. `enum OracleSpan` → `enum RulesText`

`OracleSpan` carries the word "Span" which implies a byte-range cursor from the parser. This type is used throughout the engine after parsing, including for intrinsic abilities constructed entirely in code. The MTG rules use "rules text" (CR 207.1) to refer to the text box content that defines a card's abilities — making `RulesText` an accurate and MTG-grounded name.

### 4. `RulesText::Parsed` → `RulesText::Active`

The variant name `Parsed` implies the entry was produced by the parser. In practice, `inject_intrinsic_abilities` and several engine functions construct these entries from code without parsing. `Active` means "the engine actively enforces this entry."

### 5. `CardDefinition::abilities: Vec<OracleSpan>` → `CardDefinition::rules_text: Vec<RulesText>`

The field `abilities` is misleading because it holds `Ignored`, `Unparsed`, and `ParsedUnimplemented` entries alongside enforced rules — not all of which are "abilities". Renaming it to `rules_text` aligns with the CR 207.1 term and with the new type name.

## Complete Rename Table

| Old | New | Kind |
|---|---|---|
| `enum Ability` | `enum Rule` | type |
| `Ability::Static(StaticAbility)` | `Rule::Static(StaticAbility)` | variant (name unchanged, container renamed) |
| `Ability::Triggered(TriggeredAbility)` | `Rule::Triggered(TriggeredAbility)` | variant (name unchanged, container renamed) |
| `Ability::Activated(ActivatedAbility)` | `Rule::Activated(ActivatedAbility)` | variant (name unchanged, container renamed) |
| `Ability::SpellEffect(SpellAbility)` | `Rule::SpellEffect(SpellEffect)` | variant + payload type rename |
| `Ability::Cycling(ManaCost)` | `Rule::Cycling(ManaCost)` | variant (name unchanged, container renamed) |
| `struct SpellAbility` | `struct SpellEffect` | type |
| `enum OracleSpan` | `enum RulesText` | type |
| `OracleSpan::Parsed(Ability)` | `RulesText::Active(Rule)` | variant |
| `OracleSpan::Ignored` | `RulesText::Ignored` | variant (name unchanged, container renamed) |
| `OracleSpan::Unparsed` | `RulesText::Unparsed` | variant (name unchanged, container renamed) |
| `OracleSpan::ParsedUnimplemented` | `RulesText::ParsedUnimplemented` | variant (name unchanged, container renamed) |
| `CardDefinition::abilities` | `CardDefinition::rules_text` | field |

## Out of Scope

All other types are unchanged: `StaticAbility`, `TriggeredAbility`, `ActivatedAbility`, `AnnotationKind`, `TextAnnotation`, `IgnoredKind`, `CostComponent`, `SpellFilter`, `CastFilter`, `TargetFilter`, `TriggerEvent`, `GameEvent`, etc.

The file `types/ability.rs` retains its name (the module still houses ability-related types).

## Affected Files

All files in `src/` that import or pattern-match on these types. Key sites:

- `src/types/ability.rs` — definitions
- `src/types/mod.rs` — re-exports
- `src/types/card.rs` — `CardDefinition::abilities` field
- `src/types/card_object.rs` — `inject_intrinsic_abilities`, field accesses
- `src/types/permanent.rs` — keyword checks, test fixtures
- `src/parser/oracle.rs` — `OracleSpan::Parsed(Ability::...)` construction throughout
- `src/engine/triggered.rs` — triggered ability iteration and test fixtures
- `src/engine/state_based_actions.rs` — ability construction
- `src/engine/cycling.rs` — cycling ability extraction
- `src/engine/stack.rs` — spell resolution
- `src/engine/casting.rs` — spell targeting
- `src/engine/activated.rs` — activated ability lookup
- `src/cards/mod.rs` — ability filtering
- `src/serve.rs` — UI action filtering and inline card definitions
