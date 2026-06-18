# Counter-Unblocked Keywords Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Implement Wither, Infect, Toxic N, Evolve, and Training — five CR 702 keywords whose implementation was blocked on the counter system, which is now in place.

**Architecture:** Types first (Task 1), then three independent tasks (Tasks 2–4) that can run in parallel: parser promotion, combat damage routing, and triggered abilities. Each task is TDD: write failing tests, implement, confirm passing, commit.

**Tech Stack:** Rust, `cargo test`, `cargo clippy --all-targets`

---

## File Map

| File | Change |
|------|--------|
| `src/types/ability.rs` | Add 5 `StaticAbility` variants + display names |
| `src/types/permanent.rs` | Add `toxic_n()` helper |
| `src/parser/oracle.rs` | Promote 5 keywords from `ParsedUnimplemented` |
| `src/engine/combat.rs` | Wither/Infect/Toxic routing in `deal_combat_damage` |
| `src/engine/triggered.rs` | Training branch in `collect_attack_triggers`; new `collect_evolve_triggers` fn |
| `src/engine/casting.rs` | Call `collect_evolve_triggers` after `collect_etb_triggers` |
| `src/engine/stack.rs` | Call `collect_evolve_triggers` after `collect_etb_triggers` |

---

## Task 1: Types Foundation

**Files:**
- Modify: `src/types/ability.rs`
- Modify: `src/types/permanent.rs`

This task must complete before Tasks 2, 3, and 4 begin. It adds the five new `StaticAbility` variants and the `toxic_n()` helper used throughout.

- [ ] **Step 1: Write failing tests in `src/types/ability.rs`**

Add to the existing `#[cfg(test)]` block at the bottom of the file:

```rust
#[test]
fn display_name_counter_keywords() {
    assert_eq!(StaticAbility::Wither.display_name(), "Wither");
    assert_eq!(StaticAbility::Infect.display_name(), "Infect");
    assert_eq!(StaticAbility::ToxicN(2).display_name(), "Toxic 2");
    assert_eq!(StaticAbility::Evolve.display_name(), "Evolve");
    assert_eq!(StaticAbility::Training.display_name(), "Training");
}
```

- [ ] **Step 2: Confirm the tests fail**

```bash
cargo test 2>&1 | grep -E "^test result|FAILED|error\["
```

Expected: compile error — `StaticAbility::Wither` does not exist.

- [ ] **Step 3: Add the five `StaticAbility` variants**

In `src/types/ability.rs`, extend the `StaticAbility` enum. Add after the existing `ProtectionFromColor` line:

```rust
    Wither,                          // CR 702.80
    Infect,                          // CR 702.90
    ToxicN(u32),                     // CR 702.164
    Evolve,                          // CR 702.100
    Training,                        // CR 702.149
```

Then add the five arms to `display_name()` (inside the `match self` block):

```rust
            Self::Wither => "Wither".to_string(),
            Self::Infect => "Infect".to_string(),
            Self::ToxicN(n) => format!("Toxic {n}"),
            Self::Evolve => "Evolve".to_string(),
            Self::Training => "Training".to_string(),
```

- [ ] **Step 4: Write failing test for `toxic_n()` in `src/types/permanent.rs`**

Add to the existing `#[cfg(test)]` block at the bottom of `src/types/permanent.rs`:

```rust
#[test]
fn toxic_n_returns_some_for_toxic_creature() {
    use crate::types::{Ability, OracleSpan, ability::StaticAbility};
    let mut def = test_db().get("Grizzly Bears").unwrap().clone();
    def.abilities = vec![OracleSpan::Parsed(Ability::Static(StaticAbility::ToxicN(3)))];
    let perm = PermanentState::new(&def);
    assert_eq!(perm.toxic_n(), Some(3));
}

#[test]
fn toxic_n_returns_none_for_vanilla_creature() {
    let perm = grizzly_bears_perm();
    assert_eq!(perm.toxic_n(), None);
}
```

- [ ] **Step 5: Confirm test fails**

```bash
cargo test 2>&1 | grep -E "^test result|FAILED|error\["
```

Expected: compile error — `PermanentState` has no method `toxic_n`.

- [ ] **Step 6: Add `toxic_n()` to `PermanentState` in `src/types/permanent.rs`**

Add after the existing `bushido_n()` method (around line 61–69):

```rust
    pub fn toxic_n(&self) -> Option<u32> {
        self.definition.abilities.iter().find_map(|span| {
            if let OracleSpan::Parsed(Ability::Static(StaticAbility::ToxicN(n))) = span {
                Some(*n)
            } else {
                None
            }
        })
    }
```

- [ ] **Step 7: Run tests and confirm all pass**

```bash
cargo test 2>&1 | grep -E "^test result|FAILED|error\["
```

Expected: `test result: ok.`

- [ ] **Step 8: Commit**

```bash
git add src/types/ability.rs src/types/permanent.rs
git commit -m "feat: add Wither, Infect, ToxicN, Evolve, Training StaticAbility variants and toxic_n() helper"
```

---

## Task 2: Parser Promotion

**Files:**
- Modify: `src/parser/oracle.rs`

Requires Task 1. Promotes five keywords from `ParsedUnimplemented` to fully parsed `StaticAbility` values.

- [ ] **Step 1: Write failing parser tests**

Add to the `#[cfg(test)]` block in `src/parser/oracle.rs` (after the existing `parse_bushido_n_keyword` test):

