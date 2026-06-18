# EffectStep Damage Keywords Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Make `EffectStep::DealDamage` carry source keyword flags (Lifelink, Deathtouch, Wither, Infect) so that activated abilities, triggered abilities, and spells dealing damage correctly apply keyword effects on resolution.

**Architecture:** Replace `DealDamage(u32)` with `DealDamage(DamageStep)` where `DamageStep` holds the amount plus four boolean keyword flags. Flags default to `false` (parser always produces flag-less steps); an `inject_source_flags` helper reads the source permanent's static abilities at stack-push time and fills them in. Resolution in `execute_effect_steps` branches on the flags to apply -1/-1 counters (Wither/Infect), deathtouch flag, poison counters (Infect on players), and controller life gain (Lifelink).

**Tech Stack:** Rust, `cargo test`, `cargo clippy --all-targets`

**Spec:** `docs/superpowers/specs/2026-06-18-effectstep-damage-keywords-design.md`

## Global Constraints

- Run `cargo test 2>&1 | grep -E "^test result|FAILED|error\["` after every task — all tests must pass before committing.
- Run `cargo clippy --all-targets 2>&1 | grep -E "^error|^warning"` before the final commit and fix any warnings.
- CR references in comments must be verified against `docs/CR.txt` via `grep '^<rule>\.' docs/CR.txt` before committing.
- `DamageStep` derives `Default` — never add `toxic_n` or `combat` fields (excluded by design; see spec).
- Commit message format: `feat: <description>` with `Co-Authored-By: Claude Sonnet 4.6 <noreply@anthropic.com>` trailer.

---

### Task 1: Define `DamageStep` and compile-fix all call sites

This is a pure type migration. No behaviour changes — all flags default to `false`, existing tests must still pass.

**Files:**
- Modify: `src/types/effect.rs:30`
- Modify: `src/engine/stack.rs:128-142` (arm compiles; full logic replaced in Task 2)
- Modify: `src/engine/stack.rs` tests — three `DealDamage(3)` and one `DealDamage(3)` in fizzle test
- Modify: `src/engine/activated.rs:607` (test helper)
- Modify: `src/parser/oracle.rs:260`
- Modify: `src/serve.rs:442` (display), `src/serve.rs:2397`, `src/serve.rs:2442`, `src/serve.rs:2485` (tests)

**Interfaces:**
- Produces: `DamageStep { amount: u32, lifelink: bool, deathtouch: bool, wither: bool, infect: bool }` and `EffectStep::DealDamage(DamageStep)` — used by all subsequent tasks.

- [ ] **Step 1: Add `DamageStep` to `src/types/effect.rs`**

Replace the existing `DealDamage(u32)` line and add the struct above the enum. The file currently has `DealDamage(u32)` at line 30.

```rust
// In src/types/effect.rs, add this struct before the EffectStep enum:

/// CR 702.15b, 702.2b, 702.80a, 702.90b/c — source keyword flags snapshotted at
/// stack-push time. All flags default to false; the parser always produces flag-less
/// steps and inject_source_flags fills them in at runtime.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct DamageStep {
    pub amount: u32,
    pub lifelink: bool,
    pub deathtouch: bool,
    pub wither: bool,
    pub infect: bool,
}
```

Then change the variant inside `EffectStep`:
```rust
// Before:
DealDamage(u32),

// After:
DealDamage(DamageStep),
```

- [ ] **Step 2: Fix the match arm in `src/engine/stack.rs` (lines 128–142)**

This is a minimal fix to make it compile. The full logic is replaced in Task 2.

```rust
// Replace the entire DealDamage arm (currently lines 128-142):

// TODO CR 702.2b/702.15b/702.80a/702.90b/c: keyword flags checked at resolution.
// Flags are injected by inject_source_flags at stack-push time.
EffectStep::DealDamage(s) => match targets.first() {
    Some(EffectTarget::Object { id }) => {
        if let Some(perm) = state.battlefield.get_mut(id) {
            perm.damage_marked += s.amount;
        }
    }
    Some(EffectTarget::Player { id }) => {
        if let Some(player) = state.get_player_mut(*id) {
            player.life -= s.amount as i32;
        }
    }
    _ => {}
},
```

