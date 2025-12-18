use rust_decimal::Decimal;
use rust_decimal_macros::dec;

#[derive(Debug, Clone)]
pub struct PriceSim {
    mid: Decimal,
    step: Decimal,
}

impl PriceSim {
    pub fn new(start: Decimal) -> Self {
        Self {
            mid: start,
            step: dec!(0.1),
        }
    }

    pub fn tick(&mut self) -> Decimal {
        self.mid += self.step;
        self.mid
    }
}
