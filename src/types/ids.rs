#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct ObjectId(pub u64);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
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
}
