use rust_decimal::Decimal;
use rust_decimal::prelude::Signed;
use rust_decimal_macros::dec;

#[derive(Debug, Clone)]
pub struct Position {
    pub net_qty: Decimal,
    pub avg_price: Decimal,
    pub realized_pnl: Decimal,
}

impl Position {
    pub fn new() -> Self {
        Self {
            net_qty: dec!(0),
            avg_price: dec!(0),
            realized_pnl: dec!(0),
        }
    }

    pub fn apply_fill(&mut self, qty: Decimal, price: Decimal) {
        // same direction → adjust avg
        if self.net_qty == dec!(0) || self.net_qty.signum() == qty.signum() {
            let new_qty = self.net_qty + qty;
            if new_qty != dec!(0) {
                self.avg_price =
                    (self.avg_price * self.net_qty.abs() + price * qty.abs())
                    / new_qty.abs();
            }
            self.net_qty = new_qty;
        } else {
            // reducing or flipping position
            let closing_qty = self.net_qty.abs().min(qty.abs());
            let pnl = closing_qty * (price - self.avg_price) * self.net_qty.signum();
            self.realized_pnl += pnl;
            self.net_qty += qty;

            if self.net_qty == dec!(0) {
                self.avg_price = dec!(0);
            }
        }
    }
}
