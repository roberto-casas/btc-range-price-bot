use anyhow::Result;
use futures_util::{SinkExt, StreamExt};
use serde::{Deserialize, Serialize};
use tokio::sync::{broadcast, watch};
use tokio_tungstenite::{connect_async, tungstenite::Message};
use tracing::{debug, error, info, warn};
use url::Url;

/// A live price update received via the Polymarket CLOB WebSocket
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PriceUpdate {
    pub token_id: String,
    pub price: f64,
    pub side: String,
    pub timestamp_ms: u64,
}

// Polymarket CLOB WebSocket endpoint
const WS_URL: &str = "wss://ws-subscriptions-clob.polymarket.com/ws/market";

// ── Subscription message types ────────────────────────────────────────────────

#[derive(Serialize)]
struct SubscribeMsg<'a> {
    #[serde(rename = "type")]
    msg_type: &'a str,
    markets: Vec<&'a str>,
}

/// Raw tick from Polymarket WebSocket feed (simplified)
#[derive(Deserialize, Debug)]
struct RawTick {
    #[serde(rename = "asset_id", default)]
    asset_id: Option<String>,
    #[serde(rename = "price", default)]
    price: Option<serde_json::Value>,
    #[serde(rename = "side", default)]
    side: Option<String>,
}

/// Start a WebSocket listener that dynamically subscribes to market token IDs
/// received via a `watch` channel and broadcasts price updates.
///
/// The task runs indefinitely in the background; cancel it by dropping the
/// `broadcast::Sender` handle.
///
/// When new token IDs arrive on `token_rx`, the listener reconnects and
/// subscribes to the updated list.
pub async fn start_ws_listener(
    mut token_rx: watch::Receiver<Vec<String>>,
    tx: broadcast::Sender<PriceUpdate>,
) -> Result<()> {
    let url = Url::parse(WS_URL)?;

    loop {
        let token_ids = token_rx.borrow_and_update().clone();

        if token_ids.is_empty() {
            info!("No tokens to subscribe to yet. Waiting for scanner results...");
            // Wait until the scanner sends us token IDs
            if token_rx.changed().await.is_err() {
                // Sender dropped — shut down
                info!("Token watch channel closed. Stopping WebSocket listener.");
                return Ok(());
            }
            continue;
        }

        info!("Connecting to Polymarket WebSocket at {WS_URL}...");
        let ws_stream = match connect_async(url.as_str()).await {
            Ok((ws, _)) => ws,
            Err(e) => {
                warn!("WebSocket connection failed: {e}. Retrying in 5s...");
                tokio::time::sleep(std::time::Duration::from_secs(5)).await;
                continue;
            }
        };

        info!("WebSocket connected. Subscribing to {} tokens...", token_ids.len());
        let (mut writer, mut reader) = ws_stream.split();

        // Subscribe to markets
        let sub = SubscribeMsg {
            msg_type: "subscribe",
            markets: token_ids.iter().map(String::as_str).collect(),
        };
        let sub_json = serde_json::to_string(&sub)?;
        if let Err(e) = writer.send(Message::Text(sub_json.into())).await {
            warn!("Failed to send subscription: {e}. Reconnecting...");
            continue;
        }

        // Process incoming messages until an error occurs or tokens are updated
        loop {
            tokio::select! {
                msg = reader.next() => {
                    match msg {
                        Some(Ok(Message::Text(text))) => {
                            debug!("WS msg: {text}");
                            process_ws_message(&text, &tx);
                        }
                        Some(Ok(Message::Ping(data))) => {
                            let _ = writer.send(Message::Pong(data)).await;
                        }
                        Some(Ok(Message::Close(_))) => {
                            warn!("WebSocket closed by server. Reconnecting...");
                            break;
                        }
                        Some(Err(e)) => {
                            error!("WebSocket error: {e}. Reconnecting...");
                            break;
                        }
                        None => {
                            warn!("WebSocket stream ended. Reconnecting...");
                            break;
                        }
                        _ => {}
                    }
                }
                result = token_rx.changed() => {
                    if result.is_err() {
                        info!("Token watch channel closed. Stopping WebSocket listener.");
                        return Ok(());
                    }
                    info!("Token list updated. Reconnecting to re-subscribe...");
                    break;
                }
            }
        }

        tokio::time::sleep(std::time::Duration::from_secs(3)).await;
    }
}

fn process_ws_message(text: &str, tx: &broadcast::Sender<PriceUpdate>) {
    // The CLOB WS sends arrays of tick objects
    let ticks: Vec<serde_json::Value> = match serde_json::from_str(text) {
        Ok(v) => v,
        Err(_) => {
            // May be a single object
            match serde_json::from_str::<serde_json::Value>(text) {
                Ok(v) if v.is_object() => vec![v],
                _ => return,
            }
        }
    };

    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0);

    for tick in ticks {
        let raw: RawTick = match serde_json::from_value(tick) {
            Ok(r) => r,
            Err(_) => continue,
        };

        let token_id = match raw.asset_id {
            Some(id) if !id.is_empty() => id,
            _ => continue,
        };

        let price_val = match raw.price {
            Some(serde_json::Value::Number(n)) => n.as_f64(),
            Some(serde_json::Value::String(s)) => s.parse::<f64>().ok(),
            _ => None,
        };
        let price = match price_val {
            Some(p) if (0.001..=0.999).contains(&p) => p,
            _ => continue,
        };

        let side = raw.side.unwrap_or_else(|| "buy".to_string());

        let update = PriceUpdate {
            token_id,
            price,
            side,
            timestamp_ms: now,
        };

        // Ignore send errors (no active receivers is fine)
        let _ = tx.send(update);
    }
}
