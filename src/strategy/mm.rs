use std::collections::VecDeque;
use rust_decimal::prelude::Signed;
use std::time::{Duration, Instant};

use rust_decimal::Decimal;
use rust_decimal_macros::dec;
use tokio::sync::{broadcast, mpsc, oneshot};
use tracing::info;

use crate::market::types::{MarketEvent, Trade, AggressorSide};
use crate::oms::event::OmsEvent;
use crate::oms::order::Side;

/* ===================== CONSTANTS ===================== */

const MIN_NOTIONAL: Decimal = dec!(10);
const SAFETY_MARGIN: Decimal = dec!(0.85);
const MAX_ABS_QTY: Decimal = dec!(1160);

const FLOW_WINDOW: usize = 12;
const FLOW_IMBALANCE_THRESH: Decimal = dec!(0.65);
const TOXIC_COOLDOWN: Duration = Duration::from_secs(6);

const MIN_PCT_MOVE: Decimal = dec!(0.0010);
const REFRESH_INTERVAL: Duration = Duration::from_secs(10);

/* ===================== FLOW ===================== */

#[derive(Debug, Clone, Copy, PartialEq)]
enum Flow {
    Up,
    Down,
    Neutral,
    Cooldown,
}

struct TradeFlow {
    trades: VecDeque<Trade>,
    mids: VecDeque<Decimal>,          // <-- summary fix
    flow: Flow,
    last_toxic: Option<Instant>,
}

impl TradeFlow {
    fn new() -> Self {
        Self {
            trades: VecDeque::with_capacity(FLOW_WINDOW),
            mids: VecDeque::with_capacity(FLOW_WINDOW),
            flow: Flow::Neutral,
            last_toxic: None,
        }
    }

    fn on_trade(&mut self, t: Trade) {
        if self.trades.len() == FLOW_WINDOW {
            self.trades.pop_front();
        }
        self.trades.push_back(t);
    }

    fn on_mid(&mut self, mid: Decimal) {
        if self.mids.len() == FLOW_WINDOW {
            self.mids.pop_front();
        }
        self.mids.push_back(mid);
        self.recompute();
    }

    fn recompute(&mut self) {
        if self.trades.len() < FLOW_WINDOW || self.mids.len() < FLOW_WINDOW {
            self.flow = Flow::Neutral;
            return;
        }

        /* -------- SUMMARY FIX: PRICE DRIFT -------- */

        let first = self.mids.front().unwrap();
        let last = self.mids.back().unwrap();
        let drift = pct_change(*last, *first);

        if drift.abs() >= MIN_PCT_MOVE {
            self.flow = if drift > dec!(0) {
                info!("[MM] PRICE DRIFT UP {}", drift);
                Flow::Up
            } else {
                info!("[MM] PRICE DRIFT DOWN {}", drift);
                Flow::Down
            };
            self.last_toxic = Some(Instant::now());
            return;
        }

        /* -------- SIGNED FLOW FIX -------- */

        let mut signed_impact = dec!(0);
        for t in &self.trades {
            let ntl = t.price * t.qty;
            match t.side {
                AggressorSide::Buy => signed_impact += ntl,
                AggressorSide::Sell => signed_impact -= ntl,
            }
        }

        if signed_impact.abs() > dec!(0) {
            let signed_dir = signed_impact.signum();
            let price_dir = drift.signum();

            if signed_dir == price_dir && signed_dir != dec!(0) {
                self.flow = if signed_dir > dec!(0) {
                    info!("[MM] SIGNED IMPACT UP {}", signed_impact);
                    Flow::Up
                } else {
                    info!("[MM] SIGNED IMPACT DOWN {}", signed_impact);
                    Flow::Down
                };
                self.last_toxic = Some(Instant::now());
                return;
            }
        }

        /* -------- FALLBACK: NOTIONAL IMBALANCE -------- */

        let mut buy = dec!(0);
        let mut sell = dec!(0);

        for t in &self.trades {
            let ntl = t.price * t.qty;
            match t.side {
                AggressorSide::Buy => buy += ntl,
                AggressorSide::Sell => sell += ntl,
            }
        }

        let total = buy + sell;
        if total == dec!(0) {
            self.flow = Flow::Neutral;
            return;
        }

        let buy_ratio = buy / total;
        let sell_ratio = sell / total;

        if buy_ratio >= FLOW_IMBALANCE_THRESH {
            info!("[MM] NOTIONAL BUY IMBALANCE {}", buy_ratio);
            self.flow = Flow::Up;
            self.last_toxic = Some(Instant::now());
        } else if sell_ratio >= FLOW_IMBALANCE_THRESH {
            info!("[MM] NOTIONAL SELL IMBALANCE {}", sell_ratio);
            self.flow = Flow::Down;
            self.last_toxic = Some(Instant::now());
        } else {
            self.flow = Flow::Neutral;
        }
    }

    fn effective_flow(&self) -> Flow {
        match self.flow {
            Flow::Neutral => {
                if let Some(ts) = self.last_toxic {
                    if ts.elapsed() < TOXIC_COOLDOWN {
                        info!("[MM] COOLDOWN ACTIVE");
                        return Flow::Cooldown;
                    }
                }
                Flow::Neutral
            }
            f => f,
        }
    }
}

/* ===================== HELPERS ===================== */

fn snap_to_tick(px: Decimal, tick: Decimal) -> Decimal {
    (px / tick).floor() * tick
}

