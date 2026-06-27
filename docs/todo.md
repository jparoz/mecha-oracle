# General issues
- Can we run dead code analysis on library re-exports? That is, check that all library exports are actually used in the binary?
- Players should discard to hand size at the cleanup step (see CR 402.2)
- `can_pay_cost_components` (costs.rs) always returns true for mana (by design), so the serve.rs UI action filter will show X-cost activated abilities as available regardless of pool size. When X-cost activated abilities are added, thread `x_value` into `can_pay_cost_components` and the serve.rs call site (line 606).
- Enchantments aren't displayed in the UI (probably not artifacts either)
- Battlegrowth has yellow text, but is correctly enforced.


# Parsed but unimplemented keyword abilities
Keywords below are parsed and shown cyan+underlined in the UI but have no rules enforcement yet.

---

## ✅ Unblocked — implementable now

*(Ward, Landwalk, Battle Cry, Fear, Intimidate, Protection from color, Hexproof from color, and all Protection from X variants — all implemented.)*

---

## ⚰️  Zone-change and non-battlefield activations block

*(Zone-change: MoveZone effect step implemented June 2026. Persist and Undying implemented June 2026. Counter system (Wither/Infect/Toxic/Evolve/Training) implemented June 2026.)*
*(Cycling already works as a special case in `engine/cycling.rs` — the framework below generalises that.)*

Remaining prerequisites: graveyard contents addressable by `ObjectId`, a general activated-ability-from-other-zones framework in `engine/activated.rs` (currently battlefield-only), and "dies" trigger event preserved across zone changes.

### Graveyard activations / cast-from-graveyard
- **Scavenge [cost]** (702.97): activated — exile from graveyard, put +1/+1 counters on a creature.
- **Unearth [cost]** (702.84): activated — return from graveyard temporarily (exile at EOT or if it would leave the battlefield).
- **Flashback [cost]** (702.34): cast from graveyard for the Flashback cost.
- **Escape [cost]** (702.138): cast from graveyard by exiling N other cards.
- **Dredge N** (702.52): replace a draw with "mill N, return this card".
- **Delve** (702.66): exile cards from graveyard to pay generic mana when casting.

### Exile-zone activations
- **Suspend N—[cost]** (702.62): exile with N time counters; cast when last counter is removed.
- **Cascade** (702.85): exile cards off top until a cheaper one is found, cast it free.

### Hand activations
- **Foretell [cost]** (702.143): exile face-down during your turn; cast later for reduced cost.

---

## 🃏 Alternative casting block

Requires a framework for alternative costs / face-down casting at the stack level.
Currently `casting.rs` only handles standard `ManaCost` payment.

- **Morph [cost]** (702.37): cast face-down as a 2/2 for {3}; turn face-up later.
- **Bestow [cost]** (702.103): cast as Aura or as a creature depending on targets.
- **Emerge [cost]** (702.119): sacrifice a creature to reduce the mana cost.
- **Mutate [cost]** (702.140): merge onto existing creature.

---

## ⚙️  Combat / effect extensions (moderate, each needs a targeted addition)

- **Provoke** (702.39): when this attacks, target creature must block it if able. Needs a
  `must_block: Vec<(ObjectId, ObjectId)>` field on `CombatState` enforced in
  `declare_blockers`, plus a targeted attack trigger.
- **Annihilator N** (702.86): when this attacks, defending player sacrifices N permanents.
  Needs `EffectStep::SacrificeN(u32, PermanentFilter)` (sacrifice already exists as a
  `CostComponent` — this reuses that concept as an effect).
- **Extort** (702.101): "whenever you cast a spell, you may pay {W/B}; if you do, each
  opponent loses 1 life and you gain that much life." Needs an optional-payment triggered
  ability variant and `EffectStep::LoseLife`.
- **Convoke** (702.51): tap creatures to pay generic/colored mana when casting. Needs a
  cost-reduction hook in `casting.rs` before mana is drawn from the pool.
- **Crew N** (702.122): tap creatures totalling power ≥ N to animate a Vehicle until EOT.
  Needs a "Vehicle" subtype check, `EffectStep::BecomeCreature`, and an activated ability
  evaluated outside the normal mana path.

---

## 🧩 Complex / bespoke (no shared prerequisite; each needs its own state)

- **Soulbond** (702.95): pair with another creature when either ETBs; both gain an ability
  while paired. Needs a `paired_with: Option<ObjectId>` field on `PermanentState` and
  pair/unpair logic on zone changes.

---

# Road to 99% card coverage (no ParsedUnimplemented)

