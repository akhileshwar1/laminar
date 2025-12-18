use rust_decimal_macros::dec;
use tokio::time::{sleep, Duration};

use laminar::oms::event::OmsEvent;
use laminar::oms::order::Side;
use laminar::oms::runtime::start_oms;

#[tokio::main]
async fn main() {
    println!("[MAIN] starting laminar");

    let oms = start_oms();
    let tx = oms.sender();

    tx.send(OmsEvent::SetTarget { qty: dec!(1.0) })
        .await
        .unwrap();

    sleep(Duration::from_millis(100)).await;

    tx.send(OmsEvent::CreateOrder {
        side: Side::Buy,
        qty: dec!(1.0),
    })
    .await
    .unwrap();

    sleep(Duration::from_millis(100)).await;

    // Fake exchange events
    // (you'll replace this with a simulated broker soon)
    use laminar::oms::order::OrderId;
    use uuid::Uuid;

    let fake_order_id = OrderId(Uuid::new_v4());

    tx.send(OmsEvent::OrderAccepted {
        order_id: fake_order_id,
    })
    .await
        .unwrap();

    sleep(Duration::from_millis(100)).await;

    tx.send(OmsEvent::Fill {
        order_id: fake_order_id,
        qty: dec!(0.4),
        price: dec!(100.0),
    })
    .await
        .unwrap();

    sleep(Duration::from_millis(100)).await;

    tx.send(OmsEvent::Fill {
        order_id: fake_order_id,
        qty: dec!(0.6),
        price: dec!(101.0),
    })
    .await
        .unwrap();

    sleep(Duration::from_secs(1)).await;

    println!("[MAIN] done");
}
