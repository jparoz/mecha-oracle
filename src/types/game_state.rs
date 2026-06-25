use super::ability::Cost;
use super::card_object::CardObject;
use super::effect::{Effect, EffectTarget};
use super::ids::{ObjectId, PlayerId};
use super::mana::ManaPool;
use super::permanent::PermanentState;
use super::player::Player;
use super::stack::{StackId, StackObject};
use std::collections::{HashMap, VecDeque};

// Re-export so callers that imported Phase/Step from game_state continue to work.
pub use super::step::{Phase, Step};

/// Snapshot of mana pools and tapped lands taken when the first land is tapped for mana
/// in a priority window. Allows `reset_mana` to undo all taps within that window.
/// Cleared by `advance_step` at the end of every phase/step.
#[derive(Debug, Clone)]
pub struct ManaCheckpoint {
    /// Mana pool state for every player at the moment the first mana tap was made.
    pub pools: HashMap<PlayerId, ManaPool>,
    /// Lands tapped for mana since the checkpoint was created, in tap order.
    pub tapped_lands: Vec<ObjectId>,
}

/// All mutable state related to the current combat phase (CR 506–510).
/// Cleared at the start of each combat by `advance_step` moving through `BeginningOfCombat`.
#[derive(Debug, Clone)]
pub struct CombatState {
    /// Creatures declared as attackers this combat.
    pub attackers: Vec<ObjectId>,
    /// attacker_id → list of blockers in damage-assignment order.
    pub blocking_map: HashMap<ObjectId, Vec<ObjectId>>,
    /// True after the first-strike combat damage round has resolved (CR 510.4).
    pub first_strike_done: bool,
    /// True if attackers have been declared (even if empty).
    pub attackers_declared: bool,
    /// True if blockers have been declared (even if empty).
    pub blockers_declared: bool,
}

impl CombatState {
    /// Returns a `CombatState` with no attackers, no blockers, and all flags cleared.
    pub fn empty() -> Self {
        Self {
            attackers: vec![],
            blocking_map: HashMap::new(),
            first_strike_done: false,
            attackers_declared: false,
            blockers_declared: false,
        }
    }
}

/// A one-shot trigger registered by an engine effect (e.g. Dash — CR 702.109a).
// Fires at the start of the matching step; drained from the list after firing.
#[derive(Debug, Clone)]
pub struct DelayedTrigger {
    /// The step at which this trigger fires.
    pub fires_on_step: Step,
    /// The effect to execute when the trigger resolves.
    pub effect: crate::types::effect::Effect,
    /// Pre-declared targets for the effect.
    pub targets: Vec<crate::types::effect::EffectTarget>,
    /// The player who controls this trigger.
    pub controller: PlayerId,
}

/// CR 118.12: an inline cost-payment obligation raised during the resolution
/// of a spell or ability. Set by `EffectStep::Payment`; cleared by
/// `pay_pending_cost` or `decline_pending_cost`.
#[derive(Debug, Clone)]
pub struct PendingPayment {
    /// The player who must pay or decline.
    pub paying_player: PlayerId,
    pub cost: Cost,
    /// Steps to execute if the player pays (often empty).
    pub on_paid: Effect,
    /// Steps to execute if the player declines (e.g. `CounterSpell`).
    pub on_declined: Effect,
    /// Steps after the payment decision that always run (for future use).
    pub continuation: Effect,
    /// Targets from the resolving stack object; passed to on_paid/on_declined.
    pub targets: Vec<EffectTarget>,
    /// Controller of the spell/ability containing the Payment step.
    pub controller: PlayerId,
}