```rust
#[test]
fn parse_wither_keyword() {
    let spans = parse_perm("Wither", "");
    assert_eq!(spans, vec![parsed(StaticAbility::Wither)]);
}

#[test]
fn parse_infect_keyword() {
    let spans = parse_perm("Infect", "");
    assert_eq!(spans, vec![parsed(StaticAbility::Infect)]);
}

#[test]
fn parse_evolve_keyword() {
    let spans = parse_perm("Evolve", "");
    assert_eq!(spans, vec![parsed(StaticAbility::Evolve)]);
}

#[test]
fn parse_training_keyword() {
    let spans = parse_perm("Training", "");
    assert_eq!(spans, vec![parsed(StaticAbility::Training)]);
}

#[test]
fn parse_toxic_n_keyword() {
    let spans = parse_perm("Toxic 3", "");
    assert_eq!(
        spans,
        vec![OracleSpan::Parsed(Ability::Static(StaticAbility::ToxicN(3)))]
    );
}
```

- [ ] **Step 2: Run tests to confirm they fail**

```bash
cargo test parser 2>&1 | grep -E "^test result|FAILED|error\["
```

Expected: five `FAILED` — each keyword is currently `ParsedUnimplemented`.

- [ ] **Step 3: Remove the four simple keywords from the `ParsedUnimplemented` match arm**

In `src/parser/oracle.rs`, find the large `match s { ... => ParsedUnimplemented }` arm in `parse_keyword`. Remove these four lines (search for them individually; surrounding context may vary):

```
        "wither" |
```
```
        "infect" |
```
```
        "evolve" |
```
```
        "training" |
```

- [ ] **Step 4: Add the four simple keywords to the `parse_keyword` simple-keyword dispatch**

In `src/parser/oracle.rs`, find the section near other simple keyword promotions (e.g. the `"fear"` or `"intimidate"` match arms that return `Parsed(Ability::Static(...))`). Add:

```rust
    "wither" => return OracleSpan::Parsed(Ability::Static(StaticAbility::Wither)),
    "infect" => return OracleSpan::Parsed(Ability::Static(StaticAbility::Infect)),
    "evolve" => return OracleSpan::Parsed(Ability::Static(StaticAbility::Evolve)),
    "training" => return OracleSpan::Parsed(Ability::Static(StaticAbility::Training)),
```

**Placement note:** `parse_keyword` applies transformations in order. The simple keywords must appear **before** the fallback that returns `ParsedUnimplemented`. Look for the pattern used by `"fear"` and `"intimidate"` — insert nearby.

- [ ] **Step 5: Remove `"toxic "` from the parameterized `ParsedUnimplemented` list and add a `ToxicN` parse branch**

Find `s.starts_with("toxic ")` in the `is_parameterized_parsedunimplemented` helper (or equivalent inline check) and remove it.

Then add a `ToxicN` parse branch immediately after the `BushidoN` parse block (around line 451–455):

```rust
    // CR 702.164 Toxic N
    if let Some(rest) = s.strip_prefix("toxic ")
        && let Ok(n) = rest.trim().parse::<u32>()
    {
        return OracleSpan::Parsed(Ability::Static(StaticAbility::ToxicN(n)));
    }
```

- [ ] **Step 6: Run parser tests and confirm all pass**

```bash
cargo test parser 2>&1 | grep -E "^test result|FAILED|error\["
```

Expected: `test result: ok.`

- [ ] **Step 7: Run full test suite**

```bash
cargo test 2>&1 | grep -E "^test result|FAILED|error\["
```

Expected: `test result: ok.`

- [ ] **Step 8: Commit**

```bash
git add src/parser/oracle.rs
git commit -m "feat: promote Wither, Infect, ToxicN, Evolve, Training from ParsedUnimplemented"
```

---

## Task 3: Wither / Infect / Toxic in `deal_combat_damage`

**Files:**
- Modify: `src/engine/combat.rs`

Requires Task 1. Modifies the combat damage function to route wither/infect damage as -1/-1 counters and infect/toxic damage as poison counters.

### Rules recap
- CR 702.80a Wither: damage dealt to creatures → -1/-1 counters, not marked damage.
- CR 702.90a Infect: damage to creatures → -1/-1 counters; damage to players → poison counters, not life loss.
- CR 702.164a Toxic N: additionally gives N poison counters when dealing combat damage to a player.
- Lifelink (CR 702.15a) still counts the full damage dealt regardless of routing form.
- Deathtouch is unchanged.

- [ ] **Step 1: Write failing tests in `src/engine/combat.rs`**

Add to the `#[cfg(test)]` block at the bottom of `src/engine/combat.rs`:

