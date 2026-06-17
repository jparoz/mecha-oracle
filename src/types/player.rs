use super::counter::CounterKind;
use super::ids::PlayerId;
use super::mana::ManaPool;
use std::collections::HashMap;

#[derive(Debug, Clone)]
pub struct Player {
    pub id: PlayerId,
    pub name: String,
    pub life: i32,
    pub mana_pool: ManaPool,
    pub has_lost: bool,
    /// Counters on this player (CR 122), e.g. poison (CR 122.1f).
    pub counters: HashMap<CounterKind, u32>,
}

impl Player {
    pub fn new(id: PlayerId, name: impl Into<String>) -> Self {
        Self {
            id,
            name: name.into(),
            life: 20,
            mana_pool: ManaPool::default(),
            has_lost: false,
            counters: HashMap::new(),
        }
    }

    pub fn counter_count(&self, kind: &CounterKind) -> u32 {
        *self.counters.get(kind).unwrap_or(&0)
    }

    pub fn add_counters(&mut self, kind: CounterKind, n: u32) {
        *self.counters.entry(kind).or_insert(0) += n;
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

    #[test]
    fn player_counter_count_returns_zero_for_absent_key() {
        use crate::types::CounterKind;
        let p = Player::new(PlayerId(0), "Alice");
        assert_eq!(p.counter_count(&CounterKind::Poison), 0);
        assert_eq!(
            p.counter_count(&CounterKind::PtModifier {
                power: 1,
                toughness: 1
            }),
            0
        );
    }

    #[test]
    fn player_add_counters_accumulates() {
        use crate::types::CounterKind;
        let mut p = Player::new(PlayerId(0), "Alice");
        p.add_counters(CounterKind::Poison, 4);
        p.add_counters(CounterKind::Poison, 5);
        assert_eq!(p.counter_count(&CounterKind::Poison), 9);
    }
}
