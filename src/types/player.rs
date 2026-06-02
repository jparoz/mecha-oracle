use super::ids::PlayerId;
use super::mana::ManaPool;

#[derive(Debug, Clone)]
pub struct Player {
    pub id: PlayerId,
    pub name: String,
    pub life: i32,
    pub mana_pool: ManaPool,
    pub has_lost: bool,
}

impl Player {
    pub fn new(id: PlayerId, name: impl Into<String>) -> Self {
        Self {
            id,
            name: name.into(),
            life: 20,
            mana_pool: ManaPool::default(),
            has_lost: false,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn player_starts_at_20_life() {
        let p = Player::new(PlayerId(0), "Alice");
        assert_eq!(p.life, 20);
        assert!(!p.has_lost);
        assert!(p.mana_pool.is_empty());
    }
}
