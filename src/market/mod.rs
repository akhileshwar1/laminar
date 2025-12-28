pub mod types;

#[cfg(feature = "hyperliquid")]
pub mod hyperliquid;

use tokio::sync::broadcast;
use crate::market::types::MarketEvent;

/// Market data source abstraction
pub trait MarketAdapter: Send + Sync {
    /// Subscribe to market events (book + tape)
    fn subscribe(&self) -> broadcast::Receiver<MarketEvent>;

    /// Start the adapter
    fn start(&self);
}