```rust
#[test]
fn wither_deals_minus_counters_to_blocker_not_marked_damage() {
    // CR 702.80a: Wither routes creature damage as -1/-1 counters.
    use crate::types::CounterKind;
    let mut gs = make_combat_state();
    let attacker_id = keyword_creature(&mut gs, PlayerId(0), 2, 2, vec![StaticAbility::Wither]);
    let blocker_id = keyword_creature(&mut gs, PlayerId(1), 3, 3, vec![]);
    gs.combat.attackers = vec![attacker_id];
    gs.combat.blocking_map = [(attacker_id, vec![blocker_id])].into();

    let gs = deal_combat_damage(gs);

    let perm = &gs.battlefield[&blocker_id];
    assert_eq!(perm.damage_marked, 0, "Wither damage must not be marked damage");
    assert_eq!(
        perm.counter_count(&CounterKind::PtModifier { power: -1, toughness: -1 }),
        2,
        "Wither attacker (power 2) should give 2 × -1/-1 counters to blocker"
    );
}

#[test]
fn wither_unblocked_still_deals_life_damage_to_player() {
    // CR 702.80a: Wither only changes creature damage; player damage is still life loss.
    use crate::types::CounterKind;
    let mut gs = make_combat_state();
    let attacker_id = keyword_creature(&mut gs, PlayerId(0), 2, 2, vec![StaticAbility::Wither]);
    gs.combat.attackers = vec![attacker_id];
    gs.combat.blocking_map = [(attacker_id, vec![])].into();

    let gs = deal_combat_damage(gs);

    let defender = gs.players.iter().find(|p| p.id == PlayerId(1)).unwrap();
    assert_eq!(defender.life, 18, "Wither unblocked attacker deals normal life damage to player");
    assert_eq!(defender.counter_count(&CounterKind::Poison), 0);
}

#[test]
fn infect_deals_minus_counters_to_blocker_and_poison_to_player() {
    // CR 702.90a: Infect → -1/-1 to creatures, poison to players.
    use crate::types::CounterKind;
    let mut gs = make_combat_state();
    let attacker_id = keyword_creature(&mut gs, PlayerId(0), 3, 3, vec![StaticAbility::Infect]);
    gs.combat.attackers = vec![attacker_id];
    gs.combat.blocking_map = [(attacker_id, vec![])].into(); // unblocked → hits player

    let gs = deal_combat_damage(gs);

    let defender = gs.players.iter().find(|p| p.id == PlayerId(1)).unwrap();
    assert_eq!(defender.life, 20, "Infect does not reduce player life");
    assert_eq!(
        defender.counter_count(&CounterKind::Poison),
        3,
        "Infect gives poison counters equal to damage"
    );
}

#[test]
fn infect_blocked_deals_minus_counters_not_marked_damage() {
    use crate::types::CounterKind;
    let mut gs = make_combat_state();
    let attacker_id = keyword_creature(&mut gs, PlayerId(0), 2, 2, vec![StaticAbility::Infect]);
    let blocker_id = keyword_creature(&mut gs, PlayerId(1), 3, 3, vec![]);
    gs.combat.attackers = vec![attacker_id];
    gs.combat.blocking_map = [(attacker_id, vec![blocker_id])].into();

    let gs = deal_combat_damage(gs);

    let perm = &gs.battlefield[&blocker_id];
    assert_eq!(perm.damage_marked, 0);
    assert_eq!(
        perm.counter_count(&CounterKind::PtModifier { power: -1, toughness: -1 }),
        2
    );
}

#[test]
fn toxic_adds_poison_counters_in_addition_to_life_damage() {
    // CR 702.164a: Toxic N gives N additional poison counters when dealing combat damage to a player.
    use crate::types::CounterKind;
    let mut gs = make_combat_state();
    let attacker_id = keyword_creature(&mut gs, PlayerId(0), 2, 2, vec![StaticAbility::ToxicN(2)]);
    gs.combat.attackers = vec![attacker_id];
    gs.combat.blocking_map = [(attacker_id, vec![])].into();

    let gs = deal_combat_damage(gs);

    let defender = gs.players.iter().find(|p| p.id == PlayerId(1)).unwrap();
    assert_eq!(defender.life, 18, "Toxic does not suppress life damage");
    assert_eq!(
        defender.counter_count(&CounterKind::Poison),
        2,
        "Toxic 2 gives 2 poison counters"
    );
}

#[test]
fn infect_and_toxic_together_stack_poison() {
    // A creature with both Infect and Toxic 2 that deals 3 combat damage to a player:
    // 3 poison from Infect + 2 poison from Toxic 2 = 5 poison total, 0 life loss.
    use crate::types::CounterKind;
    let mut gs = make_combat_state();
    let attacker_id = keyword_creature(
        &mut gs, PlayerId(0), 3, 3,
        vec![StaticAbility::Infect, StaticAbility::ToxicN(2)],
    );
    gs.combat.attackers = vec![attacker_id];
    gs.combat.blocking_map = [(attacker_id, vec![])].into();

    let gs = deal_combat_damage(gs);

    let defender = gs.players.iter().find(|p| p.id == PlayerId(1)).unwrap();
    assert_eq!(defender.life, 20);
    assert_eq!(defender.counter_count(&CounterKind::Poison), 5);
}

#[test]
fn lifelink_still_triggers_with_wither() {
    // CR 702.15a: Lifelink counts total damage dealt regardless of form.
    let mut gs = make_combat_state();
    let attacker_id = keyword_creature(
        &mut gs, PlayerId(0), 2, 2,
        vec![StaticAbility::Wither, StaticAbility::Lifelink],
    );
    let blocker_id = keyword_creature(&mut gs, PlayerId(1), 3, 3, vec![]);
    gs.combat.attackers = vec![attacker_id];
    gs.combat.blocking_map = [(attacker_id, vec![blocker_id])].into();

    let gs = deal_combat_damage(gs);

    let attacker_controller = gs.players.iter().find(|p| p.id == PlayerId(0)).unwrap();
    assert_eq!(attacker_controller.life, 22, "Lifelink should gain 2 life (2 wither damage dealt)");
}

#[test]
fn wither_blocker_deals_minus_counters_to_attacker() {
    // CR 702.80a: Wither applies to damage from any source with the keyword.
    use crate::types::CounterKind;
    let mut gs = make_combat_state();
    let attacker_id = keyword_creature(&mut gs, PlayerId(0), 3, 3, vec![]);
    let blocker_id = keyword_creature(&mut gs, PlayerId(1), 2, 2, vec![StaticAbility::Wither]);
    gs.combat.attackers = vec![attacker_id];
    gs.combat.blocking_map = [(attacker_id, vec![blocker_id])].into();

    let gs = deal_combat_damage(gs);

    let perm = &gs.battlefield[&attacker_id];
    assert_eq!(perm.damage_marked, 0, "Blocker with Wither should not leave marked damage");
    assert_eq!(
        perm.counter_count(&CounterKind::PtModifier { power: -1, toughness: -1 }),
        2
    );
}
```

- [ ] **Step 2: Run tests to confirm they fail**

```bash
cargo test "combat" 2>&1 | grep -E "^test result|FAILED|error\["
```

