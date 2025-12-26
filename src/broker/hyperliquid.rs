use std::sync::Arc;

use tokio::time::Duration;
use std::collections::HashSet;
use hex;

use tokio::sync::{mpsc, Mutex};
use tokio::task::JoinHandle;

use rust_decimal::Decimal;
use rust_decimal_macros::dec;
use rust_decimal::RoundingStrategy;
use rust_decimal::prelude::ToPrimitive;
use rust_decimal::prelude::FromPrimitive;

use ethers::signers::Wallet;
use ethers::core::k256::ecdsa::SigningKey;
use tracing::{info, warn, error};
use crate::oms::account::AccountSnapshot;

use std::str::FromStr;

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
    Meta,
    ExchangeResponseStatus,
    ExchangeDataStatus
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


use std::collections::HashMap;
use anyhow::Result;

type HlWallet = Wallet<SigningKey>;

#[derive(Debug, Clone)]
pub struct SymbolRules {
    pub tick: Decimal,
    pub sz_decimals: u32,
}

pub async fn build_symbol_rules(
    base_url: BaseUrl,
) -> anyhow::Result<HashMap<String, SymbolRules>> {
    let info = InfoClient::new(None, Some(base_url)).await?;
    let meta = info.meta().await?;

    let mut map = HashMap::new();

    for asset in &meta.universe {
        info!("asset name {} asset sz {}", asset.name.clone(), asset.sz_decimals);
        map.insert(
            asset.name.clone(),
            SymbolRules {
                tick: Decimal::new(5, 1), // 0.5 ← PERP RULE
                sz_decimals: asset.sz_decimals,
            },
        );
    }

    Ok(map)
}


pub struct HyperliquidBroker {
    tx: mpsc::Sender<BrokerCommand>,
    rx: Mutex<Option<mpsc::Receiver<BrokerCommand>>>,
    oms_tx: mpsc::Sender<OmsEvent>,

    // trading
    client: Arc<ExchangeClient>,

    // read-only state
    info_client: Arc<InfoClient>,
    address: H160,

    rules: HashMap<String, SymbolRules>,
}

impl HyperliquidBroker {
    pub async fn new(
        wallet: HlWallet,
        base_url: BaseUrl,
        tx: mpsc::Sender<BrokerCommand>,
        rx: Mutex<Option<mpsc::Receiver<BrokerCommand>>>,
        oms_tx: mpsc::Sender<OmsEvent>, 
    ) -> anyhow::Result<Self> {
        // wallet address (H160)
        let address = wallet.address();

        let client = ExchangeClient::new(
            None,
            wallet,
            Some(base_url),
            None,
            None,
        )
            .await?;

        let rules =  build_symbol_rules(BaseUrl::Testnet)
            .await
            .expect("failed to load symbol rules");

        // info / read-only client
        let info_client = InfoClient::with_reconnect(None, None).await?;

        Ok(Self {
            tx,
            rx,
            oms_tx,
            client: Arc::new(client),
            info_client: Arc::new(info_client),
            address,
            rules,
        })
    }

    fn quantize_price(&self, symbol: &str, price: Decimal) -> Decimal {
        // let tick = self.rules[symbol].tick;
        // (price / tick).floor() * tick
        // Hyperliquid Perp Rule: Max decimals = 6 - szDecimals
        let max_decimals = 6 - self.rules[symbol].sz_decimals;

        // 1. Apply the decimal cap
        let mut p = price.round_dp_with_strategy(max_decimals, RoundingStrategy::ToZero);

        // 2. Apply the 5 Significant Figures Rule (Exchange-wide cap)
        let s_figs = p.to_string()
            .replace(".", "")
            .trim_start_matches('0')
            .len();

        if s_figs > 5 {
            let current_scale = p.scale();
            p = p.round_dp_with_strategy(
                current_scale - (s_figs as u32 - 5), 
                RoundingStrategy::ToZero
            );
        }

        p.normalize() // Removes trailing zeros for the API
    }

