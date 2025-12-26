use rust_decimal_macros::dec;
use tokio::time::{sleep, Duration};

use laminar::oms::event::OmsEvent;
use laminar::oms::runtime::start_oms;
use laminar::strategy::mm::run_mm_strategy;
use laminar::rms::driver::start_rms_driver;

use laminar::market::hyperliquid::HyperliquidMarket;
use laminar::market::MarketAdapter;
use tracing::{info, warn, error};

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    info!("[MAIN] starting laminar");
    use tracing_subscriber::fmt;

    fmt()
        .with_writer(std::fs::File::create("laminar.log").unwrap())
        .with_ansi(false)
        .init();

    let oms = start_oms().await;
    let tx = oms.sender();

    // set an initial target
    tx.send(OmsEvent::SetTarget { qty: dec!(0) })
        .await
        .unwrap();

    let market = HyperliquidMarket::new("TST").await?;
    market.start();

    let market_rx = market.subscribe();
    let market_rx_tui = market.subscribe();
    let market_rx_rms= market.subscribe();

    // start strategy loop
    tokio::spawn(run_mm_strategy(market_rx, tx.clone()));

    tokio::spawn(
        start_rms_driver(
            market_rx_rms,
            tx.clone(),
            Duration::from_secs(5),
        )
    );

    // run tui loop
    tokio::spawn(laminar::tui::run::run_tui(tx.clone(), market_rx_tui));

    // let it run
    tokio::signal::ctrl_c().await?;
    // sleep(Duration::from_secs(5)).await;

    info!("[MAIN] exiting");
    Ok(())
}