- [ ] **Step 3: Fix tests in `src/engine/stack.rs`**

Find all `EffectStep::DealDamage(3)` and `EffectStep::DealDamage(1)` in the test module and update them. There are four:

```rust
// All occurrences of:
EffectStep::DealDamage(3)
// become:
EffectStep::DealDamage(crate::types::effect::DamageStep { amount: 3, ..Default::default() })

// And in the fizzle test (activated ability):
EffectStep::DealDamage(3)
// becomes:
EffectStep::DealDamage(crate::types::effect::DamageStep { amount: 3, ..Default::default() })
```

- [ ] **Step 4: Fix test helper in `src/engine/activated.rs` (line ~607)**

```rust
// Before:
effect: vec![EffectStep::DealDamage(1)],

// After:
effect: vec![EffectStep::DealDamage(crate::types::effect::DamageStep { amount: 1, ..Default::default() })],
```

- [ ] **Step 5: Fix the parser in `src/parser/oracle.rs` (line 260)**

```rust
// Before:
return Some(EffectStep::DealDamage(n));

// After:
return Some(EffectStep::DealDamage(crate::types::effect::DamageStep { amount: n, ..Default::default() }));
```

- [ ] **Step 6: Fix `src/serve.rs`**

Line 442 — display format:
```rust
// Before:
EffectStep::DealDamage(n) => format!("Deal {n} damage"),

// After:
EffectStep::DealDamage(s) => format!("Deal {} damage", s.amount),
```

Lines 2397, 2442, 2485 — test construction (all three are `EffectStep::DealDamage(2)` or `EffectStep::DealDamage(1)`):
```rust
// Before:
effect: vec![EffectStep::DealDamage(2)],
// After:
effect: vec![EffectStep::DealDamage(mecha_oracle::types::effect::DamageStep { amount: 2, ..Default::default() })],

// Before (line 2485):
effect: vec![EffectStep::DealDamage(1)],
// After:
effect: vec![EffectStep::DealDamage(mecha_oracle::types::effect::DamageStep { amount: 1, ..Default::default() })],
```

Note: `serve.rs` is outside the library crate so uses `mecha_oracle::types::effect::DamageStep`. Check the existing import style in the file — if `EffectStep` is imported directly, the path may differ.

- [ ] **Step 7: Verify it compiles and all tests pass**

```bash
cargo test 2>&1 | grep -E "^test result|FAILED|error\["
```

Expected: `test result: ok.` with no failures.

- [ ] **Step 8: Commit**

```bash
git add src/types/effect.rs src/engine/stack.rs src/engine/activated.rs \
        src/parser/oracle.rs src/serve.rs
git commit -m "$(cat <<'EOF'
feat: replace DealDamage(u32) with DealDamage(DamageStep)

DamageStep carries keyword flags (lifelink, deathtouch, wither, infect)
that default to false. No behaviour change — flag injection and resolution
logic follow in subsequent commits.

Co-Authored-By: Claude Sonnet 4.6 <noreply@anthropic.com>
EOF
)"
```

---

### Task 2: TDD the updated resolution logic in `execute_effect_steps`

Write tests first with DamageStep flags pre-set, verify they fail, then implement the resolution arm.

**Files:**
- Modify: `src/engine/stack.rs` — add tests, replace the DealDamage arm

**Interfaces:**
- Consumes: `DamageStep` from Task 1; `PermanentState::damaged_by_deathtouch`, `PermanentState::add_counters`, `PlayerState::add_counters` from existing types; `check_and_apply_sbas` from `state_based_actions`.
- Produces: correct per-flag resolution behaviour in `execute_effect_steps`.

- [ ] **Step 1: Write all failing tests**

Add this block to the `tests` module at the bottom of `src/engine/stack.rs`. Each test constructs a `StackObject` with pre-set `DamageStep` flags and calls `resolve_top`.

