# Combat Declaration Auto-Skip Design

**Date:** 2026-06-12

## Overview

Automatically skip the Declare Attackers and Declare Blockers steps (no player interaction required) when no legal declarations exist. Also filter the UI to only show blocker assignments that are actually legal for a given attacker, preventing invalid choices from appearing at all.

## Goals

1. Skip the Declare Attackers step when the active player has no creatures that can attack.
2. Skip the Declare Blockers step when no defending creature can legally block any of the current attackers (or when there are no attackers).
3. Filter blocker-assignment UI actions to only show valid blocker-attacker pairings (evasion-aware).

## Architecture

### New function: `can_block_attacker` in `engine/combat.rs`

```rust
pub fn can_block_attacker(state: &GameState, blocker_id: ObjectId, attacker_id: ObjectId) -> bool
```

Checks all per-pair legality rules from CR 509.1:
- CR 702.9b: Flying — attacker with flying can only be blocked by creatures with flying or reach.
- CR 702.28b: Shadow — blocker and attacker must both have shadow, or both lack it.
- CR 702.31b: Horsemanship — attacker with horsemanship can only be blocked by creatures with horsemanship.
- CR 702.118b: Skulk — attacker with skulk can't be blocked by creatures with greater power.
- CR 702.147a: Decayed — creature with decayed can't block.
- Calls the existing `can_block()` predicate on the blocker as a prerequisite.

Menace is **not** included here — it constrains the whole declaration (≥2 blockers per menace attacker), not any individual pairing. It stays as a post-declaration check in `declare_blockers`.

`declare_blockers` is refactored to call `can_block_attacker` for each pair instead of repeating the same checks inline.

### UI filtering in `serve.rs`

In `compute_battlefield_actions`, the blocker-assignment action loop currently iterates over all attackers for any `can_block()` creature. Change it to additionally call `can_block_attacker` per (blocker, attacker) pair, skipping pairs that are illegal.

### Auto-skip in `apply_step_start_loop` (`serve.rs`)

Two new helper predicates:

```rust
fn has_valid_attackers(state: &GameState) -> bool
```
Returns true if any active-player creature satisfies `can_attack(cmt)`.

```rust
fn has_valid_blockers(state: &GameState) -> bool
```
Returns true if any (defending creature, current attacker) pair satisfies `can_block_attacker`. Returns false if there are no attackers.

`apply_step_start_loop` is extended: after `apply_step_start`, before the break-or-advance decision, check these predicates. If they return false for the current step, auto-call `declare_attackers(state, active, &[])` or `declare_blockers(state, defender, &[])` and continue advancing. This keeps the auto-skip on the same code path as a human player declaring with an empty list.

## Data Flow

```
apply_step_start_loop
  → apply_step_start (unchanged for DA/DB steps)
  → if DeclareAttackers && !has_valid_attackers → declare_attackers([], ...) → advance_step
  → if DeclareBlockers && !has_valid_blockers  → declare_blockers([], ...)  → advance_step
  → otherwise break (player interaction needed)
```

## Testing

- Unit tests for `can_block_attacker`: verify each evasion rule (flying, shadow, horsemanship, skulk, decayed).
- Unit tests for `has_valid_attackers` / `has_valid_blockers`.
- Integration tests in `serve.rs`: verify auto-skip fires correctly when active player has no eligible attackers; verify auto-skip fires when all defenders are blocked by evasion; verify no auto-skip when at least one valid option exists.
- Existing `declare_blockers` tests must continue to pass (refactor is pure extraction, behaviour unchanged).

## Out of Scope

- Protection abilities (e.g., "protection from red") — not yet implemented in the engine.
- Banding, landwalk, and other niche evasion abilities — not yet in the engine.
- Forced-block effects (e.g., "must block if able") — not yet in scope.
