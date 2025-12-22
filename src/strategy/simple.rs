use rust_decimal::Decimal;
use rust_decimal_macros::dec;
use tokio::sync::{mpsc, oneshot};
use tokio::time::{sleep, Duration};

use crate::oms::event::OmsEvent;
use crate::oms::order::Side;
use tracing::{info, warn, error};

pub async fn run_strategy(oms_tx: mpsc::Sender<OmsEvent>) {
    loop {
        // ask OMS for delta
        let (tx, rx) = oneshot::channel();
        oms_tx
            .send(OmsEvent::GetDelta { reply: tx })
            .await
            .unwrap();

        let delta = rx.await.unwrap();
        info!("[STRAT] delta = {}", delta);

        if delta != dec!(0) {
            info!("[STRAT] cancelling stale orders");
            oms_tx.send(OmsEvent::CancelAll).await.unwrap();

            if delta > dec!(0) {
                oms_tx.send(OmsEvent::CreateOrder {
                    side: Side::Buy,
                    qty: delta,
                    price: dec!(100),
                }).await.unwrap();
            } else {
                oms_tx.send(OmsEvent::CreateOrder {
                    side: Side::Sell,
                    qty: delta.abs(),
                    price: dec!(100),
                }).await.unwrap();
            }
        }

        sleep(Duration::from_secs(1)).await;
    }
}
