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
    MarketSnapshot,
    OrderBook,
    BookLevel,
};
use crate::market::MarketAdapter;

pub struct HyperliquidMarket {
    symbol: String,
    tx: broadcast::Sender<MarketSnapshot>,
}

impl HyperliquidMarket {
    pub async fn new(symbol: &str) -> anyhow::Result<Self> {
        let (tx, _) = broadcast::channel(1024);

        Ok(Self {
            symbol: symbol.to_string(),
            tx,
        })
    }
}

impl MarketAdapter for HyperliquidMarket {
    fn subscribe(&self) -> broadcast::Receiver<MarketSnapshot> {
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

            info.subscribe(
                Subscription::L2Book { coin: symbol.clone() },
                msg_tx,
            )
            .await
            .expect("failed to subscribe to L2Book");

            info!("Hyperliquid L2Book subscribed for {}", symbol);

            while let Some(msg) = msg_rx.recv().await {
                match msg {
                    Message::L2Book(book) => {
                        // HL: levels[0] = bids, levels[1] = asks
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

                        let _ = tx.send(snapshot);
                    }

                    Message::NoData => {
                        warn!("Hyperliquid stream returned NoData");
                    }

                    Message::HyperliquidError(err) => {
                        warn!("Hyperliquid error: {}", err);
                    }

                    _ => {
                        // Ignore unrelated messages
                    }
                }
            }

            warn!("Hyperliquid market stream exited for {}", symbol);
        });
    }
}