Expected: multiple FAILEDs — no Wither/Infect/Toxic routing exists yet.

- [ ] **Step 3: Add new accumulators to `deal_combat_damage`**

In `src/engine/combat.rs`, find the four accumulator declarations (around line 340–343):

```rust
    let mut damage_to_players: HashMap<PlayerId, i32> = HashMap::new();
    let mut damage_to_objects: HashMap<ObjectId, u32> = HashMap::new();
    let mut lifelink_gain: HashMap<PlayerId, i32> = HashMap::new();
    let mut deathtouch_targets: HashSet<ObjectId> = HashSet::new();
```

Add two more immediately after them:

```rust
    // Wither (CR 702.80a) / Infect (CR 702.90a): creature damage as -1/-1 counters.
    let mut wither_to_objects: HashMap<ObjectId, u32> = HashMap::new();
    // Infect (CR 702.90a) / Toxic N (CR 702.164a): player damage as poison counters.
    let mut poison_to_players: HashMap<PlayerId, u32> = HashMap::new();
```

- [ ] **Step 4: Extend attacker keyword extraction to include `has_wither`, `has_infect`, `toxic_n`**

Find the attacker keyword extraction block (around line 350–368) that destructures into `(atk_power, has_trample, has_deathtouch, has_lifelink, atk_controller)`. Extend it:

```rust
        let (atk_power, has_trample, has_deathtouch, has_lifelink, has_wither, has_infect, atk_controller) = {
            let obj = match state.objects.get(&attacker_id) {
                Some(o) => o,
                None => continue,
            };
            let power = state
                .battlefield
                .get(&attacker_id)
                .and_then(|p| p.effective_power())
                .map(|p| p.max(0) as u32)
                .unwrap_or(0);
            (
                power,
                obj.has_keyword(StaticAbility::Trample),
                obj.has_keyword(StaticAbility::Deathtouch),
                obj.has_keyword(StaticAbility::Lifelink),
                obj.has_keyword(StaticAbility::Wither),
                obj.has_keyword(StaticAbility::Infect),
                obj.controller,
            )
        };
        let toxic_n = state.battlefield.get(&attacker_id).and_then(|p| p.toxic_n());
```

Also declare a variable to track which player (if any) this attacker hit, for Toxic:

```rust
        let mut attacked_player: Option<PlayerId> = None;
```

- [ ] **Step 5: Route attacker→blocker creature damage through `wither_to_objects`**

Find the blocker assignment loop (the `for &blocker_id in &blockers` inner loop around line 378–405). Change the line:

```rust
                *damage_to_objects.entry(blocker_id).or_insert(0) += assign;
```

to:

```rust
                if has_wither || has_infect {
                    *wither_to_objects.entry(blocker_id).or_insert(0) += assign;
                } else {
                    *damage_to_objects.entry(blocker_id).or_insert(0) += assign;
                }
```

- [ ] **Step 6: Route attacker player damage and trample remainder**

Find the unblocked attacker branch (around line 373–376):

```rust
        if blockers.is_empty() {
            *damage_to_players.entry(defending_player).or_insert(0) += atk_power as i32;
            total_damage_dealt = atk_power;
        } else {
```

Replace with:

```rust
        if blockers.is_empty() {
            if has_infect {
                *poison_to_players.entry(defending_player).or_insert(0) += atk_power;
            } else {
                *damage_to_players.entry(defending_player).or_insert(0) += atk_power as i32;
            }
            total_damage_dealt = atk_power;
            if atk_power > 0 {
                attacked_player = Some(defending_player);
            }
        } else {
```

Find the trample remainder block (around line 407–419). The current code is:

```rust
            if remaining > 0 {
                if has_trample {
                    *damage_to_players.entry(defending_player).or_insert(0) += remaining as i32;
                    total_damage_dealt += remaining;
                } else if let Some(&last) = blockers.last() {
                    *damage_to_objects.entry(last).or_insert(0) += remaining;
                    if has_deathtouch {
                        deathtouch_targets.insert(last);
                    }
                    total_damage_dealt += remaining;
                }
            }
```

Replace with:

```rust
            if remaining > 0 {
                if has_trample {
                    if has_infect {
                        *poison_to_players.entry(defending_player).or_insert(0) += remaining;
                    } else {
                        *damage_to_players.entry(defending_player).or_insert(0) += remaining as i32;
                    }
                    total_damage_dealt += remaining;
                    attacked_player = Some(defending_player);
                } else if let Some(&last) = blockers.last() {
                    if has_wither || has_infect {
                        *wither_to_objects.entry(last).or_insert(0) += remaining;
                    } else {
                        *damage_to_objects.entry(last).or_insert(0) += remaining;
                    }
                    if has_deathtouch {
                        deathtouch_targets.insert(last);
                    }
                    total_damage_dealt += remaining;
                }
            }
```

- [ ] **Step 7: Add Toxic N poison after player damage is settled**

Add immediately after the `if has_lifelink && total_damage_dealt > 0` block and before the blocker loop:

```rust
        // CR 702.164a: Toxic N — additional poison counters when this deals combat damage to a player.
        if let (Some(n), Some(pid)) = (toxic_n, attacked_player) {
            *poison_to_players.entry(pid).or_insert(0) += n;
        }
```

- [ ] **Step 8: Extend blocker keyword extraction to include `blk_wither` and `blk_infect`**

Find the blocker keyword extraction (around line 431–447):

```rust
            let (blk_power, blk_deathtouch, blk_lifelink, blk_controller) = {
                let obj = match state.objects.get(&blocker_id) {
                    Some(o) => o,
                    None => continue,
                };
                let power = ...;
                (
                    power,
                    obj.has_keyword(StaticAbility::Deathtouch),
                    obj.has_keyword(StaticAbility::Lifelink),
                    obj.controller,
                )
            };
```

