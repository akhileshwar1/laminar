#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TradingState {
    Running,
    Flattening,
    Halted,
}
