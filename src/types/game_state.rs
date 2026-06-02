use super::card_object::CardObject;
use super::ids::{ObjectId, PlayerId};
use super::player::Player;
use std::collections::HashMap;

#[derive(Debug, Clone, PartialEq)]
pub enum Phase {
    Beginning,
    PreCombatMain,
    Combat,
    PostCombatMain,
    Ending,
}

#[derive(Debug, Clone, PartialEq)]
pub enum Step {
    Untap,
    Upkeep,
    Draw,
    Main,
    BeginningOfCombat,
    DeclareAttackers,
    DeclareBlockers,
    CombatDamage,
    EndOfCombat,
    End,
    Cleanup,
}

#[derive(Debug, Clone)]
pub struct CombatState {
    /// Creatures declared as attackers this combat.
    pub attackers: Vec<ObjectId>,
    /// attacker_id → list of blockers in damage-assignment order.
    pub blocking_map: HashMap<ObjectId, Vec<ObjectId>>,
}

impl CombatState {
    pub fn empty() -> Self {
        Self {
            attackers: vec![],
            blocking_map: HashMap::new(),
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
    pub battlefield: Vec<ObjectId>,
    pub stack: Vec<ObjectId>,
    pub exile: Vec<ObjectId>,
    pub active_player: PlayerId,
    pub priority_player: PlayerId,
    pub phase: Phase,
    pub step: Step,
    pub turn_number: u32,
    pub lands_played_this_turn: u32,
    pub combat: CombatState,
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
            battlefield: vec![],
            stack: vec![],
            exile: vec![],
            active_player: active,
            priority_player: active,
            phase: Phase::Beginning,
            step: Step::Untap,
            turn_number: 1,
            lands_played_this_turn: 0,
            combat: CombatState::empty(),
            next_object_id: 1,
            game_over: false,
        }
    }

    pub fn alloc_id(&mut self) -> ObjectId {
        let id = ObjectId(self.next_object_id);
        self.next_object_id += 1;
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
        assert_eq!(gs.phase, Phase::Beginning);
        assert_eq!(gs.step, Step::Untap);
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
}
