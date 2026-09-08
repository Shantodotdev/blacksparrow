//! # AI Search & GEO Readiness Auditor
//!
//! Evaluates a website's readiness for Generative Engine Optimization (GEO) and
//! citations by AI search models (ChatGPT Search, Perplexity, Claude, Gemini).
//! Inspects `/robots.txt` for retrieval and training crawlers and probes `/llms.txt`.

use crate::crawler::client::{FetchOptions, HttpClient};
use crate::crawler::robots::RobotsTxt;
use crate::error::{SeoError, SeoResult};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::time::Duration;
use url::Url;

/// Risk level indicating whether an origin is blocking or harming AI search engine citations.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum AiSearchRisk {
    /// All major search/retrieval bots are allowed and /llms.txt is present.
    Low,
    /// Missing /llms.txt or non-critical AI bots blocked.
    Medium,
    /// Critical AI citation/search engines (PerplexityBot, OAI-SearchBot) are blocked.
    High,
}

impl std::fmt::Display for AiSearchRisk {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Low => write!(f, "LOW"),
            Self::Medium => write!(f, "MEDIUM"),
            Self::High => write!(f, "HIGH"),
        }
    }
}

/// Comprehensive readiness audit report for AI search engines.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AiReadinessReport {
    /// Base normalized domain root (e.g. `https://example.com`).
    pub base_url: String,
    /// Whether `/robots.txt` was reachable.
    pub robots_found: bool,
    /// Whether `/llms.txt` is published and returned HTTP 200 OK.
    pub llms_txt_found: bool,
    /// Initial summary snippet extracted from `/llms.txt`, if present.
    pub llms_txt_summary: Option<String>,
    /// Whether the extended `/llms-full.txt` file is published.
    pub llms_full_txt_found: bool,
    /// Status of AI search and real-time retrieval bots (e.g. PerplexityBot, OAI-SearchBot).
    pub retrieval_bots: HashMap<String, String>,
    /// Status of bulk AI foundation training bots (e.g. GPTBot, ClaudeBot, CCBot).
    pub training_bots: HashMap<String, String>,
    /// Overall citation risk rating.
    pub citation_search_risk: AiSearchRisk,
    /// Concrete, actionable recommendations to improve AI search visibility.
    pub recommendations: Vec<String>,
}

/// AI search crawlers that perform real-time retrieval and provide citation source links.
pub const RETRIEVAL_BOTS: &[&str] = &[
    "OAI-SearchBot",
    "ChatGPT-User",
    "PerplexityBot",
    "Claude-User",
    "Google-Extended",
];

/// AI crawlers that scrape content for model training data.
pub const TRAINING_BOTS: &[&str] = &[
    "GPTBot",
    "ClaudeBot",
    "CCBot",
    "Bytespider",
    "Meta-ExternalAgent",
    "Amazonbot",
    "Applebot-Extended",
];

/// Probes a website's `/robots.txt` and `/llms.txt` to produce an AI search readiness assessment.
///
/// # Errors
///
/// Returns [`SeoError::Url`] if the input URL is invalid or malformed.
pub async fn audit_ai_readiness(
    target_url: &str,
    user_agent: &str,
    timeout: Duration,
) -> SeoResult<AiReadinessReport> {
    let parsed = Url::parse(target_url)
        .map_err(|e| SeoError::Url(format!("Invalid target URL '{target_url}': {e}")))?;

    let origin = format!("{}://{}", parsed.scheme(), parsed.authority());
    let robots_url = format!("{}/robots.txt", origin);
    let llms_url = format!("{}/llms.txt", origin);
    let llms_full_url = format!("{}/llms-full.txt", origin);

    let client = HttpClient::new(FetchOptions {
        user_agent: user_agent.to_string(),
        timeout,
        connect_timeout: Duration::from_secs(5),
        max_redirects: 5,
        ..Default::default()
    })?;

    // 1. Probe /robots.txt
    let (robots_found, robots_txt) = match client.fetch(&robots_url).await {
        Ok(res) if res.status_code == 200 => (true, Some(RobotsTxt::parse(&res.body))),
        _ => (false, None),
    };

    let mut retrieval_bots = HashMap::new();
    for &bot in RETRIEVAL_BOTS {
        let status = match &robots_txt {
            Some(r) => {
                if r.is_allowed(bot, "/") {
                    "ALLOWED".to_string()
                } else {
                    "DISALLOWED".to_string()
                }
            }
            None => "ALLOWED".to_string(),
        };
        retrieval_bots.insert(bot.to_string(), status);
    }

    let mut training_bots = HashMap::new();
    for &bot in TRAINING_BOTS {
        let status = match &robots_txt {
            Some(r) => {
                if r.is_allowed(bot, "/") {
                    "ALLOWED".to_string()
                } else {
                    "DISALLOWED".to_string()
                }
            }
            None => "ALLOWED".to_string(),
        };
        training_bots.insert(bot.to_string(), status);
    }

    // 2. Probe /llms.txt
    let (llms_txt_found, llms_txt_summary) = match client.fetch(&llms_url).await {
        Ok(res) if res.status_code == 200 => {
            let snippet = res
                .body
                .lines()
                .filter(|l| !l.trim().is_empty())
                .take(3)
                .collect::<Vec<_>>()
                .join("\n");
            let summary = if snippet.is_empty() {
                None
            } else {
                Some(snippet)
            };
            (true, summary)
        }
        _ => (false, None),
    };

    // 3. Probe /llms-full.txt
    let llms_full_txt_found = match client.fetch(&llms_full_url).await {
        Ok(res) => res.status_code == 200,
        Err(_) => false,
    };

    // 4. Assess Citation Search Risk & Formulate Recommendations
    let mut recommendations = Vec::new();

    let perplexity_disallowed = retrieval_bots
        .get("PerplexityBot")
        .map(|s| s == "DISALLOWED")
        .unwrap_or(false);
    let oai_search_disallowed = retrieval_bots
        .get("OAI-SearchBot")
        .map(|s| s == "DISALLOWED")
        .unwrap_or(false);

    let citation_search_risk = if perplexity_disallowed || oai_search_disallowed {
        AiSearchRisk::High
    } else if !llms_txt_found {
        AiSearchRisk::Medium
    } else {
        AiSearchRisk::Low
    };

    if perplexity_disallowed {
        recommendations.push(
            "PerplexityBot is disallowed in robots.txt. Your website cannot be cited as a reference in Perplexity search responses. Remove 'Disallow: /' for PerplexityBot.".to_string(),
        );
    }
    if oai_search_disallowed {
        recommendations.push(
            "OAI-SearchBot is disallowed in robots.txt. ChatGPT Search will not retrieve or cite your content in real-time answers. Allow OAI-SearchBot to regain visibility.".to_string(),
        );
    }
    if !llms_txt_found {
        recommendations.push(
            "Missing /llms.txt file. Publish a markdown summary at /llms.txt providing an authoritative index and technical overview for AI agents and LLMs.".to_string(),
        );
    }
    if !llms_full_txt_found && llms_txt_found {
        recommendations.push(
            "Publishing /llms-full.txt alongside /llms.txt allows AI models to consume complete documentation or product catalogues in a single high-density context file.".to_string(),
        );
    }

    Ok(AiReadinessReport {
        base_url: origin,
        robots_found,
        llms_txt_found,
        llms_txt_summary,
        llms_full_txt_found,
        retrieval_bots,
        training_bots,
        citation_search_risk,
        recommendations,
    })
}
