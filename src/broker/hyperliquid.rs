use std::sync::Arc;

use tokio::sync::{mpsc, Mutex};
use tokio::task::JoinHandle;

use rust_decimal::Decimal;
use rust_decimal::prelude::ToPrimitive;
use rust_decimal::prelude::FromPrimitive;

use ethers::signers::Wallet;
use ethers::core::k256::ecdsa::SigningKey;

use hyperliquid_rust_sdk::{
    BaseUrl,
    ExchangeClient,
    ClientLimit,
    ClientOrder,
    ClientOrderRequest,
    ClientCancelRequest,
    ClientCancelRequestCloid,
    InfoClient,
    Subscription,
    Message,
};
use ethers::types::H160;
use alloy::primitives::Address;
use tokio::sync::mpsc::UnboundedSender;
use ethers::signers::Signer;
use uuid::Uuid;

use crate::broker::{Broker};
use crate::broker::types::BrokerCommand;
use crate::oms::order::{OrderId, Side};
use crate::oms::event::OmsEvent;

type HlWallet = Wallet<SigningKey>;

pub struct HyperliquidBroker {
    tx: mpsc::Sender<BrokerCommand>,
    rx: Mutex<Option<mpsc::Receiver<BrokerCommand>>>,
    oms_tx: mpsc::Sender<OmsEvent>,
    client: Arc<ExchangeClient>,
}

impl HyperliquidBroker {
    pub async fn new(
        wallet: HlWallet,
        base_url: BaseUrl,
        tx: mpsc::Sender<BrokerCommand>,
        rx: Mutex<Option<mpsc::Receiver<BrokerCommand>>>,
        oms_tx: mpsc::Sender<OmsEvent>, 
    ) -> anyhow::Result<Self> {
        let client = ExchangeClient::new(
            None,
            wallet,
            Some(base_url),
            None,
            None,
        )
            .await?;

        Ok(Self {
            tx,
            rx,
            oms_tx,
            client: Arc::new(client),
        })
    }
}

impl Broker for HyperliquidBroker {
    fn command_sender(&self) -> mpsc::Sender<BrokerCommand> {
        self.tx.clone()
    }

    fn start(self : Arc<Self>) {
        let client = self.client.clone();
        let oms_tx = self.oms_tx.clone();
        let client_ws = self.client.clone();
        let oms_tx_ws = self.oms_tx.clone();

        // ===============================
        // REST COMMAND LOOP
        // ===============================

        tokio::spawn(async move {
            // let mut rx = rx.lock().await;
            let mut rx = self
            .rx
            .lock()
            .await
            .take()
            .expect("broker already started");

            while let Some(cmd) = rx.recv().await {
                match cmd {
                    BrokerCommand::PlaceLimit {
                        order_id,
                        side,
                        qty,
                        price,
                    } => {
                        let is_buy = matches!(side, Side::Buy);

                        let order = ClientOrderRequest {
                            asset: "BTC".to_string(), // ← for now, hardcoded
                            is_buy,
                            reduce_only: false,
                            limit_px: price.to_f64().unwrap(),
                            sz: qty.to_f64().unwrap(),
                            cloid: Some(order_id.0), // ← use OMS order_id
                            order_type: ClientOrder::Limit(ClientLimit {
                                tif: "Gtc".to_string(),
                            }),
                        };

                        let res = client.order(order, None).await;

                        match res {
                            Ok(r) => {
                                let _ = oms_tx
                                    .send(OmsEvent::OrderAccepted { order_id })
                                    .await;
                                println!(
                                    "[BROKER][HL] order {:?} accepted → {:?}",
                                    order_id, r
                                );
                            }
                            Err(e) => {
                                println!(
                                    "[BROKER][HL] order {:?} failed → {:?}",
                                    order_id, e
                                );
                            }
                        }
                    }

                    BrokerCommand::Cancel { order_id } => {
                        let cancel = ClientCancelRequestCloid {
                            asset: "BTC".to_string(),
                            cloid: order_id.0,
                        };

                        let res = client.cancel_by_cloid(cancel, None).await;
                        let _ = oms_tx
                                .send(OmsEvent::CancelConfirmed { order_id })
                                .await;

                        println!("[BROKER][HL] cancel {:?} → {:?}", order_id, res);
                    }

                }
            }
        });

        // ===============================
        // WS FILL LISTENER
        // ===============================
        

        tokio::spawn(async move {
            let base_url = if client_ws.http_client.base_url.contains("testnet") {
                BaseUrl::Testnet
            } else {
                BaseUrl::Mainnet
            };

            let mut info = InfoClient::new(None, Some(base_url))
                .await
                .expect("info client");

            let wallet_addr: H160 = client_ws.wallet.address();

            let (msg_tx, mut msg_rx) =
                tokio::sync::mpsc::unbounded_channel::<Message>();

            info.subscribe(
                Subscription::UserFills { user: wallet_addr },
                msg_tx,
            )
                .await
                .expect("subscribe user fills");

            println!("[BROKER][HL] WS subscribed to user fills");

            while let Some(msg) = msg_rx.recv().await {
                match msg {
                    Message::UserFills ( user_fills ) => {
                        for fill in user_fills.data.fills {
                            let qty = fill.sz.parse::<f64>().unwrap();
                            let price = fill.px.parse::<f64>().unwrap();

                            if let Some(cloid) = fill.cloid.as_deref() {
                                if let Ok(uuid) = Uuid::parse_str(cloid) {
                                    let _ = oms_tx_ws
                                        .send(OmsEvent::Fill {
                                            order_id: OrderId(uuid),
                                            qty: Decimal::from_f64(qty).expect("invalid qty f64"),
                                            price: Decimal::from_f64(price).expect("invalid price f64"),
                                        })
                                    .await;
                                }
                            }
                        }
                    }
                    _ => {}
                }
            }
        });
    }
}
