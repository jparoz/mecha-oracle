/// The game zone a card object occupies (CR 400.1).
/// `GameState` uses this field on `CardObject` to track where each card is.
/// The `Battlefield` zone is additionally indexed by `GameState.battlefield`
/// which holds the associated `PermanentState`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Zone {
    Library,
    Hand,
    Battlefield,
    Graveyard,
    Stack,
    Exile,
    Command,
}

/// Determines whose player-specific zone is used as the destination in a `MoveZone` step,
/// or who controls a permanent entering the battlefield.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ZoneOwner {
    /// The card's original owner (the player who started the game with it).
    CardOwner,
    /// The card's controller at the time of the zone change.
    CardController,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn zone_owner_is_copy() {
        let a = ZoneOwner::CardOwner;
        let b = a; // Copy
        assert_eq!(a, b);
        assert_ne!(ZoneOwner::CardOwner, ZoneOwner::CardController);
    }
}
