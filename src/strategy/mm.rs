use std::time::{Duration, Instant};
use rust_decimal::Decimal;
use rust_decimal_macros::dec;
use tokio::sync::{mpsc, oneshot, broadcast};
use tracing::info;

use crate::market::types::MarketSnapshot;
use crate::oms::event::OmsEvent;
use crate::oms::order::Side;

fn snap_to_tick(price: Decimal, tick: Decimal) -> Decimal {
    (price / tick).floor() * tick
}

fn pct_change(a: Decimal, b: Decimal) -> Decimal {
    if b == dec!(0) {
        dec!(0)
    } else {
        (a - b).abs() / b
    }
}

pub async fn run_mm_strategy(
    mut market_rx: broadcast::Receiver<MarketSnapshot>,
    oms_tx: mpsc::Sender<OmsEvent>,
) {
    let base_qty = dec!(0.001);

    // --- refresh controls ---
    let min_pct_move = dec!(0.0002); // 2 bps
    let min_refresh_interval = Duration::from_secs(2);

    let mut last_bid: Option<Decimal> = None;
    let mut last_ask: Option<Decimal> = None;
    let mut last_refresh = Instant::now() - min_refresh_interval;

    loop {
        let snapshot = match market_rx.recv().await {
            Ok(s) => s,
            Err(_) => continue,
        };

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

        // inventory delta
        let (tx, rx) = oneshot::channel();
        let _ = oms_tx.send(OmsEvent::GetDelta { reply: tx }).await;
        let delta = rx.await.unwrap_or(dec!(0));

        let skew = delta * dec!(0.05);

        let bid = snap_to_tick(mid - spread / dec!(2) + skew, dec!(1));
        let ask = snap_to_tick(mid + spread / dec!(2) + skew, dec!(1));

        // --- refresh decision ---
        let price_moved = match (last_bid, last_ask) {
            (Some(lb), Some(la)) => {
                pct_change(bid, lb) > min_pct_move
                    || pct_change(ask, la) > min_pct_move
            }
            _ => true,
        };

        let time_expired = last_refresh.elapsed() >= min_refresh_interval;

        if !(price_moved || time_expired) {
            continue; // let quotes rest
        }

        info!(
            "[MM] refresh mid={} delta={} bid={} ask={}",
            mid, delta, bid, ask
        );

        last_bid = Some(bid);
        last_ask = Some(ask);
        last_refresh = Instant::now();

        // cancel + re-quote
        let _ = oms_tx.send(OmsEvent::CancelAll).await;

        let _ = oms_tx
            .send(OmsEvent::CreateOrder {
                side: Side::Buy,
                qty: base_qty,
                price: bid,
            })
            .await;

        let _ = oms_tx
            .send(OmsEvent::CreateOrder {
                side: Side::Sell,
                qty: base_qty,
                price: ask,
            })
            .await;
    }
}
