use std::collections::VecDeque;
use std::time::{Duration, Instant};

use rust_decimal::Decimal;
use rust_decimal_macros::dec;
use tokio::sync::{broadcast, mpsc, oneshot};
use tracing::info;

use crate::market::types::{MarketEvent, Trade, AggressorSide};
use crate::oms::event::OmsEvent;
use crate::oms::order::Side;

const MIN_NOTIONAL: Decimal = dec!(10);
const SAFETY_MARGIN: Decimal = dec!(0.85);
const MAX_ABS_QTY: Decimal = dec!(650);

// ---- toxic flow ----
const FLOW_WINDOW: usize = 12;
const FLOW_IMBALANCE_THRESH: Decimal = dec!(0.65); // 65% one-sided

#[derive(Debug, Clone, Copy, PartialEq)]
enum Flow {
    Up,
    Down,
    Neutral,
}

struct TradeFlow {
    trades: VecDeque<Trade>,
    flow: Flow,
}

impl TradeFlow {
    fn new() -> Self {
        Self {
            trades: VecDeque::with_capacity(FLOW_WINDOW),
            flow: Flow::Neutral,
        }
    }

    fn on_trade(&mut self, t: Trade) {
        info!("[MM] ON Trade");
        if self.trades.len() == FLOW_WINDOW {
            self.trades.pop_front();
        }
        self.trades.push_back(t);
        self.recompute();
    }

    fn recompute(&mut self) {
        if self.trades.len() < FLOW_WINDOW {
            info!("[MM] Length less skipping!!!!");
            self.flow = Flow::Neutral;
            return;
        }

        let mut buy_notional = dec!(0);
        let mut sell_notional = dec!(0);

        for t in &self.trades {
            let notional = t.price * t.qty;
            if t.side == AggressorSide::Buy {
                buy_notional += notional;
                info!("[MM] add buy notional {} {}", buy_notional, notional);
            } else {
                sell_notional += notional;
                info!("[MM] add sell notional {} {}", sell_notional, notional);
            }
        }

        let total = buy_notional + sell_notional;

        info!("[MM] total notional {} ", total);

        if total == dec!(0) {
            info!("[MM] total notiional 0 skipping!!!!");
            self.flow = Flow::Neutral;
            return;
        }

        let buy_ratio = buy_notional / total;
        let sell_ratio = sell_notional / total;
        info!("[MM] buy ratio and sell ratio {} {}", buy_ratio, sell_ratio);

        self.flow = if buy_ratio >= FLOW_IMBALANCE_THRESH {
            Flow::Up
        } else if sell_ratio >= FLOW_IMBALANCE_THRESH {
            Flow::Down
        } else {
            Flow::Neutral
        };
    }
}

// ---- helpers ----

fn snap_to_tick(price: Decimal, tick: Decimal) -> Decimal {
    (price / tick).floor() * tick
}

fn pct_change(a: Decimal, b: Decimal) -> Decimal {
    if b == dec!(0) {
        dec!(0)
    } else {
        (a - b) / b
    }
}

fn inventory_ratios(delta: Decimal) -> (Decimal, Decimal) {
    let k = dec!(0.5);

    let bid_ratio = (dec!(1.0) + delta * k)
        .clamp(dec!(0.0), dec!(2.0));

    let ask_ratio = (dec!(2.0) - bid_ratio)
        .clamp(dec!(0.0), dec!(2.0));

    (bid_ratio, ask_ratio)
}

// ---- MM loop ----

pub async fn run_mm_strategy(
    mut market_rx: broadcast::Receiver<MarketEvent>,
    oms_tx: mpsc::Sender<OmsEvent>,
) {
    let min_pct_move = dec!(0.0002);
    let min_refresh_interval = Duration::from_millis(1000);

    let mut last_bid: Option<Decimal> = None;
    let mut last_ask: Option<Decimal> = None;
    let mut last_refresh = Instant::now() - min_refresh_interval;

    let mut flow_engine = TradeFlow::new();

    loop {
        let event = match market_rx.recv().await {
            Ok(e) => e,
            Err(_) => continue,
        };

        match event {
            MarketEvent::Trade(t) => {
                info!("[MM] Trade is: {:?}", t);
                flow_engine.on_trade(t);
                continue;
            }

            MarketEvent::Snapshot(snapshot) => {
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

                // --- margin sizing ---
                let max_qty =
                    (account.available_margin / mid) * SAFETY_MARGIN;

                let base_qty = max_qty.min(MAX_ABS_QTY);

                if base_qty * mid < MIN_NOTIONAL {
                    if net_pos != dec!(0) {
                        info!("[MM] margin exhausted, flatten {}", net_pos);

                        let is_buy = net_pos < dec!(0);
                        let px = if is_buy {
                            best_ask * dec!(1.05)
                        } else {
                            best_bid * dec!(0.95)
                        };

                        let _ = oms_tx
                            .send(OmsEvent::Flatten {
                                qty: net_pos,
                                limit_px: px,
                            })
                        .await;
                        }
                    continue;
                }

                // --- spread ---
                let half_spread = mid * dec!(0.0005) / dec!(2);

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

                let mut bid_qty = base_qty * bid_ratio;
                let mut ask_qty = base_qty * ask_ratio;

                // --- TOXIC FLOW GATING ---
                match flow_engine.flow {
                    Flow::Up => {
                        info!("[MM] TOXIC BUY FLOW → disabling sells");
                        ask_qty = dec!(0);
                    }
                    Flow::Down => {
                        info!("[MM] TOXIC SELL FLOW → disabling buys");
                        bid_qty = dec!(0);
                    }
                    Flow::Neutral => {
                        info!("[MM] NEUTRAL FLOW ");
                    }
                }

                // --- margin caps ---
                bid_qty = bid_qty
                    .min((account.available_margin * SAFETY_MARGIN) / bid)
                    .max(dec!(0));

                ask_qty = ask_qty
                    .min((account.available_margin * SAFETY_MARGIN) / ask)
                    .max(dec!(0));

                if bid_qty * bid < MIN_NOTIONAL {
                    bid_qty = dec!(0);
                }
                if ask_qty * ask < MIN_NOTIONAL {
                    ask_qty = dec!(0);
                }

                if bid_qty == dec!(0) && ask_qty == dec!(0) {
                    continue;
                }

                let price_moved = match (last_bid, last_ask) {
                    (Some(lb), Some(la)) => {
                        pct_change(bid, lb).abs() > min_pct_move
                            || pct_change(ask, la).abs() > min_pct_move
                    }
                    _ => true,
                };

                if !price_moved && last_refresh.elapsed() < min_refresh_interval {
                    continue;
                }

                last_bid = Some(bid);
                last_ask = Some(ask);
                last_refresh = Instant::now();

                info!(
                    "[MM][{:?}] bid={}({}) ask={}({}) delta={}",
                    flow_engine.flow, bid, bid_qty, ask, ask_qty, delta
                );

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
    }
}
