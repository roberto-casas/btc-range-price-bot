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

/// Maximum assets per subscription message to avoid server rejections.
const MAX_ASSETS_PER_SUB: usize = 50;

/// Interval between keep-alive pings.
const PING_INTERVAL_SECS: u64 = 15;

// ── Subscription message types ────────────────────────────────────────────────

/// Initial handshake message — opens the market channel.
#[derive(Serialize)]
struct HandshakeMsg<'a> {
    assets_ids: Vec<&'a str>,
    #[serde(rename = "type")]
    msg_type: &'a str,
}

/// Subscribe/unsubscribe message — adds or removes asset subscriptions
/// without reconnecting.
#[derive(Serialize)]
struct OperationMsg<'a> {
    assets_ids: Vec<&'a str>,
    operation: &'a str,
}

// ── Incoming event types ──────────────────────────────────────────────────────

#[derive(Deserialize, Debug)]
struct WsEvent {
    event_type: Option<String>,
    // For last_trade_price events (fields at top level)
    asset_id: Option<String>,
    price: Option<serde_json::Value>,
    side: Option<String>,
    timestamp: Option<serde_json::Value>,
    // For price_change events (array of changes)
    price_changes: Option<Vec<PriceChangeEntry>>,
}

#[derive(Deserialize, Debug)]
struct PriceChangeEntry {
    asset_id: Option<String>,
    price: Option<serde_json::Value>,
    side: Option<String>,
}

/// Start a WebSocket listener that dynamically subscribes to market token IDs
/// received via a `watch` channel and broadcasts price updates.
///
/// When new token IDs arrive on `token_rx`, subscriptions are updated in-place
/// without reconnecting (using the Polymarket `operation` protocol).
pub async fn start_ws_listener(
    mut token_rx: watch::Receiver<Vec<String>>,
    tx: broadcast::Sender<PriceUpdate>,
) -> Result<()> {
    let url = Url::parse(WS_URL)?;

    loop {
        // Wait until we have at least some tokens before connecting
        {
            let ids = token_rx.borrow_and_update().clone();
            if ids.is_empty() {
                info!("No tokens to subscribe to yet. Waiting for scanner results...");
                if token_rx.changed().await.is_err() {
                    info!("Token watch channel closed. Stopping WebSocket listener.");
                    return Ok(());
                }
                continue;
            }
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

        let (mut writer, mut reader) = ws_stream.split();

        // 1) Send initial handshake (empty assets_ids, type: "market")
        let handshake = HandshakeMsg {
            assets_ids: vec![],
            msg_type: "market",
        };
        let handshake_json = serde_json::to_string(&handshake)?;
        debug!("Sending handshake: {handshake_json}");
        if let Err(e) = writer.send(Message::Text(handshake_json.into())).await {
            warn!("Failed to send handshake: {e}. Reconnecting...");
            tokio::time::sleep(std::time::Duration::from_secs(3)).await;
            continue;
        }

        // 2) Subscribe to current tokens via operation messages
        let current_ids = token_rx.borrow_and_update().clone();
        if !send_subscribe_batches(&mut writer, &current_ids).await {
            tokio::time::sleep(std::time::Duration::from_secs(3)).await;
            continue;
        }

        info!(
            "WebSocket connected. Subscribed to {} tokens.",
            current_ids.len(),
        );

        // Set up periodic ping to keep the connection alive
        let mut ping_interval =
            tokio::time::interval(std::time::Duration::from_secs(PING_INTERVAL_SECS));
        ping_interval.tick().await; // consume the immediate first tick

        // Process incoming messages
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
                _ = ping_interval.tick() => {
                    if let Err(e) = writer.send(Message::Ping(vec![].into())).await {
                        warn!("Failed to send ping: {e}. Reconnecting...");
                        break;
                    }
                }
                result = token_rx.changed() => {
                    if result.is_err() {
                        info!("Token watch channel closed. Stopping WebSocket listener.");
                        return Ok(());
                    }
                    let new_ids = token_rx.borrow_and_update().clone();
                    info!("Token list updated ({} tokens). Sending new subscriptions...", new_ids.len());
                    if !send_subscribe_batches(&mut writer, &new_ids).await {
                        break; // reconnect on failure
                    }
                }
            }
        }

        tokio::time::sleep(std::time::Duration::from_secs(3)).await;
    }
}