The items below are ordered roughly by leverage — the first few blocks affect every card
with any effect text; later blocks address progressively narrower keyword mechanics.

## Block 1 — Effect step coverage (highest leverage)

`try_parse_effect_step` only handles: `Add {mana}`, draw N cards, mill N, gain N life,
`gets +N/+M until end of turn`, and `deals N damage`. Everything else becomes
`EffectStep::Unimplemented`, blocking resolution of virtually all activated abilities and
ETB triggers. Missing patterns:

- Destroy / exile target — "Destroy target creature", "Exile target permanent"
- Return to hand — "Return target creature to its owner's hand"
- Create tokens — "Create a 1/1 green Elf creature token"
- Lose life — "Each opponent loses 1 life" / "target player loses N life"
- Discard — "Target player discards a card"
- Search library / tutor — "Search your library for a card"
- Tap / untap target permanent
- Generalized counter placement on a target (currently only AddCounter on source)

## Block 2 — Activated ability cost parsing

`parse_activation_cost` only handles `{T}` and mana costs; everything else becomes
`CostComponent::Unimplemented`. Common costs that currently fall through:

- Sacrifice a [permanent type]: "Sacrifice a creature:", "Sacrifice ~:"
- Pay life: "Pay 2 life:"
- Discard a card
- Remove a counter from ~

## Block 3 — Triggered ability parser breadth

Only ETB (self-referential), Ward, and a handful of combat triggers are parsed.
Missing trigger patterns:

- "Whenever you cast a spell" / "Whenever you draw a card" (very common)
- "At the beginning of [step]" for steps other than upkeep (draw, combat, end)
- "Whenever ~ attacks/blocks" with arbitrary effects
- "When ~ dies" — triggers on graveyard zone change; extremely common
- "Whenever a [creature type/subtype] enters" — tribal synergies
- "Whenever ~ deals combat damage to a player"

## Block 4 — Graveyard / non-battlefield activated abilities

`engine/activated.rs` only handles battlefield activations. Entire class of graveyard
activations is unimplemented as a framework, blocking:
Scavenge, Unearth, Flashback, Escape, Dredge, Delve, Retrace, Jump-start.

## Block 5 — Alternative and additional casting costs

`casting.rs` handles only standard mana cost payment. No framework for:

- Alternative costs (Morph, Bestow, Overload, Surge, Spectacle, etc.)
- Additional costs — Kicker/Multikicker are parsed but the engine doesn't yet collect
  the extra mana at cast time
- Cost reductions (Convoke, Improvise, Affinity, Emerge, Delve)

## Block 6 — Continuous effect coverage

`try_parse_continuous_pt_effect` covers only three patterns. Missing:

- "Creatures you control have [keyword]" — granting evasion en masse
- "~ has [keyword]" — self-granting static abilities
- "[Subtype] creatures you control have/get…"
- Anthem effects beyond simple P/T modification (e.g. "have haste", "can't be blocked")

## Block 7 — Counter-based keyword mechanics

Counter infrastructure exists but the following keywords have no parsing→execution path:

- Modular N — ETB with N +1/+1 counters; dies → move counters to target artifact creature
- Graft N — ETB with N +1/+1 counters; other creature ETB → move one counter to it
- Bloodthirst N — ETB with N +1/+1 counters if an opponent was dealt damage this turn
- Fabricate N — ETB choice: N +1/+1 counters or N 1/1 Servo tokens
- Level up — activated ability adding level counters, granting P/T and abilities per level

## Block 8 — Combat keyword mechanics (targeted engine additions)

- Provoke (702.39) — `must_block` field on `CombatState`; target creature must block ~
- Annihilator N (702.86) — defending player sacrifices N permanents on attack trigger
- Extort (702.101) — optional payment triggered ability + `EffectStep::LoseLife`
- Crew N (702.122) — Vehicle animation activated ability outside the normal mana path

## Block 9 — Zone-change and lifecycle mechanics

- Suspend N — exile with N time counters; upkeep trigger removes one; cast when last is removed
- Cascade (702.85) — exile cards off top until a cheaper one is found, cast it free
- Madness [cost] — redirect discard to exile, allow cast from exile for alternative cost
- Rebound (702.88) — exile on resolution, cast again from exile at next upkeep

## Block 10 — Static ability engine coverage for parsed-but-ignored keywords

Several keywords are `ParsedUnimplemented` despite having clear static rules:

- Changeling — permanent has all creature types at all times (702.73)
- Devoid — permanent is colorless regardless of mana cost (702.114)
- Partner / Partner with — commander-zone rules; appears on many non-commander cards too

---