```rust
// ── DamageStep keyword resolution tests ─────────────────────────────────────

fn make_creature_on_battlefield(
    gs: &mut GameState,
    owner: PlayerId,
    power: i32,
    toughness: i32,
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
        abilities: vec![],
        text_annotations: vec![],
        power: Some(power),
        toughness: Some(toughness),
        colors: vec![],
    };
    let id = gs.alloc_id();
    let obj = CardObject::new(id, def, owner, Zone::Battlefield);
    gs.battlefield.insert(id, PermanentState::new(&obj.definition));
    gs.add_object(obj);
    id
}

fn push_damage_trigger(
    gs: &mut GameState,
    target: crate::types::effect::EffectTarget,
    step: crate::types::effect::DamageStep,
) {
    use crate::types::effect::EffectStep;
    let stack_id = gs.alloc_stack_id();
    let obj = StackObject {
        id: stack_id,
        payload: StackPayload::TriggeredAbility {
            source_id: ObjectId(99),
            effect: vec![EffectStep::DealDamage(step)],
            label: "test damage".into(),
        },
        controller: PlayerId(0),
        targets: vec![target],
        x_value: None,
    };
    gs.stack.push(stack_id);
    gs.stack_objects.insert(stack_id, obj);
}

#[test]
fn lifelink_damage_to_creature_gains_life_for_controller() {
    use crate::types::effect::{DamageStep, EffectTarget};
    let mut gs = make_state();
    let creature_id = make_creature_on_battlefield(&mut gs, PlayerId(1), 2, 4);
    let before_life = gs.get_player(PlayerId(0)).unwrap().life;
    push_damage_trigger(
        &mut gs,
        EffectTarget::Object { id: creature_id },
        DamageStep { amount: 3, lifelink: true, ..Default::default() },
    );
    let gs = resolve_top(gs);
    assert_eq!(gs.get_player(PlayerId(0)).unwrap().life, before_life + 3);
    assert_eq!(gs.battlefield[&creature_id].damage_marked, 3);
}

#[test]
fn lifelink_damage_to_player_gains_life_for_controller() {
    use crate::types::effect::{DamageStep, EffectTarget};
    let mut gs = make_state();
    let before_controller_life = gs.get_player(PlayerId(0)).unwrap().life;
    let before_target_life = gs.get_player(PlayerId(1)).unwrap().life;
    push_damage_trigger(
        &mut gs,
        EffectTarget::Player { id: PlayerId(1) },
        DamageStep { amount: 3, lifelink: true, ..Default::default() },
    );
    let gs = resolve_top(gs);
    assert_eq!(gs.get_player(PlayerId(0)).unwrap().life, before_controller_life + 3);
    assert_eq!(gs.get_player(PlayerId(1)).unwrap().life, before_target_life - 3);
}

#[test]
fn lifelink_zero_damage_gains_no_life() {
    use crate::types::effect::{DamageStep, EffectTarget};
    let mut gs = make_state();
    let before_life = gs.get_player(PlayerId(0)).unwrap().life;
    push_damage_trigger(
        &mut gs,
        EffectTarget::Player { id: PlayerId(1) },
        DamageStep { amount: 0, lifelink: true, ..Default::default() },
    );
    let gs = resolve_top(gs);
    assert_eq!(gs.get_player(PlayerId(0)).unwrap().life, before_life);
}

#[test]
fn deathtouch_nonzero_sets_damaged_by_deathtouch_and_sba_destroys() {
    use crate::types::effect::{DamageStep, EffectTarget};
    let mut gs = make_state();
    // 1/1 creature: 1 deathtouch damage → SBA destroys it.
    let creature_id = make_creature_on_battlefield(&mut gs, PlayerId(1), 1, 1);
    push_damage_trigger(
        &mut gs,
        EffectTarget::Object { id: creature_id },
        DamageStep { amount: 1, deathtouch: true, ..Default::default() },
    );
    let gs = resolve_top(gs);
    // SBA ran inside resolve_top — creature removed from battlefield.
    assert!(!gs.battlefield.contains_key(&creature_id));
}

#[test]
fn deathtouch_zero_damage_does_not_set_flag() {
    use crate::types::effect::{DamageStep, EffectTarget};
    let mut gs = make_state();
    let creature_id = make_creature_on_battlefield(&mut gs, PlayerId(1), 2, 2);
    push_damage_trigger(
        &mut gs,
        EffectTarget::Object { id: creature_id },
        DamageStep { amount: 0, deathtouch: true, ..Default::default() },
    );
    let gs = resolve_top(gs);
    assert!(!gs.battlefield[&creature_id].damaged_by_deathtouch);
    assert!(gs.battlefield.contains_key(&creature_id));
}

#[test]
fn wither_damage_to_creature_places_minus_one_counters() {
    use crate::types::effect::{DamageStep, EffectTarget};
    use crate::types::CounterKind;
    let mut gs = make_state();
    let creature_id = make_creature_on_battlefield(&mut gs, PlayerId(1), 3, 3);
    push_damage_trigger(
        &mut gs,
        EffectTarget::Object { id: creature_id },
        DamageStep { amount: 2, wither: true, ..Default::default() },
    );
    let gs = resolve_top(gs);
    let key = CounterKind::PtModifier { power: -1, toughness: -1 };
    assert_eq!(gs.battlefield[&creature_id].counter_count(&key), 2);
    assert_eq!(gs.battlefield[&creature_id].damage_marked, 0);
}

#[test]
fn wither_damage_to_player_is_regular_life_loss() {
    // CR 702.80b: wither damage to a player is still regular life loss.
    use crate::types::effect::{DamageStep, EffectTarget};
    use crate::types::CounterKind;
    let mut gs = make_state();
    let before_life = gs.get_player(PlayerId(1)).unwrap().life;
    push_damage_trigger(
        &mut gs,
        EffectTarget::Player { id: PlayerId(1) },
        DamageStep { amount: 2, wither: true, ..Default::default() },
    );
    let gs = resolve_top(gs);
    assert_eq!(gs.get_player(PlayerId(1)).unwrap().life, before_life - 2);
    assert_eq!(
        gs.get_player(PlayerId(1)).unwrap().counter_count(&CounterKind::Poison),
        0
    );
}

#[test]
fn infect_damage_to_creature_places_minus_one_counters() {
    use crate::types::effect::{DamageStep, EffectTarget};
    use crate::types::CounterKind;
    let mut gs = make_state();
    let creature_id = make_creature_on_battlefield(&mut gs, PlayerId(1), 3, 3);
    push_damage_trigger(
        &mut gs,
        EffectTarget::Object { id: creature_id },
        DamageStep { amount: 3, infect: true, ..Default::default() },
    );
    let gs = resolve_top(gs);
    let key = CounterKind::PtModifier { power: -1, toughness: -1 };
    assert_eq!(gs.battlefield[&creature_id].counter_count(&key), 3);
    assert_eq!(gs.battlefield[&creature_id].damage_marked, 0);
}

#[test]
fn infect_damage_to_player_gives_poison_counters_not_life_loss() {
    use crate::types::effect::{DamageStep, EffectTarget};
    use crate::types::CounterKind;
    let mut gs = make_state();
    let before_life = gs.get_player(PlayerId(1)).unwrap().life;
    push_damage_trigger(
        &mut gs,
        EffectTarget::Player { id: PlayerId(1) },
        DamageStep { amount: 3, infect: true, ..Default::default() },
    );
    let gs = resolve_top(gs);
    assert_eq!(gs.get_player(PlayerId(1)).unwrap().life, before_life); // no life loss
    assert_eq!(
        gs.get_player(PlayerId(1)).unwrap().counter_count(&CounterKind::Poison),
        3
    );
}

#[test]
fn wither_and_deathtouch_combined_on_creature() {
    // CR 702.80a + 702.2b: wither gives -1/-1 counters; deathtouch flag still set.
    use crate::types::effect::{DamageStep, EffectTarget};
    use crate::types::CounterKind;
    let mut gs = make_state();
    let creature_id = make_creature_on_battlefield(&mut gs, PlayerId(1), 2, 2);
    push_damage_trigger(
        &mut gs,
        EffectTarget::Object { id: creature_id },
        DamageStep { amount: 1, wither: true, deathtouch: true, ..Default::default() },
    );
    let gs = resolve_top(gs);
    let key = CounterKind::PtModifier { power: -1, toughness: -1 };
    assert_eq!(gs.battlefield[&creature_id].counter_count(&key), 1);
    // SBA runs — creature toughness is now 2-1=1, damage_marked=0, but
    // damaged_by_deathtouch would have been set. Since SBA then fires,
    // the creature is removed. Just check it's gone.
    assert!(!gs.battlefield.contains_key(&creature_id));
}

#[test]
fn vanilla_deal_damage_to_creature_unchanged() {
    // Regression: all-false flags behave exactly as before.
    use crate::types::effect::{DamageStep, EffectTarget};
    let mut gs = make_state();
    let creature_id = make_creature_on_battlefield(&mut gs, PlayerId(1), 2, 4);
    push_damage_trigger(
        &mut gs,
        EffectTarget::Object { id: creature_id },
        DamageStep { amount: 3, ..Default::default() },
    );
    let gs = resolve_top(gs);
    assert_eq!(gs.battlefield[&creature_id].damage_marked, 3);
}

#[test]
fn vanilla_deal_damage_to_player_unchanged() {
    // Regression: all-false flags behave exactly as before.
    use crate::types::effect::{DamageStep, EffectTarget};
    let mut gs = make_state();
    let before_life = gs.get_player(PlayerId(1)).unwrap().life;
    push_damage_trigger(
        &mut gs,
        EffectTarget::Player { id: PlayerId(1) },
        DamageStep { amount: 3, ..Default::default() },
    );
    let gs = resolve_top(gs);
    assert_eq!(gs.get_player(PlayerId(1)).unwrap().life, before_life - 3);
}
```

