# Type Renames Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Rename six types/fields across the codebase for accuracy and consistency, as described in `docs/superpowers/specs/2026-06-19-type-renames-design.md`.

**Architecture:** Four sequential tasks. Task 1 is the atomic step — it renames definitions and all variant-name call sites at once (Rust won't compile with a missing variant), bridging the gap with backward-compat type aliases so Tasks 2–4 can proceed incrementally. Tasks 2 and 3 clean up the alias dependencies and field name. Task 4 removes the aliases and verifies everything.

**Tech Stack:** Rust, Cargo. No new dependencies.

## Global Constraints

- Run `cargo test 2>&1 | grep -E "^test result|FAILED|error\["` after each task (not full output unless there are failures).
- Run `cargo clippy --all-targets` before the final commit in Task 4; fix any warnings before committing.
- Never use `git add -A`; stage named files only.
- Do not change any logic — these are pure mechanical renames.
- After Task 4, delete the two completed bullet points from `docs/todo.md` (the ones under "# Renames" about renaming `Ability` and `Parsed`).

---

## File Map

| File | Change |
|---|---|
| `src/types/ability.rs` | Rename `enum Ability`→`Rule`, `struct SpellAbility`→`SpellEffect`, `enum OracleSpan`→`RulesText`, variant `Parsed`→`Active`; add type aliases; update all internal usages |
| `src/types/mod.rs` | Add new canonical names to re-exports (Task 1); swap to canonical names, remove alias re-exports (Task 2) |
| `src/types/card.rs` | Rename field `abilities`→`rules_text` and update its type annotation (Task 3) |
| `src/types/card_object.rs` | Update type name refs (Task 2); field rename (Task 3) |
| `src/types/permanent.rs` | Update type name refs (Task 2); field rename (Task 3) |
| `src/parser/oracle.rs` | Update type name refs and import (Task 2); field rename (Task 3) |
| `src/engine/triggered.rs` | Update type name refs (Task 2); field rename (Task 3) |
| `src/engine/state_based_actions.rs` | Update type name refs (Task 2); field rename (Task 3) |
| `src/engine/combat.rs` | Update type name refs (Task 2); field rename (Task 3) |
| `src/engine/cycling.rs` | Update type name refs (Task 2); field rename (Task 3) |
| `src/engine/stack.rs` | Update type name refs (Task 2); field rename (Task 3) |
| `src/engine/casting.rs` | Update type name refs (Task 2); field rename (Task 3) |
| `src/engine/activated.rs` | Update type name refs (Task 2); field rename (Task 3) |
| `src/engine/targeting.rs` | Update type name refs (Task 2); field rename (Task 3) |
| `src/engine/turn.rs` | Update type name refs (Task 2); field rename (Task 3) |
| `src/engine/mana.rs` | Field rename (Task 3) |
| `src/engine/costs.rs` | Field rename (Task 3) |
| `src/cards/mod.rs` | Update type name refs (Task 2); field rename (Task 3) |
| `src/serve.rs` | Update type name refs (Task 2); field rename (Task 3) |
| `docs/todo.md` | Remove completed Renames items (Task 4) |

---

### Task 1: Rename definitions and all `::Parsed(` variant sites

This task is atomic: the variant `Parsed` no longer exists after the definition change, so **all** call sites must be updated in the same step. Type aliases are added so the old type names still compile — they are removed in Task 4.

**Files:**
- Modify: `src/types/ability.rs`
- Modify: `src/types/mod.rs`
- All `*.rs` files under `src/` (sed pass for variant rename)

**Interfaces:**
- Produces: `pub enum Rule`, `pub struct SpellEffect`, `pub enum RulesText` with variant `Active`; type aliases `Ability = Rule`, `SpellAbility = SpellEffect`, `OracleSpan = RulesText` so all other files continue to compile.

- [ ] **Step 1: Apply definition changes to `src/types/ability.rs`**

  Make the following targeted edits (line numbers are approximate — verify with your editor):

  a. Rename `struct SpellAbility` and update its doc comment (around line 387):
  ```rust
  // OLD
  /// A spell ability — the text of an instant or sorcery that takes effect when it resolves.
  /// Wraps effect steps and any targeting requirements.
  #[derive(Debug, Clone, PartialEq, Eq)]
  pub struct SpellAbility {
  ```
  ```rust
  // NEW
  /// The resolving text of an instant or sorcery (CR 113.3a).
  /// Wraps effect steps and any targeting requirements.
  #[derive(Debug, Clone, PartialEq, Eq)]
  pub struct SpellEffect {
  ```

  b. Rename `enum Ability` and update its `SpellEffect` variant's payload type (around line 426):
  ```rust
  // OLD
  #[derive(Debug, Clone, PartialEq, Eq)]
  pub enum Ability {
      Static(StaticAbility),
      Triggered(TriggeredAbility),
      Activated(ActivatedAbility),
      SpellEffect(SpellAbility),
      Cycling(ManaCost),
  }
  ```
  ```rust
  // NEW
  #[derive(Debug, Clone, PartialEq, Eq)]
  pub enum Rule {
      Static(StaticAbility),
      Triggered(TriggeredAbility),
      Activated(ActivatedAbility),
      SpellEffect(SpellEffect),
      Cycling(ManaCost),
  }
  ```

  c. Rename `enum OracleSpan` and its `Parsed` variant (around line 436):
  ```rust
  // OLD
  /// A typed span of oracle text.
  /// The ordered sequence of spans represents the full oracle text.
  #[derive(Debug, Clone, PartialEq, Eq)]
  pub enum OracleSpan {
      /// A recognised ability the engine can act on.
      Parsed(Ability),
      /// Non-rules text — displayed in italics in the UI.
      Ignored(IgnoredKind, String),
      /// Text the parser could not interpret — displayed red+underline in the UI.
      Unparsed(String),
      /// A CR 702 keyword the parser recognises by name but the engine does not yet enforce.
      /// Displayed cyan+underline in the UI.
      ParsedUnimplemented(String),
  }
  ```
  ```rust
  // NEW
  /// A classified entry in a card's rules text (CR 207.1).
  /// The ordered sequence of entries represents the full oracle text.
  #[derive(Debug, Clone, PartialEq, Eq)]
  pub enum RulesText {
      /// A rule the engine actively enforces.
      Active(Rule),
      /// Non-rules text — displayed in italics in the UI.
      Ignored(IgnoredKind, String),
      /// Text the parser could not interpret — displayed red+underline in the UI.
      Unparsed(String),
      /// A CR 702 keyword the parser recognises by name but the engine does not yet enforce.
      /// Displayed cyan+underline in the UI.
      ParsedUnimplemented(String),
  }
  ```

  d. Add backward-compat type aliases at the **bottom** of `ability.rs`, after all definitions and before `#[cfg(test)]`:
  ```rust
  // Backward-compat aliases — removed in the type-renames cleanup (Task 4).
  pub type Ability = Rule;
  pub type SpellAbility = SpellEffect;
  pub type OracleSpan = RulesText;
  ```

- [ ] **Step 2: Add new canonical names to `src/types/mod.rs` re-exports**

  Find the existing `pub use ability::{...}` block and add `Rule`, `SpellEffect`, `RulesText` to it (keep the old names — they resolve via the aliases):
  ```rust
  pub use ability::{
      Ability, Rule, ActivatedAbility, AnnotationKind, CardFilter, CastFilter, Cost, CostComponent,
      DamageTargetKind, GameEvent, IgnoredKind, LandwalkKind, OracleSpan, RulesText, PermanentFilter,
      SpellAbility, SpellEffect, SpellFilter, StaticAbility, TargetFilter, TextAnnotation,
      TriggerCondition, TriggerEvent, TriggerSubjectFilter, TriggerTargetMode, TriggeredAbility,
      TurnOwner,
  };
  ```

- [ ] **Step 3: Rename `::Parsed(` → `::Active(` across all source files**

  This is the only part of the variant rename that must happen before the code can compile. Run from the repo root:
  ```bash
  find src -name "*.rs" | xargs perl -i -pe 's/::Parsed\(/::Active\(/g'
  ```

  Verify the change was applied and nothing unexpected was touched:
  ```bash
  grep -rn "::Parsed(" src/
  ```
  Expected output: empty (no remaining matches).

  Double-check `ParsedUnimplemented` is untouched (it has no `(` immediately after `Parsed`):
  ```bash
  grep -rn "ParsedUnimplemented" src/ | wc -l
  ```
  Expected: same count as before (around 10 hits). If it changed, the sed was wrong — undo with `git checkout src/` and investigate.

- [ ] **Step 4: Run tests**

  ```bash
  cargo test 2>&1 | grep -E "^test result|FAILED|error\["
  ```
  Expected: `test result: ok` on all test suites, 0 failures. If there are compile errors, they will show as `error[...]` lines — fix them before proceeding.

- [ ] **Step 5: Commit**

  ```bash
  git add src/types/ability.rs src/types/mod.rs
  git add $(git diff --name-only src/)
  git commit -m "$(cat <<'EOF'
  refactor: rename Rule/SpellEffect/RulesText types, add compat aliases

  Renames enum Ability→Rule, struct SpellAbility→SpellEffect,
  enum OracleSpan→RulesText, and variant Parsed→Active. Backward-compat
  type aliases preserve compilation of all call sites until cleaned up.

  Co-Authored-By: Claude Sonnet 4.6 <noreply@anthropic.com>
  EOF
  )"
  ```

---

### Task 2: Update all type-name references to use canonical names

Replace alias usage (`Ability::`, `OracleSpan`, `SpellAbility`) with the canonical names (`Rule::`, `RulesText`, `SpellEffect`) across every source file, then remove the aliases.

**Files:**
- All `*.rs` files under `src/` (three sed/perl passes)
- Modify: `src/types/ability.rs` (remove aliases)
- Modify: `src/types/mod.rs` (remove old names from re-exports)

**Interfaces:**
- Consumes: Task 1 output — aliases exist, all files still compile with old names.
- Produces: All files use canonical names; aliases gone.

- [ ] **Step 1: Replace `OracleSpan` with `RulesText` everywhere**

  ```bash
  find src -name "*.rs" | xargs sed -i '' 's/OracleSpan/RulesText/g'
  ```

  Spot-check a couple of files:
  ```bash
  grep -n "OracleSpan\|RulesText" src/types/card.rs
  grep -n "OracleSpan\|RulesText" src/parser/oracle.rs | head -5
  ```
  Expected: no `OracleSpan`, only `RulesText`.

- [ ] **Step 2: Replace `SpellAbility` with `SpellEffect` everywhere**

  ```bash
  find src -name "*.rs" | xargs sed -i '' 's/SpellAbility/SpellEffect/g'
  ```

  Spot-check:
  ```bash
  grep -rn "SpellAbility" src/
  ```
  Expected: empty.

- [ ] **Step 3: Replace `Ability::` with `Rule::` everywhere (without touching `StaticAbility::` etc.)**

  The negative lookbehind `(?<![A-Za-z_])` ensures only bare `Ability::` is matched, not `StaticAbility::`, `TriggeredAbility::`, or `ActivatedAbility::`:
  ```bash
  find src -name "*.rs" | xargs perl -i -pe 's/(?<![A-Za-z_])Ability::/Rule::/g'
  ```

  Verify `StaticAbility::`, `TriggeredAbility::`, `ActivatedAbility::` are untouched:
  ```bash
  grep -rn "StaticAbility::\|TriggeredAbility::\|ActivatedAbility::" src/ | wc -l
  ```
  Expected: same count as before this step (non-zero). Also verify:
  ```bash
  grep -rn "[^a-zA-Z_]Ability::\|^Ability::" src/
  ```
  Expected: empty (no bare `Ability::` remaining).

- [ ] **Step 4: Remove the backward-compat aliases from `src/types/ability.rs`**

  Delete these three lines from the bottom of `ability.rs` (above `#[cfg(test)]`):
  ```rust
  // Backward-compat aliases — removed in the type-renames cleanup (Task 4).
  pub type Ability = Rule;
  pub type SpellAbility = SpellEffect;
  pub type OracleSpan = RulesText;
  ```

- [ ] **Step 5: Update `src/types/mod.rs` re-exports to remove old names**

  Remove `Ability`, `SpellAbility`, `OracleSpan` from the `pub use ability::{...}` block. The result should be:
  ```rust
  pub use ability::{
      Rule, ActivatedAbility, AnnotationKind, CardFilter, CastFilter, Cost, CostComponent,
      DamageTargetKind, GameEvent, IgnoredKind, LandwalkKind, RulesText, PermanentFilter,
      SpellEffect, SpellFilter, StaticAbility, TargetFilter, TextAnnotation,
      TriggerCondition, TriggerEvent, TriggerSubjectFilter, TriggerTargetMode, TriggeredAbility,
      TurnOwner,
  };
  ```

- [ ] **Step 6: Run tests**

  ```bash
  cargo test 2>&1 | grep -E "^test result|FAILED|error\["
  ```
  Expected: `test result: ok` on all suites, 0 failures.

- [ ] **Step 7: Commit**

  ```bash
  git add src/types/ability.rs src/types/mod.rs
  git add $(git diff --name-only src/)
  git commit -m "$(cat <<'EOF'
  refactor: replace backward-compat aliases with canonical type names

  All call sites now use Rule/SpellEffect/RulesText directly.
  Removes the temporary Ability/SpellAbility/OracleSpan aliases.

  Co-Authored-By: Claude Sonnet 4.6 <noreply@anthropic.com>
  EOF
  )"
  ```

---

### Task 3: Rename `CardDefinition::abilities` field to `rules_text`

Rename the field on `CardDefinition` and every access site across the codebase.

**Files:**
- Modify: `src/types/card.rs` (field declaration and `has_unparsed` method)
- All other `*.rs` files under `src/` (field access sites via perl pass)

**Interfaces:**
- Consumes: Task 2 output — all type names are canonical.
- Produces: `CardDefinition` exposes `rules_text: Vec<RulesText>` and no `abilities` field.

- [ ] **Step 1: Rename field declaration in `src/types/card.rs`**

  Change the field and its type annotation in `CardDefinition` (around line 60):
  ```rust
  // OLD
  pub abilities: Vec<OracleSpan>,
  ```
  ```rust
  // NEW
  pub rules_text: Vec<RulesText>,
  ```

  Also update the `has_unparsed` method body in the same file (around line 68):
  ```rust
  // OLD
  pub fn has_unparsed(&self) -> bool {
      self.abilities
          .iter()
          .any(|s| matches!(s, OracleSpan::Unparsed(_)))
  }
  ```
  ```rust
  // NEW
  pub fn has_unparsed(&self) -> bool {
      self.rules_text
          .iter()
          .any(|s| matches!(s, RulesText::Unparsed(_)))
  }
  ```

  Also update the import at the top of `card.rs`:
  ```rust
  // OLD
  use super::ability::{OracleSpan, TextAnnotation};
  ```
  ```rust
  // NEW
  use super::ability::{RulesText, TextAnnotation};
  ```

- [ ] **Step 2: Rename all field access sites and struct literal fields across remaining files**

  The word `abilities` (as a standalone word, not part of `mana_abilities` etc.) must become `rules_text`. Use perl with word boundaries:
  ```bash
  find src -name "*.rs" | xargs perl -i -pe 's/\babilities\b/rules_text/g'
  ```

  Note: local variables that were named `abilities` (e.g. `let abilities: Vec<_>`) will also be renamed to `rules_text`. This is intentional and correct — they were shadowing the field name.

  Verify no `abilities` remain where they should have been renamed:
  ```bash
  grep -rn "\babilities\b" src/
  ```
  Expected: empty. (Any remaining hits would be a missed rename — fix them manually.)

- [ ] **Step 3: Run tests**

  ```bash
  cargo test 2>&1 | grep -E "^test result|FAILED|error\["
  ```
  Expected: `test result: ok` on all suites, 0 failures.

- [ ] **Step 4: Commit**

  ```bash
  git add src/types/card.rs
  git add $(git diff --name-only src/)
  git commit -m "$(cat <<'EOF'
  refactor: rename CardDefinition::abilities → rules_text

  Aligns the field name with its type (Vec<RulesText>) and with the
  CR 207.1 term for the text box content.

  Co-Authored-By: Claude Sonnet 4.6 <noreply@anthropic.com>
  EOF
  )"
  ```

---

### Task 4: Final cleanup — remove todo items, run clippy, verify

**Files:**
- Modify: `docs/todo.md`

**Interfaces:**
- Consumes: Tasks 1–3 complete; codebase fully using canonical names.
- Produces: Clean clippy output, green tests, todo items removed.

- [ ] **Step 1: Delete completed items from `docs/todo.md`**

  Remove the following two bullet points (and the sub-bullet) from the `# Renames` section:
  ```
  - Consider renaming `Ability` (in `types/ability.rs`) to `Rule` or `RulesText` ...
      - Also rename SpellAbility to something else like SpellEffect
  - Rename Parsed to something more like Ast, because we sometimes manually insert ...
  ```

  The `# Renames` section should be empty (or the heading may be removed if no items remain).

- [ ] **Step 2: Run clippy and fix any warnings**

  ```bash
  cargo clippy --all-targets 2>&1 | grep -E "warning|error"
  ```

  If there are warnings, run the auto-fixer first:
  ```bash
  cargo clippy --fix --all-targets
  ```
  Then re-run to confirm clean:
  ```bash
  cargo clippy --all-targets 2>&1 | grep -E "warning|error"
  ```
  Expected: no warnings or errors (only the trailing `Finished` line).

- [ ] **Step 3: Run full test suite**

  ```bash
  cargo test 2>&1 | grep -E "^test result|FAILED|error\["
  ```
  Expected: `test result: ok` on all suites, 0 failures.

- [ ] **Step 4: Commit**

  ```bash
  git add docs/todo.md
  git add -p src/  # stage any clippy auto-fixes
  git commit -m "$(cat <<'EOF'
  chore: remove completed rename todo items, fix clippy warnings

  Co-Authored-By: Claude Sonnet 4.6 <noreply@anthropic.com>
  EOF
  )"
  ```
