// src/engine/stack.rs
use super::EngineError;
use crate::types::{GameState, PlayerId};

pub fn pass_priority(_state: GameState, _player_id: PlayerId) -> Result<GameState, EngineError> {
    todo!()
}

pub fn resolve_top(state: GameState) -> GameState {
    state
}