/// The complete, canonical game state. All engine functions take ownership of this struct
/// and return a (possibly modified) copy — there is no interior mutability.
///
/// Zones are represented as:
/// - `objects`: all cards by id (any zone)
/// - `libraries`, `hands`, `graveyards`: player-keyed ordered vecs of ids
/// - `battlefield`: id → `PermanentState` (battlefield only)
/// - `stack`/`stack_objects`: ordered vec of ids + id-keyed lookup
/// - `exile`: unordered vec of ids
#[derive(Debug, Clone)]
pub struct GameState {
    /// All card objects that exist in the game, keyed by their unique id.
    pub objects: HashMap<ObjectId, CardObject>,
    pub players: Vec<Player>,
    pub libraries: HashMap<PlayerId, Vec<ObjectId>>,
    pub hands: HashMap<PlayerId, Vec<ObjectId>>,
    pub graveyards: HashMap<PlayerId, Vec<ObjectId>>,
    pub battlefield: HashMap<ObjectId, PermanentState>,
    pub stack: Vec<StackId>,
    pub stack_objects: HashMap<StackId, StackObject>,
    pub next_stack_id: u64,
    pub consecutive_passes: u32,
    pub exile: Vec<ObjectId>,
    pub active_player: PlayerId,
    pub priority_player: PlayerId,
    pub(crate) step: Step,
    pub turn_number: u32,
    pub lands_played_this_turn: u32,
    pub combat: CombatState,
    pub mana_checkpoint: Option<ManaCheckpoint>,
    /// Extra steps queued for dynamic insertion (e.g. second combat damage step per CR 510.4,
    /// or extra combat phases from card effects). `advance_step` pops from this before
    /// following the static turn sequence.
    pub(crate) extra_steps: VecDeque<Step>,
    pub next_object_id: u64,
    pub game_over: bool,
    pub pending_payment: Option<PendingPayment>,
    pub delayed_triggers: Vec<DelayedTrigger>,
}

impl GameState {
    /// Constructs an initial game state for the given players.
    /// Starts at turn 1, step Untap, with player 0 as active and priority player.
    /// Libraries, hands, and graveyards are initialised as empty; cards must be added via `add_object`.
    pub fn new(players: Vec<Player>) -> Self {
        assert!(!players.is_empty());
        let active = players[0].id;
        let mut libraries = HashMap::new();
        let mut hands = HashMap::new();
        let mut graveyards = HashMap::new();
        for p in &players {
            libraries.insert(p.id, vec![]);
            hands.insert(p.id, vec![]);
            graveyards.insert(p.id, vec![]);
        }
        Self {
            objects: HashMap::new(),
            players,
            libraries,
            hands,
            graveyards,
            battlefield: HashMap::new(),
            stack: vec![],
            stack_objects: HashMap::new(),
            next_stack_id: 1,
            consecutive_passes: 0,
            exile: vec![],
            active_player: active,
            priority_player: active,
            step: Step::Untap,
            turn_number: 1,
            lands_played_this_turn: 0,
            combat: CombatState::empty(),
            mana_checkpoint: None,
            extra_steps: VecDeque::new(),
            next_object_id: 1,
            game_over: false,
            pending_payment: None,
            delayed_triggers: vec![],
        }
    }

    /// Returns the current step. The `step` field is `pub(crate)`; use this accessor externally.
    pub fn step(&self) -> Step {
        self.step
    }

    /// Returns the phase of the current step. Convenience wrapper around `Step::phase`.
    pub fn phase(&self) -> Phase {
        self.step.phase()
    }

    /// Allocates a monotonically increasing `ObjectId`. IDs are never reused within a game.
    pub fn alloc_id(&mut self) -> ObjectId {
        let id = ObjectId(self.next_object_id);
        self.next_object_id += 1;
        id
    }

    /// Allocates a monotonically increasing `StackId` for a new stack object.
    pub fn alloc_stack_id(&mut self) -> StackId {
        let id = StackId(self.next_stack_id);
        self.next_stack_id += 1;
        id
    }

    /// Inserts a card object into the `objects` map. Does not modify any zone list —
    /// the caller must also push `obj.id` to the appropriate zone vec (library, hand, etc.).
    pub fn add_object(&mut self, obj: CardObject) {
        self.objects.insert(obj.id, obj);
    }

    /// Returns a shared reference to the player with `id`, or `None` if not found.
    pub fn get_player(&self, id: PlayerId) -> Option<&Player> {
        self.players.iter().find(|p| p.id == id)
    }

    /// Returns a mutable reference to the player with `id`, or `None` if not found.
    pub fn get_player_mut(&mut self, id: PlayerId) -> Option<&mut Player> {
        self.players.iter_mut().find(|p| p.id == id)
    }

