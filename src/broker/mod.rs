pub mod sim;
pub mod types;

mod hyperliquid;

pub use hyperliquid::HyperliquidBroker;
use std::sync::Arc;

use tokio::sync::mpsc;

use crate::broker::types::BrokerCommand;

pub trait Broker: Send + Sync {
    fn command_sender(&self) -> mpsc::Sender<BrokerCommand>;
    fn start(self: Arc<Self>);
}
