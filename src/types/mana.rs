#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ManaColor {
    White,
    Blue,
    Black,
    Red,
    Green,
    Colorless,
}

#[derive(Debug, Clone, PartialEq, Default)]
pub struct ManaCost {
    pub generic: u32,
    pub white: u32,
    pub blue: u32,
    pub black: u32,
    pub red: u32,
    pub green: u32,
    pub colorless: u32,
}

impl ManaCost {
    pub fn total_colored(&self) -> u32 {
        self.white + self.blue + self.black + self.red + self.green + self.colorless
    }

    pub fn converted_mana_cost(&self) -> u32 {
        self.generic + self.total_colored()
    }
}

#[derive(Debug, Clone, PartialEq, Default)]
pub struct ManaPool {
    pub white: u32,
    pub blue: u32,
    pub black: u32,
    pub red: u32,
    pub green: u32,
    pub colorless: u32,
}

impl ManaPool {
    pub fn add(&mut self, color: ManaColor, amount: u32) {
        match color {
            ManaColor::White    => self.white    += amount,
            ManaColor::Blue     => self.blue     += amount,
            ManaColor::Black    => self.black    += amount,
            ManaColor::Red      => self.red      += amount,
            ManaColor::Green    => self.green    += amount,
            ManaColor::Colorless => self.colorless += amount,
        }
    }

    pub fn total(&self) -> u32 {
        self.white + self.blue + self.black + self.red + self.green + self.colorless
    }

    pub fn is_empty(&self) -> bool {
        self.total() == 0
    }
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
    fn mana_cost_cmc() {
        let cost = ManaCost { generic: 1, green: 1, ..Default::default() };
        assert_eq!(cost.converted_mana_cost(), 2);
    }

    #[test]
    fn mana_cost_total_colored_excludes_generic() {
        let cost = ManaCost { generic: 3, red: 2, ..Default::default() };
        assert_eq!(cost.total_colored(), 2);
        assert_eq!(cost.converted_mana_cost(), 5);
    }
}
