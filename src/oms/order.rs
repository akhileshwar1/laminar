use rust_decimal::Decimal;
use rust_decimal_macros::dec;
use uuid::Uuid;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct OrderId(pub Uuid);

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
    pub id: OrderId,
    pub side: Side,
    pub original_qty: Decimal,
    pub state: OrderState,
}

impl Order {
    pub fn new(side: Side, qty: Decimal) -> Self {
        Self {
            id: OrderId(Uuid::new_v4()),
            side,
            original_qty: qty,
            state: OrderState::New,
        }
    }

    /// Called when venue acks the order
    pub fn on_accepted(&mut self) {
        assert!(matches!(self.state, OrderState::New));
        self.state = OrderState::Open {
            remaining: self.original_qty,
        };
    }

    /// Called when a fill arrives
    pub fn on_fill(&mut self, fill_qty: Decimal) {
        let remaining = match self.state {
            OrderState::Open { remaining }
            | OrderState::PartiallyFilled { remaining } => remaining,
            _ => panic!("Fill on non-live order"),
        };

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
