# General issues
- Can we run dead code analysis on library re-exports? That is, check that all library exports are actually used in the binary?
- Players should discard to hand size at the cleanup step (see CR 402.2)
- `can_pay_cost_components` (costs.rs) always returns true for mana (by design), so the serve.rs UI action filter will show X-cost activated abilities as available regardless of pool size. When X-cost activated abilities are added, thread `x_value` into `can_pay_cost_components` and the serve.rs call site (line 606).
- SpellEffect actually should be called SpellAbility, that's what it's called when there's text which happens when an instant or sorcery is resolving. (CR 113.3.)
- `StaticAbility` (types/ability.rs) is misnamed — per CR 113.3, many of its variants are triggered abilities, not static abilities: Exalted (CR 702.83a), Flanking (CR 702.25a), BushidoN (CR 702.45a), Melee (CR 702.121a), Prowess (CR 702.108a), BattleCry (CR 702.91a), Evolve (CR 702.100a), Training (CR 702.149a), Persist (CR 702.79a), Undying (CR 702.93a). Decayed (CR 702.147a) is a mix of static ("can't block") and triggered ("sacrifice at end of combat"). The enum should be renamed to `KeywordAbility` or similar to reflect that it stores CR 702 keyword markers regardless of rules category.


# Parsed but unimplemented keyword abilities
Keywords below are parsed and shown cyan+underlined in the UI but have no rules enforcement yet.

---

## ✅ Unblocked — implementable now

*(Ward, Landwalk, Battle Cry, Fear, Intimidate, Protection from color — all implemented.)*

### Protection from X — partial (blocking + targeting by color only)

- **Damage prevention (D in DEBT)**: prevent all damage from sources with protected quality — requires a "protection check" in the combat damage path and the DealDamage effect step.
- **Enchant/Equip prevention (E in DEBT)**: can't be enchanted or equipped by things with protected quality — requires aura attachment rules (future work).
- **Protection from non-color qualities**: protection from artifacts, from instants, from a specific creature type, from a card name (e.g. "protection from Eldrazi") — each needs a richer `ProtectionQuality` enum beyond just `ManaColor`.
- **Protection from everything** (CR 702.16e): shorthand for all qualities — needs `StaticAbility::ProtectionFromAll`.
- **Hexproof from color** (CR 702.11e, e.g. "hexproof from black") — related, but a separate keyword; currently ParsedUnimplemented.

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
- **Kicker [cost]** (702.33): optional additional cost with conditional extra effect.
- **Dash [cost]** (702.109): alternative cost; return to hand at end of combat.
- **Evoke [cost]** (702.74): alternative cost; sacrifice on ETB.
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
