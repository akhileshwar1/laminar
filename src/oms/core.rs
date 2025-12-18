use rust_decimal::Decimal;
use rust_decimal_macros::dec;

#[derive(Debug, Clone, Copy)]
pub struct Quantity(pub Decimal);

impl Quantity {
    pub fn zero() -> Self {
        Quantity(dec!(0))
    }
}

use std::ops::{Add, Sub};

impl Add for Quantity {
    type Output = Quantity;
    fn add(self, rhs: Quantity) -> Quantity {
        Quantity(self.0 + rhs.0)
    }
}

impl Sub for Quantity {
    type Output = Quantity;
    fn sub(self, rhs: Quantity) -> Quantity {
        Quantity(self.0 - rhs.0)
    }
}

#[derive(Debug)]
pub struct OmsCore {
    /// What the strategy wants
    target_position: Quantity,

    /// What has actually been filled
    filled_position: Quantity,

    /// Quantities of all open orders (signed)
    open_orders: Vec<Quantity>,
}

impl OmsCore {
    pub fn new() -> Self {
        Self {
            target_position: Quantity::zero(),
            filled_position: Quantity::zero(),
            open_orders: Vec::new(),
        }
    }

    /// Strategy intent
    pub fn set_target_position(&mut self, qty: Quantity) {
        self.target_position = qty;
    }

    /// Exchange truth
    pub fn on_fill(&mut self, fill_qty: Quantity) {
        self.filled_position = self.filled_position + fill_qty;
    }

    /// OMS bookkeeping
    pub fn add_open_order(&mut self, qty: Quantity) {
        self.open_orders.push(qty);
    }

    pub fn remove_open_order(&mut self, qty: Quantity) {
        if let Some(pos) = self.open_orders.iter().position(|q| q.0 == qty.0) {
            self.open_orders.remove(pos);
        }
    }

    /// Sum of all pending exposure
    pub fn open_exposure(&self) -> Quantity {
        self.open_orders
            .iter()
            .copied()
            .fold(Quantity::zero(), |acc, q| acc + q)
    }

    /// The ONE number the OMS cares about
    ///
    /// delta = target - (filled + open) to enforce the invariant i.e T = P + O
    pub fn delta(&self) -> Quantity {
        self.target_position - (self.filled_position + self.open_exposure())
    }

    pub fn clear_open_orders(&mut self) {
        self.open_orders.clear();
    }
}


#[cfg(test)]
mod tests {
    use super::*;
    use rust_decimal_macros::dec;


    #[test]
    fn basic_reconciliation() {
        let mut oms = OmsCore::new();

        oms.set_target_position(Quantity(dec!(1.0)));
        assert_eq!(oms.delta().0, dec!(1.0));

        oms.add_open_order(Quantity(dec!(1.0)));
        assert_eq!(oms.delta().0, dec!(0.0));

        oms.on_fill(Quantity(dec!(0.4)));
        assert_eq!(oms.delta().0, dec!(-0.4));

        oms.set_target_position(Quantity(dec!(0.2)));
        assert_eq!(oms.delta().0, dec!(-1.2));
    }

    #[test]
    fn flip_direction() {
        let mut oms = OmsCore::new();

        oms.set_target_position(Quantity(dec!(1.0)));
        oms.add_open_order(Quantity(dec!(1.0)));

        oms.on_fill(Quantity(dec!(1.0)));
        oms.remove_open_order(Quantity(dec!(1.0)));

        oms.set_target_position(Quantity(dec!(-0.5)));
        assert_eq!(oms.delta().0, dec!(-1.5));

    }
}