- [ ] **Step 2: Run tests to confirm they fail**

```bash
cargo test 2>&1 | grep -E "^test result|FAILED|error\["
```

Expected: multiple `FAILED` lines for the new tests. If any new test passes unexpectedly, the test may be wrong — investigate before proceeding.

- [ ] **Step 3: Replace the `DealDamage` arm in `execute_effect_steps`**

In `src/engine/stack.rs`, replace the entire `DealDamage` arm (the minimal one from Task 1) with:

```rust
// CR 702.15b, 702.2b, 702.80a/b, 702.90b/c
EffectStep::DealDamage(s) => {
    let amount = s.amount;
    match targets.first() {
        Some(EffectTarget::Object { id }) => {
            if let Some(perm) = state.battlefield.get_mut(id) {
                if s.wither || s.infect {
                    // CR 702.80a / 702.90b: damage to a creature from a wither/infect
                    // source becomes -1/-1 counters instead of marked damage.
                    perm.add_counters(
                        crate::types::CounterKind::PtModifier { power: -1, toughness: -1 },
                        amount,
                    );
                } else {
                    perm.damage_marked += amount;
                }
                if s.deathtouch && amount > 0 {
                    // CR 702.2b: any nonzero damage from a deathtouch source is lethal.
                    perm.damaged_by_deathtouch = true;
                }
            }
        }
        Some(EffectTarget::Player { id }) => {
            if let Some(player) = state.get_player_mut(*id) {
                if s.infect {
                    // CR 702.90c: infect damage to a player is poison counters, not life loss.
                    player.add_counters(crate::types::CounterKind::Poison, amount);
                } else {
                    // CR 702.80b: wither damage to a player is still regular life loss.
                    player.life -= amount as i32;
                }
            }
        }
        _ => {}
    }
    if s.lifelink && amount > 0 {
        // CR 702.15b: source's controller gains life equal to damage dealt.
        if let Some(player) = state.get_player_mut(controller) {
            player.life += amount as i32;
        }
    }
}
```

