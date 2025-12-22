use rust_decimal::Decimal;
use tokio::sync::oneshot;

use super::order::{OrderId, Side};
use crate::oms::snapshot::OmsSnapshot;

#[derive(Debug)]
pub enum OmsEvent {
    // strategy → OMS
    SetTarget {
        qty: Decimal,
    },

    CreateOrder {
        side: Side,
        qty: Decimal,
        price: Decimal,
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

    GetDelta {
        reply: oneshot::Sender<rust_decimal::Decimal>,
    },

    CancelAll,

    GetSnapshot {
        reply: oneshot::Sender<OmsSnapshot>,
    },

    // internal
    Tick,
}
