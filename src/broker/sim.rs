use std::sync::Arc;
use tokio::sync::{mpsc, Mutex};
use tokio::time::{sleep, Duration};
use rust_decimal_macros::dec;

use crate::broker::{Broker, types::BrokerCommand};
use crate::oms::event::OmsEvent;

struct SimBrokerInner {
    cmd_rx: mpsc::Receiver<BrokerCommand>,
    oms_tx: mpsc::Sender<OmsEvent>,
}

pub struct SimBroker {
    cmd_tx: mpsc::Sender<BrokerCommand>,
    inner: Arc<Mutex<SimBrokerInner>>,
}

impl SimBroker {
    pub fn new(
        cmd_rx: mpsc::Receiver<BrokerCommand>,
        cmd_tx: mpsc::Sender<BrokerCommand>,
        oms_tx: mpsc::Sender<OmsEvent>,
    ) -> Self {
        Self {
            cmd_tx,
            inner: Arc::new(Mutex::new(SimBrokerInner {
                cmd_rx,
                oms_tx,
            })),
        }
    }
}

impl Broker for SimBroker {
    fn command_sender(&self) -> mpsc::Sender<BrokerCommand> {
        self.cmd_tx.clone()
    }

    fn start(self: Arc<Self>) {
        let inner = self.inner.clone();

        tokio::spawn(async move {
            let mut inner = inner.lock().await;

            while let Some(cmd) = inner.cmd_rx.recv().await {
                match cmd {
                    BrokerCommand::PlaceLimit {
                        order_id,
                        qty,
                        price,
                        ..
                    } => {
                        println!("[SIM] place {:?} qty={} @ {}", order_id, qty, price);

                        sleep(Duration::from_millis(50)).await;
                        let _ = inner.oms_tx
                            .send(OmsEvent::OrderAccepted { order_id })
                            .await;

                        sleep(Duration::from_millis(50)).await;
                        let _ = inner.oms_tx
                            .send(OmsEvent::Fill {
                                order_id,
                                qty: qty * dec!(0.4),
                                price,
                            })
                        .await;

                        sleep(Duration::from_millis(50)).await;
                        let _ = inner.oms_tx
                            .send(OmsEvent::Fill {
                                order_id,
                                qty: qty * dec!(0.6),
                                price,
                            })
                        .await;
                        }

                    BrokerCommand::Cancel { order_id } => {
                        sleep(Duration::from_millis(30)).await;
                        let _ = inner.oms_tx
                            .send(OmsEvent::CancelConfirmed { order_id })
                            .await;
                        }
                }
            }
        });
    }
}
