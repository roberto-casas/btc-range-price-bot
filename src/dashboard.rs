use axum::{
    extract::{Query, State},
    http::StatusCode,
    response::{Html, IntoResponse, Json},
    routing::get,
    Router,
};
use serde::Deserialize;
use std::{net::SocketAddr, sync::Arc};
use tokio::sync::RwLock;
use tower_http::cors::CorsLayer;
use tracing::info;

use crate::backtesting::run_backtest;
use crate::scanner::fetch_historical_btc;
use crate::types::ScanResult;

/// Shared application state accessible from all route handlers
#[derive(Clone)]
pub struct AppState {
    pub scan_result: Arc<RwLock<Option<ScanResult>>>,
    pub http_client: reqwest::Client,
}

impl AppState {
    pub fn new(http_client: reqwest::Client) -> Self {
        Self {
            scan_result: Arc::new(RwLock::new(None)),
            http_client,
        }
    }
}

/// Build the Axum router with all routes
pub fn build_router(state: AppState) -> Router {
    Router::new()
        .route("/", get(index_handler))
        .route("/api/data", get(data_handler))
        .route("/api/health", get(health_handler))
        .route("/api/backtest", get(backtest_handler))
        .with_state(state)
        .layer(CorsLayer::permissive())
}

/// Serve the dashboard HTML
async fn index_handler() -> impl IntoResponse {
    Html(DASHBOARD_HTML)
}

/// Return current scan results as JSON
async fn data_handler(State(state): State<AppState>) -> impl IntoResponse {
    let result = state.scan_result.read().await;
    match result.as_ref() {
        Some(data) => Json(serde_json::to_value(data).unwrap_or_default()).into_response(),
        None => (
            StatusCode::SERVICE_UNAVAILABLE,
            Json(serde_json::json!({
                "error": "No scan data available yet. Please wait for the first scan to complete."
            })),
        )
            .into_response(),
    }
}

/// Health check endpoint
async fn health_handler() -> impl IntoResponse {
    Json(serde_json::json!({ "status": "ok" }))
}

/// Query parameters for the backtesting endpoint
#[derive(Deserialize)]
pub struct BacktestQuery {
    pub low_ratio: Option<f64>,
    pub high_ratio: Option<f64>,
    pub duration: Option<i64>,
    pub yes_price_low: Option<f64>,
    pub yes_price_high: Option<f64>,
    pub history_days: Option<u32>,
}

/// Run a backtest and return results as JSON
async fn backtest_handler(
    Query(params): Query<BacktestQuery>,
    State(state): State<AppState>,
) -> impl IntoResponse {
    let low_ratio = params.low_ratio.unwrap_or(0.90).clamp(0.50, 0.99);
    let high_ratio = params.high_ratio.unwrap_or(1.10).clamp(1.01, 2.00);
    let duration = params.duration.unwrap_or(7).clamp(1, 90);
    let yes_price_low = params.yes_price_low.unwrap_or(0.60).clamp(0.01, 0.99);
    let yes_price_high = params.yes_price_high.unwrap_or(0.70).clamp(0.01, 0.99);
    let history_days = params.history_days.unwrap_or(90).clamp(7, 365);

    if low_ratio >= high_ratio {
        return (
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({ "error": "low_ratio must be less than high_ratio" })),
        )
            .into_response();
    }

    match fetch_historical_btc(&state.http_client, history_days).await {
        Ok(candles) => {
            let summary = run_backtest(
                &candles,
                low_ratio,
                high_ratio,
                duration,
                yes_price_low,
                yes_price_high,
            );
            Json(serde_json::to_value(summary).unwrap_or_default()).into_response()
        }
        Err(e) => (
            StatusCode::BAD_GATEWAY,
            Json(serde_json::json!({ "error": format!("Failed to fetch historical data: {e}") })),
        )
            .into_response(),
    }
}

