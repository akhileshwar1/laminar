use rust_decimal::Decimal;

/// One price level
#[derive(Debug, Clone)]
pub struct BookLevel {
    pub price: Decimal,
    pub qty: Decimal,
}

/// Full order book (both sides, full depth)
#[derive(Debug, Clone)]
pub struct OrderBook {
    pub bids: Vec<BookLevel>, // descending prices
    pub asks: Vec<BookLevel>, // ascending prices
}

/// A single immutable market snapshot
#[derive(Debug, Clone)]
pub struct MarketSnapshot {
    pub symbol: String,
    pub book: OrderBook,
    pub timestamp_ms: u64,
}

