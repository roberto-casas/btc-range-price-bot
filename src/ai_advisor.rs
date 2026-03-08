//! OpenAI-powered risk advisor for BTC range trading.
//!
//! Uses GPT-4o-mini as a **risk gate** — not a price predictor.
//! The model receives recent price data, volatility metrics, and the proposed
//! range, then returns a structured risk assessment indicating whether market
//! conditions make the trade unusually dangerous.
//!
//! This is opt-in: set `OPENAI_API_KEY` to enable, otherwise the module is a
//! no-op and all trades pass through.

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use tracing::{info, warn};

const OPENAI_CHAT_URL: &str = "https://api.openai.com/v1/chat/completions";
const MODEL: &str = "gpt-4o-mini";
const REQUEST_TIMEOUT_SECS: u64 = 30;

// ── Public types ─────────────────────────────────────────────────────────────

/// Structured risk assessment returned by the AI advisor.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RiskAssessment {
    /// Overall risk level: "low", "medium", "high", or "extreme"
    pub risk_level: String,
    /// Confidence 0.0–1.0 that the proposed range will hold
    pub confidence: f64,
    /// Suggested adjustment to the low boundary (negative = tighten)
    pub suggested_low_adjust_pct: f64,
    /// Suggested adjustment to the high boundary (positive = widen)
    pub suggested_high_adjust_pct: f64,
    /// Human-readable reasoning
    pub reasoning: String,
    /// Whether the advisor recommends skipping this trade entirely
    pub skip_trade: bool,
    /// Key risk factors identified
    pub risk_factors: Vec<String>,
}

impl Default for RiskAssessment {
    fn default() -> Self {
        Self {
            risk_level: "low".to_string(),
            confidence: 0.8,
            suggested_low_adjust_pct: 0.0,
            suggested_high_adjust_pct: 0.0,
            reasoning: "AI advisor not available — using default assessment.".to_string(),
            skip_trade: false,
            risk_factors: vec![],
        }
    }
}

/// Context passed to the AI advisor for analysis.
#[derive(Debug, Clone, Serialize)]
pub struct AdvisorContext {
    pub btc_price: f64,
    pub proposed_low_threshold: f64,
    pub proposed_high_threshold: f64,
    pub low_pct_from_spot: f64,
    pub high_pct_from_spot: f64,
    pub days_until_expiry: i64,
    pub profit_pct: f64,
    pub recent_prices: Vec<f64>,
    pub daily_volatility_pct: Option<f64>,
    pub atr_14_pct: Option<f64>,
}

// ── OpenAI request/response types ────────────────────────────────────────────

#[derive(Serialize)]
struct ChatRequest {
    model: String,
    messages: Vec<ChatMessage>,
    temperature: f64,
    max_tokens: u32,
    response_format: ResponseFormat,
}

#[derive(Serialize)]
struct ResponseFormat {
    #[serde(rename = "type")]
    fmt_type: String,
}

#[derive(Serialize)]
struct ChatMessage {
    role: String,
    content: String,
}

#[derive(Deserialize)]
struct ChatResponse {
    choices: Vec<ChatChoice>,
}

#[derive(Deserialize)]
struct ChatChoice {
    message: ChatMessageResp,
}

#[derive(Deserialize)]
struct ChatMessageResp {
    content: String,
}

// ── AI response parsing ──────────────────────────────────────────────────────

#[derive(Deserialize)]
struct AiRiskResponse {
    risk_level: String,
    confidence: f64,
    suggested_low_adjust_pct: Option<f64>,
    suggested_high_adjust_pct: Option<f64>,
    reasoning: String,
    skip_trade: bool,
    risk_factors: Vec<String>,
}

// ── Public API ───────────────────────────────────────────────────────────────

/// Check if the OpenAI advisor is available (API key is set).
pub fn is_available() -> bool {
    std::env::var("OPENAI_API_KEY").map(|k| !k.is_empty()).unwrap_or(false)
}

/// Request a risk assessment from the OpenAI API.
/// Returns `RiskAssessment::default()` if the API key is missing or the call fails.
pub async fn assess_risk(
    client: &reqwest::Client,
    ctx: &AdvisorContext,
) -> RiskAssessment {
    let api_key = match std::env::var("OPENAI_API_KEY") {
        Ok(k) if !k.is_empty() => k,
        _ => {
            return RiskAssessment::default();
        }
    };

    match call_openai(client, &api_key, ctx).await {
        Ok(assessment) => assessment,
        Err(e) => {
            warn!("AI advisor call failed: {e} — using default assessment");
            RiskAssessment::default()
        }
    }
}

/// Assess multiple pairs and return risk assessments.
pub async fn assess_pairs(
    client: &reqwest::Client,
    contexts: &[AdvisorContext],
) -> Vec<RiskAssessment> {
    let mut results = Vec::with_capacity(contexts.len());
    for ctx in contexts {
        results.push(assess_risk(client, ctx).await);
    }
    results
}

// ── Internal ─────────────────────────────────────────────────────────────────

