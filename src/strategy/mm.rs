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

fn inventory_ratios(delta: Decimal) -> (Decimal, Decimal) {
    // delta = target - current position
    // target is 0, so delta < 0 => net short

    let max_ratio = dec!(2.0);
    let min_ratio = dec!(0.0);

    let k = dec!(0.5); // aggressiveness

    let bid_ratio = (dec!(1.0) + (delta * k))
        .clamp(min_ratio, max_ratio);

    let ask_ratio = (dec!(2.0) - bid_ratio)
        .clamp(min_ratio, max_ratio);

    (bid_ratio, ask_ratio)
}

pub async fn run_mm_strategy(
    mut market_rx: broadcast::Receiver<MarketSnapshot>,
    oms_tx: mpsc::Sender<OmsEvent>,
) {
    // let base_qty = dec!(0.00013);
    // --- refresh controls ---
    let min_pct_move = dec!(0.0002); // 2 bps
    let min_refresh_interval = Duration::from_secs(10);

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

        info!("best_bid {} best_ask {}", best_bid, best_ask);

        let (tx, rx) = oneshot::channel();
        let _ = oms_tx.send(OmsEvent::GetAccountSnapshot { reply: tx }).await;

        let account = match rx.await {
            Ok(a) => a,
            Err(_) => continue,
        };
        info!("account : net_position {}", account.net_position);

        let mid = (best_bid + best_ask) / dec!(2);
        let safety = dec!(0.9);
        let max_qty = (account.available_margin / mid) * safety;

        // optional absolute cap
        let base_qty = max_qty.min(dec!(650));

        if base_qty <= dec!(0) {
            info!("[MM] no margin available, skipping quote");
            continue;
        }

        // let spread = dec!(20)*(best_ask - best_bid);
        let spread_pct = dec!(0.0005);
        let abs_spread = mid * spread_pct;
        let half_spread = abs_spread / dec!(2);

        // inventory delta
        let (tx, rx) = oneshot::channel();
        let _ = oms_tx.send(OmsEvent::GetDelta { reply: tx }).await;
        let delta = rx.await.unwrap_or(dec!(0));

        let tick = dec!(0.0000001);

        let bid = snap_to_tick(mid - half_spread, tick)
            .min(snap_to_tick(best_bid - tick, tick));

        let ask = snap_to_tick(mid + half_spread, tick)
            .max(snap_to_tick(best_ask + tick, tick));

        if bid >= ask {
            continue;
        }
        let (bid_ratio, ask_ratio) = inventory_ratios(delta);
        info!("bid_qty {} ask_qty {}",base_qty *bid_ratio, base_qty *ask_ratio);

        let bid_qty = (base_qty * bid_ratio)
            .max(dec!(0));

        let ask_qty = (base_qty * ask_ratio)
            .max(dec!(0));


        info!("bid_qty {} ask_qty {}",base_qty *bid_ratio, base_qty *ask_ratio);

        if bid_qty == dec!(0) && ask_qty == dec!(0) {
            continue;
        }
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
                qty: bid_qty,
                price: bid,
            })
            .await;

        let _ = oms_tx
            .send(OmsEvent::CreateOrder {
                side: Side::Sell,
                qty: ask_qty,
                price: ask,
            })
            .await;
    }
}
