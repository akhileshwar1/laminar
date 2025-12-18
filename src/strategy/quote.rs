use rust_decimal::Decimal;
use crate::oms::order::Side;

#[derive(Debug, Clone)]
pub struct Quote {
    pub side: Side,
    pub qty: Decimal,
    pub price: Decimal,
}
