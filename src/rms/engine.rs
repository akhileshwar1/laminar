use rust_decimal::Decimal;
use rust_decimal_macros::dec;
use tokio::sync::mpsc;
use tracing::{info, warn};

use crate::oms::event::OmsEvent;
use crate::oms::account::AccountSnapshot;
use crate::market::types::MarketSnapshot;
use crate::rms::types::{RiskConfig, RiskState};

pub struct RiskEngine {
    cfg: RiskConfig,
    state: RiskState,
    oms_tx: mpsc::Sender<OmsEvent>,
}

impl RiskEngine {
    pub fn new(
        start_equity: Decimal,
        cfg: RiskConfig,
        oms_tx: mpsc::Sender<OmsEvent>,
    ) -> Self {
        Self {
            cfg,
            state: RiskState {
                start_equity,
                killed: false,
            },
            oms_tx,
        }
    }

    pub async fn on_snapshot(
        &mut self,
        acct: &AccountSnapshot,
        market: &MarketSnapshot,
    ) {
        if self.state.killed {
            return;
        }

        let equity = acct.equity;
        let tick = dec!(0.0000001);

        let dd = (self.state.start_equity - equity)
            / self.state.start_equity;

        info!(
            "[RMS] drawdown : start={} equity={} dd={}",
            self.state.start_equity, equity, dd
        );

        if /* dd == dec!(0) */ dd >= self.cfg.max_drawdown_pct {
            self.state.killed = true;

            warn!(
                "[RMS] drawdown breach: start={} equity={} dd={}",
                self.state.start_equity, equity, dd
            );

            let best_bid = match market.book.bids.first() {
                Some(l) => l.price,
                None => dec!(0),
            };

            let best_ask = match market.book.asks.first() {
                Some(l) => l.price,
                None => dec!(0),
            };

            let net_position = acct.net_position;
            let is_buy = net_position < dec!(0); // short → buy to flatten

            let extreme_price = if is_buy {
                best_ask * dec!(1.05)   // cross the book upward
            } else {
                best_bid * dec!(0.95)   // cross the book downward
            };

            let _ = self.oms_tx.send(OmsEvent::RiskKill {
                reason: format!(
                    "drawdown {} >= {}",
                    dd, self.cfg.max_drawdown_pct
                ),
                qty: net_position,
                limit_px: extreme_price, // market flatten
            }).await;
        }
    }

}
