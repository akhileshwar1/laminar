use std::time::{Duration, Instant};

use rust_decimal::Decimal;
use rust_decimal_macros::dec;
use tokio::sync::{broadcast, mpsc, oneshot};
use tracing::info;

use crate::market::types::MarketSnapshot;
use crate::oms::event::OmsEvent;
use crate::oms::order::Side;

const MIN_NOTIONAL: Decimal = dec!(10);
const SAFETY_MARGIN: Decimal = dec!(0.85);
const MAX_ABS_QTY: Decimal = dec!(650);

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
    let k = dec!(0.5); // aggressiveness

    let bid_ratio = (dec!(1.0) + delta * k)
        .clamp(dec!(0.0), dec!(2.0));

    let ask_ratio = (dec!(2.0) - bid_ratio)
        .clamp(dec!(0.0), dec!(2.0));

    (bid_ratio, ask_ratio)
}

pub async fn run_mm_strategy(
    mut market_rx: broadcast::Receiver<MarketSnapshot>,
    oms_tx: mpsc::Sender<OmsEvent>,
) {
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

        let mid = (best_bid + best_ask) / dec!(2);
        let tick = dec!(0.0000001);

        // --- account snapshot ---
        let (tx, rx) = oneshot::channel();
        let _ = oms_tx
            .send(OmsEvent::GetAccountSnapshot { reply: tx })
            .await;

        let account = match rx.await {
            Ok(a) => a,
            Err(_) => continue,
        };

        let net_pos = account.net_position;

        // --- margin-derived base qty ---
        let max_qty_by_margin =
            (account.available_margin / mid) * SAFETY_MARGIN;

        let base_qty = max_qty_by_margin.min(MAX_ABS_QTY);

        // --- no margin but position exists → flatten ---
        if base_qty * mid  <= dec!(10) {
            info!("[MM] MARGIN EXHAUSTED");
            if net_pos != dec!(0) {
                info!(
                    "[MM] margin exhausted, flattening position {}",
                    net_pos
                );
                let is_buy = net_pos < dec!(0); // short → buy to flatten

                let extreme_price = if is_buy {
                    best_ask * dec!(1.05)   // cross the book upward
                } else {
                    best_bid * dec!(0.95)   // cross the book downward
                };
                let _ = oms_tx
                    .send(OmsEvent::Flatten {
                        qty: net_pos,
                        limit_px : extreme_price,
                    })
                    .await;
            }
            continue;
        }

        // --- spread ---
        let spread_pct = dec!(0.0005);
        let half_spread = mid * spread_pct / dec!(2);


        let bid = snap_to_tick(mid - half_spread, tick)
            .min(snap_to_tick(best_bid - tick, tick));

        let ask = snap_to_tick(mid + half_spread, tick)
            .max(snap_to_tick(best_ask + tick, tick));

        if bid >= ask {
            continue;
        }

        // --- inventory skew ---
        let (tx, rx) = oneshot::channel();
        let _ = oms_tx.send(OmsEvent::GetDelta { reply: tx }).await;
        let delta = rx.await.unwrap_or(dec!(0));

        let (bid_ratio, ask_ratio) = inventory_ratios(delta);

        let raw_bid_qty = base_qty * bid_ratio;
        let raw_ask_qty = base_qty * ask_ratio;

        // --- per-side margin caps ---
        let max_bid_qty = (account.available_margin * SAFETY_MARGIN) / bid;
        let max_ask_qty = (account.available_margin * SAFETY_MARGIN) / ask;

        let bid_qty = raw_bid_qty
            .min(max_bid_qty)
            .max(dec!(0));

        let ask_qty = raw_ask_qty
            .min(max_ask_qty)
            .max(dec!(0));

        // --- minimum notional filter ---
        let bid_qty = if bid_qty * bid >= MIN_NOTIONAL {
            bid_qty
        } else {
            dec!(0)
        };

        let ask_qty = if ask_qty * ask >= MIN_NOTIONAL {
            ask_qty
        } else {
            dec!(0)
        };

        if bid_qty == dec!(0) && ask_qty == dec!(0) {
            info!("[MM] SKIPPING: Quotes are 0");
            continue;
        }

        // --- refresh gating ---
        let price_moved = match (last_bid, last_ask) {
            (Some(lb), Some(la)) => {
                pct_change(bid, lb) > min_pct_move
                    || pct_change(ask, la) > min_pct_move
            }
            _ => true,
        };

        let time_expired = last_refresh.elapsed() >= min_refresh_interval;

        if !(price_moved || time_expired) {
            continue;
        }

        last_bid = Some(bid);
        last_ask = Some(ask);
        last_refresh = Instant::now();

        info!(
            "[MM] quote bid={}({}) ask={}({}) delta={}",
            bid, bid_qty, ask, ask_qty, delta
        );

        // --- cancel + re-quote ---
        let _ = oms_tx.send(OmsEvent::CancelAll).await;

        if bid_qty > dec!(0) {
            let _ = oms_tx
                .send(OmsEvent::CreateOrder {
                    side: Side::Buy,
                    qty: bid_qty,
                    price: bid,
                })
                .await;
        }

        if ask_qty > dec!(0) {
            let _ = oms_tx
                .send(OmsEvent::CreateOrder {
                    side: Side::Sell,
                    qty: ask_qty,
                    price: ask,
                })
                .await;
        }
    }
}
