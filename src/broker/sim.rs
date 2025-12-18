use tokio::sync::mpsc;
use rust_decimal::Decimal;
use rust_decimal_macros::dec;
use tokio::time::{sleep, Duration};

use crate::oms::event::OmsEvent;
use crate::oms::order::{OrderId, Side};

#[derive(Clone)]
pub struct SimBroker {
    oms_tx: mpsc::Sender<OmsEvent>,
}

impl SimBroker {
    pub fn new(oms_tx: mpsc::Sender<OmsEvent>) -> Self {
        Self { oms_tx }
    }

    pub async fn place_order(&self, order_id: OrderId, side: Side, qty: Decimal) {
        // simulate exchange ack
        sleep(Duration::from_millis(50)).await;

        let _ = self
            .oms_tx
            .send(OmsEvent::OrderAccepted { order_id })
            .await;

        // simulate fills
        let fill1 = qty * dec!(0.4);
        let fill2 = qty - fill1;

        sleep(Duration::from_millis(50)).await;
        let _ = self
            .oms_tx
            .send(OmsEvent::Fill {
                order_id,
                qty: fill1,
                price: dec!(100.0),
            })
        .await;

        sleep(Duration::from_millis(50)).await;
        let _ = self
            .oms_tx
            .send(OmsEvent::Fill {
                order_id,
                qty: fill2,
                price: dec!(101.0),
            })
        .await;
    }
}
