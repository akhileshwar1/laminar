pub mod types;

#[cfg(feature = "hyperliquid")]
pub mod hyperliquid;

use tokio::sync::broadcast;
use crate::market::types::MarketSnapshot;

/// Market data source abstraction
pub trait MarketAdapter: Send + Sync {
    /// Subscribe to market snapshots
    fn subscribe(&self) -> broadcast::Receiver<MarketSnapshot>;

    /// Start the adapter (spawn tasks, connect sockets, etc.)
    fn start(&self);
}

