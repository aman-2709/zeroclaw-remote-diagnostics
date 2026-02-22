//! Natural-language inference for command parsing.
//!
//! Converts operator text ("read DTCs", "show log stats") into
//! structured `ParsedIntent` with tool_name, tool_args, and confidence.
//!
//! Two tiers:
//! - **Rule-based** (local): pattern matching for known commands, ~80% coverage.
//! - **Bedrock** (cloud): AWS Bedrock API for complex queries (Phase 2).

pub mod rules;

use async_trait::async_trait;
use zc_protocol::commands::ParsedIntent;

/// Trait for inference engines that parse natural language into tool intents.
#[async_trait]
pub trait InferenceEngine: Send + Sync {
    /// Parse natural-language text into a tool invocation intent.
    /// Returns None if the engine cannot parse the input.
    async fn parse(&self, text: &str) -> Option<ParsedIntent>;

    /// Name of this inference tier (for logging/audit).
    fn tier_name(&self) -> &str;
}

pub use rules::RuleBasedEngine;