    /// CR 302.6 — the turn number at which `controller`'s most recent untap step occurred.
    /// Used to evaluate summoning sickness: a permanent is sick if `controller_since_turn >=
    /// controllers_most_recent_turn(controller)`.
    pub fn controllers_most_recent_turn(&self, controller: PlayerId) -> u32 {
        let n = self.players.len() as u32;
        let active_idx = self
            .players
            .iter()
            .position(|p| p.id == self.active_player)
            .unwrap_or(0) as u32;
        let ctrl_idx = self
            .players
            .iter()
            .position(|p| p.id == controller)
            .unwrap_or(0) as u32;
        let offset = (active_idx + n - ctrl_idx) % n;
        self.turn_number.saturating_sub(offset)
    }

    /// Returns the id of the player who is not `player`. Panics in non-two-player games.
    pub fn opponent_of(&self, player: PlayerId) -> PlayerId {
        self.players
            .iter()
            .find(|p| p.id != player)
            .expect("opponent not found")
            .id
    }

    /// Returns true if `game_over` is set or any player has `has_lost = true`.
    pub fn is_game_over(&self) -> bool {
        self.game_over || self.players.iter().any(|p| p.has_lost)
    }

    /// Returns the id of the surviving player, or `None` if the game is still in progress.
    pub fn winner(&self) -> Option<PlayerId> {
        if !self.is_game_over() {
            return None;
        }
        self.players.iter().find(|p| !p.has_lost).map(|p| p.id)
    }
}

#[cfg(test)]
mod tests {
    use super::StackId;
    use super::*;
    use crate::types::player::Player;

    fn two_player_state() -> GameState {
        GameState::new(vec![
            Player::new(PlayerId(0), "Alice"),
            Player::new(PlayerId(1), "Bob"),
        ])
    }

    #[test]
    fn new_game_starts_at_turn_1_untap() {
        let gs = two_player_state();
        assert_eq!(gs.turn_number, 1);
        assert_eq!(gs.phase(), Phase::Beginning);
        assert_eq!(gs.step(), Step::Untap);
        assert_eq!(gs.active_player, PlayerId(0));
    }

    #[test]
    fn opponent_of_returns_other_player() {
        let gs = two_player_state();
        assert_eq!(gs.opponent_of(PlayerId(0)), PlayerId(1));
        assert_eq!(gs.opponent_of(PlayerId(1)), PlayerId(0));
    }

    #[test]
    fn game_not_over_initially() {
        let gs = two_player_state();
        assert!(!gs.is_game_over());
        assert_eq!(gs.winner(), None);
    }

    #[test]
    fn winner_when_opponent_loses() {
        let mut gs = two_player_state();
        gs.get_player_mut(PlayerId(1)).unwrap().has_lost = true;
        assert!(gs.is_game_over());
        assert_eq!(gs.winner(), Some(PlayerId(0)));
    }

    #[test]
    fn alloc_id_increments() {
        let mut gs = two_player_state();
        assert_eq!(gs.alloc_id(), ObjectId(1));
        assert_eq!(gs.alloc_id(), ObjectId(2));
    }

    #[test]
    fn extra_steps_starts_empty() {
        let gs = two_player_state();
        assert!(gs.extra_steps.is_empty());
    }

    #[test]
    fn first_strike_done_starts_false() {
        let gs = two_player_state();
        assert!(!gs.combat.first_strike_done);
    }

    #[test]
    fn combat_state_empty_has_declared_flags_false() {
        let cs = CombatState::empty();
        assert!(!cs.attackers_declared);
        assert!(!cs.blockers_declared);
    }

    #[test]
    fn alloc_stack_id_increments() {
        let mut gs = two_player_state();
        assert_eq!(gs.alloc_stack_id(), StackId(1));
        assert_eq!(gs.alloc_stack_id(), StackId(2));
    }

    #[test]
    fn new_game_consecutive_passes_is_zero() {
        let gs = two_player_state();
        assert_eq!(gs.consecutive_passes, 0);
    }

    #[test]
    fn new_game_stack_objects_is_empty() {
        let gs = two_player_state();
        assert!(gs.stack_objects.is_empty());
    }

    #[test]
    fn pending_payment_starts_none() {
        let gs = two_player_state();
        assert!(gs.pending_payment.is_none());
    }
}
