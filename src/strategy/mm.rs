use rust_decimal_macros::dec;
use tokio::sync::{mpsc, oneshot};
use tokio::time::{sleep, Duration};

use crate::oms::event::OmsEvent;
use crate::oms::order::Side;
use super::price_sim::PriceSim;

pub async fn run_mm_strategy(oms_tx: mpsc::Sender<OmsEvent>) {
    let mut price = PriceSim::new(dec!(100.0));

    let spread = dec!(0.2);
    let base_qty = dec!(1.0);

    loop {
        let mid = price.tick();

        // query delta
        let (tx, rx) = oneshot::channel();
        oms_tx
            .send(OmsEvent::GetDelta { reply: tx })
            .await
            .unwrap();
        let delta = rx.await.unwrap();

        // inventory skew
        let skew = delta * dec!(0.05);

        let bid = mid - spread / dec!(2) - skew;
        let ask = mid + spread / dec!(2) - skew;

        println!(
            "[MM] mid={} delta={} bid={} ask={}",
            mid, delta, bid, ask
        );

        // cancel old quotes
        oms_tx.send(OmsEvent::CancelAll).await.unwrap();

        // place new quotes
        oms_tx
            .send(OmsEvent::CreateOrder {
                side: Side::Buy,
                qty: base_qty,
            })
        .await
            .unwrap();

        oms_tx
            .send(OmsEvent::CreateOrder {
                side: Side::Sell,
                qty: base_qty,
            })
        .await
            .unwrap();

        sleep(Duration::from_secs(1)).await;
    }
}
