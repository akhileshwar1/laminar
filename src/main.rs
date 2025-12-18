use rust_decimal_macros::dec;
use tokio::time::{sleep, Duration};

use laminar::oms::event::OmsEvent;
use laminar::oms::runtime::start_oms;
use laminar::strategy::mm::run_mm_strategy;

#[tokio::main]
async fn main() {
    println!("[MAIN] starting laminar");

    let oms = start_oms();
    let tx = oms.sender();

    // set an initial target
    tx.send(OmsEvent::SetTarget { qty: dec!(1.0) })
        .await
        .unwrap();

    // start strategy loop
    tokio::spawn(run_mm_strategy(tx.clone()));

    // let it run
    sleep(Duration::from_secs(5)).await;

    println!("[MAIN] exiting");
}
