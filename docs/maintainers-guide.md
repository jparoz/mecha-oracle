# Mecha-Oracle Maintainer's Guide

This document is for future-me. It assumes you are fluent in Rust, know MTG well as a player, and are comfortable looking up CR rule references. It does **not** explain the rules of the game, only how they are encoded in this engine.

The Comprehensive Rules are at `docs/CR.txt` and are the final word on any rules question. All CR references in the code use format `(NNN.MMx)`.

---

## Table of Contents

1. [Project Purpose and Scope](#1-project-purpose-and-scope)
2. [Repository Layout](#2-repository-layout)
3. [Architecture Overview](#3-architecture-overview)
4. [The Three-Layer Card Model](#4-the-three-layer-card-model)
5. [The Mana System](#5-the-mana-system)
6. [The Stack and Resolution Model](#6-the-stack-and-resolution-model)
7. [Turn Structure](#7-turn-structure)
8. [Combat System](#8-combat-system)
9. [Triggered Abilities — Current Split and Known Issue](#9-triggered-abilities--current-split-and-known-issue)
10. [Module Reference](#10-module-reference)
    - [src/types/](#srctype)
    - [src/engine/](#srcengine)
    - [src/parser/](#srcparser)
    - [src/cards/](#srccards)
    - [src/serve.rs](#srcserers)
    - [src/main.rs](#srcmainrs)
11. [Key Data Flows](#11-key-data-flows)
12. [Maintainer Recipes](#12-maintainer-recipes)
13. [Known Limitations and Bugs](#13-known-limitations-and-bugs)
14. [Testing Approach](#14-testing-approach)

---

## 1. Project Purpose and Scope

Mecha-Oracle is a Magic: The Gathering rules-enforcement engine written in Rust. It is not a game client, AI agent, or complete simulator — it is the rules layer: it takes game actions as function calls and returns a new `GameState`, or an `EngineError` explaining why the action was illegal.

**What it does:**
- Maintains a complete `GameState` (all zones, all objects, all player state)
- Enforces legality of player actions (casting, land drops, activations, priority)
- Runs the stack — push, pass, resolve, trigger
- Runs state-based actions after every state change
- Tracks combat, mana pools, delayed triggers, pending payments
- Serves a basic browser UI via Axum for human play

**What it does not do:**
- AI or deck-building
- Complete card coverage (oracle parser handles a subset of text patterns)
- Multiplayer (exactly two players, hardcoded)
- Many zone-change mechanics (graveyard activations, Morph, Mutate, etc.)

---

## 2. Repository Layout

```
mecha-oracle/
├── src/
│   ├── lib.rs               — four pub mod declarations, nothing else
│   ├── main.rs              — CLI: demo | serve | update-cards
│   ├── serve.rs             — Axum HTTP server, all browser API endpoints
│   ├── types/               — pure data; no game logic
│   │   ├── ability.rs       — ALL ability, trigger, effect, and annotation types
│   │   ├── card.rs          — CardDefinition (static oracle data)
│   │   ├── card_object.rs   — CardObject (per-game-instance card)
│   │   ├── counter.rs       — CounterKind
│   │   ├── effect.rs        — EffectStep, Effect, DamageStep, EffectTarget
│   │   ├── game_state.rs    — GameState (master struct), CombatState, DelayedTrigger
│   │   ├── ids.rs           — ObjectId(u64), PlayerId(u8)
│   │   ├── mana.rs          — ManaPip, ManaCost, ManaPool, PaymentPlan
│   │   ├── mod.rs           — re-exports and a few type aliases
│   │   ├── permanent.rs     — PermanentState (battlefield-specific card state)
│   │   ├── player.rs        — Player
│   │   ├── stack.rs         — StackId, StackPayload, StackObject
│   │   ├── step.rs          — Step (12 variants), Phase (5 variants)
│   │   └── zone.rs          — Zone, ZoneOwner
│   ├── engine/              — all game logic; takes GameState, returns GameState
│   │   ├── mod.rs           — EngineError, continuous_pt_bonus, has_protection_from
│   │   ├── activated.rs     — activate_ability
│   │   ├── casting.rs       — cast_spell, play_land
│   │   ├── combat.rs        — declare_attackers, declare_blockers, deal_combat_damage
│   │   ├── costs.rs         — pay_cost_components, can_pay_cost_components, PendingPayment helpers
│   │   ├── cycling.rs       — cycle_card
│   │   ├── equip.rs         — activate_equip
│   │   ├── mana.rs          — tap_land_for_mana, reset_mana, greedy_payment_plan
│   │   ├── stack.rs         — pass_priority, resolve_top, execute_effect_steps
│   │   ├── state_based_actions.rs — check_and_apply_sbas, find_sbas, apply_sbas
│   │   ├── targeting.rs     — is_legal_target, legal_targets
│   │   ├── triggered.rs     — all trigger-collector functions
│   │   └── turn.rs          — advance_step, apply_step_start, draw_card, cleanup_step
│   ├── parser/
│   │   ├── mod.rs           — pub use oracle::{parse_instant_or_sorcery, parse_permanent}
│   │   └── oracle.rs        — oracle text → Vec<RulesText> + Vec<TextAnnotation>
│   └── cards/
│       ├── mod.rs           — CardDatabase (load + query)
│       ├── scryfall.rs      — Scryfall JSON → CardDefinition
│       └── downloader.rs    — fetch from Scryfall bulk-data API
├── tests/
│   ├── scripted_game.rs     — integration tests: full game sequences
│   ├── targeting.rs         — targeting legality tests
│   └── cr_examples.rs       — rule-by-rule validation tests
└── docs/
    ├── CR.txt               — full Comprehensive Rules (local copy)
    ├── todo.md              — running bug/feature list
    ├── design-notes-effectstep-damage.md   — DealDamage keyword propagation gap
    └── design-notes-trigger-architecture.md — trigger system split, proposed unification
```

---

## 3. Architecture Overview

```
┌────────────────────────────────────────────────────────────────────────────┐
│  External API surface                                                      │
│  serve.rs (Axum HTTP)  ──►  engine/* functions  ──►  new GameState        │
│  main.rs demo loop     ──►  (same engine functions)                        │
└────────────────────────────────────────────────────────────────────────────┘

┌─────────────────────┐    ┌──────────────────────────────────────────────┐
│  cards/             │    │  types/                                       │
│  CardDatabase       │    │  All pure data structs; no game logic         │
│  Scryfall JSON      │───►│  CardDefinition → CardObject → PermanentState│
│  downloader         │    │  GameState (master struct, all zones)         │
└─────────────────────┘    └──────────────────────────────────────────────┘
                                        │
                                        ▼
                           ┌──────────────────────────────────────────────┐
                           │  parser/oracle.rs                             │
                           │  oracle_text → Vec<RulesText>                 │
                           │  (run at CardObject construction time)        │
                           └──────────────────────────────────────────────┘
                                        │
                                        ▼
                           ┌──────────────────────────────────────────────┐
                           │  engine/                                      │
                           │  All rules enforcement                        │
                           │  Takes (GameState, args) → GameState|Err     │
                           │                                               │
                           │  casting.rs  ──► stack.rs ──► triggered.rs   │
                           │  turn.rs     ──► state_based_actions.rs       │
                           │  combat.rs   ──► state_based_actions.rs       │
                           │  mana.rs     ──► costs.rs                     │
                           │  targeting.rs (called from casting, activated)│
                           └──────────────────────────────────────────────┘
```

**Key invariant:** Engine functions take ownership of `GameState` and return a (possibly modified) `GameState`. The caller thread owns the state between calls. There is no interior mutability in game state.

**Thread safety:** `serve.rs` wraps `GameState` in `Arc<Mutex<GameState>>`. Every HTTP handler locks the mutex, clones nothing — it takes the game state by `MutexGuard` and calls engine functions. This is safe because the Axum server is single-game-instance.

---

## 4. The Three-Layer Card Model

Every card in the game exists at three levels:

```
CardDefinition          — static oracle data; shared across all copies of a card
    │ (Arc'd inside CardObject)
    ▼
CardObject              — one per card instance in the game
    │ (id, zone, controller, owner, rules_text from parser)
    ▼
PermanentState          — battlefield-only additional state
    (tapped, damage, counters, attached_to, pt_boost_until_eot, ...)
```

**`CardDefinition`** (`src/types/card.rs`):
Lives in `CardDatabase`. Holds: name, mana_cost, type_line, oracle_text, and the parsed `rules_text: Vec<RulesText>` (populated at parse time, not build time). Also holds `text_annotations: Vec<TextAnnotation>` for UI highlighting. The `colors` field is authoritative from Scryfall — not derived from mana cost (CR 105.2 multi-face cards can have colors their pips don't show).

**`CardObject`** (`src/types/card_object.rs`):
Created by `CardObject::new(id, def, controller, zone)`. Holds the `CardDefinition` (cloned — each game has its own copy of the definition with injected intrinsic abilities). `inject_intrinsic_abilities(def)` is called during construction and adds `{T}: Add {color}` activated abilities for basic land subtypes (CR 305.6). Plains, Forest, etc. get one ability; Savannah (dual land) gets two.

`has_keyword(kw)` scans `rules_text` looking for `RulesText::Active(Rule::Static(kw))`. This is the correct way to check for keywords on a card in any zone.

**`PermanentState`** (`src/types/permanent.rs`):
Created when a card enters the battlefield (in `execute_effect_steps`, `MoveZone { to: Battlefield }` branch). Destroyed when a permanent leaves the battlefield. `GameState.battlefield: HashMap<ObjectId, PermanentState>` tracks all battlefield permanents.

Key fields:
- `controller_since_turn: u32` — for summoning sickness check
- `counters: Vec<(CounterKind, u32)>` — +1/+1, -1/-1, poison, named
- `pt_boost_until_eot: Option<PTDelta>` — ephemeral (Battle Cry, Bushido N, etc.)
- `attached_to: Option<ObjectId>` — Aura/Equipment attachment point

`effective_power(bonus)` and `effective_toughness(bonus)` compute the visible P/T: base + counters + pt_boost_until_eot + continuous `bonus` (the `bonus` param is computed separately by `continuous_pt_bonus`).

---

## 5. The Mana System

Defined in `src/types/mana.rs`. Implements CR 107.4.

**`ManaPip`** — one mana symbol:
```
White | Blue | Black | Red | Green | Colorless  — basic colored/colorless
Generic(u32)                                    — {N}
X                                               — {X}
Hybrid(ManaColor, ManaColor)                    — {W/U} etc.
GenericHybrid(u32, ManaColor)                   — {2/W} etc.
ColorlessHybrid(ManaColor)                      — {C/W} etc.
Phyrexian(ManaColor)                            — {W/P}
HybridPhyrexian(ManaColor, ManaColor)           — {W/U/P}
Snow                                            — {S}
```

**`ManaCost`** is `Vec<ManaPip>`. `mana_value()` sums pip values (X counts as 0).

**`ManaPool`** (`src/types/mana.rs`):
Has 6 color fields plus 6 `snow_X` shadow fields. The invariant `snow_X ≤ X` must always hold — snow mana is a subset of the total mana of that color. Snow mana is used to pay `ManaPip::Snow`.

**`PaymentPlan`** (`src/types/mana.rs`):
Maps each pip to a source: which color/count from the pool pays each generic or hybrid pip, and `blood: u32` for Phyrexian life payments (1 blood = 2 life).

**`greedy_payment_plan`** (`src/engine/mana.rs`):
Two-pass greedy allocator. Pass 1: non-X pips (colored, Hybrid, Phyrexian, Snow, Generic). Pass 2: X pip (uses `x_value` multiplied). Returns `None` if the pool cannot cover the cost.

**`pay_mana_cost`** (`src/engine/mana.rs`):
Validates plan against pool AND against cost pips, then atomically deducts. This is the mutation site — `greedy_payment_plan` just reads, `pay_mana_cost` writes.

**Mana checkpoint** (`ManaCheckpoint` in `src/types/game_state.rs`):
Created lazily on first `tap_land_for_mana` call within a priority window. Stores a snapshot of all pools and which lands were tapped. `reset_mana` restores from the checkpoint. This is used by the serve.rs UI to let players undo taps before committing. Cleared in `advance_step`.

---

## 6. The Stack and Resolution Model

**`StackObject`** (`src/types/stack.rs`):
```rust
pub struct StackObject {
    pub id: StackId,
    pub payload: StackPayload,   // Spell | TriggeredAbility | ActivatedAbility
    pub controller: PlayerId,
    pub targets: Vec<EffectTarget>,
    pub x_value: Option<u32>,
    pub cast_mode: CastMode,     // Standard | Kicked | Multikicked(n) | Dashed | Evoked
}
```

`GameState.stack: Vec<StackId>` is the stack order (top = last). `GameState.stack_objects: HashMap<StackId, StackObject>` is the lookup.

**`pass_priority`** (`src/engine/stack.rs`):
Increments `consecutive_passes`. Once `consecutive_passes >= num_players` (2), either:
- Stack is non-empty → call `resolve_top`
- Stack is empty → call `advance_step`

**`resolve_top`** (`src/engine/stack.rs`):
Pops the top stack object and resolves it by calling `execute_effect_steps`. Then runs `apply_sbas_and_push_triggers`. Then resets priority to active player.

For **Spell** payloads:
- Permanent spells (Creature, Artifact, Enchantment, Land): move to battlefield
  - Aura: if no legal target remains (CR 303.4g), goes to graveyard instead
  - Evoke: schedule sacrifice trigger
  - Dash: schedule return-to-hand delayed trigger
- Instant/sorcery: execute effects, then move to graveyard
  - All-illegal-targets: fizzle — no effects, goes to graveyard (CR 608.2b)

For **TriggeredAbility**/**ActivatedAbility**: execute the `effect` steps.

**`execute_effect_steps`** (`src/engine/stack.rs`):
Walks an `Effect = Vec<EffectStep>` and applies each step. Notable cases:
- `MoveZone { to: Battlefield }`: creates `PermanentState`, collects ETB triggers, fires `EntersTheBattlefield` game event
- `Payment { cost, on_paid, on_declined }`: creates `PendingPayment` and returns early (interrupt)
- `DealDamage(step)`: marks damage or subtracts life (see §13 for known gap)
- `CounterSpell`: removes spell from stack, moves card to graveyard

**`inject_source_flags`** (`src/engine/stack.rs`):
Called when pushing abilities onto the stack. Snapshots lifelink/deathtouch/wither/infect/source_colors/source_card_types/source_subtypes from the source permanent's current state into any `DealDamage` steps. This is Last Known Information — the source's keywords at activation time govern the damage, not at resolution time.

---

## 7. Turn Structure

**`Step`** (`src/types/step.rs`):
```
Untap → Upkeep → Draw → PreCombatMain →
BeginningOfCombat → DeclareAttackers → DeclareBlockers →
FirstStrikeDamage → CombatDamage → EndOfCombat →
PostCombatMain → End → Cleanup
```

`Phase` groups these into 5 phases (Untap/Draw, Precombat Main, Combat, Postcombat Main, End).

**`advance_step`** (`src/engine/turn.rs`):
Empties all mana pools (CR 106.4), clears `mana_checkpoint`. Pops `extra_steps` queue first (for first-strike injection). Otherwise advances to the next step in the canonical sequence.

**`apply_step_start`** (`src/engine/turn.rs`):
Fires `GameEvent::PhaseStep(step)` for triggered abilities. Drains `delayed_triggers`. Dispatches to step-specific logic:
- `Untap`: untaps all permanents controlled by active player; doesn't use the stack; no player receives priority
- `Draw`: active player draws a card (except first turn for player 1)
- `Cleanup`: clears `damage_marked`, `damaged_by_deathtouch`, `pt_boost_until_eot` on all permanents; discards to hand size (TODO: not yet implemented — see §13)

**`extra_steps`** (`GameState`):
A `VecDeque<Step>`. When first-strike creatures are in combat, `deal_combat_damage` pushes `Step::CombatDamage` here so a second full damage step fires for double-strikers. `advance_step` pops from this queue before the normal sequence.

**`delayed_triggers`** (`GameState.delayed_triggers: Vec<DelayedTrigger>`):
`DelayedTrigger { fires_on_step, effect, targets, controller }`. Drained in `apply_step_start` — any trigger matching the current step fires by pushing its effect onto the stack. Used for:
- Dash return-to-hand at `Step::End`
- Evoke ETB sacrifice trigger

---

## 8. Combat System

All in `src/engine/combat.rs`.

**`declare_attackers(state, player_id, attacker_ids)`:**
Validates: active player, correct step, each attacker is on the battlefield, controlled by player, not tapped (unless Vigilant), not summoning sick. Taps non-Vigilant attackers. Fires `GameEvent::Attacks { attacker_id }` for each. Collects attack triggers (Exalted, Melee, Battle Cry, Training — see §9).

**`declare_blockers(state, player_id, blocking_map)`:**
`blocking_map: HashMap<ObjectId, Vec<ObjectId>>` maps attacker → blockers. Validates each blocker can legally block each attacker via `can_block_attacker`. After all blockers declared, checks Menace (each blocked attacker must have ≥2 blockers). Fires `Blocks` and `BecomesBlocked` events.

**`can_block_attacker(state, attacker_id, blocker_id)`:**
Checks flying/reach, shadow, horsemanship, skulk, fear, intimidate, landwalk, protection, decayed. Each is referenced to a CR rule.

**`deal_combat_damage(state, player_id)`:**
Two-round system. First call: assigns first-strikers only (if any exist). If double-strikers also present, pushes `Step::CombatDamage` to `extra_steps`. Second call (or only call when no first-strikers): all remaining attackers.

Damage assignment uses dedicated accumulator maps (`damage_to_players`, `damage_to_objects`, `lifelink_gain`, `deathtouch_targets`, `wither_to_objects`, `poison_to_players`) before applying any effects. This ensures simultaneous application.

Trample with deathtouch: 1 damage = lethal threshold, so Tramplers need only assign 1 to blockers before trampling over. Wither/Infect deal −1/−1 counters to creatures instead of marked damage. Infect deals poison counters to players instead of life loss.

After damage, fires `DealsCombatDamage` events, then runs `apply_sbas_and_push_triggers`.

---

## 9. Triggered Abilities — Current Split and Known Issue

**There are two parallel, incompatible trigger systems.** This is the most important architectural debt. See `docs/design-notes-trigger-architecture.md` for full context.

**Track 1 — Data-driven (`TriggeredAbility` in rules_text):**
Parser produces `RulesText::Active(Rule::Triggered(TriggeredAbility { trigger, condition, target_mode, effect }))`. The engine's `collect_etb_triggers` (in `triggered.rs`) scans rules text for these. Any card that has a Typed ETB ability works without engine changes. `TriggerEvent` and `TriggerCondition` enums govern matching.

**Track 2 — Hardcoded collector functions:**
Keywords that can't be expressed with the current `TriggerEvent` enum fall into ad-hoc collector functions:
- `collect_attack_triggers` — Exalted, Melee, Battle Cry, Training
- `collect_block_triggers` — Flanking, Bushido N
- `collect_evolve_triggers` — Evolve
- `collect_cast_triggers` — Prowess
- `collect_ward_triggers` — Ward (fires at targeting time, not stack push)

When adding a new triggered keyword, you must decide which track it goes into. If the `TriggerEvent` enum can express the condition, use Track 1. If not, add a collector function in `triggered.rs` and call it from the appropriate engine location (e.g., `casting.rs` for cast triggers, `declare_attackers` for attack triggers).

---

## 10. Module Reference

### src/types/

#### `ability.rs`

The largest type file. Contains everything ability-related.

**`LandwalkKind`**: `LandType(String) | Nonbasic`  
**`ProtectionQuality`**: `Color(ManaColor) | CardType(CardType) | CreatureType(String) | Everything`  
`source_matches_quality(quality, colors, types, subtypes)` — checks if a source (described by its colors/types/subtypes) matches a protection quality. Used by targeting and combat protection checks.

**`KeywordAbility`**: ~40 variants. Most are unit variants (Trample, Flying, etc.). Parameterized ones:
- `BushidoN(u32)`, `ToxicN(u32)`, `AnnihilatorN(u32)` — N value
- `Landwalk(LandwalkKind)` — subtype or nonbasic
- `ProtectionFrom(ProtectionQuality)`, `HexproofFrom(ProtectionQuality)`

**`TurnOwner`**: `You | Opponent | Any` — CR 109.5 "you" means the ability's controller.

**`TriggerSubjectFilter`**: What the ETB-entering (or attacking, etc.) permanent must satisfy for the trigger to fire. All fields are `Option` — `None` means "don't filter on this."  
Fields: `is_self: Option<bool>`, `controller: Option<TurnOwner>`, `card_types: Vec<CardType>`, `subtypes: Vec<String>`.  
Default (all None, empty vecs) matches any permanent.

**`TriggerCondition`**: Closed enum of game-state predicates checked at trigger-fire time:
- `ExactlyOneAttacker` — Exalted
- `AttackingAlongsideGreaterPowerCreature` — Training
- `EnteringCreatureHasGreaterPower/Toughness/PowerOrToughness` — Evolve variants
- `Always` — unconditional

**`GameEvent`**: Runtime events with concrete IDs. Produced by engine code and collected by `apply_sbas_and_push_triggers`. Variants include `EntersTheBattlefield`, `Dies`, `Attacks`, `Blocks`, `BecomesBlocked`, `DealsCombatDamage`, `SpellCast`, `PhaseStep`, `DrawsCard`, `TargetedBy`.

**`TriggerEvent`**: Pattern events with filters. Stored in `TriggeredAbility.trigger`. Matched against `GameEvent` in `triggered.rs`. Currently only covers ETB and a few others; this is the gap that forces Track 2 for complex abilities.

**`TriggeredAbility`**: `{ trigger, condition, target_mode, effect }`.  
`target_mode: TriggerTargetMode` controls how `targets` is populated on the resulting `StackObject`:  
- `None` — no targets
- `Source` — target is the source permanent itself
- `Subject` — target is the triggering subject (e.g., the creature that ETB'd)
- `AllOtherAttackers` — targets all other attackers (Battle Cry style)

**`ActivatedAbility`**: `{ cost: Vec<CostComponent>, target_requirements: Vec<TargetFilter>, effect: Effect }`.

**`CostComponent`**: `Tap | Mana(ManaCost) | PayLife(u32) | Sacrifice(u32, PermanentFilter) | Discard(u32, CardFilter) | Unimplemented(String)`.  
Note: `Tap` cost is handled by the caller before `pay_cost_components` — it is not paid inside that function.

**`PermanentFilter`**: `{ controller, card_types, subtypes, colors, object_ids }`. When `object_ids` is non-empty, only those specific permanents match (used for targeting specific objects).

**`CastMode`**: `Standard | Kicked | Multikicked(u32) | Dashed | Evoked`. Serialized snake_case for the HTTP API.

**`SpellAbility`**: `{ target_requirements: Vec<TargetFilter>, steps: Vec<EffectStep> }` — for instant/sorcery spells (CR 113.3a).

**`Rule`**: Master enum of all rule types:
```
Static(KeywordAbility)
Triggered(TriggeredAbility)
Activated(ActivatedAbility)
SpellAbility(SpellAbility)
Cycling(ManaCost)
Continuous(ContinuousEffect)
Aura { enchants: TargetFilter, effect: Effect }
Equip { cost: Cost, effect: Effect }
Kicker { cost: Cost }
Multikicker { cost: Cost }
Dash { cost: ManaCost }
Evoke { cost: ManaCost }
```

**`RulesText`**: The tagged union that lives in `CardDefinition.rules_text`:
```
Active(Rule)                     — engine enforces this
Ignored(IgnoredKind, String)     — reminder text or flavor words
Unparsed(String)                 — oracle text the parser couldn't handle
ParsedUnimplemented(String)      — parser understood it, engine doesn't act on it
```

`has_unparsed()` on `CardDefinition` returns true if any `Unparsed` entries exist (partial parse).

**`TextAnnotation`**: `{ start: usize, end: usize, kind: AnnotationKind }`. Byte offsets into `oracle_text`. Non-overlapping, in source order. Used by the serve.rs UI to color-code oracle text in the browser.

#### `card.rs`

**`TypeLine`**: `{ supertypes: Vec<Supertype>, card_types: Vec<CardType>, subtypes: Vec<String> }`. All are vecs to handle multi-type cards.

**`CardDefinition`**: `{ name, mana_cost: Option<ManaCost>, type_line, oracle_text, rules_text, text_annotations, power: Option<i32>, toughness: Option<i32>, colors: Vec<ManaColor> }`. `colors` comes from Scryfall and is authoritative.

#### `card_object.rs`

**`inject_intrinsic_abilities(def: &mut CardDefinition)`**: Called in `CardObject::new`. Checks `type_line.subtypes` for basic land subtype names (Plains, Island, etc.). For each, prepends a `Rule::Activated` mana ability. This runs on every `CardObject::new` call, so each game instance gets its own copy of the abilities. Does not modify the `CardDatabase`'s `CardDefinition` — only the per-object copy.

**`CardObject.is_creature()`**: Checks `definition.type_line.is_creature()`.

#### `game_state.rs`

**`GameState`** is the master struct. Key fields:
- `objects: HashMap<ObjectId, CardObject>` — all cards in all zones
- `libraries: HashMap<PlayerId, Vec<ObjectId>>` — ordered (top of library = last element)
- `hands: HashMap<PlayerId, Vec<ObjectId>>`
- `graveyards: HashMap<PlayerId, Vec<ObjectId>>`
- `battlefield: HashMap<ObjectId, PermanentState>` — indexed by card ObjectId
- `stack: Vec<StackId>` — ordered (top = last)
- `stack_objects: HashMap<StackId, StackObject>`
- `exile: Vec<ObjectId>`
- `players: Vec<Player>` — always exactly 2, index = PlayerId
- `active_player: PlayerId`, `priority_player: PlayerId`
- `turn_number: u32`, `lands_played_this_turn: u32`
- `combat: CombatState`
- `mana_checkpoint: Option<ManaCheckpoint>`
- `extra_steps: VecDeque<Step>` — for first-strike second round
- `pending_payment: Option<PendingPayment>` — for Ward/cost payment interrupts
- `delayed_triggers: Vec<DelayedTrigger>`
- `consecutive_passes: u32`
- `game_over: bool`

`step()` returns the current step (field is `pub(crate)` — use the accessor).  
`opponent_of(pid)` works for exactly two players; panics if called in a non-two-player state.  
`controllers_most_recent_turn(pid)` returns `turn_number` if `pid` is the active player, else `turn_number + 1`. Used for summoning sickness: `summoning_sick(controllers_most_recent_turn(controller))` gives the correct answer for both active and non-active players (CR 302.6).

**`PendingPayment`**: Interrupts stack resolution for cost obligations (Ward, etc.):
```rust
pub struct PendingPayment {
    pub paying_player: PlayerId,
    pub cost: Vec<CostComponent>,
    pub on_paid: Effect,       // steps to run if cost is paid
    pub on_declined: Effect,   // steps to run if cost is declined
    pub continuation: Effect,  // rest of the interrupted effect
    pub targets: Vec<EffectTarget>,
    pub controller: PlayerId,
}
```
When `execute_effect_steps` hits a `Payment` step, it stores `PendingPayment` and returns early. The HTTP API then presents the player with "pay or decline" options. `pay_pending_cost` and `decline_pending_cost` in `costs.rs` resume execution.

#### `permanent.rs`

**`summoning_sick(current_turn: u32) -> bool`**: `controller_since_turn >= current_turn`. When a creature ETBs on turn 3, `controller_since_turn = 3` and `current_turn = 3`, so `3 >= 3` = sick. On turn 4, `3 >= 4` = false = ready. This works correctly for non-active players because `controllers_most_recent_turn` adjusts the comparison turn.

**`effective_power(bonus: i32) -> Option<i32>`**: base + counter adjustments + pt_boost_until_eot + bonus. Returns `None` if the card has no power (non-creatures). The `bonus` parameter is the result of `continuous_pt_bonus` (from Auras, Equipment, global Continuous effects).

**`counters: Vec<(CounterKind, u32)>`**: Simple vec of (kind, count) pairs. `add_counters`, `remove_counters`, `counter_count` handle it. Counter kinds can overlap — +1/+1 and −1/−1 counters coexist until SBAs cancel them (CR 704.5q).

#### `effect.rs`

**`DamageStep`**: Snapshot struct for a DealDamage step. Carries `amount` plus keyword flags (`lifelink`, `deathtouch`, `wither`, `infect`, `source_colors`, `source_card_types`, `source_subtypes`, `toxic_n: Option<u32>`). Flags are injected at stack-push time by `inject_source_flags`, not at parse time.

**`EffectStep`** variants:
- `AddMana(ManaPool)` — adds mana directly to pool (no activation needed; used by mana ability resolution)
- `Mill(u32)` — moves N cards from top of library to graveyard
- `DrawCard(u32)` — draws N cards (fires `DrawsCard` events if `fire_events`)
- `GainLife(u32)` — adds life to effect controller
- `BoostPermanentPT(PTDelta)` — sets `pt_boost_until_eot` on targets (cleared in Cleanup)
- `AddCounter { kind, count }` — adds counters to target
- `MoveZone { from, to, to_player }` — the zone-change workhorse
- `DealDamage(DamageStep)` — deals damage with keyword flags
- `CounterSpell` — counters the targeted spell
- `Payment { cost, on_paid, on_declined }` — creates `PendingPayment` interrupt
- `Unimplemented(String)` — no-op, for parsed-but-not-implemented steps
- `Attach { source_id }` — attaches source (Aura/Equipment) to target permanent

#### `stack.rs` (types)

**`StackPayload`**:
```rust
pub enum StackPayload {
    Spell { card_id: ObjectId },
    TriggeredAbility { source_id: ObjectId, effect: Effect, label: String },
    ActivatedAbility { source_id: ObjectId, effect: Effect, label: String },
}
```

The `label` on non-spell payloads is for UI display (e.g., "Cycling", "Bushido", "Kicker trigger").

---

### src/engine/

#### `mod.rs`

**`EngineError`**: 17 variants covering all illegal-action cases. Returned as `Err` from all engine functions. HTTP API translates these to 4xx responses.

**`continuous_pt_bonus(state, target_id) -> i32`**: Two passes:
1. All permanents with `Rule::Continuous` in their rules_text — check if the continuous effect's `subject_filter` matches `target_id`.
2. All Auras attached to `target_id` (`perm.attached_to == Some(target_id)`) — apply `Aura.effect` steps that modify P/T. Same for Equipment.

Returns a single integer — total bonus to both power and toughness (the effect is assumed symmetric; for asymmetric effects this would need a struct).

**`has_protection_from(obj, colors, types, subtypes) -> bool`**: Checks all `ProtectionFrom` entries in obj's rules_text. Used in combat and targeting.

#### `mana.rs`

**`tap_land_for_mana(state, player_id, land_id, ability_index)`**: Lazily creates `ManaCheckpoint` on first call. Validates the land is on the battlefield, controlled by player, not tapped (and not summoning sick without Haste). Marks it tapped, resolves the AddMana ability directly (not via stack — mana abilities don't use the stack, CR 605.3).

**`reset_mana(state, player_id)`**: Restores mana pools and untaps all lands recorded in `mana_checkpoint`. Clears the checkpoint. Called from serve.rs "reset mana" UI button.

**`land_produces(subtypes) -> ManaColor`**: Maps `["Plains"] → White`, etc. This function needs expansion if dual-type lands need proper color mapping (currently each subtype triggers its own ability injection).

#### `casting.rs`

**`play_land(state, player_id, card_id)`**: Special action (not a spell, not the stack). Validates: active player, main phase, empty stack, at most `lands_played_this_turn < 1` (no land-drop extensions implemented). Moves card from hand to battlefield. Does not trigger spellcasting-related abilities.

**`cast_spell(state, player_id, object_id, targets, x_value, cast_mode)`**: Full spell-cast sequence:
1. Validate priority, card in hand, timing (sorcery vs instant speed via `is_instant_speed`)
2. Validate targets against `SpellAbility.target_requirements`
3. Determine cost: base mana cost, modified by CastMode (Kicked/Multikicked adds kicker cost, Dashed uses Dash cost, Evoked uses Evoke cost)
4. Call `costs::pay_cost_components`
5. Move card from Hand to Stack, create `StackObject`
6. Fire `GameEvent::SpellCast` (triggers Prowess etc.)
7. Fire `GameEvent::TargetedBy` for each target (triggers Ward)

**`is_instant_speed(state, player_id, card_object)`**: Returns true if: card is instant, or card has Flash, or player has priority on their own main phase with empty stack (sorcery speed).

#### `stack.rs`

**`pass_priority(state, player_id)`**: The main game action driver. After both players pass consecutively:
- Non-empty stack → `resolve_top`
- Empty stack → `advance_step`

Priority resets to active player after any resolution or step advance.

**`resolve_x_in_cost(effect, x_value)`**: Substitutes `ManaPip::X` with `ManaPip::Generic(x_value)` in any `Payment` steps within the effect. Called before pushing a spell's resolution effect.

**`apply_sbas_and_push_triggers(state)`**: Helper: runs `check_and_apply_sbas`, then calls the typed trigger dispatch, then collects any waiting triggers onto the stack.

#### `turn.rs`

**`draw_card(state, player_id, fire_events: bool)`**: If library empty, sets `has_lost = true` and `game_over = true` (CR 704.5b). Otherwise moves top card to hand, fires `DrawsCard` event if `fire_events`. The `fire_events = false` path is used during game setup (drawing opening hand) to avoid spurious trigger dispatch.

**`skip_to_first_main(state)`**: Used in `serve.rs` game setup after dealing opening hands. Advances to `Step::PreCombatMain` without triggering draw-step logic.

**`cleanup_step(state)`**: Clears `damage_marked`, `damaged_by_deathtouch`, `pt_boost_until_eot` on all permanents. Currently does NOT discard to hand size (CR 402.2) — see §13.

**`end_of_combat_step(state)`**: Sacrifices all Decayed attackers that survived combat (CR 702.147a). Uses `EffectStep::MoveZone` into graveyard.

**`start_next_turn(state)`**: Rotates `active_player`, increments `turn_number`, resets `lands_played_this_turn = 0`. Calls `apply_step_start(Untap)`.

#### `state_based_actions.rs`

**`check_and_apply_sbas(state)`**: Loops until no new SBAs are found. Returns the modified state.

**`find_sbas(state) -> Vec<SBA>`**: Checks:
- `PlayerLoses` for life ≤ 0 (CR 704.5a)
- `PlayerLoses` for poison ≥ 10 (CR 704.5c)
- `MoveToGraveyard` for toughness ≤ 0 (CR 704.5f)
- `MoveToGraveyard` for damage ≥ toughness (CR 704.5g)
- `MoveToGraveyard` for damaged by deathtouch (CR 704.5h) — NOT exempt from Indestructible
- `CancelCounters` for +1/+1 vs −1/−1 (CR 704.5q)
- `AuraToGraveyard` for illegal aura attachment (CR 704.5m)
- `DetachEquipment` for illegal equipment attachment (CR 704.5n), including protection check

**`apply_sbas`**: Processes the SBA vec. Indestructible permanents are exempt from damage/toughness death but NOT from -1/-1 counter toughness death. Dies triggers are collected BEFORE the zone change happens (CR 603.10a — last known information). Persist/Undying: if a creature dying from SBAs has Persist (no −1/−1 counter) or Undying (no +1/+1 counter), a triggered return ability is created and pushed onto the stack.

#### `targeting.rs`

**`is_legal_target(state, target, filter, caster, source_colors, source_card_types, source_subtypes) -> bool`**:
- Objects: must be on battlefield, pass filter (Creature/Player/Any), not Shroud, not Hexproof (opponent check), not HexproofFrom (quality + opponent check), not ProtectionFrom (quality check)
- Players: must not have `has_lost`, must pass filter (Player | Any)
- StackObjects: must be a Spell (not triggered/activated ability), card types must satisfy the SpellFilter

**`legal_targets(state, player_id, source_id, filter, cast_mode) -> Vec<EffectTarget>`**: Returns all currently legal targets for a given source + filter. Called from serve.rs to populate the UI's target-selection list.

#### `triggered.rs`

Contains `subject_filter_matches`, `trigger_condition_satisfied`, and all collector functions.

**`subject_filter_matches(filter, subject_id, source_id, source_controller, state)`**: Returns true if the subject (entering/attacking creature, etc.) satisfies the filter. Checks `is_self`, `controller`, `card_types`, `subtypes`.

**`trigger_condition_satisfied(condition, subject_id, source_id, state)`**: Evaluates `TriggerCondition` variants. Note: continuous effects are NOT applied during trigger condition checks (bonus is hard-coded to 0 — see TODO comment in code). This means an anthem (+1/+1 to all creatures) won't affect whether Evolve fires.

Collector functions per trigger type — each returns `Vec<StackObject>`:
- `collect_etb_triggers` — ETB from `Rule::Triggered` in rules_text
- `collect_attack_triggers` — Exalted, Melee, Battle Cry, Training
- `collect_block_triggers` — Flanking, Bushido N
- `collect_evolve_triggers` — Evolve (checks ETB subject P/T vs source P/T)
- `collect_cast_triggers` — Prowess (noncreature spell cast triggers)
- `collect_ward_triggers` — Ward (targeted ability, fires at targeting time)
- `collect_draw_triggers` — draw-related triggers (if any)

#### `costs.rs`

**`pay_cost_components(state, player_id, components, x_value) -> Result<GameState, EngineError>`**:
- `Mana(cost)`: calls `greedy_payment_plan`, then `pay_mana_cost`
- `PayLife(n)`: directly deducts if player has enough life
- `Tap`, `Sacrifice`, `Discard`, `Unimplemented`: not handled here (callers handle Tap before calling; Sacrifice/Discard not yet implemented)

**`can_pay_cost_components(state, player_id, object_id, components) -> bool`**:
- `Tap`: checks not tapped, not summoning sick (or has Haste)
- Mana/life: always returns true (structural feasibility only — actual affordability is deferred to payment). This is a known limitation: the serve.rs UI will show X-cost abilities as available regardless of pool size.

**`pay_pending_cost` / `decline_pending_cost`**: Resume interrupted stack resolution after Ward payment decision.

#### `cycling.rs`

**`cycle_card(state, card_id, player_id)`**: Validates card in hand with `Rule::Cycling(cost)`. Pays mana cost. Discards card immediately (as cost — does NOT go on the stack). Creates a `StackObject` with `ActivatedAbility { effect: [DrawCard(1)] }`. The draw effect resolves on the stack; the discard is immediate cost.

#### `equip.rs`

**`activate_equip(state, equipment_id, target_creature_id, player_id)`**: Validates sorcery-speed timing (active player, main phase, empty stack), both equipment and target on battlefield controlled by player, target is creature. Finds `Rule::Equip { cost, effect }`, checks cost feasibility, pays cost, pushes a `StackObject` with `ActivatedAbility { effect: [Attach { source_id: equipment_id }] }`.

#### `activated.rs`

**`activate_ability(state, object_id, ability_index, activating_player, x_value, declared_targets)`**: Validates object on battlefield, controlled by player, ability index valid. For non-mana abilities: validates target count and legality. Handles Tap cost: calls `can_pay_cost_components` (Tap check), then taps the permanent. Calls `pay_cost_components`. Creates `ManaCheckpoint` if not present. Pushes `StackObject` onto stack. For mana abilities (produce mana): calls `execute_effect_steps` directly (mana abilities don't use the stack, CR 605.3).

---

### src/parser/

#### `oracle.rs`

Two public entry points:
- `parse_permanent(oracle_text) -> (Vec<RulesText>, Vec<TextAnnotation>)`
- `parse_instant_or_sorcery(oracle_text) -> (Vec<RulesText>, Vec<TextAnnotation>)`

The parser operates line-by-line on oracle text. For each line, it attempts to match against known patterns in priority order:
1. Ability words / flavor words → `Ignored(AbilityWord, ...)`
2. Reminder text (`(...)` at start of line) → `Ignored(ReminderText, ...)`
3. Keyword list lines (comma-separated keywords) → each keyword parsed into `Rule::Static(KeywordAbility::...)`
4. Kicker/Multikicker/Dash/Evoke lines → `Rule::Kicker/Multikicker/Dash/Evoke`
5. Cycling line → `Rule::Cycling`
6. Landwalk patterns → `Rule::Static(KeywordAbility::Landwalk(...))`
7. Protection patterns → `Rule::Static(KeywordAbility::ProtectionFrom(...))`
8. Ward pattern → `Rule::Static(KeywordAbility::Ward(cost))`
9. Activated ability pattern (`cost: effect`) → `Rule::Activated` or `Rule::Aura/Equip`
10. Triggered ability patterns ("When/Whenever/At ...") → `Rule::Triggered`
11. Continuous effect patterns ("... gets +N/+N") → `Rule::Continuous`
12. "Deals N damage" spells → `Rule::SpellAbility`
13. Fallthrough → `Unparsed(line)`

`find_at_depth_zero`, `split_at_depth_zero` — depth-aware string utilities that treat `(...)` as non-splitting context, used to split on `;` or `,` without breaking reminder text like `{W/P} (Phyrexian White)`.

`find_colon_at_depth_zero` — tracks both `()` and `{}` nesting, for splitting `{T}, {W}: Add {G}` correctly.

`strip_reminder_text` — removes `(...)` from a line for keyword matching.

`TextAnnotation` entries are emitted whenever something is tagged for UI coloring: reminder text, ability words, `ParsedUnimplemented`, and `Unparsed` spans.

---

### src/cards/

#### `mod.rs` (CardDatabase)

**`CardDatabase::open()`**: Uses `directories::ProjectDirs` to find the platform user data dir (e.g., `~/Library/Application Support/mecha-oracle/oracle_cards.json` on macOS). Calls `from_path`.

**`CardDatabase::from_path(path)`**: Reads the Scryfall bulk JSON file. For each entry calls `scryfall::parse_entry`. Counts: fully_parsed, partially_parsed (has Unparsed spans), tokens, art_cards, un_cards, skipped. Logs a summary at INFO level.

**`CardDatabase::get(name)`**: Case-insensitive lookup by card name.  
**`CardDatabase::get_token(name)`**: Lookup in token table.

#### `scryfall.rs`

**`parse_entry(json) -> Result<ParsedEntry, String>`**:
- Skips: tokens (layout == "token"), art cards (layout == "art_series"), un-cards (games contains "arena" but not "paper", or in un-sets by set code), double-faced cards without oracle_text, cards without a name.
- Parses: name, mana_cost (from `{W}{U}` format → `Vec<ManaPip>`), type_line (split on `—`), oracle_text, colors (list of color char strings), power/toughness (as i32, handles `*` as None).
- Calls `parse_permanent` or `parse_instant_or_sorcery` based on card type.
- Returns `ParsedEntry::Card(CardDefinition) | Token | ArtCard | UnCard`.

Mana cost parsing: `{2/W}` → `GenericHybrid(2, White)`, `{W/P}` → `Phyrexian(White)`, `{X}` → `X`, `{S}` → `Snow`, etc.

#### `downloader.rs`

**`update_cards()`**: Fetches the Scryfall bulk-data index to find the `oracle_cards` URL. Downloads the full JSON to the user data directory. Progress logged via `tracing`.

---

### src/serve.rs

Axum HTTP server. All routes are JSON (except `/` which returns HTML). State is `Arc<Mutex<GameState>>`.

**Routes:**
- `GET /` — returns the game UI HTML (hardcoded in the binary)
- `GET /state` — returns the full `GameState` serialized as JSON (massive; used by the UI to redraw everything)
- `POST /tap-land` — calls `tap_land_for_mana`
- `POST /reset-mana` — calls `reset_mana`
- `POST /play-land` — calls `play_land`
- `POST /cast` — calls `cast_spell` with targets, x_value, cast_mode
- `POST /pass` — calls `pass_priority`
- `POST /attack` — calls `declare_attackers`
- `POST /block` — calls `declare_blockers`
- `POST /damage` — calls `deal_combat_damage`
- `POST /activate` — calls `activate_ability`
- `POST /activate-equip` — calls `activate_equip`
- `POST /cycle` — calls `cycle_card`
- `POST /pay-ward` — calls `pay_pending_cost`
- `POST /decline-ward` — calls `decline_pending_cost`
- `POST /advance` — calls `advance_step` (debug/testing shortcut)
- `GET /legal-targets` — calls `legal_targets` for a given source + filter

**Game setup** (`build_game_state`): reads deck config JSON (array of two string arrays, each a list of card names), looks up each card in the database, builds `CardObject`s, assigns to libraries. If `--shuffle`, seeds from `SystemTime::now().subsec_nanos()`. Deals 7 cards to each player, then calls `skip_to_first_main`.

**Action filtering** in `/state` response: the serve.rs layer computes which actions are available to each player for UI display. This involves checking `can_pay_cost_components` for activated abilities, priority checks for casting, step-appropriate checks for land drops, etc.

---

### src/main.rs

**`demo` subcommand**: Hard-codes two players with a hand of Grizzly Bears and Forests. Runs the game loop (advance step / resolve actions) for up to 200 steps or until `game_over`. Good for quick smoke testing without a browser.

**`serve --shuffle <deck>` subcommand**: Loads `CardDatabase`, reads the deck config JSON from `<deck>`, calls `build_game_state`, binds Axum on `127.0.0.1:3000`.

**`update-cards` subcommand**: Calls `downloader::update_cards()`. No game state involved.

Logging: `tracing_subscriber` is initialized; `-v` flag enables DEBUG level. Default is INFO.

---

## 11. Key Data Flows

### Casting a Spell

```
serve.rs: POST /cast
    → cast_spell(state, player_id, card_id, targets, x_value, cast_mode)
        → validate priority, zone (Hand), timing
        → validate targets via is_legal_target
        → compute cost (base + kicker/dash/evoke modifiers)
        → pay_cost_components (mana deducted, lands already tapped)
        → move card: Hand → Stack (in objects map + stack + stack_objects)
        → fire GameEvent::SpellCast → collect_cast_triggers → push Prowess triggers
        → fire GameEvent::TargetedBy for each target → collect_ward_triggers
        → reset consecutive_passes = 0, priority to opponent
```

### Resolving the Top of Stack

```
serve.rs: POST /pass (both players)
    → pass_priority
        → consecutive_passes >= 2
        → resolve_top
            → pop StackObject from stack/stack_objects
            → if Spell + permanent: execute_effect_steps([MoveZone { to: Battlefield }])
                → create PermanentState in battlefield
                → fire GameEvent::EntersTheBattlefield
                → collect_etb_triggers, collect_evolve_triggers
                → push triggers onto stack
            → if Spell + instant/sorcery: execute_effect_steps(spell_ability.steps)
            → apply_sbas_and_push_triggers
            → reset priority to active player
```

### State-Based Actions Loop

```
check_and_apply_sbas(state):
    loop:
        sbas = find_sbas(state)
        if sbas.is_empty() → break
        apply_sbas(state, sbas):
            for each SBA:
                collect dies triggers (before zone change — LKI)
                move to graveyard / detach / cancel counters
            push collected triggers onto stack
```

### Ward Payment Interrupt

```
cast_spell → fire TargetedBy → collect_ward_triggers → push Ward trigger
pass_priority × 2 → resolve Ward trigger
    → execute_effect_steps([Payment { cost: Ward cost, on_paid: [], on_declined: [CounterSpell] }])
        → state.pending_payment = Some(PendingPayment { ... })
        → return early

serve.rs: POST /pay-ward
    → pay_pending_cost → pay_cost_components (Ward cost)
    → execute on_paid (empty) + continuation (empty)
    → spell stays on stack, priority resets

serve.rs: POST /decline-ward  
    → decline_pending_cost
    → execute on_declined ([CounterSpell])
    → spell removed from stack
```

---

## 12. Maintainer Recipes

### Adding a New Keyword Ability (e.g., Rampage N)

1. Add `RampageN(u32)` to `KeywordAbility` in `src/types/ability.rs`.
2. Add parsing in `src/parser/oracle.rs` — likely in the keyword-list loop, matching "Rampage N".
3. If it has combat effects: add handling in the appropriate place in `combat.rs`. Rampage triggers when blocked by multiple creatures, so add to `collect_block_triggers` in `triggered.rs` or handle in `declare_blockers`.
4. Write a test in `tests/cr_examples.rs` verifying the CR rule it corresponds to.

### Adding a New ETB Triggered Ability (data-driven via Track 1)

If the trigger is "When this ETBs, do X" where X is expressible as `Effect`:
1. Add the oracle text pattern to `oracle.rs` — parse into `Rule::Triggered(TriggeredAbility { trigger: TriggerEvent::EntersTheBattlefield { subject_is_self: true }, condition: TriggerCondition::Always, ... })`.
2. No engine changes needed — `collect_etb_triggers` already handles this.

If the trigger condition is more complex (e.g., "when another creature ETBs under your control with greater power than this"):
1. Add the condition variant to `TriggerCondition` if needed.
2. Implement `trigger_condition_satisfied` branch for the new variant.
3. Parse into `Rule::Triggered` in oracle.rs.

### Adding a New Effect Step (e.g., Scry N)

1. Add `ScryN(u32)` to `EffectStep` in `src/types/effect.rs`.
2. Add handling in `execute_effect_steps` in `engine/stack.rs` — move N cards from top of library, player inspects, puts back in chosen order.
3. Add parsing in `oracle.rs` for "Scry N" in spell text.
4. The serve.rs UI will need a new interaction path if Scry requires player input (card ordering). This is non-trivial — you'd need a `PendingScry` interrupt similar to `PendingPayment`.

### Adding a New CastMode Alternative Cost

The pattern for Kicker/Dash/Evoke is established. For a new one (e.g., Overload):
1. Add `Overloaded` to `CastMode` in `src/types/ability.rs`.
2. Add `Rule::Overload { cost: ManaCost }` to `Rule`.
3. In `casting.rs`: handle the new CastMode in cost computation and spell behavior.
4. In `resolve_top` in `stack.rs`: handle any special resolution logic (Overload replaces "target creature" with "each creature").
5. In `oracle.rs`: parse "Overload [cost]" lines.
6. In `serve.rs`: add the Overloaded option to cast actions.

### Adding a Graveyard-Activated Ability (e.g., Unearth)

Currently `activate_ability` requires the source to be on the battlefield. To support graveyard activations:
1. In `activated.rs`: add a zone check branch — if the ability's CostComponent includes graveyard-sourced patterns, allow activation from graveyard.
2. Add a `GraveyardActivation` variant to the cost or a separate pathway.
3. Ensure `can_pay_cost_components` handles the new zone requirement.
4. Handle "until end of turn" and "exile at EOT" for Unearth via `DelayedTrigger`.

### Debugging a Rules Bug

1. Write a failing test in `tests/cr_examples.rs` first.
2. Enable DEBUG logging: `RUST_LOG=debug cargo run -- demo 2>&1 | grep -i "relevant keyword"`.
3. The `GameState` is fully serializable — print it with `serde_json::to_string_pretty(&state)` at any point.
4. SBA bugs: add a `tracing::debug!` call in `find_sbas` before returning.
5. Trigger bugs: add a debug print in the relevant collector function.

---

## 13. Known Limitations and Bugs

### Critical / Rules-Breaking

**1. `EffectStep::DealDamage` doesn't propagate source keywords** (`docs/design-notes-effectstep-damage.md`):
Lifelink, Deathtouch, Wither, Infect, and Toxic N are not applied when a non-combat damage ability resolves. Combat damage is a separate code path and works correctly. `inject_source_flags` is implemented for stack push time, but not yet called at all DealDamage construction sites (spell abilities parsed from oracle text in oracle.rs cannot receive flags at parse time). The full fix requires injecting at stack-push time for every ActivatedAbility and TriggeredAbility push site.

**2. Cleanup step doesn't discard to hand size** (CR 402.2):
`cleanup_step` in `turn.rs` clears damage/boost state but does not check hand size and prompt discards. No player can currently accumulate more than 7 cards in a normal game, but any draw effect that gives extra cards without discard is incorrect.

**3. `can_pay_cost_components` always returns true for mana**:
The structural feasibility check in `costs.rs` intentionally defers mana affordability to actual payment. But `serve.rs` uses `can_pay_cost_components` to filter available actions. X-cost activated abilities therefore appear available regardless of pool size. Thread `x_value` through the check when X-cost abilities are added.

**4. Trigger condition checks ignore continuous effects** (`triggered.rs`):
`trigger_condition_satisfied` passes `bonus = 0` to `effective_power`. An anthem (+1/+1 to all) that pushes a creature's power above another's won't affect whether Evolve fires. This is correct for many cases but wrong for global P/T modification (CR 611.3a consideration).

### Architectural

**5. Split trigger system** (`docs/design-notes-trigger-architecture.md`):
Complex triggered keywords (Exalted, Ward, Prowess, etc.) live in hardcoded collector functions instead of the typed `TriggerEvent`/`TriggeredAbility` system. The `TriggerEvent` enum needs expansion before this can be unified.

**6. No multiplayer support**:
`opponent_of` panics if there are not exactly two players. All game setup hardcodes two players.

**7. No library-order tracking for individual cards**:
Libraries are `Vec<ObjectId>` with top-of-library = last element. Mill, shuffle, library search are not fully implemented. There's no handle on "the 4th card from the top."

### Unimplemented Mechanics (Parsed but Not Enforced)

See `docs/todo.md` for the full list. Summary of major gaps:

**Zone-change activations**: Scavenge, Unearth, Flashback, Escape, Dredge, Delve — all require graveyard-zone activation framework.

**Exile-zone mechanics**: Suspend, Cascade — require exile zone addressability and per-card state.

**Alternative casting**: Morph, Bestow, Emerge, Mutate — require face-down state and alternative cost frameworks.

**Combat extensions**: Provoke (must-block targeting), Annihilator N (sacrifice on attack), Convoke (tap-to-pay), Crew N (Vehicle animation).

**Soulbond**: Requires `paired_with: Option<ObjectId>` on `PermanentState`.

**Extort**: Requires optional-payment triggered ability and `EffectStep::LoseLife`.

### UI / Serve

**8. No undo beyond mana reset**:
`reset_mana` undoes land taps within a priority window. There is no broader undo for non-mana actions (casting, attacking, etc.).

**9. Dead code in library re-exports**:
There's no automated check that all `pub` exports from the library are consumed by the binary. Low risk but worth a periodic audit.

---

## 14. Testing Approach

Tests live in `tests/`. All use Rust's standard integration test framework (separate from the library, so they only use public API).

**`tests/scripted_game.rs`**: Full game sequences — set up a GameState manually, perform a series of legal actions (cast, pass, resolve, etc.), assert the resulting state. Good for regression testing game flows.

**`tests/targeting.rs`**: Legality checks for `is_legal_target` — create a game with specific permanents and verify that shroud, hexproof, protection, etc. block targeting correctly.

**`tests/cr_examples.rs`**: Rule-by-rule validation. Each test corresponds to a specific CR rule and verifies the engine's behavior matches it. When adding a new mechanic, add a test here first (TDD approach recommended).

**Running tests:**
```bash
# Default — just the summary
cargo test 2>&1 | grep -E "^test result|FAILED|error\["

# Full output for debugging failures
cargo test 2>&1 | tee /tmp/test_out.txt; tail -100 /tmp/test_out.txt
```

**Linting:** Before finalizing any change:
```bash
cargo clippy --fix --all-targets
cargo clippy --all-targets  # verify clean
```

**Specific rules references:** Before writing any CR reference number in code or docs, grep-verify it:
```bash
grep '^303\.4g\.' docs/CR.txt
```

Format: `(303.4g)` — no "CR" prefix unless genuinely ambiguous.