/// Start the HTTP server on the given port
pub async fn start_server(state: AppState, port: u16) -> anyhow::Result<()> {
    let addr = SocketAddr::from(([127, 0, 0, 1], port));
    info!("Dashboard listening on http://127.0.0.1:{port}");
    info!("  → Main dashboard: http://127.0.0.1:{port}/");
    info!("  → JSON API:       http://127.0.0.1:{port}/api/data");

    let router = build_router(state);
    let listener = tokio::net::TcpListener::bind(addr).await?;
    axum::serve(listener, router).await?;
    Ok(())
}

// ── Embedded dashboard HTML ───────────────────────────────────────────────────

/// The full dashboard UI is embedded directly in the binary so that the server
/// has no external file dependencies.
const DASHBOARD_HTML: &str = r#"<!DOCTYPE html>
<html lang="en">
<head>
  <meta charset="UTF-8" />
  <meta name="viewport" content="width=device-width, initial-scale=1.0" />
  <title>PolyDelta — BTC Range Bot</title>
  <style>
    *, *::before, *::after { box-sizing: border-box; margin: 0; padding: 0; }
    :root {
      --bg:        #0d0e14;
      --surface:   #12141c;
      --card:      #161b26;
      --border:    #1e2535;
      --border2:   #252d42;
      --text:      #e2e8f0;
      --muted:     #64748b;
      --green:     #00c076;
      --red:       #f43f5e;
      --blue:      #3b82f6;
      --yellow:    #f59e0b;
    }
    body {
      font-family: -apple-system, BlinkMacSystemFont, 'Inter', sans-serif;
      background: var(--bg);
      color: var(--text);
      min-height: 100vh;
      padding-bottom: 48px;
    }
    header {
      background: var(--bg);
      border-bottom: 1px solid var(--border);
      height: 60px;
      display: flex;
      align-items: center;
      justify-content: center;
      position: sticky;
      top: 0;
      z-index: 10;
    }
    .header-inner {
      width: 1400px;
      display: flex;
      justify-content: space-between;
      align-items: center;
      padding: 0 1rem;
    }
    .logo { font-size: 21px; font-weight: 700; color: var(--blue); letter-spacing: -0.5px; }
    .logo span { color: var(--text); }
    .header-meta { display: flex; flex-direction: column; align-items: flex-end; font-size: 12px; color: var(--muted); }
    .btc-badge::before { content: 'BTC '; color: var(--muted); }
    main { max-width: 1400px; margin: 3rem auto 0; padding: 0 1rem; }
    .dry-run-banner {
      background: rgba(245,158,11,0.12);
      border: 1px solid rgba(245,158,11,0.4);
      border-radius: 8px;
      color: var(--yellow);
      font-size: 13px;
      font-weight: 600;
      padding: 10px 16px;
      margin-bottom: 20px;
      display: none;
    }
    .toolbar {
      display: flex;
      align-items: center;
      gap: 10px;
      margin-bottom: 20px;
      flex-wrap: wrap;
    }
    .toolbar-label { font-size: 12px; font-weight: 500; color: var(--muted); text-transform: uppercase; letter-spacing: 0.5px; margin-right: 4px; }
    .sort-btn {
      background: var(--card);
      border: 1px solid var(--border2);
      border-radius: 6px;
      color: var(--muted);
      font-family: inherit;
      font-size: 12px;
      font-weight: 500;
      padding: 6px 14px;
      cursor: pointer;
      display: flex;
      align-items: center;
      gap: 8px;
      transition: border-color 0.15s, color 0.15s;
    }
    .sort-btn:hover { border-color: rgba(59,130,246,0.5); color: var(--blue); }
    .sort-btn.active { border-color: rgba(59,130,246,0.5); color: rgba(0,147,253); background: rgba(0,147,253,0.12); }
    .count-badge { margin-left: auto; font-size: 12px; color: var(--muted); }
    .month-filters { display: flex; align-items: center; gap: 8px; margin-bottom: 16px; flex-wrap: wrap; }
    .month-filter-label { font-size: 12px; font-weight: 500; color: var(--muted); text-transform: uppercase; letter-spacing: 0.5px; margin-right: 4px; }
    .month-btn {
      background: var(--card);
      border: 1px solid var(--border2);
      border-radius: 6px;
      color: var(--muted);
      font-family: inherit;
      font-size: 12px;
      font-weight: 500;
      padding: 5px 12px;
      cursor: pointer;
      transition: border-color 0.15s, color 0.15s;
    }
    .month-btn:hover { border-color: rgba(59,130,246,0.5); color: var(--blue); }
    .month-btn.active { border-color: rgba(59,130,246,0.5); color: var(--blue); background: rgba(59,130,246,0.12); }
    #cards {
      display: grid;
      grid-template-columns: repeat(auto-fill, minmax(324px, 1fr));
      gap: 16px;
    }
    .card-link { text-decoration: none; color: inherit; display: block; transition: transform 0.15s; }
    .card-link:hover { transform: translateY(-2px); }
    .card {
      display: flex;
      flex-direction: column;
      background: var(--card);
      border: 1px solid var(--border);
      border-radius: 14px;
      padding: 18px 20px;
      transition: border-color 0.15s, box-shadow 0.15s;
      gap: 16px;
    }
    .card:hover { border-color: var(--blue); box-shadow: 0 4px 16px rgba(59,130,246,0.15); }
    .card-range { display: flex; align-items: center; justify-content: space-between; }
    .range-label { font-size: 11px; font-weight: 600; color: var(--muted); text-transform: uppercase; letter-spacing: 0.6px; }
    .range-values { font-size: 17px; font-weight: 700; color: var(--text); letter-spacing: -0.3px; }
    .profit-chip { background: rgba(0,192,118,0.12); border: 1px solid rgba(0,192,118,0.25); border-radius: 6px; color: var(--green); font-size: 13px; font-weight: 600; padding: 3px 9px; }
    .card-divider { height: 1px; background: var(--border); }
    .legs { display: grid; grid-template-columns: 1fr 1fr; gap: 10px; }
    .leg { background: rgba(255,255,255,0.025); border: 1px solid var(--border); border-radius: 8px; padding: 10px 12px; }
    .leg-header { display: flex; align-items: center; gap: 6px; margin-bottom: 6px; }
    .leg-badge { font-size: 10px; font-weight: 700; border-radius: 4px; padding: 1px 6px; text-transform: uppercase; letter-spacing: 0.4px; }
    .leg-badge.yes { background: rgba(0,192,118,0.15); color: var(--green); border: 1px solid rgba(0,192,118,0.25); }
    .leg-badge.no  { background: rgba(244,63,94,0.15);  color: var(--red);   border: 1px solid rgba(244,63,94,0.25); }
    .leg-price { font-size: 15px; font-weight: 700; color: var(--text); }
    .leg-delta { font-size: 11px; color: var(--muted); margin-top: 2px; }
    .leg-token { font-size: 11px; color: var(--muted); margin-top: 2px; }
    .leg-token span { color: var(--text); font-weight: 500; }
    .stats { display: grid; grid-template-columns: repeat(3,1fr); gap: 8px; padding: 10px 12px; border: 1px solid var(--border); background: rgba(255,255,255,0.025); border-radius: 8px; }
    .stat { text-align: center; }
    .stat-label { font-size: 10px; font-weight: 500; color: var(--muted); text-transform: uppercase; letter-spacing: 0.5px; margin-bottom: 2px; }
    .stat-value { font-size: 14px; font-weight: 600; color: var(--text); }
    .stat-value.green { color: var(--green); }
    .stat-value.yellow { color: var(--yellow); }
    .cost-breakdown { background: rgba(59,130,246,0.08); border: 1px solid rgba(59,130,246,0.2); border-radius: 8px; padding: 10px 14px; display: flex; flex-direction: column; gap: 6px; }
    .cost-breakdown-label { font-size: 10px; font-weight: 600; color: var(--muted); text-transform: uppercase; letter-spacing: 0.5px; }
    .cost-breakdown-values { display: flex; align-items: center; gap: 8px; font-size: 13px; font-weight: 600; }
    .cost-yes { color: var(--green); }
    .cost-no  { color: var(--red); }
    .cost-plus { color: var(--muted); font-size: 11px; }
    .card-footer { display: flex; align-items: center; justify-content: space-between; font-size: 12px; color: var(--muted); }
    .expiry-dot { display: inline-block; width: 6px; height: 6px; border-radius: 50%; background: var(--green); margin-right: 5px; vertical-align: middle; }
    .expiry-dot.soon   { background: var(--yellow); }
    .expiry-dot.urgent { background: var(--red); }
    .q-container { display: flex; flex-direction: column; gap: 8px; }
    .q-row { font-size: 11px; color: var(--muted); white-space: nowrap; overflow: hidden; text-overflow: ellipsis; }
    .message { display: flex; flex-direction: column; align-items: center; padding: 80px 20px; color: var(--muted); font-size: 15px; }
    .message .icon { font-size: 36px; margin-bottom: 12px; }
    ::-webkit-scrollbar { width: 6px; }
    ::-webkit-scrollbar-track { background: transparent; }
    ::-webkit-scrollbar-thumb { background: var(--border2); border-radius: 3px; }
    /* Backtest tab */
    .tabs { display: flex; gap: 8px; margin-bottom: 20px; }
    .tab-btn {
      background: var(--card);
      border: 1px solid var(--border2);
      border-radius: 6px;
      color: var(--muted);
      font-family: inherit;
      font-size: 13px;
      font-weight: 500;
      padding: 8px 18px;
      cursor: pointer;
    }
    .tab-btn.active { border-color: rgba(59,130,246,0.5); color: var(--blue); background: rgba(59,130,246,0.12); }
    .tab-panel { display: none; }
    .tab-panel.active { display: block; }
    .backtest-form {
      background: var(--card);
      border: 1px solid var(--border);
      border-radius: 12px;
      padding: 24px;
      max-width: 540px;
      margin-bottom: 24px;
    }
    .backtest-form h2 { font-size: 16px; font-weight: 600; margin-bottom: 16px; }
    .form-row { display: flex; gap: 12px; margin-bottom: 12px; flex-wrap: wrap; }
    .form-group { display: flex; flex-direction: column; gap: 4px; flex: 1; min-width: 120px; }
    .form-group label { font-size: 11px; font-weight: 500; color: var(--muted); text-transform: uppercase; letter-spacing: 0.5px; }
    .form-group input {
      background: var(--surface);
      border: 1px solid var(--border2);
      border-radius: 6px;
      color: var(--text);
      font-family: inherit;
      font-size: 13px;
      padding: 8px 10px;
    }
    .form-group input:focus { outline: none; border-color: rgba(59,130,246,0.5); }
    .run-btn {
      background: rgba(59,130,246,0.15);
      border: 1px solid rgba(59,130,246,0.4);
      border-radius: 6px;
      color: var(--blue);
      font-family: inherit;
      font-size: 13px;
      font-weight: 600;
      padding: 10px 24px;
      cursor: pointer;
      margin-top: 4px;
    }
    .run-btn:hover { background: rgba(59,130,246,0.25); }
    #backtest-results { margin-top: 8px; }
    .bt-summary {
      display: grid;
      grid-template-columns: repeat(auto-fill, minmax(160px, 1fr));
      gap: 12px;
      margin-bottom: 20px;
    }
    .bt-stat {
      background: var(--card);
      border: 1px solid var(--border);
      border-radius: 10px;
      padding: 14px 16px;
      text-align: center;
    }
    .bt-stat-label { font-size: 10px; font-weight: 500; color: var(--muted); text-transform: uppercase; letter-spacing: 0.5px; margin-bottom: 6px; }
    .bt-stat-value { font-size: 22px; font-weight: 700; color: var(--text); }
    .bt-stat-value.green { color: var(--green); }
    .bt-stat-value.red   { color: var(--red); }
    .bt-stat-value.yellow { color: var(--yellow); }
    .bt-trades-table { width: 100%; border-collapse: collapse; font-size: 12px; }
    .bt-trades-table th { color: var(--muted); text-transform: uppercase; letter-spacing: 0.5px; font-size: 10px; font-weight: 600; padding: 8px 10px; border-bottom: 1px solid var(--border); text-align: left; }
    .bt-trades-table td { padding: 8px 10px; border-bottom: 1px solid var(--border); }
    .bt-trades-table tr:last-child td { border-bottom: none; }
    .won-yes { color: var(--green); font-weight: 600; }
    .won-no  { color: var(--red);   font-weight: 600; }
  </style>
