use tokio::sync::mpsc;

use super::engine::OmsEngine;
use super::event::OmsEvent;
use crate::broker::sim::SimBroker;

pub struct OmsRuntime {
    sender: mpsc::Sender<OmsEvent>,
}

impl OmsRuntime {
    pub fn sender(&self) -> mpsc::Sender<OmsEvent> {
        self.sender.clone()
    }
}

pub fn start_oms() -> OmsRuntime {
    let (tx, mut rx) = mpsc::channel::<OmsEvent>(1024);
    let broker = SimBroker::new(tx.clone());

    
    tokio::spawn(async move {
        let mut oms = OmsEngine::new();

        println!("[OMS] started");

        while let Some(event) = rx.recv().await {
            println!("[OMS] event received: {:?}", event);

            match event {
                OmsEvent::SetTarget { qty } => {
                    oms.set_target_position(qty);
                    println!("[OMS] target set → delta = {}", oms.delta());
                }

                OmsEvent::CreateOrder { side, qty } => {
                    let oid = oms.create_order(side, qty);
                    println!(
                        "[OMS] order created {:?} {:?} qty={}",
                        oid, side, qty
                    );
                    let broker = broker.clone();
                    tokio::spawn(async move {
                        broker.place_order(oid, side, qty).await;
                    });
                }

                OmsEvent::OrderAccepted { order_id } => {
                    oms.on_order_accepted(order_id);
                    println!(
                        "[OMS] order accepted {:?}, delta={}",
                        order_id,
                        oms.delta()
                    );
                }

                OmsEvent::Fill {
                    order_id,
                    qty,
                    price,
                } => {
                    oms.on_fill(order_id, qty, price);
                    let pos = oms.position();
                    println!(
                        "[OMS] fill {:?} qty={} price={} → net={} avg={} pnl={}",
                        order_id,
                        qty,
                        price,
                        pos.net_qty,
                        pos.avg_price,
                        pos.realized_pnl
                    );
                }

                OmsEvent::CancelConfirmed { order_id } => {
                    oms.on_cancel_confirmed(order_id);
                    println!(
                        "[OMS] cancel confirmed {:?}, delta={}",
                        order_id,
                        oms.delta()
                    );
                }

                OmsEvent::GetDelta { reply } => {
                    let _ = reply.send(oms.delta());
                }

                OmsEvent::Tick => {
                    println!("[OMS] tick → delta={}", oms.delta());
                }
            }
        }

        println!("[OMS] channel closed, exiting");
    });


    OmsRuntime { sender: tx }
}