Replace with:

```rust
            let (blk_power, blk_wither, blk_infect, blk_deathtouch, blk_lifelink, blk_controller) = {
                let obj = match state.objects.get(&blocker_id) {
                    Some(o) => o,
                    None => continue,
                };
                let power = state
                    .battlefield
                    .get(&blocker_id)
                    .and_then(|p| p.effective_power())
                    .map(|p| p.max(0) as u32)
                    .unwrap_or(0);
                (
                    power,
                    obj.has_keyword(StaticAbility::Wither),
                    obj.has_keyword(StaticAbility::Infect),
                    obj.has_keyword(StaticAbility::Deathtouch),
                    obj.has_keyword(StaticAbility::Lifelink),
                    obj.controller,
                )
            };
```

Then change the blocker→attacker damage routing (around line 449–458):

```rust
            if blk_power > 0 {
                *damage_to_objects.entry(attacker_id).or_insert(0) += blk_power;
                if blk_deathtouch {
                    deathtouch_targets.insert(attacker_id);
                }
                if blk_lifelink {
                    *lifelink_gain.entry(blk_controller).or_insert(0) += blk_power as i32;
                }
            }
```

to:

```rust
            if blk_power > 0 {
                if blk_wither || blk_infect {
                    *wither_to_objects.entry(attacker_id).or_insert(0) += blk_power;
                } else {
                    *damage_to_objects.entry(attacker_id).or_insert(0) += blk_power;
                }
                if blk_deathtouch {
                    deathtouch_targets.insert(attacker_id);
                }
                if blk_lifelink {
                    *lifelink_gain.entry(blk_controller).or_insert(0) += blk_power as i32;
                }
            }
```

- [ ] **Step 9: Apply the new accumulators at the end of the function**

Find the application block (around line 461–483). After the existing four apply-blocks and before `check_and_apply_sbas(state)`, add:

```rust
    // Apply wither/infect counter damage to creatures.
    for (oid, n) in wither_to_objects {
        if let Some(perm) = state.battlefield.get_mut(&oid) {
            perm.add_counters(CounterKind::PtModifier { power: -1, toughness: -1 }, n);
        }
    }
    // Apply infect/toxic poison counters to players.
    for (pid, n) in poison_to_players {
        if let Some(p) = state.get_player_mut(pid) {
            p.add_counters(CounterKind::Poison, n);
        }
    }
```

Also add `CounterKind` to the imports at the top of `deal_combat_damage` (or as a `use` at the top of the function body):

```rust
    use crate::types::CounterKind;
```

- [ ] **Step 10: Run tests and confirm all pass**

```bash
cargo test "combat" 2>&1 | grep -E "^test result|FAILED|error\["
cargo test 2>&1 | grep -E "^test result|FAILED|error\["
```

Both expected: `test result: ok.`

- [ ] **Step 11: Commit**

```bash
git add src/engine/combat.rs
git commit -m "feat: implement Wither, Infect, and Toxic N in deal_combat_damage (CR 702.80, 702.90, 702.164)"
```

---

## Task 4: Training and Evolve Triggered Abilities

**Files:**
- Modify: `src/engine/triggered.rs`
- Modify: `src/engine/casting.rs`
- Modify: `src/engine/stack.rs`

Requires Task 1. Adds Training counter triggers and Evolve counter triggers.

### Part A: Training

- [ ] **Step 1: Write failing Training tests in `src/engine/triggered.rs`**

Add to the `#[cfg(test)]` block at the bottom of `triggered.rs`:

```rust
#[test]
fn training_fires_when_alongside_greater_power_attacker() {
    use crate::engine::triggered::collect_attack_triggers;
    use crate::types::CounterKind;
    use crate::types::stack::StackPayload;
    use crate::types::effect::{EffectStep, EffectTarget};

    let mut gs = two_player_state();

    let training_def = CardDefinition {
        name: "Training Creature".into(),
        mana_cost: None,
        type_line: TypeLine {
            supertypes: vec![],
            card_types: vec![CardType::Creature],
            subtypes: vec![],
        },
        oracle_text: String::new(),
        abilities: vec![OracleSpan::Parsed(Ability::Static(StaticAbility::Training))],
        text_annotations: vec![],
        power: Some(1),
        toughness: Some(1),
        colors: vec![],
    };
    let training_id = place_on_battlefield(&mut gs, training_def, PlayerId(0));

    let big_def = CardDefinition {
        name: "Big Creature".into(),
        mana_cost: None,
        type_line: TypeLine {
            supertypes: vec![],
            card_types: vec![CardType::Creature],
            subtypes: vec![],
        },
        oracle_text: String::new(),
        abilities: vec![],
        text_annotations: vec![],
        power: Some(3),
        toughness: Some(3),
        colors: vec![],
    };
    let big_id = place_on_battlefield(&mut gs, big_def, PlayerId(0));

    gs.combat.attackers = vec![training_id, big_id];

    let triggers = collect_attack_triggers(&mut gs);

    let training_triggers: Vec<_> = triggers
        .iter()
        .filter(|t| {
            matches!(&t.payload, StackPayload::TriggeredAbility { source_id, .. } if *source_id == training_id)
        })
        .collect();
    assert_eq!(training_triggers.len(), 1, "Training should fire exactly once");
    let StackPayload::TriggeredAbility { effect, .. } = &training_triggers[0].payload else {
        panic!("expected TriggeredAbility");
    };
    assert_eq!(
        *effect,
        vec![EffectStep::AddCounter {
            kind: CounterKind::PtModifier { power: 1, toughness: 1 },
            count: 1,
        }]
    );
    assert_eq!(
        training_triggers[0].targets,
        vec![EffectTarget::Object { id: training_id }]
    );
}

#[test]
fn training_does_not_fire_when_alone() {
    use crate::engine::triggered::collect_attack_triggers;

    let mut gs = two_player_state();
    let training_def = CardDefinition {
        name: "Training Creature".into(),
        mana_cost: None,
        type_line: TypeLine {
            supertypes: vec![],
            card_types: vec![CardType::Creature],
            subtypes: vec![],
        },
        oracle_text: String::new(),
        abilities: vec![OracleSpan::Parsed(Ability::Static(StaticAbility::Training))],
        text_annotations: vec![],
        power: Some(2),
        toughness: Some(2),
        colors: vec![],
    };
    let training_id = place_on_battlefield(&mut gs, training_def, PlayerId(0));
    gs.combat.attackers = vec![training_id];

    let triggers = collect_attack_triggers(&mut gs);
    assert!(triggers.is_empty(), "Training should not fire when attacking alone");
}

#[test]
fn training_does_not_fire_alongside_equal_or_lesser_power() {
    use crate::engine::triggered::collect_attack_triggers;

    let mut gs = two_player_state();
    let make_def = |name: &str, power: i32| CardDefinition {
        name: name.into(),
        mana_cost: None,
        type_line: TypeLine {
            supertypes: vec![],
            card_types: vec![CardType::Creature],
            subtypes: vec![],
        },
        oracle_text: String::new(),
        abilities: if name == "Training" {
            vec![OracleSpan::Parsed(Ability::Static(StaticAbility::Training))]
        } else {
            vec![]
        },
        text_annotations: vec![],
        power: Some(power),
        toughness: Some(power),
        colors: vec![],
    };
    let training_id = place_on_battlefield(&mut gs, make_def("Training", 2), PlayerId(0));
    let equal_id = place_on_battlefield(&mut gs, make_def("Equal", 2), PlayerId(0));
    gs.combat.attackers = vec![training_id, equal_id];

    let triggers = collect_attack_triggers(&mut gs);
    let training_triggers: Vec<_> = triggers
        .iter()
        .filter(|t| {
            matches!(&t.payload, StackPayload::TriggeredAbility { source_id, .. } if *source_id == training_id)
        })
        .collect();
    assert!(training_triggers.is_empty(), "Training requires strictly greater power");
}
```

- [ ] **Step 2: Confirm tests fail**

```bash
cargo test "triggered" 2>&1 | grep -E "^test result|FAILED|error\["
```

Expected: compile error or FAILEDs — Training variant does not exist in dispatch yet.

- [ ] **Step 3: Add Training branch to `collect_attack_triggers` in `src/engine/triggered.rs`**

Find the end of `collect_attack_triggers` (before the closing `result`), after the BattleCry block. Add:

```rust
    // Training (CR 702.149a): put +1/+1 counter when attacking alongside a creature with greater power.
    let training_attackers: Vec<ObjectId> = attackers
        .iter()
        .filter(|&&id| {
            state
                .battlefield
                .get(&id)
                .map(|p| p.has_keyword(StaticAbility::Training))
                .unwrap_or(false)
        })
        .copied()
        .collect();

    for attacker_id in training_attackers {
        let my_power = state
            .battlefield
            .get(&attacker_id)
            .and_then(|p| p.effective_power())
            .unwrap_or(0);
        let has_greater_power_ally = attackers
            .iter()
            .filter(|&&id| id != attacker_id)
            .any(|&id| {
                state
                    .battlefield
                    .get(&id)
                    .and_then(|p| p.effective_power())
                    .map(|p| p > my_power)
                    .unwrap_or(false)
            });
        if !has_greater_power_ally {
            continue;
        }
        let sid = state.alloc_stack_id();
        use crate::types::counter::CounterKind;
        use crate::types::effect::EffectTarget;
        result.push(StackObject {
            id: sid,
            payload: StackPayload::TriggeredAbility {
                source_id: attacker_id,
                effect: vec![EffectStep::AddCounter {
                    kind: CounterKind::PtModifier { power: 1, toughness: 1 },
                    count: 1,
                }],
                label: "Training".into(),
            },
            controller: attacking_player,
            targets: vec![EffectTarget::Object { id: attacker_id }],
            x_value: None,
        });
    }
```

- [ ] **Step 4: Run Training tests and confirm they pass**

```bash
cargo test "training" 2>&1 | grep -E "^test result|FAILED|error\["
```

Expected: `test result: ok.`

### Part B: Evolve

- [ ] **Step 5: Write failing Evolve tests in `src/engine/triggered.rs`**

Add to the `#[cfg(test)]` block:

