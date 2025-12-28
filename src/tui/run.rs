use tokio::sync::{mpsc, oneshot, broadcast};

use rust_decimal::Decimal;

use rust_decimal_macros::dec;
use crate::market::types::MarketEvent;
use ratatui::{Terminal, backend::CrosstermBackend};
use std::io::stdout;
use ratatui::crossterm::{terminal, execute};
use ratatui::crossterm::event::{self, Event, KeyCode};

use crate::oms::event::OmsEvent;
use crate::tui::{app::TuiApp, ui::draw};
use std::time::Duration;

fn snap_to_tick(price: Decimal, tick: Decimal) -> Decimal {
    (price / tick).floor() * tick
}

pub async fn run_tui(
    oms_tx: mpsc::Sender<OmsEvent>,
    mut market_rx: broadcast::Receiver<MarketEvent>,
) -> anyhow::Result<()> {
    terminal::enable_raw_mode()?;
        execute!(stdout(), terminal::EnterAlternateScreen)?;

        let backend = CrosstermBackend::new(stdout());
        let mut terminal = Terminal::new(backend)?;

        let mut app = TuiApp::default();

        let res = loop {
            if event::poll(Duration::from_millis(50))? {
                if let Event::Key(key) = event::read()? {
                    match key.code {
                        KeyCode::Char('q')
                            | KeyCode::Esc
                            | KeyCode::Char('c')
                            if key.modifiers.contains(event::KeyModifiers::CONTROL) =>
                            {
                                break Ok(());
                            }
                        _ => {}
                    }
                }
            }

            // poll OMS snapshot
            let (tx, rx) = tokio::sync::oneshot::channel();
            let _ = oms_tx.send(OmsEvent::GetSnapshot { reply: tx }).await;
            if let Ok(snapshot) = rx.await {
                app.snapshot = Some(snapshot);
                let snapshot = match market_rx.try_recv() {
                    Ok(MarketEvent::Snapshot(s)) => s,
                    _ => continue,
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
                let spread = dec!(20)*(best_ask - best_bid);

                // query inventory delta
                let (tx, rx) = oneshot::channel();
                oms_tx
                    .send(OmsEvent::GetDelta { reply: tx })
                    .await
                    .unwrap();
                let delta = rx.await.unwrap();
                let skew = delta * dec!(0.05);

                let raw_bid = mid - spread / dec!(2) + skew;
                let raw_ask = mid + spread / dec!(2) + skew;

                let bid = snap_to_tick(raw_bid, dec!(1));
                let ask = snap_to_tick(raw_ask, dec!(1));

                app.mid = mid;
                app.spread = spread;
                app.skew = skew;
                app.bid = bid;
                app.ask = ask;
            }

            terminal.draw(|f| draw(f, &app))?;

            tokio::time::sleep(Duration::from_millis(100)).await;
        };
        terminal::disable_raw_mode()?;
        execute!(stdout(), terminal::LeaveAlternateScreen)?;
        res
    }
