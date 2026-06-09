# Meta issues
- Can we run dead code analysis on library re-exports? That is, check that all library exports are actually used in the binary?
- Make the UI layer in Javascript as thin as possible; have it query the server for a list of allowed actions, and populate the UI with the provided actions. Similar for valid targets, mana costs, etc. The web UI should be literally just a bunch of buttons which are connected to server API calls; there should be no validation done in JS, only in Rust.
- Consider renaming `Ability` (in `types/ability.rs`) to `Rule` or `RulesText` — it looks odd to see `Ability::SpellEffect` as a variant on a type named `Ability`, even though it's the right place structurally.

# UI issues
- Tooltips are displayed behind the middle row (with the turn steps); tooltips should always be in front of everything else.
- When there's not enough horizontal space to fit all the turn steps, the turn step bar wraps weirdly; probably should prefer to shrink the text (maybe truncate to the initials of each step or something), or else wrap in rows (so that a single step isn't isolated from the rest, but they're split 50/50 or something).

# Gameplay issues
- On each main phase of each player's turn, the player is still allowed to play lands/creatures/etc. after they've passed priority. This is both a UI issue (the options shouldn't be visible after passing priority), and a server rules issue (the commands to play a land or cast a spell should be forbidden). The server needs to check who has priority before playing lands/casting spells.
- I can still use spacebar to advance the steps after the game has finished; this should be forbidden.
- Players should discard to hand size at the cleanup step (see 402.2)
- Cards in hand are marked as summoning sick (maybe just a UI issue?)



# Parsed but unimplemented keyword abilities
Keywords below are parsed and shown cyan+underlined in the UI but have no rules enforcement yet.
Grouped roughly by complexity of implementation.

## Evasion / blocking restrictions (combat, moderate)
- **Fear** (702.36): unblockable except by artifact/black — requires color tracking
- **Intimidate** (702.13): unblockable except by artifact/same-color — requires color tracking
- **Landwalk variants** (702.14): unblockable if defending player controls matching land — requires land type lookup
- **Protection from X** (702.16): comprehensive (can't be targeted, blocked, enchanted, damaged by X) — requires targeting + color systems
- **Shroud** (702.18) / **Hexproof** (702.11): can't be targeted — requires targeting system
- **Ward** (702.21): targeting tax — requires targeting system

## Static creature modifiers (engine, low-to-moderate effort)
- **Infect** (702.90): deals damage as -1/-1 counters / poison counters — requires counter system
- **Wither** (702.80): deals damage as -1/-1 counters — requires counter system
- **Toxic N** (702.164): gives poison counters on combat damage — requires poison counter tracking

## Combat triggers (triggered ability system required)
- **Exalted** (702.83): attacking alone gives +1/+1 until end of turn
- **Flanking** (702.25): blockers without flanking get -1/-1 until end of turn
- **Battle Cry** (702.91): attacking creatures get +1/+0
- **Melee** (702.121): gets +1/+1 for each opponent attacked
- **Provoke** (702.39): force a creature to block
- **Annihilator N** (702.86): sacrifice N permanents when attacker attacks
- **Bushido N** (702.45): +N/+N when blocked or blocking

## Graveyard / recursion (zone change + timing systems required)
- **Persist** (702.79): returns from graveyard with -1/-1 counter if no -1/-1 counter
- **Undying** (702.93): returns from graveyard with +1/+1 counter if no +1/+1 counter
- **Unearth [cost]** (702.84): activated — return from graveyard temporarily
- **Scavenge [cost]** (702.97): activated — exile from graveyard to put +1/+1 counters
- **Escape** (702.138): cast from graveyard by exiling other cards
- **Flashback [cost]** (702.34): cast from graveyard
- **Dredge N** (702.52): replace draw with mill N, return this card

## Alternative casting (casting system extensions)
- **Morph [cost]** (702.37): cast face-down as 2/2 for {3}
- **Kicker [cost]** (702.33): additional optional cost
- **Cycling [cost]** (702.29): discard to draw a card
- **Dash [cost]** (702.109): alternative cost, returns at end of turn
- **Evoke [cost]** (702.74): alternative cost, sacrifice on ETB
- **Bestow [cost]** (702.103): cast as Aura or creature
- **Emerge [cost]** (702.119): sacrifice creature to reduce cost
- **Mutate [cost]** (702.140): merge onto existing creature
- **Suspend N—[cost]** (702.62): exile with time counters, cast when last removed
- **Foretell [cost]** (702.143): exile face-down, cast later for reduced cost

## Other triggered / activated (triggered ability system required)
- **Prowess** (702.108): +1/+1 until end of turn when you cast non-creature spell
- **Evolve** (702.100): put +1/+1 counter when creature with greater power/toughness enters
- **Training** (702.149): put +1/+1 counter when attacking with greater-power creature
- **Soulbond** (702.95): pair with another creature for mutual ability
- **Extort** (702.101): pay {W/B} when you cast a spell to drain each opponent
- **Cascade** (702.85): exile cards until you find a cheaper one, cast it free
- **Convoke** (702.51): tap creatures to pay for spells
- **Delve** (702.66): exile cards from graveyard to pay generic mana

## Vehicle / artifact mechanics
- **Crew N** (702.122): tap creatures with total power N to turn Vehicle into creature
