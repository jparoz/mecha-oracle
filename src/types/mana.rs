#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ManaColor {
    White,
    Blue,
    Black,
    Red,
    Green,
    Colorless,
}

impl std::fmt::Display for ManaColor {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            ManaColor::White => "W",
            ManaColor::Blue => "U",
            ManaColor::Black => "B",
            ManaColor::Red => "R",
            ManaColor::Green => "G",
            ManaColor::Colorless => "C",
        })
    }
}

/// CR 107.4: every distinct mana symbol that can appear in a mana cost.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ManaPip {
    White,
    Blue,
    Black,
    Red,
    Green,
    Colorless,
    Generic(u32),
    X,
    /// {W/U}, {W/B}, {U/B}, {U/R}, {B/R}, {B/G}, {R/G}, {R/W}, {G/W}, {G/U}
    Hybrid(ManaColor, ManaColor),
    /// {2/W}…{2/G} — pay N generic or 1 color
    GenericHybrid(u32, ManaColor),
    /// {C/W}…{C/G} — pay 1 colorless or 1 color
    ColorlessHybrid(ManaColor),
    /// {W/P}…{G/P} — pay color or 2 life
    Phyrexian(ManaColor),
    /// {W/U/P}…{G/U/P} — pay either color or 2 life
    HybridPhyrexian(ManaColor, ManaColor),
    Snow,
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct ManaCost {
    pub pips: Vec<ManaPip>,
}

impl ManaCost {
    /// CR 202.3: X counts 0; GenericHybrid(n,_) counts n; all others count 1.
    pub fn mana_value(&self) -> u32 {
        self.pips
            .iter()
            .map(|pip| match pip {
                ManaPip::White
                | ManaPip::Blue
                | ManaPip::Black
                | ManaPip::Red
                | ManaPip::Green
                | ManaPip::Colorless
                | ManaPip::Hybrid(_, _)
                | ManaPip::ColorlessHybrid(_)
                | ManaPip::Phyrexian(_)
                | ManaPip::HybridPhyrexian(_, _)
                | ManaPip::Snow => 1,
                ManaPip::Generic(n) => *n,
                ManaPip::GenericHybrid(n, _) => *n,
                ManaPip::X => 0,
            })
            .sum()
    }

    pub fn has_x(&self) -> bool {
        self.pips.iter().any(|p| matches!(p, ManaPip::X))
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct ManaPool {
    pub white: u32,
    pub blue: u32,
    pub black: u32,
    pub red: u32,
    pub green: u32,
    pub colorless: u32,
    // snow-tagged subset — invariant: snow_X <= X
    pub snow_white: u32,
    pub snow_blue: u32,
    pub snow_black: u32,
    pub snow_red: u32,
    pub snow_green: u32,
    pub snow_colorless: u32,
}

impl ManaPool {
    pub fn add(&mut self, color: ManaColor, amount: u32) {
        match color {
            ManaColor::White => self.white += amount,
            ManaColor::Blue => self.blue += amount,
            ManaColor::Black => self.black += amount,
            ManaColor::Red => self.red += amount,
            ManaColor::Green => self.green += amount,
            ManaColor::Colorless => self.colorless += amount,
        }
    }

    /// Increment both the color field and its snow shadow.
    pub fn add_snow(&mut self, color: ManaColor, amount: u32) {
        self.add(color, amount);
        match color {
            ManaColor::White => self.snow_white += amount,
            ManaColor::Blue => self.snow_blue += amount,
            ManaColor::Black => self.snow_black += amount,
            ManaColor::Red => self.snow_red += amount,
            ManaColor::Green => self.snow_green += amount,
            ManaColor::Colorless => self.snow_colorless += amount,
        }
    }

    pub fn total(&self) -> u32 {
        self.white + self.blue + self.black + self.red + self.green + self.colorless
    }

    pub fn total_snow(&self) -> u32 {
        self.snow_white
            + self.snow_blue
            + self.snow_black
            + self.snow_red
            + self.snow_green
            + self.snow_colorless
    }

    pub fn is_empty(&self) -> bool {
        self.total() == 0
    }
}

/// Describes exactly how a player pays a mana cost.
/// 1 blood = 2 life deducted (Phyrexian mana payment).
#[derive(Debug, Clone, PartialEq, Eq, Default, serde::Deserialize)]
pub struct PaymentPlan {
    /// Some(n) iff cost contains {X}; None otherwise.
    pub x_value: Option<u32>,
    // mana to deduct from pool
    pub white: u32,
    pub blue: u32,
    pub black: u32,
    pub red: u32,
    pub green: u32,
    pub colorless: u32,
    // snow-tagged mana to deduct — must be <= corresponding color field
    pub snow_white: u32,
    pub snow_blue: u32,
    pub snow_black: u32,
    pub snow_red: u32,
    pub snow_green: u32,
    pub snow_colorless: u32,
    /// Phyrexian life payments: 1 blood = 2 life.
    pub blood: u32,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mana_pool_add_and_total() {
        let mut pool = ManaPool::default();
        pool.add(ManaColor::Green, 2);
        pool.add(ManaColor::Red, 1);
        assert_eq!(pool.total(), 3);
        assert_eq!(pool.green, 2);
        assert_eq!(pool.red, 1);
    }

    #[test]
    fn mana_pool_starts_empty() {
        assert!(ManaPool::default().is_empty());
    }

    #[test]
    fn mana_value_generic_and_color() {
        let cost = ManaCost {
            pips: vec![ManaPip::Generic(1), ManaPip::Green],
        };
        assert_eq!(cost.mana_value(), 2);
    }

    #[test]
    fn mana_value_x_counts_zero() {
        let cost = ManaCost {
            pips: vec![ManaPip::X, ManaPip::Red],
        };
        assert_eq!(cost.mana_value(), 1);
        assert!(cost.has_x());
    }

    #[test]
    fn mana_value_generic_hybrid_uses_numeric_component() {
        let cost = ManaCost {
            pips: vec![ManaPip::GenericHybrid(2, ManaColor::Green)],
        };
        assert_eq!(cost.mana_value(), 2);
    }

    #[test]
    fn add_snow_increments_both_color_and_snow_shadow() {
        let mut pool = ManaPool::default();
        pool.add_snow(ManaColor::Green, 2);
        assert_eq!(pool.green, 2);
        assert_eq!(pool.snow_green, 2);
        assert_eq!(pool.total(), 2);
        assert_eq!(pool.total_snow(), 2);
    }

    #[test]
    fn add_non_snow_does_not_affect_snow_shadow() {
        let mut pool = ManaPool::default();
        pool.add(ManaColor::Green, 1);
        assert_eq!(pool.green, 1);
        assert_eq!(pool.snow_green, 0);
        assert_eq!(pool.total_snow(), 0);
    }

    #[test]
    fn payment_plan_default_is_zero_blood_no_x() {
        let plan = PaymentPlan::default();
        assert_eq!(plan.blood, 0);
        assert!(plan.x_value.is_none());
    }
}