```rust
#[test]
fn evolve_fires_when_larger_creature_etbs() {
    use crate::engine::triggered::collect_evolve_triggers;
    use crate::types::CounterKind;
    use crate::types::stack::StackPayload;
    use crate::types::effect::{EffectStep, EffectTarget};

    let mut gs = two_player_state();

    // Evolve creature already on the battlefield (1/1)
    let evolve_def = CardDefinition {
        name: "Evolve Creature".into(),
        mana_cost: None,
        type_line: TypeLine {
            supertypes: vec![],
            card_types: vec![CardType::Creature],
            subtypes: vec![],
        },
        oracle_text: String::new(),
        abilities: vec![OracleSpan::Parsed(Ability::Static(StaticAbility::Evolve))],
        text_annotations: vec![],
        power: Some(1),
        toughness: Some(1),
        colors: vec![],
    };
    let evolve_id = place_on_battlefield(&mut gs, evolve_def, PlayerId(0));

    // A 3/3 enters — greater power AND toughness
    let entering_def = CardDefinition {
        name: "Entering Creature".into(),
        mana_cost: None,
        type_line: TypeLine {
            supertypes: vec![],
            card_types: vec![CardType::Creature],
            subtypes: vec![],
        },
        oracle_text: String::new(),
        abilities: vec![],
        text_annotations: vec![],
        power: Some(3),
        toughness: Some(3),
        colors: vec![],
    };
    let entering_id = place_on_battlefield(&mut gs, entering_def, PlayerId(0));

    let triggers = collect_evolve_triggers(&mut gs, entering_id);

    assert_eq!(triggers.len(), 1);
    let StackPayload::TriggeredAbility { source_id, effect, .. } = &triggers[0].payload else {
        panic!("expected TriggeredAbility");
    };
    assert_eq!(*source_id, evolve_id);
    assert_eq!(
        *effect,
        vec![EffectStep::AddCounter {
            kind: CounterKind::PtModifier { power: 1, toughness: 1 },
            count: 1,
        }]
    );
    assert_eq!(
        triggers[0].targets,
        vec![EffectTarget::Object { id: evolve_id }]
    );
}

#[test]
fn evolve_fires_when_entering_has_greater_power_only() {
    use crate::engine::triggered::collect_evolve_triggers;

    let mut gs = two_player_state();

    let evolve_def = CardDefinition {
        name: "Evolve Creature".into(),
        mana_cost: None,
        type_line: TypeLine { supertypes: vec![], card_types: vec![CardType::Creature], subtypes: vec![] },
        oracle_text: String::new(),
        abilities: vec![OracleSpan::Parsed(Ability::Static(StaticAbility::Evolve))],
        text_annotations: vec![],
        power: Some(2),
        toughness: Some(4),
        colors: vec![],
    };
    place_on_battlefield(&mut gs, evolve_def, PlayerId(0));

    // 3/1: greater power (3 > 2) but lesser toughness (1 < 4) — should still trigger
    let entering_def = CardDefinition {
        name: "Entering".into(),
        mana_cost: None,
        type_line: TypeLine { supertypes: vec![], card_types: vec![CardType::Creature], subtypes: vec![] },
        oracle_text: String::new(),
        abilities: vec![],
        text_annotations: vec![],
        power: Some(3),
        toughness: Some(1),
        colors: vec![],
    };
    let entering_id = place_on_battlefield(&mut gs, entering_def, PlayerId(0));

    let triggers = collect_evolve_triggers(&mut gs, entering_id);
    assert_eq!(triggers.len(), 1, "Greater power alone should trigger Evolve");
}

#[test]
fn evolve_does_not_fire_when_entering_is_smaller() {
    use crate::engine::triggered::collect_evolve_triggers;

    let mut gs = two_player_state();
    let evolve_def = CardDefinition {
        name: "Evolve Creature".into(),
        mana_cost: None,
        type_line: TypeLine { supertypes: vec![], card_types: vec![CardType::Creature], subtypes: vec![] },
        oracle_text: String::new(),
        abilities: vec![OracleSpan::Parsed(Ability::Static(StaticAbility::Evolve))],
        text_annotations: vec![],
        power: Some(3),
        toughness: Some(3),
        colors: vec![],
    };
    place_on_battlefield(&mut gs, evolve_def, PlayerId(0));

    let small_def = CardDefinition {
        name: "Small Creature".into(),
        mana_cost: None,
        type_line: TypeLine { supertypes: vec![], card_types: vec![CardType::Creature], subtypes: vec![] },
        oracle_text: String::new(),
        abilities: vec![],
        text_annotations: vec![],
        power: Some(1),
        toughness: Some(1),
        colors: vec![],
    };
    let entering_id = place_on_battlefield(&mut gs, small_def, PlayerId(0));

    let triggers = collect_evolve_triggers(&mut gs, entering_id);
    assert!(triggers.is_empty(), "Entering creature smaller on both axes should not trigger Evolve");
}

#[test]
fn evolve_does_not_fire_for_opponent_creature_entering() {
    use crate::engine::triggered::collect_evolve_triggers;

    let mut gs = two_player_state();
    let evolve_def = CardDefinition {
        name: "Evolve Creature".into(),
        mana_cost: None,
        type_line: TypeLine { supertypes: vec![], card_types: vec![CardType::Creature], subtypes: vec![] },
        oracle_text: String::new(),
        abilities: vec![OracleSpan::Parsed(Ability::Static(StaticAbility::Evolve))],
        text_annotations: vec![],
        power: Some(1),
        toughness: Some(1),
        colors: vec![],
    };
    place_on_battlefield(&mut gs, evolve_def, PlayerId(0));

    let opponent_def = CardDefinition {
        name: "Opponent Creature".into(),
        mana_cost: None,
        type_line: TypeLine { supertypes: vec![], card_types: vec![CardType::Creature], subtypes: vec![] },
        oracle_text: String::new(),
        abilities: vec![],
        text_annotations: vec![],
        power: Some(5),
        toughness: Some(5),
        colors: vec![],
    };
    // Enters under PlayerId(1) — should not trigger PlayerId(0)'s Evolve
    let entering_id = place_on_battlefield(&mut gs, opponent_def, PlayerId(1));

    let triggers = collect_evolve_triggers(&mut gs, entering_id);
    assert!(triggers.is_empty(), "Opponent's creature entering should not trigger friendly Evolve");
}
```

- [ ] **Step 6: Confirm tests fail**

```bash
cargo test "evolve" 2>&1 | grep -E "^test result|FAILED|error\["
```

Expected: compile error — `collect_evolve_triggers` does not exist.

- [ ] **Step 7: Add `collect_evolve_triggers` to `src/engine/triggered.rs`**

Add the new public function after `collect_ward_triggers` (before the `#[cfg(test)]` block):

