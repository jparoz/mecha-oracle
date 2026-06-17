use super::ability::{Ability, OracleSpan, StaticAbility};
use super::card::CardDefinition;
use super::counter::CounterKind;
use std::collections::HashMap;

/// Temporary power/toughness modification accumulated from until-end-of-turn effects
/// (e.g. Exalted, Prowess). Applied in `effective_power`/`effective_toughness` and
/// cleared to zero in `cleanup_step` (CR 514.2).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct PTDelta {
    pub power: i32,
    pub toughness: i32,
}

#[derive(Debug, Clone)]
pub struct PermanentState {
    /// Cloned from CardObject on enter-battlefield.
    /// If CardDefinition ever becomes mutable (copy effects, aura modifications, etc.)
    /// this copy will need to be kept in sync — either by re-cloning on mutation or by
    /// switching to Arc<CardDefinition>.
    pub definition: CardDefinition,
    /// Printed P/T is on the definition; this diverges once effects are applied.
    pub current_power: Option<i32>,
    pub current_toughness: Option<i32>,
    pub tapped: bool,
    /// CR 302.6 — the turn number when this permanent came under its current controller's
    /// control. Use `summoning_sick(controllers_most_recent_turn)` to check sickness;
    /// `u32::MAX` means "entered this turn" (always sick until explicitly set).
    pub controller_since_turn: u32,
    pub damage_marked: u32,
    /// CR 704.5h — flagged when deathtouch damage lands; cleared by SBAs.
    pub damaged_by_deathtouch: bool,
    pub pt_boost_until_eot: PTDelta,
    /// Counters on this permanent (CR 122).
    pub counters: HashMap<CounterKind, u32>,
}

impl PermanentState {
    pub fn new(definition: &CardDefinition) -> Self {
        Self {
            definition: definition.clone(),
            current_power: definition.power,
            current_toughness: definition.toughness,
            tapped: false,
            controller_since_turn: u32::MAX,
            damage_marked: 0,
            damaged_by_deathtouch: false,
            pt_boost_until_eot: PTDelta::default(),
            counters: HashMap::new(),
        }
    }

    pub fn has_keyword(&self, kw: StaticAbility) -> bool {
        self.definition
            .abilities
            .iter()
            .any(|span| matches!(span, OracleSpan::Parsed(Ability::Static(k)) if *k == kw))
    }

    /// Returns the Bushido parameter N if this permanent has Bushido N, otherwise None.
    pub fn bushido_n(&self) -> Option<u32> {
        self.definition.abilities.iter().find_map(|span| {
            if let OracleSpan::Parsed(Ability::Static(StaticAbility::BushidoN(n))) = span {
                Some(*n)
            } else {
                None
            }
        })
    }

    pub fn is_creature(&self) -> bool {
        self.definition.type_line.is_creature()
    }

    pub fn is_land(&self) -> bool {
        self.definition.type_line.is_land()
    }

    pub fn effective_power(&self) -> Option<i32> {
        self.current_power.map(|p| {
            let counter_bonus: i32 = self
                .counters
                .iter()
                .filter_map(|(kind, &count)| match kind {
                    CounterKind::PtModifier { power, .. } => Some(power * count as i32),
                    _ => None,
                })
                .sum();
            p + self.pt_boost_until_eot.power + counter_bonus
        })
    }

    pub fn effective_toughness(&self) -> Option<i32> {
        self.current_toughness.map(|t| {
            let counter_bonus: i32 = self
                .counters
                .iter()
                .filter_map(|(kind, &count)| match kind {
                    CounterKind::PtModifier { toughness, .. } => Some(toughness * count as i32),
                    _ => None,
                })
                .sum();
            t + self.pt_boost_until_eot.toughness + counter_bonus
        })
    }

    /// CR 302.6 — a creature is summoning sick if it has not been under its controller's
    /// control continuously since the beginning of their most recent turn.
    /// Pass `controllers_most_recent_turn` from `GameState::controllers_most_recent_turn`.
    /// Returns false for non-creatures (sickness only restricts creature abilities).
    pub fn summoning_sick(&self, controllers_most_recent_turn: u32) -> bool {
        self.is_creature() && self.controller_since_turn >= controllers_most_recent_turn
    }

