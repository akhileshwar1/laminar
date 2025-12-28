#![cfg(feature = "hyperliquid")]

use tokio::sync::{broadcast, mpsc};
use tracing::{info, warn};

use rust_decimal::Decimal;
use rust_decimal::prelude::FromStr;

use hyperliquid_rust_sdk::{
    InfoClient,
    Subscription,
    Message,
};

use crate::market::types::{
    MarketEvent,
    MarketSnapshot,
    OrderBook,
    BookLevel,
    Trade,
    AggressorSide,
};
use crate::market::MarketAdapter;

pub struct HyperliquidMarket {
    symbol: String,
    tx: broadcast::Sender<MarketEvent>,
}

impl HyperliquidMarket {
    pub async fn new(symbol: &str) -> anyhow::Result<Self> {
        let (tx, _) = broadcast::channel(4096);

        Ok(Self {
            symbol: symbol.to_string(),
            tx,
        })
    }
}

impl MarketAdapter for HyperliquidMarket {
    fn subscribe(&self) -> broadcast::Receiver<MarketEvent> {
        self.tx.subscribe()
    }

    fn start(&self) {
        let symbol = self.symbol.clone();
        let tx = self.tx.clone();

        tokio::spawn(async move {
            let mut info = InfoClient::with_reconnect(None, None)
                .await
                .expect("failed to create InfoClient");

            let (msg_tx, mut msg_rx) = mpsc::unbounded_channel();

            // ---- L2 book ----
            info.subscribe(
                Subscription::L2Book { coin: symbol.clone() },
                msg_tx.clone(),
            )
            .await
            .expect("failed to subscribe to L2Book");

            // ---- Trades (tape) ----
            info.subscribe(
                Subscription::Trades { coin: symbol.clone() },
                msg_tx,
            )
            .await
            .expect("failed to subscribe to Trades");

            info!("HL market subscribed (L2 + Trades) for {}", symbol);

            while let Some(msg) = msg_rx.recv().await {
                match msg {
                    // -------- L2 BOOK --------
                    Message::L2Book(book) => {
                        let bids = book.data.levels[0]
                            .iter()
                            .filter_map(|l| {
                                Some(BookLevel {
                                    price: Decimal::from_str(&l.px).ok()?,
                                    qty: Decimal::from_str(&l.sz).ok()?,
                                })
                            })
                            .collect::<Vec<_>>();

                        let asks = book.data.levels[1]
                            .iter()
                            .filter_map(|l| {
                                Some(BookLevel {
                                    price: Decimal::from_str(&l.px).ok()?,
                                    qty: Decimal::from_str(&l.sz).ok()?,
                                })
                            })
                            .collect::<Vec<_>>();

                        let snapshot = MarketSnapshot {
                            symbol: symbol.clone(),
                            book: OrderBook { bids, asks },
                            timestamp_ms: book.data.time,
                        };

                        let _ = tx.send(MarketEvent::Snapshot(snapshot));
                    }

                    // -------- TRADE --------
                    Message::Trades(trades) => {
                        for t in trades.data.iter() {
                            let price = match Decimal::from_str(&t.px) {
                                Ok(p) => p,
                                Err(_) => continue,
                            };

                            let qty = match Decimal::from_str(&t.sz) {
                                Ok(q) => q,
                                Err(_) => continue,
                            };

                            // info!("[MARKET] actual trade {:?}", t);
                            let side = match t.side.as_str() {
                                "B" => AggressorSide::Buy,
                                "A" => AggressorSide::Sell,
                                _ => continue,
                            };

                            let trade = Trade {
                                symbol: symbol.clone(),
                                price,
                                qty,
                                side,
                                timestamp_ms: t.time,
                            };

                            let _ = tx.send(MarketEvent::Trade(trade));
                        }
                    }

                    Message::NoData => {
                        warn!("HL stream returned NoData");
                    }

                    Message::HyperliquidError(err) => {
                        warn!("HL error: {}", err);
                    }

                    _ => {}
                }
            }

            warn!("HL market stream exited for {}", symbol);
        });
    }
}
