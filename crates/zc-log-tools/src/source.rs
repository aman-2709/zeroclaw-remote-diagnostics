//! Log source abstraction — read log data from files, mocks, or other backends.

use async_trait::async_trait;

use crate::error::{LogError, LogResult};

/// Abstraction for reading log data from various sources.
///
/// Analogous to `CanInterface` in `zc-canbus-tools` — enables mocking
/// for tests and swappable backends (file, journald socket, etc.).
#[async_trait]
pub trait LogSource: Send + Sync {
    /// Read all lines from the given path/identifier.
    async fn read_lines(&self, path: &str) -> LogResult<Vec<String>>;

    /// Read the last `count` lines from the given path.
    async fn tail_lines(&self, path: &str, count: usize) -> LogResult<Vec<String>>;

    /// Check if a source path exists and is readable.
    async fn exists(&self, path: &str) -> bool;

    /// List available log sources (e.g., known log file paths).
    async fn list_sources(&self) -> LogResult<Vec<String>>;
}

/// Reads logs from the local filesystem.
pub struct FileLogSource;

#[async_trait]
impl LogSource for FileLogSource {
    async fn read_lines(&self, path: &str) -> LogResult<Vec<String>> {
        let content = tokio::fs::read_to_string(path).await.map_err(|e| {
            if e.kind() == std::io::ErrorKind::NotFound {
                LogError::NotFound(path.to_string())
            } else {
                LogError::Io(format!("{path}: {e}"))
            }
        })?;
        Ok(content.lines().map(String::from).collect())
    }

    async fn tail_lines(&self, path: &str, count: usize) -> LogResult<Vec<String>> {
        let all = self.read_lines(path).await?;
        let start = all.len().saturating_sub(count);
        Ok(all[start..].to_vec())
    }

    async fn exists(&self, path: &str) -> bool {
        tokio::fs::metadata(path).await.is_ok()
    }

    async fn list_sources(&self) -> LogResult<Vec<String>> {
        let candidates = [
            "/var/log/syslog",
            "/var/log/messages",
            "/var/log/kern.log",
            "/var/log/auth.log",
            "/var/log/daemon.log",
        ];
        let mut found = Vec::new();
        for path in &candidates {
            if self.exists(path).await {
                found.push((*path).to_string());
            }
        }
        Ok(found)
    }
}