```rust
/// CR 702.100b: collect Evolve triggers for battlefield permanents when `entering_id` ETBs.
/// For each friendly creature with Evolve, if the entering creature has greater power or
/// toughness, push a +1/+1 counter trigger onto the stack for that Evolve creature.
pub fn collect_evolve_triggers(state: &mut GameState, entering_id: ObjectId) -> Vec<StackObject> {
    use crate::types::counter::CounterKind;
    use crate::types::effect::EffectTarget;

    // Evolve only cares about creatures entering (CR 702.100b).
    let entering_is_creature = state
        .battlefield
        .get(&entering_id)
        .map(|p| p.is_creature())
        .unwrap_or(false);
    if !entering_is_creature {
        return vec![];
    }

    let Some(entering_obj) = state.objects.get(&entering_id) else {
        return vec![];
    };
    let controller = entering_obj.controller;

    let entering_power = state
        .battlefield
        .get(&entering_id)
        .and_then(|p| p.effective_power());
    let entering_toughness = state
        .battlefield
        .get(&entering_id)
        .and_then(|p| p.effective_toughness());

    let evolve_ids: Vec<ObjectId> = state
        .battlefield
        .keys()
        .filter(|&&id| {
            id != entering_id
                && state
                    .objects
                    .get(&id)
                    .map(|o| o.controller == controller)
                    .unwrap_or(false)
                && state
                    .battlefield
                    .get(&id)
                    .map(|p| p.has_keyword(StaticAbility::Evolve))
                    .unwrap_or(false)
        })
        .copied()
        .collect();

    evolve_ids
        .into_iter()
        .filter_map(|evolve_id| {
            let perm = state.battlefield.get(&evolve_id)?;
            let my_power = perm.effective_power().unwrap_or(0);
            let my_toughness = perm.effective_toughness().unwrap_or(0);
            let qualifies = entering_power.map(|ep| ep > my_power).unwrap_or(false)
                || entering_toughness.map(|et| et > my_toughness).unwrap_or(false);
            if !qualifies {
                return None;
            }
            let sid = state.alloc_stack_id();
            Some(StackObject {
                id: sid,
                payload: StackPayload::TriggeredAbility {
                    source_id: evolve_id,
                    effect: vec![EffectStep::AddCounter {
                        kind: CounterKind::PtModifier { power: 1, toughness: 1 },
                        count: 1,
                    }],
                    label: "Evolve".into(),
                },
                controller,
                targets: vec![EffectTarget::Object { id: evolve_id }],
                x_value: None,
            })
        })
        .collect()
}
```

- [ ] **Step 8: Run Evolve tests and confirm they pass**

```bash
cargo test "evolve" 2>&1 | grep -E "^test result|FAILED|error\["
```

Expected: `test result: ok.`

### Part C: Wire Evolve into ETB call sites

- [ ] **Step 9: Add Evolve call in `src/engine/casting.rs`**

In `src/engine/casting.rs`, find the ETB trigger block (around line 62–68):

```rust
    let triggers = crate::engine::triggered::collect_etb_triggers(&mut state, object_id);
    for trigger in triggers {
        let id = trigger.id;
        state.stack.push(id);
        state.stack_objects.insert(id, trigger);
    }
```

Add immediately after it:

```rust
    let evolve_triggers = crate::engine::triggered::collect_evolve_triggers(&mut state, object_id);
    for trigger in evolve_triggers {
        let id = trigger.id;
        state.stack.push(id);
        state.stack_objects.insert(id, trigger);
    }
```

- [ ] **Step 10: Add Evolve call in `src/engine/stack.rs`**

In `src/engine/stack.rs`, find the ETB trigger block (around line 249–254):

```rust
                let triggers = collect_etb_triggers(&mut state, card_id);
                for trigger in triggers {
                    let id = trigger.id;
                    state.stack.push(id);
                    state.stack_objects.insert(id, trigger);
                }
```

Add `collect_evolve_triggers` to the import at the top of the file (line 4):

```rust
use super::{
    EngineError,
    state_based_actions::check_and_apply_sbas,
    triggered::{collect_etb_triggers, collect_evolve_triggers},
    turn::{advance_step, draw_card},
};
```

Then add the evolve call immediately after the ETB block:

```rust
                let evolve_triggers = collect_evolve_triggers(&mut state, card_id);
                for trigger in evolve_triggers {
                    let id = trigger.id;
                    state.stack.push(id);
                    state.stack_objects.insert(id, trigger);
                }
```

- [ ] **Step 11: Run full test suite**

```bash
cargo test 2>&1 | grep -E "^test result|FAILED|error\["
```

Expected: `test result: ok.`

- [ ] **Step 12: Commit**

```bash
git add src/engine/triggered.rs src/engine/casting.rs src/engine/stack.rs
git commit -m "feat: implement Training and Evolve triggered abilities (CR 702.149, 702.100)"
```

---

## Final: Clippy Clean-up

After all tasks are merged/complete:

- [ ] **Step 1: Run clippy auto-fix**

```bash
cargo clippy --fix --all-targets
```

- [ ] **Step 2: Run clippy and confirm clean**

```bash
cargo clippy --all-targets 2>&1 | grep -E "error|warning"
```

Expected: no errors or warnings.

- [ ] **Step 3: Run full test suite one final time**

```bash
cargo test 2>&1 | grep -E "^test result|FAILED|error\["
```

Expected: `test result: ok.`

- [ ] **Step 4: Commit clippy fixes (if any)**

```bash
git add -u
git commit -m "chore: clippy clean-up after counter keyword implementation"
```

---

## Parallelisation Note

Tasks 2, 3, and 4 are fully independent (each touches a different set of files) and can be executed by parallel subagents once Task 1 is committed. Task 4C (call sites) must happen after Task 4B (Evolve function exists).