    fn quantize_qty(&self, symbol: &str, qty: Decimal) -> Decimal {
        let decimals = self.rules[symbol].sz_decimals;
        qty.round_dp_with_strategy(decimals, RoundingStrategy::ToZero)
    }

}

async fn fetch_account_snapshot(
    info: &InfoClient,
    address: H160,
) -> anyhow::Result<AccountSnapshot> {
    let state = info.user_state(address).await?;

    let cms = &state.cross_margin_summary;

    let equity = Decimal::from_str(&cms.account_value)?;
    let raw_usd = Decimal::from_str(&cms.total_raw_usd)?;
    let unrealized_pnl = equity - raw_usd;

    let mut net_position = dec!(0);
    for ap in &state.asset_positions {
        if ap.position.coin == "TST" {
            net_position = Decimal::from_str(&ap.position.szi)?;
            break;
        }
    }

    Ok(AccountSnapshot {
        equity,
        used_margin: Decimal::from_str(&cms.total_margin_used)?,
        available_margin: Decimal::from_str(&state.withdrawable)?,
        unrealized_pnl,
        realized_pnl: dec!(0), // HL doesn’t expose cleanly
        net_position,
    })
}


fn has_error_status(r: &ExchangeResponseStatus) -> bool {
    match r {
        ExchangeResponseStatus::Ok(resp) => {
            resp.data
                .as_ref()
                .map(|d| {
                    d.statuses.iter().any(|s| matches!(s, ExchangeDataStatus::Error(_)))
                })
                .unwrap_or(false)
        }
        ExchangeResponseStatus::Err(_) => true,
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
        let oms_tx_balance = self.oms_tx.clone();
        let info_client = self.info_client.clone();
        let address = self.address;

        let seen_trades = Arc::new(Mutex::new(HashSet::<String>::new()));
        let seen_trades_ws = seen_trades.clone();

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
                        let symbol = "TST";

                        let price_dec = self.quantize_price(symbol, price);
                        let qty_dec   = self.quantize_qty(symbol, qty);

                        // IMPORTANT: convert only after quantization
                        let price_f64 = price_dec
                            .to_f64()
                            .expect("price not representable as f64");

                        let qty_f64 = qty_dec
                            .to_f64()
                            .expect("qty not representable as f64");

                        let order = ClientOrderRequest {
                            asset: symbol.to_string(), // ← for now, hardcoded
                            is_buy,
                            reduce_only: false,
                            limit_px: price_f64,
                            sz: qty_f64,
                            cloid: Some(order_id.0), // ← use OMS order_id
                            order_type: ClientOrder::Limit(ClientLimit {
                                tif: "Gtc".to_string(),
                            }),
                        };

                        let res = client.order(order, None).await;

                        match res {
                            Ok(r) => {
                                if has_error_status(&r) {
                                    let _ = oms_tx
                                        .send(OmsEvent::OrderRejected { order_id })
                                        .await;
                                    info!(
                                        "[BROKER][HL] order {:?} rejected → {:?}",
                                        order_id, r
                                    );
                                    // DO NOT send OrderAccepted
                                } else {
                                    let _ = oms_tx
                                        .send(OmsEvent::OrderAccepted { order_id })
                                        .await;

                                    info!(
                                        "[BROKER][HL] order {:?} accepted → {:?}",
                                        order_id, r
                                    );
                                }
                            }

                            Err(e) => {
                                info!(
                                    "[BROKER][HL] order {:?} transport failed → {:?}",
                                    order_id, e
                                );
                            }
                        }


                    }

                    BrokerCommand::Cancel { order_id } => {
                        let cancel = ClientCancelRequestCloid {
                            asset: "TST".to_string(),
                            cloid: order_id.0,
                        };

                        let res = client.cancel_by_cloid(cancel, None).await;
                        
                        match res {
                            Ok(r) => {
                                if has_error_status(&r) {
                                    info!(
                                        "[BROKER][HL] cancel {:?} rejected → {:?}",
                                        order_id, r
                                    );
                                } else {
                                    let _ = oms_tx
                                        .send(OmsEvent::CancelConfirmed { order_id })
                                        .await;
                                }
                            }

                            Err(e) => {
                                info!(
                                    "[BROKER][HL] cancel {:?} transport failed → {:?}",
                                    order_id, e
                                );
                            }
                        }


                    }

                    BrokerCommand::Flatten { qty, limit_px, } => {
                        let symbol = "TST";
                        let is_buy = false; // default

                        let side = if qty > dec!(0) {
                            // qty already abs()'d in OMS
                            // SELL reduces long
                            Side::Sell
                        } else {
                            Side::Buy
                        };

                        let is_buy = matches!(side, Side::Buy);

                        let qty_f64 = qty.abs()
                            .to_f64()
                            .expect("flatten qty not representable");

                        let limit_px = self.quantize_price(symbol, limit_px);

                        let order = ClientOrderRequest {
                            asset: symbol.to_string(),
                            is_buy,
                            reduce_only: true, // CRITICAL
                            limit_px: limit_px.to_f64().unwrap(),
                            sz: qty_f64,
                            cloid: None,
                            order_type: ClientOrder::Limit(ClientLimit {
                                tif: "Ioc".to_string(),     // immediate-or-cancel,
                            }),
                        };

                        info!("[BROKER][HL] flatten market {:?}", side);

                        match client.order(order, None).await {
                            Ok(r) => {
                                info!("[BROKER][HL] flatten ok → {:?}", r);
                            }
                            Err(e) => {
                                error!("[BROKER][HL] flatten failed → {:?}", e);
                            }
                        }
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

            info!("[BROKER][HL] WS subscribed to user fills");

            while let Some(msg) = msg_rx.recv().await {
                info!("[HL][WS][RAW] {:?}", msg);
                match msg {
                    Message::UserFills ( user_fills ) => {
                        if user_fills.data.is_snapshot == Some(true) {
                            continue;
                        }

                        for fill in user_fills.data.fills {
                            info!("in fill!!!!!");
                            let mut seen = seen_trades_ws.lock().await;
                            if !seen.insert(fill.hash.clone()) {
                                continue;
                            }
                            drop(seen); // guarantees idempotency on fills.
                            let qty = fill.sz.parse::<f64>().unwrap();
                            let price = fill.px.parse::<f64>().unwrap();

                            info!("in filler!!!!!");
                            if let Some(cloid) = fill.cloid.as_deref() {

                                
                                let cloid = cloid.strip_prefix("0x").unwrap_or(cloid);

                                if cloid.len() == 32 {
                                    if let Ok(bytes) = hex::decode(cloid) {
                                        let bytes: Vec<u8> = bytes;
                                        if let Ok(uuid) = Uuid::from_slice(&bytes) {
                                            info!("[WS] fill uuid is {}", uuid);
                                            let _ = oms_tx_ws
                                                .send(OmsEvent::Fill {
                                                    order_id: OrderId(uuid),
                                                    qty: Decimal::from_f64(qty).unwrap(),
                                                    price: Decimal::from_f64(price).unwrap(),
                                                })
                                            .await;
                                        }
                                    }
                                }

                            }
                        }
                    }
                    _ => {}
                }
            }
        });
        
        // ===============================
        // POLL ACCOUNT SNAPSHOT 
        // ===============================

        tokio::spawn(async move {
            let mut interval = tokio::time::interval(Duration::from_secs(5));

            loop {
                interval.tick().await;

                match fetch_account_snapshot(&info_client, address).await {
                    Ok(snapshot) => {
                        let _ = oms_tx_balance
                            .send(OmsEvent::UpdateAccountSnapshot { snapshot })
                            .await;
                        }
                    Err(e) => {
                        warn!("account snapshot failed: {:?}", e);
                    }
                }
            }
        });

    }
}