- [ ] **Step 4: Run all tests**

```bash
cargo test 2>&1 | grep -E "^test result|FAILED|error\["
```

Expected: `test result: ok.` — all tests pass including the new ones.

- [ ] **Step 5: Commit**

```bash
git add src/engine/stack.rs
git commit -m "$(cat <<'EOF'
feat: implement keyword-aware DealDamage resolution (lifelink, deathtouch, wither, infect)

Branches on DamageStep flags in execute_effect_steps:
- Wither/Infect on creatures → -1/-1 counters (CR 702.80a, 702.90b)
- Infect on players → poison counters (CR 702.90c)
- Wither on players → regular life loss (CR 702.80b)
- Deathtouch nonzero → damaged_by_deathtouch flag (CR 702.2b)
- Lifelink → controller gains life (CR 702.15b)

Co-Authored-By: Claude Sonnet 4.6 <noreply@anthropic.com>
EOF
)"
```

---

### Task 3: Add `inject_source_flags` and call it in `activated.rs`

Write and test the helper in isolation, then wire it into the activated ability path.

**Files:**
- Modify: `src/engine/stack.rs` — add `inject_source_flags`, `has_damage_kw`, and unit tests
- Modify: `src/engine/activated.rs` — call `inject_source_flags` when building `ActivatedAbility` payload