/// Send subscription messages in batches using the `operation: "subscribe"` format.
/// Returns `true` on success.
async fn send_subscribe_batches(
    writer: &mut futures_util::stream::SplitSink<
        tokio_tungstenite::WebSocketStream<
            tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>,
        >,
        Message,
    >,
    token_ids: &[String],
) -> bool {
    for chunk in token_ids.chunks(MAX_ASSETS_PER_SUB) {
        let msg = OperationMsg {
            assets_ids: chunk.iter().map(String::as_str).collect(),
            operation: "subscribe",
        };
        let json = match serde_json::to_string(&msg) {
            Ok(j) => j,
            Err(e) => {
                warn!("Failed to serialize subscription: {e}");
                return false;
            }
        };
        debug!("Sending subscribe batch ({} tokens): {json}", chunk.len());
        if let Err(e) = writer.send(Message::Text(json.into())).await {
            warn!("Failed to send subscription batch: {e}");
            return false;
        }
    }
    true
}

fn process_ws_message(text: &str, tx: &broadcast::Sender<PriceUpdate>) {
    let event: WsEvent = match serde_json::from_str(text) {
        Ok(e) => e,
        Err(_) => {
            // Try parsing as an array of events
            if let Ok(events) = serde_json::from_str::<Vec<serde_json::Value>>(text) {
                for val in events {
                    if let Ok(ev) = serde_json::from_value::<WsEvent>(val) {
                        process_event(&ev, tx);
                    }
                }
            }
            return;
        }
    };
    process_event(&event, tx);
}

fn process_event(event: &WsEvent, tx: &broadcast::Sender<PriceUpdate>) {
    let now_ms = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0);

    match event.event_type.as_deref() {
        Some("last_trade_price") => {
            if let Some(update) = extract_update(
                event.asset_id.as_deref(),
                &event.price,
                event.side.as_deref(),
                event
                    .timestamp
                    .as_ref()
                    .and_then(parse_timestamp_ms)
                    .unwrap_or(now_ms),
            ) {
                let _ = tx.send(update);
            }
        }
        Some("price_change") => {
            if let Some(ref changes) = event.price_changes {
                for pc in changes {
                    if let Some(update) = extract_update(
                        pc.asset_id.as_deref(),
                        &pc.price,
                        pc.side.as_deref(),
                        now_ms,
                    ) {
                        let _ = tx.send(update);
                    }
                }
            }
        }
        _ => {
            // book, tick_size_change, etc. — ignore for price tracking
        }
    }
}

fn extract_update(
    asset_id: Option<&str>,
    price_val: &Option<serde_json::Value>,
    side: Option<&str>,
    timestamp_ms: u64,
) -> Option<PriceUpdate> {
    let token_id = asset_id.filter(|id| !id.is_empty())?;

    let price = match price_val {
        Some(serde_json::Value::Number(n)) => n.as_f64(),
        Some(serde_json::Value::String(s)) => s.parse::<f64>().ok(),
        _ => None,
    }?;

    if !(0.001..=0.999).contains(&price) {
        return None;
    }

    Some(PriceUpdate {
        token_id: token_id.to_string(),
        price,
        side: side.unwrap_or("BUY").to_uppercase(),
        timestamp_ms,
    })
}

fn parse_timestamp_ms(val: &serde_json::Value) -> Option<u64> {
    match val {
        serde_json::Value::Number(n) => n.as_u64(),
        serde_json::Value::String(s) => s.parse().ok(),
        _ => None,
    }
}
