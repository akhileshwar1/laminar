use std::time::Duration;
use rust_decimal_macros::dec;
use tokio::sync::{mpsc, oneshot, broadcast};
use tokio::time::interval;
use tracing::{info, warn};

use crate::market::types::MarketSnapshot;
use crate::oms::event::OmsEvent;
use crate::rms::engine::RiskEngine;
use crate::rms::types::RiskConfig;

/// Periodically polls account state and feeds RMS.
/// This task NEVER places orders directly.
pub async fn start_rms_driver(
    mut market_rx: broadcast::Receiver<MarketSnapshot>,
    oms_tx: mpsc::Sender<OmsEvent>,
    poll_interval: Duration,
) -> anyhow::Result<()> {

    // --- wait for first market snapshot ---
    let mut last_market = market_rx.recv().await?;

    // --- initial account snapshot (baseline equity) ---
    let start_account = {
        let (tx, rx) = oneshot::channel();
        let _ = oms_tx.send(OmsEvent::GetAccountSnapshot { reply: tx }).await;
        rx.await?
    };

    info!("[RMS] starting with equity={}", start_account.equity);

    let mut rms = RiskEngine::new(
        start_account.equity,
        RiskConfig {
            max_drawdown_pct: dec!(0.10), // 10%
        },
        oms_tx.clone(),
    );

    let mut ticker = interval(poll_interval);

    loop {
        tokio::select! {
            // update market snapshot continuously
            Ok(mkt) = market_rx.recv() => {
                last_market = mkt;
            }

            // periodic risk evaluation
            _ = ticker.tick() => {
                let (tx, rx) = oneshot::channel();
                let _ = oms_tx.send(OmsEvent::GetAccountSnapshot { reply: tx }).await;

                match rx.await {
                    Ok(acct) => {
                        rms.on_snapshot(&acct, &last_market).await;
                    }
                    Err(e) => {
                        warn!("[RMS] failed to fetch account snapshot: {:?}", e);
                    }
                }
            }
        }
    }
}