**Interfaces:**
- Consumes: `Effect` (= `Vec<EffectStep>`), `OracleSpan`, `StaticAbility`, `DamageStep` from earlier tasks.
- Produces: `pub(crate) fn inject_source_flags(effect: Effect, source_abilities: &[OracleSpan]) -> Effect` — used by activated.rs and Task 4.

- [ ] **Step 1: Write unit tests for `inject_source_flags`**

Add to the `tests` module in `src/engine/stack.rs`:

```rust
// ── inject_source_flags unit tests ──────────────────────────────────────────

#[test]
fn inject_source_flags_sets_lifelink_from_abilities() {
    use crate::engine::stack::inject_source_flags;
    use crate::types::ability::{Ability, StaticAbility};
    use crate::types::effect::{DamageStep, EffectStep};
    use crate::types::OracleSpan;

    let abilities = vec![OracleSpan::Parsed(Ability::Static(StaticAbility::Lifelink))];
    let effect = vec![EffectStep::DealDamage(DamageStep { amount: 2, ..Default::default() })];
    let result = inject_source_flags(effect, &abilities);
    match &result[0] {
        EffectStep::DealDamage(s) => {
            assert!(s.lifelink);
            assert!(!s.deathtouch);
            assert!(!s.wither);
            assert!(!s.infect);
            assert_eq!(s.amount, 2);
        }
        other => panic!("expected DealDamage, got {other:?}"),
    }
}

#[test]
fn inject_source_flags_sets_wither_and_infect() {
    use crate::engine::stack::inject_source_flags;
    use crate::types::ability::{Ability, StaticAbility};
    use crate::types::effect::{DamageStep, EffectStep};
    use crate::types::OracleSpan;

    let abilities = vec![
        OracleSpan::Parsed(Ability::Static(StaticAbility::Wither)),
        OracleSpan::Parsed(Ability::Static(StaticAbility::Infect)),
    ];
    let effect = vec![EffectStep::DealDamage(DamageStep { amount: 1, ..Default::default() })];
    let result = inject_source_flags(effect, &abilities);
    match &result[0] {
        EffectStep::DealDamage(s) => {
            assert!(s.wither);
            assert!(s.infect);
        }
        other => panic!("expected DealDamage, got {other:?}"),
    }
}

#[test]
fn inject_source_flags_empty_abilities_leaves_flags_false() {
    use crate::engine::stack::inject_source_flags;
    use crate::types::effect::{DamageStep, EffectStep};

    let effect = vec![EffectStep::DealDamage(DamageStep { amount: 5, ..Default::default() })];
    let result = inject_source_flags(effect, &[]);
    match &result[0] {
        EffectStep::DealDamage(s) => {
            assert!(!s.lifelink && !s.deathtouch && !s.wither && !s.infect);
            assert_eq!(s.amount, 5);
        }
        other => panic!("expected DealDamage, got {other:?}"),
    }
}

#[test]
fn inject_source_flags_non_deal_damage_step_passes_through() {
    use crate::engine::stack::inject_source_flags;
    use crate::types::effect::EffectStep;

    let effect = vec![EffectStep::DrawCard(1)];
    let result = inject_source_flags(effect, &[]);
    assert!(matches!(result[0], EffectStep::DrawCard(1)));
}
```

