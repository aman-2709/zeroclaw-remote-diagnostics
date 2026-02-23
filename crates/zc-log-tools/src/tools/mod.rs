//! Log analysis tool implementations.
//!
//! 5 tools: search_logs, analyze_errors, log_stats, tail_logs, query_journal.

pub mod analyze_errors;
pub mod log_stats;
pub mod query_journal;
pub mod search_logs;
pub mod tail_logs;

pub use analyze_errors::AnalyzeErrors;
pub use log_stats::LogStats;
pub use query_journal::QueryJournal;
pub use search_logs::SearchLogs;
pub use tail_logs::TailLogs;

use crate::types::LogTool;

/// Return all available log analysis tools.
pub fn all_tools() -> Vec<Box<dyn LogTool>> {
    vec![
        Box::new(SearchLogs),
        Box::new(AnalyzeErrors),
        Box::new(LogStats),
        Box::new(TailLogs),
        Box::new(QueryJournal),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn all_tools_have_unique_names() {
        let tools = all_tools();
        let mut names: Vec<_> = tools.iter().map(|t| t.name()).collect();
        let original_len = names.len();
        names.sort();
        names.dedup();
        assert_eq!(names.len(), original_len, "tool names must be unique");
    }

    #[test]
    fn all_tools_have_valid_schemas() {
        for tool in all_tools() {
            let schema = tool.parameters_schema();
            assert!(
                schema.is_object(),
                "{} schema must be an object",
                tool.name()
            );
            assert!(
                schema["properties"].is_object(),
                "{} must have properties",
                tool.name()
            );
            assert!(
                schema["required"].is_array(),
                "{} must have required",
                tool.name()
            );
        }
    }

    #[test]
    fn all_tools_count() {
        assert_eq!(all_tools().len(), 5);
    }

    #[test]
    fn all_tools_have_descriptions() {
        for tool in all_tools() {
            assert!(
                !tool.description().is_empty(),
                "{} must have a description",
                tool.name()
            );
        }
    }
}
