use rust_decimal::Decimal;

#[derive(Debug, Clone)]
pub struct RiskConfig {
    pub max_drawdown_pct: Decimal, // e.g. 0.10
}

#[derive(Debug, Clone)]
pub struct RiskState {
    pub start_equity: Decimal,
    pub killed: bool,
}