- [ ] **Step 2: Run tests to verify they fail** (function doesn't exist yet)

```bash
cargo test inject_source_flags 2>&1 | grep -E "^test result|FAILED|error\["
```

Expected: compilation error (`inject_source_flags` not found).

- [ ] **Step 3: Add `inject_source_flags` and `has_damage_kw` to `src/engine/stack.rs`**

Add these functions near the top of the `stack.rs` file (before `execute_effect_steps`):

```rust
/// Reads keyword flags from `source_abilities` and injects them into any `DealDamage`
/// steps in `effect`. Called at stack-push time so flags are snapshotted from the
/// source's current state (CR 702.2e — last-known information at activation time).
pub(crate) fn inject_source_flags(
    effect: crate::types::effect::Effect,
    source_abilities: &[crate::types::OracleSpan],
) -> crate::types::effect::Effect {
    use crate::types::ability::{Ability, StaticAbility};
    use crate::types::effect::{DamageStep, EffectStep};
    use crate::types::OracleSpan;

    effect
        .into_iter()
        .map(|step| match step {
            EffectStep::DealDamage(s) => EffectStep::DealDamage(DamageStep {
                lifelink:   has_damage_kw(source_abilities, &StaticAbility::Lifelink),
                deathtouch: has_damage_kw(source_abilities, &StaticAbility::Deathtouch),
                wither:     has_damage_kw(source_abilities, &StaticAbility::Wither),
                infect:     has_damage_kw(source_abilities, &StaticAbility::Infect),
                ..s
            }),
            other => other,
        })
        .collect()
}

fn has_damage_kw(
    abilities: &[crate::types::OracleSpan],
    kw: &crate::types::ability::StaticAbility,
) -> bool {
    use crate::types::ability::Ability;
    use crate::types::OracleSpan;
    abilities.iter().any(|span| {
        matches!(span, OracleSpan::Parsed(Ability::Static(k)) if k == kw)
    })
}
```

- [ ] **Step 4: Run tests for the helper**

```bash
cargo test inject_source_flags 2>&1 | grep -E "^test result|FAILED|error\["
```

Expected: all four `inject_source_flags` tests pass.

- [ ] **Step 5: Wire `inject_source_flags` into `activated.rs`**

In `src/engine/activated.rs`, find the block that constructs the `StackObject` for non-mana activated abilities (around line 184). The `effect` field currently is `ability.effect.clone()`. Change it to inject flags:

```rust
// Before (around line 184-197):
let stack_obj = crate::types::StackObject {
    id: stack_id,
    payload: crate::types::StackPayload::ActivatedAbility {
        source_id: object_id,
        effect: ability.effect.clone(),
        label,
    },
    controller: activating_player,
    targets: declared_targets,
    x_value,
};

// After:
let source_abilities: Vec<crate::types::OracleSpan> = state
    .battlefield
    .get(&object_id)
    .map(|p| p.definition.abilities.clone())
    .unwrap_or_default();
let stack_obj = crate::types::StackObject {
    id: stack_id,
    payload: crate::types::StackPayload::ActivatedAbility {
        source_id: object_id,
        effect: crate::engine::stack::inject_source_flags(ability.effect.clone(), &source_abilities),
        label,
    },
    controller: activating_player,
    targets: declared_targets,
    x_value,
};
```

- [ ] **Step 6: Run all tests**

```bash
cargo test 2>&1 | grep -E "^test result|FAILED|error\["
```

Expected: `test result: ok.`

- [ ] **Step 7: Commit**

```bash
git add src/engine/stack.rs src/engine/activated.rs
git commit -m "$(cat <<'EOF'
feat: add inject_source_flags and wire into activated ability stack-push

Snaps keyword flags from the source permanent's abilities at activation time
so DealDamage steps carry correct Lifelink/Deathtouch/Wither/Infect flags
when the activated ability resolves. CR 702.2e — last-known at activation.

Co-Authored-By: Claude Sonnet 4.6 <noreply@anthropic.com>
EOF
)"
```

---

### Task 4: Inject flags in triggered abilities and the spell resolution branch

Complete the injection coverage: triggered ability collectors and the spell path in `resolve_top`.

**Files:**
- Modify: `src/engine/triggered.rs` — `collect_etb_triggers`
- Modify: `src/engine/stack.rs` — `resolve_top` spell branch

**Interfaces:**
- Consumes: `inject_source_flags` from Task 3.
- Produces: complete injection coverage — all paths through `execute_effect_steps` that carry `DealDamage` steps have had their flags set.

- [ ] **Step 1: Inject in `collect_etb_triggers`**

In `src/engine/triggered.rs`, `collect_etb_triggers` currently maps `(controller, t.effect.clone(), label)` entries into `StackObject`s. Change the `effect` field to inject flags from the entering permanent's abilities:

```rust
// In the .map() closure that builds StackObject (around line 36-50):

// Before:
entries
    .into_iter()
    .map(|(controller, effect, label)| {
        let id = state.alloc_stack_id();
        StackObject {
            id,
            payload: StackPayload::TriggeredAbility {
                source_id: entering_id,
                effect,
                label,
            },
            controller,
            targets: vec![],
            x_value: None,
        }
    })
    .collect()

// After:
let source_abilities: Vec<crate::types::OracleSpan> = state
    .objects
    .get(&entering_id)
    .map(|o| o.definition.abilities.clone())
    .unwrap_or_default();
entries
    .into_iter()
    .map(|(controller, effect, label)| {
        let id = state.alloc_stack_id();
        StackObject {
            id,
            payload: StackPayload::TriggeredAbility {
                source_id: entering_id,
                effect: crate::engine::stack::inject_source_flags(effect, &source_abilities),
                label,
            },
            controller,
            targets: vec![],
            x_value: None,
        }
    })
    .collect()
```

Note: the other `collect_*_triggers` functions (`collect_cast_triggers`, `collect_block_triggers`, `collect_attack_triggers`, `collect_evolve_triggers`, `collect_ward_triggers`) construct their effects programmatically using `BoostPermanentPT`, `AddCounter`, and `Payment` steps — none produce `DealDamage`. Injection there is a no-op and is omitted for clarity. If future work adds `DealDamage` steps to those paths, inject at that time.

- [ ] **Step 2: Inject in the spell resolution branch of `resolve_top`**

In `src/engine/stack.rs`, find the spell branch inside `resolve_top` where steps are read from the card definition and passed to `execute_effect_steps` (around line 266-300). After extracting `steps`, inject source flags from the spell card's own abilities:

```rust
// After this block that extracts steps from the card definition:
let steps: Vec<EffectStep> = state
    .objects
    .get(&card_id)
    .map(|obj| { ... })
    .unwrap_or_default();

// Add:
let spell_abilities: Vec<crate::types::OracleSpan> = state
    .objects
    .get(&card_id)
    .map(|o| o.definition.abilities.clone())
    .unwrap_or_default();
let steps = inject_source_flags(steps, &spell_abilities);
```

Note: instants and sorceries do not carry Lifelink, Deathtouch, Wither, or Infect as static abilities in their oracle text, so this injection is always a no-op in practice. It is included for correctness and uniformity.

- [ ] **Step 3: Run all tests**

```bash
cargo test 2>&1 | grep -E "^test result|FAILED|error\["
```

Expected: `test result: ok.`

- [ ] **Step 4: Run clippy and fix any warnings**

```bash
cargo clippy --all-targets 2>&1 | grep -E "^error|^warning"
```

Fix any warnings that appear. Common ones: unused imports after refactoring, redundant clones. Run `cargo clippy --fix --all-targets` as a first pass.

- [ ] **Step 5: Final test run**

```bash
cargo test 2>&1 | grep -E "^test result|FAILED|error\["
```

Expected: `test result: ok.`

- [ ] **Step 6: Commit**

```bash
git add src/engine/triggered.rs src/engine/stack.rs
git commit -m "$(cat <<'EOF'
feat: inject source keyword flags into triggered ability and spell DealDamage steps

collect_etb_triggers now snapshots flags from the entering permanent's abilities.
resolve_top injects into spell steps at resolution time (no-op for standard
instants/sorceries; correct by construction for any future keyword-bearing spells).

Co-Authored-By: Claude Sonnet 4.6 <noreply@anthropic.com>
EOF
)"
```
