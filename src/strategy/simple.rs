use rust_decimal::Decimal;
use rust_decimal_macros::dec;
use tokio::sync::{mpsc, oneshot};
use tokio::time::{sleep, Duration};

use crate::oms::event::OmsEvent;
use crate::oms::order::Side;

pub async fn run_strategy(oms_tx: mpsc::Sender<OmsEvent>) {
    loop {
        // ask OMS for delta
        let (tx, rx) = oneshot::channel();
        oms_tx
            .send(OmsEvent::GetDelta { reply: tx })
            .await
            .unwrap();

        let delta = rx.await.unwrap();
        println!("[STRAT] delta = {}", delta);

        if delta > dec!(0) {
            println!("[STRAT] need BUY {}", delta);
            oms_tx
                .send(OmsEvent::CreateOrder {
                    side: Side::Buy,
                    qty: delta,
                })
            .await
                .unwrap();
            } else if delta < dec!(0) {
                println!("[STRAT] need SELL {}", delta.abs());
                oms_tx
                    .send(OmsEvent::CreateOrder {
                        side: Side::Sell,
                        qty: delta.abs(),
                    })
                .await
                    .unwrap();
            }

        sleep(Duration::from_secs(1)).await;
    }
}
