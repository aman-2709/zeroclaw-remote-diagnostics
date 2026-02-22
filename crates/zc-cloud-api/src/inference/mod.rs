//! Natural-language inference for command parsing.
//!
//! Converts operator text ("read DTCs", "show log stats") into
//! structured `ParsedIntent` with tool_name, tool_args, and confidence.
//!
//! Two tiers:
//! - **Rule-based** (local): pattern matching for known commands, ~80% coverage.
//! - **Bedrock** (cloud): AWS Bedrock Converse API for complex queries.

pub mod bedrock;
pub mod rules;
pub mod tiered;

use async_trait::async_trait;
use zc_protocol::commands::ParsedIntent;

/// Result of inference: the parsed intent plus which tier produced it.
#[derive(Debug, Clone)]
pub struct ParseResult {
    /// The parsed tool invocation intent.
    pub intent: ParsedIntent,
    /// Which inference tier produced this result (e.g. "local", "bedrock").
    pub tier: String,
}

/// Trait for inference engines that parse natural language into tool intents.
#[async_trait]
pub trait InferenceEngine: Send + Sync {
    /// Parse natural-language text into a tool invocation intent.
    /// Returns None if the engine cannot parse the input.
    async fn parse(&self, text: &str) -> Option<ParseResult>;

    /// Name of this inference tier (for logging/audit).
    fn tier_name(&self) -> &str;
}

pub use rules::RuleBasedEngine;
