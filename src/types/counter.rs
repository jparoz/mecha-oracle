/// A kind of counter, per CR 122.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum CounterKind {
    /// CR 122.1a: per-counter P/T delta. +1/+1 → { power: 1, toughness: 1 }.
    PtModifier { power: i32, toughness: i32 },
    /// CR 122.1f: poison counter placed on a player.
    Poison,
    /// Fallback for named counters with no specific rules meaning (charge, time, age, …).
    Named(String),
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    #[test]
    fn pt_modifier_equality() {
        assert_eq!(
            CounterKind::PtModifier {
                power: 1,
                toughness: 1
            },
            CounterKind::PtModifier {
                power: 1,
                toughness: 1
            }
        );
        assert_ne!(
            CounterKind::PtModifier {
                power: 1,
                toughness: 1
            },
            CounterKind::PtModifier {
                power: -1,
                toughness: -1
            }
        );
    }

    #[test]
    fn named_counter_equality() {
        assert_eq!(
            CounterKind::Named("charge".to_string()),
            CounterKind::Named("charge".to_string())
        );
        assert_ne!(
            CounterKind::Named("charge".to_string()),
            CounterKind::Named("time".to_string())
        );
    }

    #[test]
    fn counter_kind_works_as_hashmap_key() {
        let mut map: HashMap<CounterKind, u32> = HashMap::new();
        map.insert(
            CounterKind::PtModifier {
                power: 1,
                toughness: 1,
            },
            3,
        );
        map.insert(CounterKind::Poison, 2);
        map.insert(CounterKind::Named("charge".to_string()), 5);
        assert_eq!(
            map[&CounterKind::PtModifier {
                power: 1,
                toughness: 1
            }],
            3
        );
        assert_eq!(map[&CounterKind::Poison], 2);
        assert_eq!(map[&CounterKind::Named("charge".to_string())], 5);
    }
}
