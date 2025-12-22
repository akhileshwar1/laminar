use rust_decimal::Decimal;
use rust_decimal_macros::dec;

use tokio::sync::{mpsc, oneshot, broadcast};

use crate::market::types::MarketSnapshot;
use crate::oms::event::OmsEvent;
use crate::oms::order::Side;

fn snap_to_tick(price: Decimal, tick: Decimal) -> Decimal {
    (price / tick).floor() * tick
}

pub async fn run_mm_strategy(
    mut market_rx: broadcast::Receiver<MarketSnapshot>,
    oms_tx: mpsc::Sender<OmsEvent>,
) {
    let base_qty = dec!(0.001);

    loop {
        // wait for next market snapshot
        let snapshot = match market_rx.recv().await {
            Ok(s) => s,
            Err(_) => continue,
        };

        // extract best bid / ask
        let best_bid = match snapshot.book.bids.first() {
            Some(l) => l.price,
            None => continue,
        };

        let best_ask = match snapshot.book.asks.first() {
            Some(l) => l.price,
            None => continue,
        };

        let mid = (best_bid + best_ask) / dec!(2);
        let spread = best_ask - best_bid;

        // query inventory delta
        let (tx, rx) = oneshot::channel();
        oms_tx
            .send(OmsEvent::GetDelta { reply: tx })
            .await
            .unwrap();
        let delta = rx.await.unwrap();

        // inventory skew
        let skew = delta * dec!(0.05);

        let raw_bid = mid - spread / dec!(2) + skew;
        let raw_ask = mid + spread / dec!(2) + skew;

        let bid = snap_to_tick(raw_bid, dec!(1.0));
        let ask = snap_to_tick(raw_ask, dec!(1.0));

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
                price: bid,
            })
        .await
            .unwrap();

        oms_tx
            .send(OmsEvent::CreateOrder {
                side: Side::Sell,
                qty: base_qty,
                price: ask,
            })
        .await
            .unwrap();
        }
}