async fn call_openai(
    client: &reqwest::Client,
    api_key: &str,
    ctx: &AdvisorContext,
) -> Result<RiskAssessment> {
    let system_prompt = build_system_prompt();
    let user_prompt = build_user_prompt(ctx);

    let request = ChatRequest {
        model: MODEL.to_string(),
        messages: vec![
            ChatMessage {
                role: "system".to_string(),
                content: system_prompt,
            },
            ChatMessage {
                role: "user".to_string(),
                content: user_prompt,
            },
        ],
        temperature: 0.2,
        max_tokens: 800,
        response_format: ResponseFormat {
            fmt_type: "json_object".to_string(),
        },
    };

    let resp = client
        .post(OPENAI_CHAT_URL)
        .bearer_auth(api_key)
        .json(&request)
        .timeout(std::time::Duration::from_secs(REQUEST_TIMEOUT_SECS))
        .send()
        .await
        .context("Failed to send OpenAI request")?;

    if !resp.status().is_success() {
        let status = resp.status();
        let body = resp.text().await.unwrap_or_default();
        anyhow::bail!("OpenAI API error {status}: {body}");
    }

    let chat_resp: ChatResponse = resp
        .json()
        .await
        .context("Failed to parse OpenAI response")?;

    let content = chat_resp
        .choices
        .first()
        .map(|c| c.message.content.clone())
        .unwrap_or_default();

    let ai_resp: AiRiskResponse = serde_json::from_str(&content)
        .context("Failed to parse AI risk response JSON")?;

    let assessment = RiskAssessment {
        risk_level: ai_resp.risk_level,
        confidence: ai_resp.confidence.clamp(0.0, 1.0),
        suggested_low_adjust_pct: ai_resp.suggested_low_adjust_pct.unwrap_or(0.0),
        suggested_high_adjust_pct: ai_resp.suggested_high_adjust_pct.unwrap_or(0.0),
        reasoning: ai_resp.reasoning,
        skip_trade: ai_resp.skip_trade,
        risk_factors: ai_resp.risk_factors,
    };

    info!(
        "[AI] Risk={} confidence={:.0}% skip={} factors={}",
        assessment.risk_level,
        assessment.confidence * 100.0,
        assessment.skip_trade,
        assessment.risk_factors.len()
    );

    Ok(assessment)
}

fn build_system_prompt() -> String {
    r#"You are a quantitative risk analyst specializing in crypto derivatives and prediction markets.

Your job is to assess whether a proposed BTC price range trade is unusually risky given current market conditions. You are NOT predicting prices — you are identifying conditions that make a range trade more likely to fail.

Focus on:
1. Recent price volatility vs the proposed range width
2. Whether the range seems too tight given recent moves
3. Whether recent price action shows momentum that could break the range
4. Time to expiry relative to potential price movement

Respond with a JSON object:
{
  "risk_level": "low" | "medium" | "high" | "extreme",
  "confidence": 0.0-1.0,
  "suggested_low_adjust_pct": number (e.g. -2.0 means tighten low by 2%),
  "suggested_high_adjust_pct": number (e.g. 2.0 means widen high by 2%),
  "reasoning": "brief explanation",
  "skip_trade": boolean,
  "risk_factors": ["factor1", "factor2"]
}

Be conservative. Only set skip_trade=true for extreme conditions. Most trades should pass with risk_level "low" or "medium"."#.to_string()
}

fn build_user_prompt(ctx: &AdvisorContext) -> String {
    let prices_str: Vec<String> = ctx.recent_prices.iter().map(|p| format!("{p:.0}")).collect();
    let vol_str = ctx
        .daily_volatility_pct
        .map(|v| format!("{v:.2}%"))
        .unwrap_or_else(|| "N/A".to_string());
    let atr_str = ctx
        .atr_14_pct
        .map(|a| format!("{a:.2}%"))
        .unwrap_or_else(|| "N/A".to_string());

    format!(
        r#"Assess this BTC range trade:

Current BTC price: ${:.0}
Proposed range: ${:.0} (low, {:.1}% from spot) – ${:.0} (high, +{:.1}% from spot)
Days to expiry: {}
Expected profit if in range: {:.1}%

Recent BTC prices (newest first, daily): [{}]
Daily volatility: {}
ATR(14): {}

Is this range trade safe given current conditions?"#,
        ctx.btc_price,
        ctx.proposed_low_threshold,
        ctx.low_pct_from_spot,
        ctx.proposed_high_threshold,
        ctx.high_pct_from_spot,
        ctx.days_until_expiry,
        ctx.profit_pct,
        prices_str.join(", "),
        vol_str,
        atr_str,
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default_assessment() {
        let a = RiskAssessment::default();
        assert_eq!(a.risk_level, "low");
        assert!(!a.skip_trade);
        assert!(a.confidence > 0.0);
    }

    #[test]
    fn test_is_available_without_key() {
        // In test environment, OPENAI_API_KEY is typically not set
        // This just verifies the function doesn't panic
        let _ = is_available();
    }

    #[test]
    fn test_build_prompts() {
        let ctx = AdvisorContext {
            btc_price: 90000.0,
            proposed_low_threshold: 82800.0,
            proposed_high_threshold: 97200.0,
            low_pct_from_spot: -8.0,
            high_pct_from_spot: 8.0,
            days_until_expiry: 7,
            profit_pct: 15.0,
            recent_prices: vec![90000.0, 89500.0, 88000.0],
            daily_volatility_pct: Some(2.1),
            atr_14_pct: Some(3.5),
        };
        let sys = build_system_prompt();
        assert!(sys.contains("risk analyst"));
        let user = build_user_prompt(&ctx);
        assert!(user.contains("$90000"));
    }
}
