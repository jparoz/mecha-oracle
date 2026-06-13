// Ward payment — see pay_ward in Task 8

use super::EngineError;
use crate::types::stack::StackId;
use crate::types::{GameState, PlayerId};

pub fn pay_ward(
    _state: GameState,
    _player_id: PlayerId,
    _trigger_id: StackId,
) -> Result<GameState, EngineError> {
    unimplemented!("pay_ward: implemented in Task 8")
}
