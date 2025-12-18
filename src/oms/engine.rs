use std::collections::HashMap;

use rust_decimal::Decimal;
use rust_decimal_macros::dec;

use super::core::{OmsCore, Quantity};
use super::order::{Order, OrderId, Side};

#[derive(Debug)]
pub struct OmsEngine {
    core: OmsCore,
    orders: HashMap<OrderId, Order>,
}

impl OmsEngine {
    pub fn new() -> Self {
        Self {
            core: OmsCore::new(),
            orders: HashMap::new(),
        }
    }

    /* ---------- Strategy-facing ---------- */

    pub fn set_target_position(&mut self, qty: Decimal) {
        self.core.set_target_position(Quantity(qty));
    }

    pub fn delta(&self) -> Decimal {
        self.core.delta().0
    }

    /* ---------- Order lifecycle ---------- */

    pub fn create_order(&mut self, side: Side, qty: Decimal) -> OrderId {
        let order = Order::new(side, qty);
        let id = order.id;
        self.orders.insert(id, order);
        id
    }

    pub fn on_order_accepted(&mut self, id: OrderId) {
        let order = self.orders.get_mut(&id).expect("unknown order");
        order.on_accepted();
        self.recompute_open_exposure();
    }

    pub fn on_fill(&mut self, id: OrderId, fill_qty: Decimal) {
        let order = self.orders.get_mut(&id).expect("unknown order");
        order.on_fill(fill_qty);

        // truth update
        let signed = fill_qty * order.side.sign();
        self.core.on_fill(Quantity(signed));

        self.recompute_open_exposure();
    }

    pub fn request_cancel(&mut self, id: OrderId) {
        let order = self.orders.get_mut(&id).expect("unknown order");
        order.on_cancel_requested();
    }

    pub fn on_cancel_confirmed(&mut self, id: OrderId) {
        let order = self.orders.get_mut(&id).expect("unknown order");
        order.on_cancel_confirmed();
        self.recompute_open_exposure();
    }

    /* ---------- Internal ---------- */

    fn recompute_open_exposure(&mut self) {
        self.core.clear_open_orders();

        for order in self.orders.values() {
            let qty = order.remaining_signed_qty();
            if qty != dec!(0) {
                self.core.add_open_order(Quantity(qty));
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rust_decimal_macros::dec;

    #[test]
    fn end_to_end_reconciliation() {
        let mut oms = OmsEngine::new();

        oms.set_target_position(dec!(1.0));
        assert_eq!(oms.delta(), dec!(1.0));

        let oid = oms.create_order(Side::Buy, dec!(1.0));
        oms.on_order_accepted(oid);

        assert_eq!(oms.delta(), dec!(0.0));

        oms.on_fill(oid, dec!(0.4));
        assert_eq!(oms.delta(), dec!(0.0));

        oms.on_fill(oid, dec!(0.6));
        assert_eq!(oms.delta(), dec!(0.0));
    }

    #[test]
    fn cancel_releases_exposure() {
        let mut oms = OmsEngine::new();

        oms.set_target_position(dec!(1.0));
        let oid = oms.create_order(Side::Buy, dec!(1.0));
        oms.on_order_accepted(oid);

        oms.request_cancel(oid);
        oms.on_cancel_confirmed(oid);

        assert_eq!(oms.delta(), dec!(1.0));
    }
}