</head>
<body>
  <header>
    <div class="header-inner">
      <div class="logo">Poly<span>Delta</span> <span style="font-size:12px;color:var(--muted);font-weight:400;">Rust</span></div>
      <div class="header-meta">
        <span id="updated-at"></span>
        <div class="btc-badge" id="btc-price"></div>
      </div>
    </div>
  </header>

  <main>
    <div class="dry-run-banner" id="dry-run-banner">
      ⚠️ DRY-RUN MODE — No real orders will be placed. All actions are simulated.
    </div>

    <div class="tabs">
      <button class="tab-btn active" onclick="showTab('scanner')">📊 Scanner</button>
      <button class="tab-btn" onclick="showTab('backtest')">📈 Backtesting</button>
    </div>

    <!-- Scanner tab -->
    <div class="tab-panel active" id="tab-scanner">
      <div class="month-filters">
        <span class="month-filter-label">Filter by</span>
        <div id="month-filters"></div>
      </div>
      <div class="toolbar">
        <span class="toolbar-label">Sort by</span>
        <button class="sort-btn active" data-key="profit_pct" onclick="setSort('profit_pct')">
          Profit % <span id="arrow-profit_pct">↓</span>
        </button>
        <button class="sort-btn" data-key="rr_reward" onclick="setSort('rr_reward')">
          R:R <span id="arrow-rr_reward"></span>
        </button>
        <button class="sort-btn" data-key="days_until" onclick="setSort('days_until')">
          Expiry <span id="arrow-days_until"></span>
        </button>
        <button class="sort-btn" data-key="cost_per_unit" onclick="setSort('cost_per_unit')">
          Entry Cost <span id="arrow-cost_per_unit"></span>
        </button>
        <span class="count-badge" id="pair-count"></span>
      </div>
      <div id="cards">
        <div class="message">
          <div class="icon">⏳</div>
          <p>Loading scanner data...</p>
        </div>
      </div>
    </div>

    <!-- Backtesting tab -->
    <div class="tab-panel" id="tab-backtest">
      <div class="backtest-form">
        <h2>Historical Backtest</h2>
        <div class="form-row">
          <div class="form-group">
            <label>Low Ratio (%)</label>
            <input type="number" id="bt-low-ratio" value="90" min="50" max="99" step="1" />
          </div>
          <div class="form-group">
            <label>High Ratio (%)</label>
            <input type="number" id="bt-high-ratio" value="110" min="101" max="200" step="1" />
          </div>
        </div>
        <div class="form-row">
          <div class="form-group">
            <label>Duration (days)</label>
            <input type="number" id="bt-duration" value="7" min="1" max="90" step="1" />
          </div>
          <div class="form-group">
            <label>YES Price Low</label>
            <input type="number" id="bt-yes-low" value="0.60" min="0.01" max="0.99" step="0.01" />
          </div>
          <div class="form-group">
            <label>YES Price High</label>
            <input type="number" id="bt-yes-high" value="0.70" min="0.01" max="0.99" step="0.01" />
          </div>
        </div>
        <div class="form-row">
          <div class="form-group">
            <label>History (days)</label>
            <input type="number" id="bt-history" value="90" min="7" max="365" step="1" />
          </div>
        </div>
        <button class="run-btn" onclick="runBacktest()">▶ Run Backtest</button>
      </div>
      <div id="backtest-results"></div>
    </div>
  </main>

  <script>
    const defaultDir = { profit_pct: 'desc', rr_reward: 'desc', days_until: 'asc', cost_per_unit: 'asc' };
    let sortKey = 'profit_pct', sortDir = 'desc';
    let allPairs = [], filteredPairs = [], selectedDate = 'all';

    async function loadData() {
      try {
        const resp = await fetch('/api/data');
        if (!resp.ok) {
          const err = await resp.json().catch(() => ({ error: 'Unknown error' }));
          showError(err.error || 'No data yet. The scanner is running…');
          return;
        }
        const data = await resp.json();
        allPairs = data.pairs || [];
        updateBaseUI(data.btc_price, data.generated_at);
        if (data.dry_run) document.getElementById('dry-run-banner').style.display = 'block';
        populateMonthFilters();
        updateSortUI();
        renderPairs();
      } catch (e) {
        showError('Could not reach the server: ' + e.message);
      }
    }

    function cardHTML(p) {
      const expiryDate = new Date(p.expiry);
      const expiryStr  = expiryDate.toLocaleDateString('en-US', { day:'numeric', month:'short', year:'numeric' });
      const dotClass   = p.days_until <= 1 ? 'urgent' : p.days_until <= 3 ? 'soon' : '';
      const daysStr    = p.days_until === 1 ? '1 day' : p.days_until + ' days';
      const lowQ  = p.low_question.length  > 70 ? p.low_question.slice(0,68)  + '…' : p.low_question;
      const highQ = p.high_question.length > 70 ? p.high_question.slice(0,68) + '…' : p.high_question;
      const rr = (1 / p.rr_reward).toFixed(2);
      return `
      <a href="${p.low_url}" target="_blank" rel="noopener" class="card-link">
        <div class="card">
          <div class="card-range">
            <div>
              <div class="range-label">Win Range</div>
              <div class="range-values">$${fmt(p.low_threshold)} – $${fmt(p.high_threshold)}</div>
            </div>
            <div class="profit-chip">+${p.profit_pct.toFixed(1)}%</div>
          </div>
          <div class="card-divider"></div>
          <div class="legs">
            <div class="leg">
              <div class="leg-header"><span class="leg-badge yes">YES</span></div>
              <div class="leg-price">$${fmt(p.low_threshold)}</div>
              <div class="leg-delta">${p.low_pct > 0 ? '+' : ''}${p.low_pct.toFixed(1)}% from spot</div>
              <div class="leg-token">@ <span>${p.yes_price_low.toFixed(4)}</span></div>
            </div>
            <div class="leg">
              <div class="leg-header"><span class="leg-badge no">NO</span></div>
              <div class="leg-price">$${fmt(p.high_threshold)}</div>
              <div class="leg-delta">${p.high_pct > 0 ? '+' : ''}${p.high_pct.toFixed(1)}% from spot</div>
              <div class="leg-token">@ <span>${p.no_price.toFixed(4)}</span></div>
            </div>
          </div>
          <div class="q-container">
            <div class="q-row" title="${esc(p.low_question)}">↓ ${esc(lowQ)}</div>
            <div class="q-row" title="${esc(p.high_question)}">↑ ${esc(highQ)}</div>
          </div>
          <div class="stats">
            <div class="stat"><div class="stat-label">Entry Cost</div><div class="stat-value">$${p.cost_per_unit.toFixed(4)}</div></div>
            <div class="stat"><div class="stat-label">Profit / Unit</div><div class="stat-value green">+$${p.profit_in_rng.toFixed(4)}</div></div>
            <div class="stat"><div class="stat-label">Risk : Reward</div><div class="stat-value yellow">${rr} : 1</div></div>
          </div>
          <div class="cost-breakdown">
            <div class="cost-breakdown-label">Example: $100 USDC</div>
            <div class="cost-breakdown-values">
              <span class="cost-yes">$${p.cost_low.toFixed(2)} YES</span>
              <span class="cost-plus">+</span>
              <span class="cost-no">$${p.cost_high.toFixed(2)} NO</span>
            </div>
          </div>
          <div class="card-divider"></div>
          <div class="card-footer">
            <span><span class="expiry-dot ${dotClass}"></span>${expiryStr} · ${daysStr}</span>
            <span>R:R 1 : ${p.rr_reward.toFixed(2)}</span>
          </div>
        </div>
      </a>`;
    }

    function renderPairs() {
      filteredPairs = selectedDate === 'all' ? allPairs : allPairs.filter(p => p.expiry_date === selectedDate);
      const sorted = [...filteredPairs].sort((a,b) => sortDir === 'desc' ? b[sortKey]-a[sortKey] : a[sortKey]-b[sortKey]);
      document.getElementById('pair-count').textContent = sorted.length + ' pair' + (sorted.length !== 1 ? 's' : '');
      document.getElementById('cards').innerHTML = sorted.length ? sorted.map(cardHTML).join('') : '<div class="message"><div class="icon">🔍</div><p>No pairs found.</p></div>';
    }

    function updateBaseUI(btcPrice, ts) {
      document.getElementById('btc-price').textContent = btcPrice ? '$' + Number(btcPrice).toLocaleString('en-US') : '';
      document.getElementById('updated-at').textContent = ts ? 'Updated ' + new Date(ts).toLocaleTimeString('en-US',{hour:'2-digit',minute:'2-digit'}) + ' UTC' : '';
    }

    function updateSortUI() {
      document.querySelectorAll('.sort-btn').forEach(btn => {
        const k = btn.dataset.key;
        btn.classList.toggle('active', k === sortKey);
        const el = document.getElementById('arrow-' + k);
        if (el) el.textContent = k === sortKey ? (sortDir === 'desc' ? '↓' : '↑') : '';
      });
    }

    function setSort(key) {
      if (sortKey === key) { sortDir = sortDir === 'desc' ? 'asc' : 'desc'; }
      else { sortKey = key; sortDir = defaultDir[key]; }
      updateSortUI(); renderPairs();
    }

    function populateMonthFilters() {
      const dates = [...new Set(allPairs.map(p => p.expiry_date))].sort((a,b) => {
        const [da,ma] = a.split('.').map(Number);
        const [db,mb] = b.split('.').map(Number);
        return ma === mb ? da - db : ma - mb;
      });
      document.getElementById('month-filters').innerHTML =
        `<button class="month-btn ${selectedDate==='all'?'active':''}" onclick="setDateFilter('all')">All</button>` +
        dates.map(d => `<button class="month-btn ${selectedDate===d?'active':''}" onclick="setDateFilter('${d}')">${d}</button>`).join('');
    }

    function setDateFilter(date) {
      selectedDate = date;
      populateMonthFilters();
      renderPairs();
    }

    function showError(msg) {
      document.getElementById('cards').innerHTML = `<div class="message"><div class="icon">⚠️</div><p>${esc(msg)}</p></div>`;
    }

    function showTab(name) {
      document.querySelectorAll('.tab-panel').forEach(p => p.classList.remove('active'));
      document.querySelectorAll('.tab-btn').forEach(b => b.classList.remove('active'));
      document.getElementById('tab-' + name).classList.add('active');
      event.target.classList.add('active');
    }

    async function runBacktest() {
      const lowRatio  = parseFloat(document.getElementById('bt-low-ratio').value) / 100;
      const highRatio = parseFloat(document.getElementById('bt-high-ratio').value) / 100;
      const duration  = parseInt(document.getElementById('bt-duration').value);
      const yesLow    = parseFloat(document.getElementById('bt-yes-low').value);
      const yesHigh   = parseFloat(document.getElementById('bt-yes-high').value);
      const history   = parseInt(document.getElementById('bt-history').value);

      const container = document.getElementById('backtest-results');
      container.innerHTML = '<div class="message"><div class="icon">⏳</div><p>Running backtest…</p></div>';

      try {
        const url = `/api/backtest?low_ratio=${lowRatio}&high_ratio=${highRatio}&duration=${duration}&yes_price_low=${yesLow}&yes_price_high=${yesHigh}&history_days=${history}`;
        const resp = await fetch(url);
        if (!resp.ok) {
          const err = await resp.json().catch(() => ({ error: 'Request failed' }));
          container.innerHTML = `<div class="message"><div class="icon">⚠️</div><p>${esc(err.error || 'Backtest failed')}</p></div>`;
          return;
        }
        const bt = await resp.json();
        renderBacktest(bt);
      } catch (e) {
        container.innerHTML = `<div class="message"><div class="icon">⚠️</div><p>${esc(e.message)}</p></div>`;
      }
    }

    function renderBacktest(bt) {
      const pnlClass  = bt.total_pnl >= 0 ? 'green' : 'red';
      const wrClass   = bt.win_rate >= 50  ? 'green' : 'red';
      const html = `
        <div class="bt-summary">
          <div class="bt-stat"><div class="bt-stat-label">Total Trades</div><div class="bt-stat-value">${bt.total_trades}</div></div>
          <div class="bt-stat"><div class="bt-stat-label">Win Rate</div><div class="bt-stat-value ${wrClass}">${bt.win_rate.toFixed(1)}%</div></div>
          <div class="bt-stat"><div class="bt-stat-label">Wins</div><div class="bt-stat-value green">${bt.winning_trades}</div></div>
          <div class="bt-stat"><div class="bt-stat-label">Losses</div><div class="bt-stat-value red">${bt.losing_trades}</div></div>
          <div class="bt-stat"><div class="bt-stat-label">Total PnL</div><div class="bt-stat-value ${pnlClass}">${bt.total_pnl >= 0 ? '+' : ''}${bt.total_pnl.toFixed(4)}</div></div>
          <div class="bt-stat"><div class="bt-stat-label">Avg Profit %</div><div class="bt-stat-value yellow">${bt.avg_profit_pct.toFixed(2)}%</div></div>
        </div>
        <div style="overflow-x:auto;">
          <table class="bt-trades-table">
            <thead>
              <tr>
                <th>Entry Date</th><th>Expiry</th><th>Range</th>
                <th>Entry Cost</th><th>BTC at Expiry</th><th>PnL</th><th>Result</th>
              </tr>
            </thead>
            <tbody>
              ${bt.trades.slice(0, 50).map(t => `
                <tr>
                  <td>${new Date(t.entry_date).toLocaleDateString()}</td>
                  <td>${new Date(t.expiry_date).toLocaleDateString()}</td>
                  <td>$${fmt(t.low_threshold)} – $${fmt(t.high_threshold)}</td>
                  <td>$${t.entry_cost.toFixed(4)}</td>
                  <td>$${fmt(t.btc_at_expiry)}</td>
                  <td class="${t.pnl >= 0 ? 'won-yes' : 'won-no'}">${t.pnl >= 0 ? '+' : ''}${t.pnl.toFixed(4)}</td>
                  <td class="${t.won ? 'won-yes' : 'won-no'}">${t.won ? '✓ WIN' : '✗ LOSS'}</td>
                </tr>
              `).join('')}
            </tbody>
          </table>
          ${bt.trades.length > 50 ? `<p style="color:var(--muted);font-size:12px;margin-top:8px;">Showing first 50 of ${bt.trades.length} trades.</p>` : ''}
        </div>`;
      document.getElementById('backtest-results').innerHTML = html;
    }

    function fmt(n) { return Number(n).toLocaleString('en-US', { maximumFractionDigits: 0 }); }
    function esc(s) { return String(s).replace(/&/g,'&amp;').replace(/</g,'&lt;').replace(/>/g,'&gt;').replace(/"/g,'&quot;'); }

    // Auto-refresh every 60 seconds
    loadData();
    setInterval(loadData, 60_000);
  </script>
</body>
</html>
"#;
