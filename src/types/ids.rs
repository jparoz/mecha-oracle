use serde::{Deserialize, Serialize};

/// A unique identifier for a card object in the game (CR 109.1).
/// Each physical card instance — including copies on the stack — gets its own ObjectId,
/// allocated monotonically from `GameState::alloc_id`. IDs are never reused within a game.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(transparent)]
pub struct ObjectId(pub u64);

/// A unique identifier for a player.
/// In the current two-player implementation: PlayerId(0) = first player, PlayerId(1) = second.
/// Stored as a u8 and serialized as a plain JSON integer for API compactness.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(transparent)]
pub struct PlayerId(pub u8);

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    #[test]
    fn object_id_equality() {
        assert_eq!(ObjectId(1), ObjectId(1));
        assert_ne!(ObjectId(1), ObjectId(2));
    }

    #[test]
    fn object_id_ordering() {
        assert!(ObjectId(1) < ObjectId(2));
    }

    #[test]
    fn player_id_as_hash_key() {
        let mut map: HashMap<PlayerId, &str> = HashMap::new();
        map.insert(PlayerId(0), "Alice");
        map.insert(PlayerId(1), "Bob");
        assert_eq!(map[&PlayerId(0)], "Alice");
        assert_eq!(map[&PlayerId(1)], "Bob");
    }

    #[test]
    fn player_id_serializes_as_inner_u8() {
        let id = PlayerId(1);
        assert_eq!(serde_json::to_string(&id).unwrap(), "1");
    }

    #[test]
    fn object_id_serializes_as_inner_u64() {
        let id = ObjectId(42);
        assert_eq!(serde_json::to_string(&id).unwrap(), "42");
    }
}
