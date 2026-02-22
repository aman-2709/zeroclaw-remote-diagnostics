//! Tiered inference engine — local-first with cloud fallback.
//!
//! Tries the local (rule-based) engine first. If it returns `None`,
//! falls back to the cloud (Bedrock) engine. The actual tier that
//! produced the result is recorded in `ParseResult.tier`.

use async_trait::async_trait;

use super::{InferenceEngine, ParseResult};

/// Composite engine that tries local inference first, then cloud.
pub struct TieredEngine {
    local: Box<dyn InferenceEngine>,
    cloud: Box<dyn InferenceEngine>,
}

impl TieredEngine {
    pub fn new(local: Box<dyn InferenceEngine>, cloud: Box<dyn InferenceEngine>) -> Self {
        Self { local, cloud }
    }
}

#[async_trait]
impl InferenceEngine for TieredEngine {
    async fn parse(&self, text: &str) -> Option<ParseResult> {
        // Try local first
        if let Some(result) = self.local.parse(text).await {
            return Some(result);
        }

        // Fall back to cloud
        tracing::debug!("local inference missed, falling back to cloud");
        self.cloud.parse(text).await
    }

    fn tier_name(&self) -> &str {
        "tiered"
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use zc_protocol::commands::ParsedIntent;

    /// Mock engine that always returns a fixed result (or None).
    struct MockEngine {
        result: Option<ParseResult>,
        name: &'static str,
    }

    impl MockEngine {
        fn hit(name: &'static str, tool: &str) -> Self {
            Self {
                result: Some(ParseResult {
                    intent: ParsedIntent {
                        tool_name: tool.into(),
                        tool_args: json!({}),
                        confidence: 0.95,
                    },
                    tier: name.into(),
                }),
                name,
            }
        }

        fn miss(name: &'static str) -> Self {
            Self { result: None, name }
        }
    }

    #[async_trait]
    impl InferenceEngine for MockEngine {
        async fn parse(&self, _text: &str) -> Option<ParseResult> {
            self.result.clone()
        }

        fn tier_name(&self) -> &str {
            self.name
        }
    }

    #[tokio::test]
    async fn local_hit_skips_cloud() {
        let engine = TieredEngine::new(
            Box::new(MockEngine::hit("local", "read_dtcs")),
            Box::new(MockEngine::hit("cloud", "read_dtcs")),
        );

        let result = engine.parse("read dtcs").await.unwrap();
        assert_eq!(result.tier, "local");
        assert_eq!(result.intent.tool_name, "read_dtcs");
    }

    #[tokio::test]
    async fn cloud_fallback_on_local_miss() {
        let engine = TieredEngine::new(
            Box::new(MockEngine::miss("local")),
            Box::new(MockEngine::hit("cloud", "read_pid")),
        );

        let result = engine.parse("what's the battery voltage?").await.unwrap();
        assert_eq!(result.tier, "cloud");
        assert_eq!(result.intent.tool_name, "read_pid");
    }

    #[tokio::test]
    async fn both_miss_returns_none() {
        let engine = TieredEngine::new(
            Box::new(MockEngine::miss("local")),
            Box::new(MockEngine::miss("cloud")),
        );

        assert!(engine.parse("hello world").await.is_none());
    }
}
