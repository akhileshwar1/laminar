use crate::oms::snapshot::OmsSnapshot;
use rust_decimal::Decimal;

#[derive(Default)]
pub struct TuiApp {
    pub snapshot: Option<OmsSnapshot>,

    // strategy diagnostics (fed from market/strategy channel later)
    pub mid: Decimal,
    pub spread: Decimal,
    pub skew: Decimal,
    pub bid: Decimal,
    pub ask: Decimal,
}
