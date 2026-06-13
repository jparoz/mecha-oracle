# Meta issues
- Can we run dead code analysis on library re-exports? That is, check that all library exports are actually used in the binary?
- Consider renaming `Ability` (in `types/ability.rs`) to `Rule` or `RulesText` — it looks odd to see `Ability::SpellEffect` as a variant on a type named `Ability`, even though it's the right place structurally.

# Gameplay issues
- Players should discard to hand size at the cleanup step (see CR 402.2)



# Parsed but unimplemented keyword abilities
Keywords below are parsed and shown cyan+underlined in the UI but have no rules enforcement yet.

---

## ✅ Unblocked — implementable now

All prerequisite systems exist: targeting (`engine/targeting.rs`), triggered ability
framework (`engine/triggered.rs` — ETB, cast, attack, block patterns), combat state
(`combat.attackers`, `blocking_map`), and the full `EffectStep` / `BoostPermanentPT`
machinery.

- **Ward N** (702.21): when this becomes the target of a spell/ability, that spell/ability
  requires an additional `{N}` payment or it's countered. Hook into `is_legal_target` /
  the stack resolution path; no new systems needed.
- **Landwalk variants** (702.14): unblockable if the defending player controls a land whose
  subtype matches (Plains-walk, Island-walk, etc.). `TypeLine::subtypes` and the
  battlefield map are both available; just a predicate in `declare_blockers`.
- **Battle Cry** (702.91): when this attacks, each other attacking creature gets +1/+0 until
  EOT. Same pattern as Exalted/Melee in `collect_attack_triggers` — generate one
  `BoostPermanentPT` trigger per other attacker. Only new detail: filter the attacker list
  to exclude the Battle Cry source.

---

## 🎨 Color-tracking block (one small prerequisite unblocks three)

Add `fn colors(card: &CardDefinition) -> Vec<ManaColor>` that derives identity from
`ManaCost` pips — the pip type (`ManaPip`) already carries color. A single helper unblocks:

- **Fear** (702.36): unblockable except by artifact/black creatures — artifact check is a
  `TypeLine` lookup; black check needs `colors()`.
- **Intimidate** (702.13): unblockable except by artifact/same-color-as-attacker — same.
- **Protection from X** (702.16): full DEBT (can't be Damaged, Enchanted, Blocked, or
  Targeted by X). The targeting leg (`is_legal_target`) is already wired; blocking and
  damage legs need the `colors()` helper. The enchantment/equipment leg is future work.

---

## 🔢 Counter system block

Add `counters: HashMap<CounterKind, u32>` to `PermanentState` and `Player`
(`CounterKind` = `PlusOnePlusOne`, `MinusOneMinusOne`, `Poison`, …). Unblocks:

- **Wither** (702.80): damage dealt as -1/-1 counters instead of marked damage.
- **Infect** (702.90): damage to creatures as -1/-1 counters; damage to players as poison
  counters.
- **Toxic N** (702.164): deals N additional poison counters on combat damage to players.
- **Evolve** (702.100): put +1/+1 counter when a creature with greater power or toughness
  ETBs under your control. (ETB trigger framework already exists.)
- **Training** (702.149): put +1/+1 counter when attacking alongside a creature with
  greater power. (Attack trigger framework already exists.)
- **Persist** (702.79): return from graveyard with -1/-1 counter if no -1/-1 counter.
  (Also needs graveyard zone-change hook — see next section.)
- **Undying** (702.93): return from graveyard with +1/+1 counter if no +1/+1 counter.
  (Also needs graveyard zone-change hook.)
- **Scavenge [cost]** (702.97): activated — exile from graveyard, put +1/+1 counters on a
  creature. (Also needs graveyard zone-change hook.)

---

## ⚰️  Graveyard / zone-change block

Requires: graveyard contents addressable by `ObjectId`, a "dies" trigger event in
`TriggerEvent`, and zone-change semantics that preserve identity. Counter system also
needed for most entries here.

- **Persist** (702.79) — see Counter block above.
- **Undying** (702.93) — see Counter block above.
- **Scavenge [cost]** (702.97) — see Counter block above.
- **Unearth [cost]** (702.84): activated — return from graveyard temporarily (exile at EOT
  or if it would leave the battlefield).
- **Escape [cost]** (702.138): cast from graveyard by exiling N other cards.
- **Flashback [cost]** (702.34): cast from graveyard for the Flashback cost.
- **Dredge N** (702.52): replace a draw with "mill N, return this card".
- **Delve** (702.66): exile cards from graveyard to pay generic mana when casting.
- **Cascade** (702.85): exile cards off top until a cheaper one is found, cast it free.

---

## 🃏 Alternative casting block

Requires a framework for alternative costs / face-down casting at the stack level.
Currently `casting.rs` only handles standard `ManaCost` payment.

- **Morph [cost]** (702.37): cast face-down as a 2/2 for {3}; turn face-up later.
- **Kicker [cost]** (702.33): optional additional cost with conditional extra effect.
- **Dash [cost]** (702.109): alternative cost; return to hand at end of combat.
- **Evoke [cost]** (702.74): alternative cost; sacrifice on ETB.
- **Bestow [cost]** (702.103): cast as Aura or as a creature depending on targets.
- **Emerge [cost]** (702.119): sacrifice a creature to reduce the mana cost.
- **Mutate [cost]** (702.140): merge onto existing creature.
- **Suspend N—[cost]** (702.62): exile with N time counters; cast when last counter
  is removed.
- **Foretell [cost]** (702.143): exile face-down during your turn; cast later for
  reduced cost.

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