    /// CR 302.5a — a creature can attack if untapped, not summoning sick (unless Haste),
    /// and not a Defender.
    pub fn can_attack(&self, controllers_most_recent_turn: u32) -> bool {
        self.is_creature()
            && !self.tapped
            && !self.has_keyword(StaticAbility::Defender)
            && (!self.summoning_sick(controllers_most_recent_turn)
                || self.has_keyword(StaticAbility::Haste))
    }

    /// CR 509.1a — a creature can block if untapped and not Decayed.
    pub fn can_block(&self) -> bool {
        self.is_creature() && !self.tapped && !self.has_keyword(StaticAbility::Decayed)
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
    use crate::cards::test_helpers::test_db;

    fn grizzly_bears_perm() -> PermanentState {
        let db = test_db();
        PermanentState::new(db.get("Grizzly Bears").unwrap())
    }

    #[test]
    fn new_permanent_enters_summoning_sick() {
        let perm = grizzly_bears_perm();
        assert!(perm.summoning_sick(1));
        assert!(!perm.can_attack(1));
    }

    #[test]
    fn creature_can_attack_after_sickness_cleared() {
        let mut perm = grizzly_bears_perm();
        perm.controller_since_turn = 0; // entered before turn 1 → not sick
        assert!(perm.can_attack(1));
    }

    #[test]
    fn tapped_creature_cannot_attack_or_block() {
        let mut perm = grizzly_bears_perm();
        perm.controller_since_turn = 0;
        perm.tapped = true;
        assert!(!perm.can_attack(1));
        assert!(!perm.can_block());
    }

    #[test]
    fn summoning_sick_creature_with_haste_can_attack() {
        use crate::types::{Ability, OracleSpan, ability::StaticAbility};
        let mut def = test_db().get("Grizzly Bears").unwrap().clone();
        def.abilities = vec![OracleSpan::Parsed(Ability::Static(StaticAbility::Haste))];
        let perm = PermanentState::new(&def); // controller_since_turn = u32::MAX → sick
        assert!(perm.can_attack(1)); // sick but has Haste
    }

    #[test]
    fn non_creature_permanent_is_never_summoning_sick() {
        let db = test_db();
        let perm = PermanentState::new(db.get("Forest").unwrap());
        assert!(!perm.summoning_sick(1));
    }

    #[test]
    fn creature_not_sick_on_controllers_next_turn() {
        let mut perm = grizzly_bears_perm();
        perm.controller_since_turn = 1; // entered turn 1
        // On controller's next turn (most_recent = 3), 1 >= 3 = false
        assert!(!perm.summoning_sick(3));
    }

    #[test]
    fn creature_still_sick_during_opponents_turn() {
        let mut perm = grizzly_bears_perm();
        perm.controller_since_turn = 1; // entered turn 1
        // During opponent's turn, controller's most_recent_turn = 1 still
        assert!(perm.summoning_sick(1));
    }

    #[test]
    fn damaged_by_deathtouch_initialises_false() {
        let perm = grizzly_bears_perm();
        assert!(!perm.damaged_by_deathtouch);
    }

    #[test]
    fn pt_delta_default_is_zero() {
        let delta = PTDelta::default();
        assert_eq!(delta.power, 0);
        assert_eq!(delta.toughness, 0);
    }

    #[test]
    fn pt_boost_until_eot_initialises_to_zero() {
        let perm = grizzly_bears_perm();
        assert_eq!(perm.pt_boost_until_eot.power, 0);
        assert_eq!(perm.pt_boost_until_eot.toughness, 0);
    }

    #[test]
    fn effective_power_includes_eot_boost() {
        let mut perm = grizzly_bears_perm();
        perm.pt_boost_until_eot.power = 3;
        assert_eq!(perm.effective_power(), Some(5)); // 2 base + 3
    }

    #[test]
    fn effective_toughness_includes_eot_boost() {
        let mut perm = grizzly_bears_perm();
        perm.pt_boost_until_eot.toughness = -1;
        assert_eq!(perm.effective_toughness(), Some(1)); // 2 base - 1
    }

    #[test]
    fn effective_power_with_negative_boost_does_not_panic() {
        let mut perm = grizzly_bears_perm();
        perm.pt_boost_until_eot.power = -5;
        assert_eq!(perm.effective_power(), Some(-3)); // 2 base - 5
    }

    #[test]
    fn bushido_n_returns_some_for_bushido_creature() {
        use crate::types::{Ability, OracleSpan, ability::StaticAbility};
        let mut def = test_db().get("Grizzly Bears").unwrap().clone();
        def.abilities = vec![OracleSpan::Parsed(Ability::Static(
            StaticAbility::BushidoN(3),
        ))];
        let perm = PermanentState::new(&def);
        assert_eq!(perm.bushido_n(), Some(3));
    }

    #[test]
    fn bushido_n_returns_none_for_vanilla_creature() {
        let perm = grizzly_bears_perm();
        assert_eq!(perm.bushido_n(), None);
    }

    #[test]
    fn ability_cycling_roundtrips() {
        use crate::types::mana::{ManaCost, ManaPip};
        use crate::types::{Ability, OracleSpan};
        let cost = ManaCost {
            pips: vec![ManaPip::Generic(2)],
        };
        let span = OracleSpan::Parsed(Ability::Cycling(cost.clone()));
        assert_eq!(span, OracleSpan::Parsed(Ability::Cycling(cost)));
    }

    #[test]
    fn counter_count_returns_zero_for_absent_key() {
        use crate::types::CounterKind;
        let perm = grizzly_bears_perm();
        assert_eq!(
            perm.counter_count(&CounterKind::Named("absent".to_string())),
            0
        );
        assert_eq!(
            perm.counter_count(&CounterKind::PtModifier {
                power: 1,
                toughness: 1
            }),
            0
        );
    }

    #[test]
    fn add_counters_accumulates() {
        use crate::types::CounterKind;
        let mut perm = grizzly_bears_perm();
        perm.add_counters(
            CounterKind::PtModifier {
                power: 1,
                toughness: 1,
            },
            2,
        );
        perm.add_counters(
            CounterKind::PtModifier {
                power: 1,
                toughness: 1,
            },
            3,
        );
        assert_eq!(
            perm.counter_count(&CounterKind::PtModifier {
                power: 1,
                toughness: 1
            }),
            5
        );
    }

    #[test]
    fn effective_power_includes_pt_modifier_counters() {
        use crate::types::CounterKind;
        let mut perm = grizzly_bears_perm(); // base 2/2
        perm.add_counters(
            CounterKind::PtModifier {
                power: 1,
                toughness: 1,
            },
            3,
        );
        assert_eq!(perm.effective_power(), Some(5)); // 2 + 3
        assert_eq!(perm.effective_toughness(), Some(5));
    }

    #[test]
    fn effective_power_unaffected_by_named_counters() {
        use crate::types::CounterKind;
        let mut perm = grizzly_bears_perm(); // base 2/2
        perm.add_counters(CounterKind::Named("test".to_string()), 5);
        perm.add_counters(CounterKind::Named("charge".to_string()), 10);
        assert_eq!(perm.effective_power(), Some(2)); // unchanged
        assert_eq!(perm.effective_toughness(), Some(2));
    }

    #[test]
    fn negative_pt_modifier_counters_reduce_power_and_toughness() {
        use crate::types::CounterKind;
        let mut perm = grizzly_bears_perm(); // base 2/2
        perm.add_counters(
            CounterKind::PtModifier {
                power: -1,
                toughness: -1,
            },
            2,
        );
        assert_eq!(perm.effective_power(), Some(0));
        assert_eq!(perm.effective_toughness(), Some(0));
    }
}
