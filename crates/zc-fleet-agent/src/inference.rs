//! Ollama local inference client for on-device NL command parsing.
//!
//! Calls the local Ollama HTTP API (`/api/chat`) to parse natural-language
//! operator commands into structured tool invocations. This handles ~80%
//! of queries at zero API cost; commands that don't match fall through to
//! cloud inference or fail with a descriptive error.

use serde::{Deserialize, Serialize};
use zc_protocol::commands::ParsedIntent;

/// System prompt shared with the cloud Bedrock engine — same 9-tool schema.
const SYSTEM_PROMPT: &str = r#"You are a command parser for an IoT fleet management platform. Your job is to parse natural-language operator commands into structured tool invocations.

Available tools:

1. read_dtcs — Read diagnostic trouble codes (DTCs) from the vehicle ECU.
   Parameters: {} (none)

2. read_vin — Read the Vehicle Identification Number.
   Parameters: {} (none)

3. read_freeze — Read freeze frame data captured when a DTC was set.
   Parameters: {} (none)

4. read_pid — Read a specific OBD-II Parameter ID (sensor value).
   Parameters: {"pid": "0x0C"} (hex string, e.g. 0x0C=RPM, 0x0D=speed, 0x05=coolant temp, 0x11=throttle, 0x2F=fuel level, 0x04=engine load, 0x0F=intake temp, 0x0E=timing advance)

5. can_monitor — Monitor raw CAN bus traffic for a duration.
   Parameters: {"duration_secs": 10} (integer, seconds)

6. search_logs — Search device logs for a pattern.
   Parameters: {"path": "/var/log/syslog", "query": "error"} (path string, query string)

7. analyze_errors — Analyze error patterns in device logs.
   Parameters: {"path": "/var/log/syslog"} (path string)

8. log_stats — Get log statistics (counts by severity, time ranges).
   Parameters: {"path": "/var/log/syslog"} (path string)

9. tail_logs — Show recent log entries.
   Parameters: {"path": "/var/log/syslog", "lines": 50} (path string, integer)

Respond with ONLY a JSON object (no markdown, no explanation):
{"tool_name": "<name>", "tool_args": {<args>}, "confidence": <0.0-1.0>}

If the command doesn't match any tool, respond with:
{"tool_name": null, "tool_args": {}, "confidence": 0.0}

Be generous in interpretation — operators use casual language. Map their intent to the closest tool."#;

/// Known tool names for validation. Must match the tools in SYSTEM_PROMPT.
const KNOWN_TOOLS: &[&str] = &[
    "read_dtcs",
    "read_vin",
    "read_freeze",
    "read_pid",
    "can_monitor",
    "search_logs",
    "analyze_errors",
    "log_stats",
    "tail_logs",
];

/// Minimum confidence threshold — below this we treat as "no match".
const MIN_CONFIDENCE: f64 = 0.3;

/// Configuration for the local Ollama inference endpoint.
#[derive(Debug, Clone, Deserialize)]
pub struct OllamaConfig {
    /// Ollama HTTP API base URL.
    #[serde(default = "default_host")]
    pub host: String,
    /// Model to use for inference.
    #[serde(default = "default_model")]
    pub model: String,
    /// Request timeout in seconds.
    #[serde(default = "default_timeout_secs")]
    pub timeout_secs: u64,
    /// Whether local inference is enabled.
    #[serde(default = "default_enabled")]
    pub enabled: bool,
}

fn default_host() -> String {
    "http://localhost:11434".into()
}
fn default_model() -> String {
    "phi3:mini".into()
}
fn default_timeout_secs() -> u64 {
    5
}
fn default_enabled() -> bool {
    true
}

impl Default for OllamaConfig {
    fn default() -> Self {
        Self {
            host: default_host(),
            model: default_model(),
            timeout_secs: default_timeout_secs(),
            enabled: default_enabled(),
        }
    }
}

