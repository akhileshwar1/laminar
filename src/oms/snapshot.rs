use rust_decimal::Decimal;
use crate::oms::order::{OrderId, Side, OrderState};

#[derive(Debug, Clone)]
pub struct OrderView {
    pub id: OrderId,
    pub side: Side,
    pub limit_price: Decimal,
    pub original_qty: Decimal,
    pub remaining_qty: Decimal,
    pub state: OrderState,
}


#[derive(Clone, Debug)]
pub struct OmsSnapshot {
    pub orders: Vec<OrderView>,
    pub net_position: Decimal,
    pub avg_price: Decimal,
}
