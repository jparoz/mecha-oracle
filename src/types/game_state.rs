use super::card_object::CardObject;
use super::ids::{ObjectId, PlayerId};
use super::mana::ManaPool;
use super::permanent::PermanentState;
use super::player::Player;
use super::stack::{StackId, StackObject};
use std::collections::{HashMap, VecDeque};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Phase {
    Beginning,
    PreCombatMain,
    Combat,
    PostCombatMain,
    Ending,
}

/// A single position in the turn sequence. Each variant maps to exactly one valid
/// (phase, step) combination, so invalid combinations are unrepresentable.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Step {
    // Beginning phase
    Untap,
    Upkeep,
    Draw,
    // Main phases — two separate steps instead of a shared `Main`
    PreCombatMain,
    PostCombatMain,
    // Combat phase
    BeginningOfCombat,
    DeclareAttackers,
    DeclareBlockers,
    CombatDamage,
    EndOfCombat,
    // Ending phase
    End,
    Cleanup,
}

impl Step {
    pub fn phase(self) -> Phase {
        match self {
            Step::Untap | Step::Upkeep | Step::Draw => Phase::Beginning,
            Step::PreCombatMain => Phase::PreCombatMain,
            Step::BeginningOfCombat
            | Step::DeclareAttackers
            | Step::DeclareBlockers
            | Step::CombatDamage
            | Step::EndOfCombat => Phase::Combat,
            Step::PostCombatMain => Phase::PostCombatMain,
            Step::End | Step::Cleanup => Phase::Ending,
        }
    }
}

#[derive(Debug, Clone)]
pub struct ManaCheckpoint {
    /// Mana pool state for every player at the moment the first mana tap was made.
    pub pools: HashMap<PlayerId, ManaPool>,
    /// Lands tapped for mana since the checkpoint was created, in tap order.
    pub tapped_lands: Vec<ObjectId>,
}

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
}

impl GameState {
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
        }
    }

    pub fn step(&self) -> Step {
        self.step
    }

    pub fn phase(&self) -> Phase {
        self.step.phase()
    }

    pub fn alloc_id(&mut self) -> ObjectId {
        let id = ObjectId(self.next_object_id);
        self.next_object_id += 1;
        id
    }

    pub fn alloc_stack_id(&mut self) -> StackId {
        let id = StackId(self.next_stack_id);
        self.next_stack_id += 1;
        id
    }

    pub fn add_object(&mut self, obj: CardObject) {
        self.objects.insert(obj.id, obj);
    }

    pub fn get_player(&self, id: PlayerId) -> Option<&Player> {
        self.players.iter().find(|p| p.id == id)
    }

    pub fn get_player_mut(&mut self, id: PlayerId) -> Option<&mut Player> {
        self.players.iter_mut().find(|p| p.id == id)
    }

    pub fn opponent_of(&self, player: PlayerId) -> PlayerId {
        self.players
            .iter()
            .find(|p| p.id != player)
            .expect("opponent not found")
            .id
    }

    pub fn is_game_over(&self) -> bool {
        self.game_over || self.players.iter().any(|p| p.has_lost)
    }

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
}
