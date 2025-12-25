use rust_decimal::Decimal;

#[derive(Debug, Clone)]
pub struct AccountSnapshot {
    pub equity: Decimal,
    pub available_margin: Decimal,
    pub used_margin: Decimal,
    pub unrealized_pnl: Decimal,
    pub realized_pnl: Decimal,
}
