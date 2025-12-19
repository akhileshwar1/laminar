pub mod sim;
pub mod types;

use tokio::sync::mpsc;

use crate::broker::types::BrokerCommand;

pub trait Broker: Send + Sync {
    fn command_sender(&self) -> mpsc::Sender<BrokerCommand>;
    fn start(&self);
}
