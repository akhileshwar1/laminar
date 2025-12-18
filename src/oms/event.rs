use rust_decimal::Decimal;

use super::order::{OrderId, Side};

#[derive(Debug)]
pub enum OmsEvent {
    // strategy → OMS
    SetTarget {
        qty: Decimal,
    },

    CreateOrder {
        side: Side,
        qty: Decimal,
    },

    // exchange → OMS (later broker)
    OrderAccepted {
        order_id: OrderId,
    },

    Fill {
        order_id: OrderId,
        qty: Decimal,
        price: Decimal,
    },

    CancelConfirmed {
        order_id: OrderId,
    },

    // internal
    Tick,
}
