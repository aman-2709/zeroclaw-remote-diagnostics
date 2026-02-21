//! CAN bus diagnostic tool implementations.

pub mod can_monitor;
pub mod read_dtcs;
pub mod read_freeze;
pub mod read_pid;
pub mod read_vin;

pub use can_monitor::CanMonitorTool;
pub use read_dtcs::ReadDtcs;
pub use read_freeze::ReadFreeze;
pub use read_pid::ReadPid;
pub use read_vin::ReadVin;

use crate::types::CanTool;

/// Returns all available CAN bus diagnostic tools.
pub fn all_tools() -> Vec<Box<dyn CanTool>> {
    vec![
        Box::new(ReadPid),
        Box::new(ReadDtcs),
        Box::new(ReadVin),
        Box::new(ReadFreeze),
        Box::new(CanMonitorTool),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn all_tools_returns_five() {
        let tools = all_tools();
        assert_eq!(tools.len(), 5);
    }

    #[test]
    fn tool_names_unique() {
        let tools = all_tools();
        let names: Vec<&str> = tools.iter().map(|t| t.name()).collect();
        let mut deduped = names.clone();
        deduped.sort();
        deduped.dedup();
        assert_eq!(names.len(), deduped.len(), "tool names must be unique");
    }

    #[test]
    fn tool_schemas_valid_json() {
        let tools = all_tools();
        for tool in &tools {
            let schema = tool.parameters_schema();
            assert!(
                schema.is_object(),
                "{} schema must be an object",
                tool.name()
            );
        }
    }
}
