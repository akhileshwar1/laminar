use rust_decimal::Decimal;
use rust_decimal_macros::dec;
use uuid::Uuid;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct OrderId(pub Uuid);

// pub type ClientOrderId = String;

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Side {
    Buy,
    Sell,
}

impl Side {
    pub fn sign(&self) -> Decimal {
        match self {
            Side::Buy => dec!(1),
            Side::Sell => dec!(-1),
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum OrderState {
    /// Created internally, not yet sent
    New,

    /// Accepted by venue, live
    Open { remaining: Decimal },

    /// Some quantity filled, still live
    PartiallyFilled { remaining: Decimal },

    /// Fully filled
    Filled,

    /// Cancel requested
    CancelPending,

    /// Cancel confirmed
    Cancelled,

    /// Rejected by venue
    Rejected,
}

#[derive(Debug, Clone)]
pub struct Order {
    pub id: OrderId,              // internal
    // pub cloid: ClientOrderId,     // external
    pub side: Side,
    pub original_qty: Decimal,
    pub limit_price: Decimal,
    pub state: OrderState,
}

impl Order {
    pub fn new(side: Side, qty: Decimal, price: Decimal) -> Self {
        Self {
            id: OrderId(Uuid::new_v4()),
            // cloid: format!("laminar-{}", Uuid::new_v4()),
            side,
            original_qty: qty,
            limit_price: price,
            state: OrderState::New,
        }
    }

    /// Called when venue acks the order
    pub fn on_accepted(&mut self) {
        match self.state {
            OrderState::New => {
                self.state = OrderState::Open {
                    remaining: self.original_qty,
                };
            }

            // Accept arrived late — already progressed
            OrderState::Open { .. }
            | OrderState::PartiallyFilled { .. }
            | OrderState::Filled
                | OrderState::Cancelled
                | OrderState::CancelPending
                | OrderState::Rejected => {
                    // Idempotent / out-of-order accept
                    return;
                }
        }
    }

    /// Called when a fill arrives
    pub fn on_fill(&mut self, fill_qty: Decimal) {
        let remaining = match self.state {
            OrderState::Open { remaining }
            | OrderState::PartiallyFilled { remaining } => remaining,
            OrderState::New=> {
                // Treat fill as implicit acceptance
                self.on_accepted();
                match self.state {
                    OrderState::Open { remaining } => remaining,
                    _ => unreachable!(),
                }
            }

            OrderState::Filled
                | OrderState::Cancelled
                | OrderState::CancelPending
                | OrderState::Rejected => {
                // Idempotency / late WS message
                return;
            }
        };
        // _ => panic!("Fill on non-live order"),
        // };

        assert!(fill_qty > dec!(0));
        assert!(fill_qty <= remaining);

        let new_remaining = remaining - fill_qty;

        self.state = if new_remaining == dec!(0) {
            OrderState::Filled
        } else {
            OrderState::PartiallyFilled {
                remaining: new_remaining,
            }
        };
    }

    pub fn on_cancel_requested(&mut self) {
        match self.state {
            OrderState::Open { .. }
            | OrderState::PartiallyFilled { .. } => {
                self.state = OrderState::CancelPending;
            }
            _ => panic!("Cancel requested on non-cancellable order"),
        }
    }

    pub fn on_cancel_confirmed(&mut self) {
        assert!(matches!(self.state, OrderState::CancelPending));
        self.state = OrderState::Cancelled;
    }

    pub fn remaining_signed_qty(&self) -> Decimal {
        match self.state {
            OrderState::Open { remaining }
            | OrderState::PartiallyFilled { remaining } => {
                remaining * self.side.sign()
            }
            _ => dec!(0),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rust_decimal_macros::dec;

    #[test]
    fn partial_fill_flow() {
        let mut o = Order::new(Side::Buy, dec!(1.0), dec!(100));
        o.on_accepted();

        o.on_fill(dec!(0.4));
        assert_eq!(
            o.state,
            OrderState::PartiallyFilled { remaining: dec!(0.6) }
        );

        o.on_fill(dec!(0.6));
        assert_eq!(o.state, OrderState::Filled);
    }

    #[test]
    fn cancel_flow() {
        let mut o = Order::new(Side::Sell, dec!(2.0), dec!(101));
        o.on_accepted();
        o.on_cancel_requested();
        o.on_cancel_confirmed();

        assert_eq!(o.state, OrderState::Cancelled);
    }

}
