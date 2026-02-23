//! Unified tool registry spanning CAN bus and log analysis tools.
//!
//! The fleet agent uses this to look up tools by name when dispatching
//! incoming command envelopes.

use std::collections::HashMap;

use zc_canbus_tools::{CanInterface, CanTool};
use zc_log_tools::{LogSource, LogTool};

/// Which subsystem a tool belongs to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ToolKind {
    CanBus,
    Log,
}

/// Metadata about a registered tool (used by tool listing API).
#[allow(dead_code)]
pub struct ToolInfo {
    pub name: String,
    pub description: String,
    pub kind: ToolKind,
    pub schema: serde_json::Value,
}

/// Unified tool registry for the fleet agent.
///
/// Holds CAN bus and log tools, indexed by name for O(1) dispatch.
pub struct ToolRegistry {
    can_tools: Vec<Box<dyn CanTool>>,
    log_tools: Vec<Box<dyn LogTool>>,
    /// Map from tool name → (kind, index into the appropriate Vec).
    index: HashMap<String, (ToolKind, usize)>,
}

impl ToolRegistry {
    /// Build a registry from CAN and log tool collections.
    pub fn new(can_tools: Vec<Box<dyn CanTool>>, log_tools: Vec<Box<dyn LogTool>>) -> Self {
        let mut index = HashMap::new();

        for (i, tool) in can_tools.iter().enumerate() {
            index.insert(tool.name().to_string(), (ToolKind::CanBus, i));
        }
        for (i, tool) in log_tools.iter().enumerate() {
            index.insert(tool.name().to_string(), (ToolKind::Log, i));
        }

        Self {
            can_tools,
            log_tools,
            index,
        }
    }

    /// Build with the default set of all tools from both crates.
    pub fn with_defaults() -> Self {
        Self::new(
            zc_canbus_tools::tools::all_tools(),
            zc_log_tools::tools::all_tools(),
        )
    }

    /// Look up a tool by name and return its kind + index.
    pub fn lookup(&self, name: &str) -> Option<(ToolKind, usize)> {
        self.index.get(name).copied()
    }

    /// Execute a CAN tool by index.
    pub async fn execute_can(
        &self,
        index: usize,
        args: serde_json::Value,
        interface: &dyn CanInterface,
    ) -> Result<serde_json::Value, String> {
        let tool = &self.can_tools[index];
        match tool.execute(args, interface).await {
            Ok(result) => serde_json::to_value(result).map_err(|e| e.to_string()),
            Err(e) => Err(e.to_string()),
        }
    }

    /// Execute a log tool by index.
    pub async fn execute_log(
        &self,
        index: usize,
        args: serde_json::Value,
        source: &dyn LogSource,
    ) -> Result<serde_json::Value, String> {
        let tool = &self.log_tools[index];
        match tool.execute(args, source).await {
            Ok(result) => serde_json::to_value(result).map_err(|e| e.to_string()),
            Err(e) => Err(e.to_string()),
        }
    }

    /// List all registered tools with metadata (used by tool listing API).
    #[allow(dead_code)]
    pub fn list_tools(&self) -> Vec<ToolInfo> {
        let mut tools = Vec::new();
        for tool in &self.can_tools {
            tools.push(ToolInfo {
                name: tool.name().to_string(),
                description: tool.description().to_string(),
                kind: ToolKind::CanBus,
                schema: tool.parameters_schema(),
            });
        }
        for tool in &self.log_tools {
            tools.push(ToolInfo {
                name: tool.name().to_string(),
                description: tool.description().to_string(),
                kind: ToolKind::Log,
                schema: tool.parameters_schema(),
            });
        }
        tools
    }

    /// Total number of registered tools.
    pub fn len(&self) -> usize {
        self.can_tools.len() + self.log_tools.len()
    }

    #[allow(dead_code)]
    pub fn is_empty(&self) -> bool {
        self.can_tools.is_empty() && self.log_tools.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn registry_with_defaults() {
        let reg = ToolRegistry::with_defaults();
        assert_eq!(reg.len(), 10); // 5 CAN + 5 log
    }

    #[test]
    fn lookup_can_tool() {
        let reg = ToolRegistry::with_defaults();
        let (kind, _idx) = reg.lookup("read_pid").unwrap();
        assert_eq!(kind, ToolKind::CanBus);
    }

    #[test]
    fn lookup_log_tool() {
        let reg = ToolRegistry::with_defaults();
        let (kind, _idx) = reg.lookup("search_logs").unwrap();
        assert_eq!(kind, ToolKind::Log);
    }

    #[test]
    fn lookup_unknown_returns_none() {
        let reg = ToolRegistry::with_defaults();
        assert!(reg.lookup("nonexistent_tool").is_none());
    }

    #[test]
    fn list_tools_has_all() {
        let reg = ToolRegistry::with_defaults();
        let tools = reg.list_tools();
        assert_eq!(tools.len(), 10);
        let names: Vec<&str> = tools.iter().map(|t| t.name.as_str()).collect();
        assert!(names.contains(&"read_pid"));
        assert!(names.contains(&"read_dtcs"));
        assert!(names.contains(&"read_vin"));
        assert!(names.contains(&"read_freeze"));
        assert!(names.contains(&"can_monitor"));
        assert!(names.contains(&"search_logs"));
        assert!(names.contains(&"analyze_errors"));
        assert!(names.contains(&"log_stats"));
        assert!(names.contains(&"tail_logs"));
        assert!(names.contains(&"query_journal"));
    }

    #[tokio::test]
    async fn execute_can_tool_dispatch() {
        let reg = ToolRegistry::with_defaults();
        let (kind, idx) = reg.lookup("read_pid").unwrap();
        assert_eq!(kind, ToolKind::CanBus);

        let mock = zc_canbus_tools::MockCanInterface::new();
        // No response queued → tool returns a result (may succeed or fail depending on
        // tool implementation), but the dispatch itself works without panic.
        let _result = reg
            .execute_can(idx, serde_json::json!({"pid": "0x0C"}), &mock)
            .await;
        // Dispatch completed without panic — that's what we're testing.
    }

    #[tokio::test]
    async fn execute_log_tool() {
        let reg = ToolRegistry::with_defaults();
        let (kind, idx) = reg.lookup("log_stats").unwrap();
        assert_eq!(kind, ToolKind::Log);

        let mock = zc_log_tools::MockLogSource::with_syslog_sample();
        let result = reg
            .execute_log(idx, serde_json::json!({"path": "/var/log/syslog"}), &mock)
            .await;
        assert!(result.is_ok());
    }
}
