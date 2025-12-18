use rust_decimal_macros::dec;
use tokio::time::{sleep, Duration};

use laminar::oms::event::OmsEvent;
use laminar::oms::order::Side;
use laminar::oms::runtime::start_oms;

#[tokio::main]
async fn main() {
    println!("[MAIN] starting laminar");

    // start OMS runtime
    let oms = start_oms();
    let tx = oms.sender();

    // 1. strategy sets target
    tx.send(OmsEvent::SetTarget { qty: dec!(1.0) })
        .await
        .unwrap();

    sleep(Duration::from_millis(100)).await;

    // 2. strategy asks OMS to create an order
    // OMS will generate OrderId internally
    // SimBroker will echo events back
    tx.send(OmsEvent::CreateOrder {
        side: Side::Buy,
        qty: dec!(1.0),
    })
    .await
        .unwrap();

    // let async system run
    sleep(Duration::from_secs(2)).await;

    println!("[MAIN] exiting");
}