fn pct_change(a: Decimal, b: Decimal) -> Decimal {
    if b == dec!(0) { dec!(0) } else { (a - b) / b }
}

fn inventory_ratios(delta: Decimal) -> (Decimal, Decimal) {
    let k = dec!(0.5);
    let bid = (dec!(1.0) + delta * k).clamp(dec!(0), dec!(2));
    let ask = (dec!(2.0) - bid).clamp(dec!(0), dec!(2));
    (bid, ask)
}

/* ===================== MM LOOP ===================== */

pub async fn run_mm_strategy(
    mut market_rx: broadcast::Receiver<MarketEvent>,
    oms_tx: mpsc::Sender<OmsEvent>,
) {
    let tick = dec!(0.0000001);

    let mut last_bid: Option<Decimal> = None;
    let mut last_ask: Option<Decimal> = None;
    let mut last_refresh = Instant::now() - REFRESH_INTERVAL;

    let mut flow = TradeFlow::new();

    loop {
        let event = match market_rx.recv().await {
            Ok(e) => e,
            Err(_) => continue,
        };

        match event {
            MarketEvent::Trade(t) => {
                info!("[MM] ON Trade {:?}", t);
                flow.on_trade(t);
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
                flow.on_mid(mid);

                /* -------- ACCOUNT -------- */

                let (tx, rx) = oneshot::channel();
                let _ = oms_tx.send(OmsEvent::GetAccountSnapshot { reply: tx }).await;
                let acct = match rx.await { Ok(a) => a, Err(_) => continue };

                let available_margin = acct.available_margin * SAFETY_MARGIN;

                /* -------- SPREAD -------- */

                let half_spread = mid * dec!(0.0005) / dec!(2);
                let bid = snap_to_tick(mid - half_spread, tick)
                    .min(snap_to_tick(best_bid - tick, tick));
                let ask = snap_to_tick(mid + half_spread, tick)
                    .max(snap_to_tick(best_ask + tick, tick));

                if bid >= ask {
                    continue;
                }

                /* -------- INVENTORY -------- */

                let (tx, rx) = oneshot::channel();
                let _ = oms_tx.send(OmsEvent::GetDelta { reply: tx }).await;
                let delta = rx.await.unwrap_or(dec!(0));

                let (bid_ratio, ask_ratio) = inventory_ratios(delta);
                info!("[MM] ratios are {} {}", bid_ratio, ask_ratio);

                /* -------- FLOW -------- */

                let effective_flow = flow.effective_flow();

                let (mut bid_w, mut ask_w) = match effective_flow {
                    Flow::Up => {
                        info!("[MM] Up — no sell quoting");
                        (bid_ratio, dec!(0))
                    }
                    Flow::Down => {
                        info!("[MM] Down — no buy quoting");
                        (dec!(0), ask_ratio)
                    }
                    Flow::Cooldown => {
                        info!("[MM] Cooldown — no quoting");
                        (dec!(0), dec!(0))
                    }
                    Flow::Neutral => {
                        info!("[MM] Neutral — quoting");
                        (bid_ratio, ask_ratio)
                    }
                };

                let weight_sum = bid_w + ask_w;
                if weight_sum == dec!(0) {
                    info!("[MM] skipping due to 0 weight");
                    continue;
                }

                bid_w /= weight_sum;
                ask_w /= weight_sum;

                /* -------- MARGIN -------- */

                let max_total_qty = (available_margin / mid).min(MAX_ABS_QTY);
                let mut bid_qty = max_total_qty * bid_w;
                let mut ask_qty = max_total_qty * ask_w;

                bid_qty = bid_qty.min(available_margin / bid);
                ask_qty = ask_qty.min(available_margin / ask);

                info!("[MM] qts are {} {} {}", max_total_qty, bid_qty, ask_qty);

                if bid_qty * bid < MIN_NOTIONAL {
                    bid_qty = dec!(0);
                }
                if ask_qty * ask < MIN_NOTIONAL {
                    ask_qty = dec!(0);
                }

                if bid_qty == dec!(0) && ask_qty == dec!(0) {
                    info!("[MM] skipping due to qts 0");
                    continue;
                }

                /* -------- REFRESH -------- */

                let price_moved = match (last_bid, last_ask) {
                    (Some(lb), Some(la)) =>
                        pct_change(bid, lb).abs() > MIN_PCT_MOVE ||
                        pct_change(ask, la).abs() > MIN_PCT_MOVE,
                    _ => true,
                };

                if !price_moved && last_refresh.elapsed() < REFRESH_INTERVAL {
                    continue;
                }

                last_bid = Some(bid);
                last_ask = Some(ask);
                last_refresh = Instant::now();

                info!(
                    "[MM][{:?}] bid={}({}) ask={}({}) delta={}",
                    effective_flow, bid, bid_qty, ask, ask_qty, delta
                );

                /* -------- EXEC -------- */

                let _ = oms_tx.send(OmsEvent::CancelAll).await;

                if bid_qty > dec!(0) {
                    let _ = oms_tx.send(OmsEvent::CreateOrder {
                        side: Side::Buy,
                        qty: bid_qty,
                        price: bid,
                    }).await;
                }

                if ask_qty > dec!(0) {
                    let _ = oms_tx.send(OmsEvent::CreateOrder {
                        side: Side::Sell,
                        qty: ask_qty,
                        price: ask,
                    }).await;
                }
            }
        }
    }
}
