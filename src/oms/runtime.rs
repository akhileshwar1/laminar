use tokio::sync::{mpsc, Mutex};
use std::sync::Arc;
use std::env;

use ethers::signers::Wallet;
use ethers::core::k256::ecdsa::SigningKey;

use super::engine::OmsEngine;
use super::event::OmsEvent;
use crate::broker::{Broker, sim::SimBroker, types::BrokerCommand};
use crate::oms::snapshot::OmsSnapshot;
use hyperliquid_rust_sdk::BaseUrl;

use tracing::{info, warn, error};

#[cfg(feature = "hyperliquid")]
use crate::broker::HyperliquidBroker;

pub struct OmsRuntime {
    sender: mpsc::Sender<OmsEvent>,
}

impl OmsRuntime {
    pub fn sender(&self) -> mpsc::Sender<OmsEvent> {
        self.sender.clone()
    }
}

pub async fn start_oms() -> OmsRuntime {
    let (tx, mut rx) = mpsc::channel::<OmsEvent>(1024);
    let (broker_tx, broker_rx) = mpsc::channel::<BrokerCommand>(1024);
    // ---- WALLET (TESTNET) ----
    let private_key = env::var("HL_TESTNET_PRIVATE_KEY")
        .expect("HL_TESTNET_PRIVATE_KEY not set");

    let wallet: Wallet<SigningKey> = private_key
        .parse()
        .expect("invalid testnet private key");

    let broker: Arc<dyn Broker> = if cfg!(feature = "hyperliquid") {
        Arc::new(
            HyperliquidBroker::new(
                wallet,
                BaseUrl::Testnet,
                broker_tx.clone(),
                Mutex::new(Some(broker_rx)),
                tx.clone(),
            )
            .await
            .expect("broker init failed"),
        )
    } else {
        Arc::new(
            SimBroker::new(
                broker_rx,
                broker_tx.clone(),
                tx.clone(),
            )
        )
    };
    
    broker.clone().start();
    drop(broker);
    
    tokio::spawn(async move {
        let mut oms = OmsEngine::new();

        info!("[OMS] started");

        while let Some(event) = rx.recv().await {
            info!("[OMS] event received: {:?}", event);

            match event {
                OmsEvent::SetTarget { qty } => {
                    oms.set_target_position(qty);
                    info!("[OMS] target set → delta = {}", oms.delta());
                }

                OmsEvent::CreateOrder { side, qty, price } => {
                    let oid = oms.create_order(side, qty, price);
                    info!(
                        "[OMS] order created {:?} {:?} qty={} price={}",
                        oid, side, qty, price
                    );
                    let broker_tx = broker_tx.clone();
                    tokio::spawn(async move {
                        broker_tx
                            .send(BrokerCommand::PlaceLimit {
                                order_id: oid,
                                side,
                                qty,
                                price,
                            })
                        .await
                            .unwrap();
                        });
                }

                OmsEvent::OrderAccepted { order_id } => {
                    oms.on_order_accepted(order_id);
                    info!(
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
                    info!(
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
                    info!(
                        "[OMS] cancel confirmed {:?}, delta={}",
                        order_id,
                        oms.delta()
                    );
                }

                OmsEvent::GetDelta { reply } => {
                    let _ = reply.send(oms.delta());
                }

                OmsEvent::CancelAll => {
                    for order_id in oms.open_order_ids() {
                        oms.request_cancel(order_id);

                        let broker_tx = broker_tx.clone();
                        tokio::spawn(async move {
                            broker_tx
                                .send(BrokerCommand::Cancel { order_id })
                                .await
                                .unwrap();
                            });
                    }
                }

                OmsEvent::GetSnapshot { reply } => {
                    let pos = oms.position();
                    let snapshot = OmsSnapshot {
                        orders: oms.order_views(),
                        net_position: pos.net_qty,
                        avg_price: pos.avg_price,
                    };
                    let _ = reply.send(snapshot);
                }


                OmsEvent::Tick => {
                    info!("[OMS] tick → delta={}", oms.delta());
                }
            }
        }

        info!("[OMS] channel closed, exiting");
    });


    OmsRuntime { sender: tx }
}
