use super::ability::{Ability, OracleSpan, StaticAbility};
use super::card::CardDefinition;

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
    /// CR 302.6 — true until controller's next untap step.
    pub summoning_sick: bool,
    pub damage_marked: u32,
    /// CR 704.5h — flagged when deathtouch damage lands; cleared by SBAs.
    pub damaged_by_deathtouch: bool,
    pub pt_boost_until_eot: PTDelta,
}

impl PermanentState {
    pub fn new(definition: &CardDefinition) -> Self {
        Self {
            definition: definition.clone(),
            current_power: definition.power,
            current_toughness: definition.toughness,
            tapped: false,
            summoning_sick: true,
            damage_marked: 0,
            damaged_by_deathtouch: false,
            pt_boost_until_eot: PTDelta::default(),
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
        // MTG P/T values are small integers; plain addition cannot overflow i32 in practice.
        self.current_power
            .map(|p| p + self.pt_boost_until_eot.power)
    }

    pub fn effective_toughness(&self) -> Option<i32> {
        // MTG P/T values are small integers; plain addition cannot overflow i32 in practice.
        self.current_toughness
            .map(|t| t + self.pt_boost_until_eot.toughness)
    }

    /// CR 302.5a — a creature can attack if untapped, not summoning sick (unless Haste),
    /// and not a Defender.
    pub fn can_attack(&self) -> bool {
        self.is_creature()
            && !self.tapped
            && !self.has_keyword(StaticAbility::Defender)
            && (!self.summoning_sick || self.has_keyword(StaticAbility::Haste))
    }

    /// CR 509.1a — a creature can block if untapped and not Decayed.
    pub fn can_block(&self) -> bool {
        self.is_creature() && !self.tapped && !self.has_keyword(StaticAbility::Decayed)
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
        assert!(perm.summoning_sick);
        assert!(!perm.can_attack());
    }

    #[test]
    fn creature_can_attack_after_sickness_cleared() {
        let mut perm = grizzly_bears_perm();
        perm.summoning_sick = false;
        assert!(perm.can_attack());
    }

    #[test]
    fn tapped_creature_cannot_attack_or_block() {
        let mut perm = grizzly_bears_perm();
        perm.summoning_sick = false;
        perm.tapped = true;
        assert!(!perm.can_attack());
        assert!(!perm.can_block());
    }

    #[test]
    fn summoning_sick_creature_with_haste_can_attack() {
        use crate::types::{Ability, OracleSpan, ability::StaticAbility};
        let mut def = test_db().get("Grizzly Bears").unwrap().clone();
        def.abilities = vec![OracleSpan::Parsed(Ability::Static(StaticAbility::Haste))];
        let mut perm = PermanentState::new(&def);
        perm.summoning_sick = true;
        assert!(perm.can_attack());
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
        perm.summoning_sick = false;
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
}