/// Ollama chat API request body.
#[derive(Serialize)]
struct ChatRequest<'a> {
    model: &'a str,
    messages: Vec<ChatMessage<'a>>,
    format: &'a str,
    stream: bool,
}

/// A single message in the chat request.
#[derive(Serialize)]
struct ChatMessage<'a> {
    role: &'a str,
    content: &'a str,
}

/// Ollama chat API response (only fields we need).
#[derive(Deserialize)]
struct ChatResponse {
    message: Option<ResponseMessage>,
}

#[derive(Deserialize)]
struct ResponseMessage {
    content: String,
}

/// Raw LLM output before validation.
#[derive(Deserialize)]
struct RawIntent {
    tool_name: Option<String>,
    #[serde(default)]
    tool_args: serde_json::Value,
    #[serde(default)]
    confidence: f64,
}

/// Client for the local Ollama inference endpoint.
pub struct OllamaClient {
    client: reqwest::Client,
    config: OllamaConfig,
}

impl OllamaClient {
    pub fn new(config: OllamaConfig) -> Self {
        let client = reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(config.timeout_secs))
            .build()
            .expect("failed to build reqwest client");
        Self { client, config }
    }

    /// Parse a natural-language command into a `ParsedIntent`.
    ///
    /// Returns `None` if Ollama is unreachable, returns garbage, the tool name
    /// is unknown, or confidence is below threshold.
    pub async fn parse(&self, text: &str) -> Option<ParsedIntent> {
        let url = format!("{}/api/chat", self.config.host);

        let body = ChatRequest {
            model: &self.config.model,
            messages: vec![
                ChatMessage {
                    role: "system",
                    content: SYSTEM_PROMPT,
                },
                ChatMessage {
                    role: "user",
                    content: text,
                },
            ],
            format: "json",
            stream: false,
        };

        let response = match self.client.post(&url).json(&body).send().await {
            Ok(resp) => resp,
            Err(e) => {
                tracing::warn!(error = %e, "ollama request failed");
                return None;
            }
        };

        if !response.status().is_success() {
            tracing::warn!(status = %response.status(), "ollama returned non-200");
            return None;
        }

        let chat_resp: ChatResponse = match response.json().await {
            Ok(r) => r,
            Err(e) => {
                tracing::warn!(error = %e, "failed to parse ollama response body");
                return None;
            }
        };

        let content = chat_resp.message?.content;

        let raw: RawIntent = match serde_json::from_str(&content) {
            Ok(r) => r,
            Err(e) => {
                tracing::warn!(error = %e, content = %content, "ollama returned invalid JSON");
                return None;
            }
        };

        // Validate tool_name is present and known
        let tool_name = raw.tool_name?;
        if !KNOWN_TOOLS.contains(&tool_name.as_str()) {
            tracing::warn!(tool_name = %tool_name, "ollama returned unknown tool");
            return None;
        }

        // Validate confidence threshold
        if raw.confidence < MIN_CONFIDENCE {
            tracing::debug!(
                confidence = raw.confidence,
                tool_name = %tool_name,
                "ollama confidence below threshold"
            );
            return None;
        }

        Some(ParsedIntent {
            tool_name,
            tool_args: raw.tool_args,
            confidence: raw.confidence,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use wiremock::matchers::{method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    /// Helper: build an Ollama chat response body.
    fn ollama_response(content: &str) -> serde_json::Value {
        serde_json::json!({
            "model": "phi3:mini",
            "message": {
                "role": "assistant",
                "content": content
            },
            "done": true
        })
    }

    /// Build an OllamaClient pointed at the mock server.
    fn client_for(server: &MockServer) -> OllamaClient {
        OllamaClient::new(OllamaConfig {
            host: server.uri(),
            model: "phi3:mini".into(),
            timeout_secs: 2,
            enabled: true,
        })
    }

    #[tokio::test]
    async fn parse_read_dtcs() {
        let server = MockServer::start().await;
        let body =
            ollama_response(r#"{"tool_name": "read_dtcs", "tool_args": {}, "confidence": 0.95}"#);
        Mock::given(method("POST"))
            .and(path("/api/chat"))
            .respond_with(ResponseTemplate::new(200).set_body_json(&body))
            .mount(&server)
            .await;

        let client = client_for(&server);
        let result = client.parse("read the diagnostic trouble codes").await;

        let intent = result.expect("should parse successfully");
        assert_eq!(intent.tool_name, "read_dtcs");
        assert_eq!(intent.tool_args, serde_json::json!({}));
        assert!((intent.confidence - 0.95).abs() < f64::EPSILON);
    }

    #[tokio::test]
    async fn parse_unknown_command() {
        let server = MockServer::start().await;
        let body = ollama_response(r#"{"tool_name": null, "tool_args": {}, "confidence": 0.0}"#);
        Mock::given(method("POST"))
            .and(path("/api/chat"))
            .respond_with(ResponseTemplate::new(200).set_body_json(&body))
            .mount(&server)
            .await;

        let client = client_for(&server);
        let result = client.parse("bake me a pizza").await;
        assert!(result.is_none());
    }

    #[tokio::test]
    async fn parse_low_confidence() {
        let server = MockServer::start().await;
        let body = ollama_response(
            r#"{"tool_name": "read_pid", "tool_args": {"pid": "0x0C"}, "confidence": 0.1}"#,
        );
        Mock::given(method("POST"))
            .and(path("/api/chat"))
            .respond_with(ResponseTemplate::new(200).set_body_json(&body))
            .mount(&server)
            .await;

        let client = client_for(&server);
        let result = client.parse("maybe check something").await;
        assert!(result.is_none(), "confidence below 0.3 should be rejected");
    }

    #[tokio::test]
    async fn parse_unknown_tool() {
        let server = MockServer::start().await;
        let body = ollama_response(
            r#"{"tool_name": "self_destruct", "tool_args": {}, "confidence": 0.99}"#,
        );
        Mock::given(method("POST"))
            .and(path("/api/chat"))
            .respond_with(ResponseTemplate::new(200).set_body_json(&body))
            .mount(&server)
            .await;

        let client = client_for(&server);
        let result = client.parse("destroy everything").await;
        assert!(result.is_none(), "unknown tool names should be rejected");
    }

    #[tokio::test]
    async fn parse_timeout() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/api/chat"))
            .respond_with(ResponseTemplate::new(200).set_delay(std::time::Duration::from_secs(10)))
            .mount(&server)
            .await;

        // Client timeout is 2s, mock delays 10s → timeout
        let client = client_for(&server);
        let result = client.parse("read DTCs").await;
        assert!(result.is_none(), "timeout should return None");
    }

    #[tokio::test]
    async fn parse_invalid_json() {
        let server = MockServer::start().await;
        let body = ollama_response("this is not json at all");
        Mock::given(method("POST"))
            .and(path("/api/chat"))
            .respond_with(ResponseTemplate::new(200).set_body_json(&body))
            .mount(&server)
            .await;

        let client = client_for(&server);
        let result = client.parse("read DTCs").await;
        assert!(result.is_none(), "invalid JSON should return None");
    }

    #[tokio::test]
    async fn config_defaults() {
        let config = OllamaConfig::default();
        assert_eq!(config.host, "http://localhost:11434");
        assert_eq!(config.model, "phi3:mini");
        assert_eq!(config.timeout_secs, 5);
        assert!(config.enabled);
    }

    #[tokio::test]
    async fn config_from_toml() {
        let toml_str = r#"
host = "http://192.168.1.50:11434"
model = "gemma:2b"
timeout_secs = 10
enabled = false
"#;
        let config: OllamaConfig = toml::from_str(toml_str).unwrap();
        assert_eq!(config.host, "http://192.168.1.50:11434");
        assert_eq!(config.model, "gemma:2b");
        assert_eq!(config.timeout_secs, 10);
        assert!(!config.enabled);
    }
}
