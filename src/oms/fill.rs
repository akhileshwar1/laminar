use rust_decimal::Decimal;
use uuid::Uuid;

use super::order::OrderId;

#[derive(Debug, Clone)]
pub struct Fill {
    pub order_id: OrderId,
    pub qty: Decimal,      // signed
    pub price: Decimal,    // execution price
    pub fill_id: Uuid,
}

impl Fill {
    pub fn new(order_id: OrderId, qty: Decimal, price: Decimal) -> Self {
        Self {
            order_id,
            qty,
            price,
            fill_id: Uuid::new_v4(),
        }
    }
}
