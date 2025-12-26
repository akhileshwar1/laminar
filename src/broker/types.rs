use rust_decimal::Decimal;
use crate::oms::order::{OrderId, Side};

#[derive(Debug, Clone)]
pub enum BrokerCommand {
    PlaceLimit {
        order_id: OrderId,
        side: Side,
        qty: Decimal,
        price: Decimal,
    },

    Cancel {
        order_id: OrderId,
    },

    /// Market order to flatten position
    Flatten {
        qty: Decimal,
        limit_px: Decimal,
    },
}
