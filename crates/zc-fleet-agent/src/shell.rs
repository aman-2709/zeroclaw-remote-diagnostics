//! Safe shell command executor for agent mode.
//!
//! Executes allowlisted system commands with strict safety checks:
//! - Only pre-approved read-only commands
//! - No shell metacharacters (prevents injection)
//! - No access to sensitive paths
//! - 5-second timeout, 64KB output cap
//! - Uses `tokio::process::Command` directly (no shell interpretation)

use std::time::Duration;
use tokio::process::Command;

/// Maximum output size in bytes (64 KB).
const MAX_OUTPUT_BYTES: usize = 64 * 1024;

/// Command execution timeout.
const TIMEOUT: Duration = Duration::from_secs(5);

/// Commands that are allowed to run.
const ALLOWED_COMMANDS: &[&str] = &[
    "cat",
    "ls",
    "df",
    "free",
    "uname",
    "uptime",
    "ps",
    "ip",
    "hostname",
    "sensors",
    "lscpu",
    "head",
    "tail",
    "wc",
    "du",
    "ss",
    "date",
    "dmesg",
    "journalctl",
    "systemctl",
    "vcgencmd",
];

/// Commands explicitly blocked (dangerous even if somehow reached).
const BLOCKED_COMMANDS: &[&str] = &[
    "rm", "dd", "sudo", "su", "kill", "killall", "pkill", "chmod", "chown", "chgrp", "curl",
    "wget", "python", "python3", "bash", "sh", "zsh", "perl", "ruby", "node", "nc", "ncat",
    "socat", "telnet", "ssh", "scp", "rsync", "mount", "umount", "mkfs", "fdisk", "parted",
    "iptables", "nft", "reboot", "shutdown", "poweroff", "halt", "init",
];

/// Shell metacharacters that indicate injection attempts.
const SHELL_METACHARACTERS: &[&str] = &[";", "|", "`", "$(", ">", "<", "&&", "||", "\n", "\r"];

/// Sensitive paths that must not be accessed.
const SENSITIVE_PATHS: &[&str] = &[
    "/etc/shadow",
    "/etc/gshadow",
    "/etc/sudoers",
    "/root",
    "/.ssh",
    "/id_rsa",
    "/id_ed25519",
    ".env",
    "credentials",
    "secrets",
];

/// Result of a shell command execution.
#[derive(Debug)]
pub struct ShellResult {
    pub stdout: String,
    pub stderr: String,
    pub exit_code: Option<i32>,
    pub truncated: bool,
}

/// Errors from shell command validation or execution.
#[derive(Debug, thiserror::Error)]
pub enum ShellError {
    #[error("command not allowed: {0}")]
    NotAllowed(String),
    #[error("blocked command: {0}")]
    Blocked(String),
    #[error("shell injection detected: {0}")]
    Injection(String),
    #[error("sensitive path access blocked: {0}")]
    SensitivePath(String),
    #[error("empty command")]
    Empty,
    #[error("command timed out after {0}s")]
    Timeout(u64),
    #[error("execution failed: {0}")]
    Exec(String),
}

/// Execute a shell command string safely.
///
/// Parses the command into tokens using `shell-words` (no shell interpretation),
/// validates against allowlists and blocklists, then executes with timeout.
pub async fn execute(command_str: &str) -> Result<ShellResult, ShellError> {
    let command_str = command_str.trim();
    if command_str.is_empty() {
        return Err(ShellError::Empty);
    }

    // Check for shell metacharacters before parsing
    for meta in SHELL_METACHARACTERS {
        if command_str.contains(meta) {
            return Err(ShellError::Injection(format!(
                "contains shell metacharacter: {meta:?}"
            )));
        }
    }

    // Parse into tokens (safe splitting, no shell interpretation)
    let tokens = shell_words::split(command_str)
        .map_err(|e| ShellError::Exec(format!("failed to parse command: {e}")))?;

    if tokens.is_empty() {
        return Err(ShellError::Empty);
    }

    let program = &tokens[0];
    let args = &tokens[1..];

    // Check blocked list first (higher priority)
    if BLOCKED_COMMANDS.contains(&program.as_str()) {
        return Err(ShellError::Blocked(program.clone()));
    }

    // Check allowed list
    if !ALLOWED_COMMANDS.contains(&program.as_str()) {
        return Err(ShellError::NotAllowed(program.clone()));
    }

    // Restrict systemctl to read-only subcommands
    if program == "systemctl" {
        let allowed_subcommands = ["status", "is-active", "is-enabled", "list-units", "show"];
        match args.first() {
            Some(sub) if allowed_subcommands.contains(&sub.as_str()) => {}
            Some(sub) => {
                return Err(ShellError::NotAllowed(format!(
                    "systemctl {sub} (only status/is-active/is-enabled/list-units/show allowed)"
                )));
            }
            None => {
                // bare "systemctl" is fine (lists units)
            }
        }
    }

    // Check all arguments for sensitive paths
    for arg in args {
        for sensitive in SENSITIVE_PATHS {
            if arg.contains(sensitive) {
                return Err(ShellError::SensitivePath(arg.clone()));
            }
        }
    }

    // Execute with timeout
    let result = tokio::time::timeout(TIMEOUT, async {
        Command::new(program)
            .args(args)
            .output()
            .await
            .map_err(|e| ShellError::Exec(format!("{program}: {e}")))
    })
    .await;

    let output = match result {
        Ok(Ok(output)) => output,
        Ok(Err(e)) => return Err(e),
        Err(_) => return Err(ShellError::Timeout(TIMEOUT.as_secs())),
    };

    // Truncate output if necessary
    let mut stdout = String::from_utf8_lossy(&output.stdout).into_owned();
    let mut truncated = false;
    if stdout.len() > MAX_OUTPUT_BYTES {
        stdout.truncate(MAX_OUTPUT_BYTES);
        // Truncate at last newline to avoid partial lines
        if let Some(pos) = stdout.rfind('\n') {
            stdout.truncate(pos + 1);
        }
        stdout.push_str("\n... [output truncated at 64KB]");
        truncated = true;
    }

    let stderr = String::from_utf8_lossy(&output.stderr).into_owned();

    Ok(ShellResult {
        stdout,
        stderr,
        exit_code: output.status.code(),
        truncated,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn allowed_command_succeeds() {
        let result = execute("uname -a").await;
        assert!(result.is_ok(), "uname should be allowed: {result:?}");
        let shell_result = result.unwrap();
        assert!(!shell_result.stdout.is_empty());
    }

    #[tokio::test]
    async fn hostname_succeeds() {
        let result = execute("hostname").await;
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn date_succeeds() {
        let result = execute("date").await;
        assert!(result.is_ok());
        assert!(!result.unwrap().stdout.is_empty());
    }

    #[tokio::test]
    async fn ls_with_path_succeeds() {
        let result = execute("ls /tmp").await;
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn blocked_command_rejected() {
        let result = execute("rm -rf /").await;
        assert!(matches!(result, Err(ShellError::Blocked(ref cmd)) if cmd == "rm"));
    }

    #[tokio::test]
    async fn sudo_blocked() {
        let result = execute("sudo ls").await;
        assert!(matches!(result, Err(ShellError::Blocked(_))));
    }

    #[tokio::test]
    async fn bash_blocked() {
        let result = execute("bash -c 'echo pwned'").await;
        assert!(matches!(result, Err(ShellError::Blocked(_))));
    }

    #[tokio::test]
    async fn unknown_command_not_allowed() {
        let result = execute("custom_binary --flag").await;
        assert!(matches!(result, Err(ShellError::NotAllowed(_))));
    }

    #[tokio::test]
    async fn pipe_injection_blocked() {
        let result = execute("ls | cat /etc/shadow").await;
        assert!(matches!(result, Err(ShellError::Injection(_))));
    }

    #[tokio::test]
    async fn semicolon_injection_blocked() {
        let result = execute("ls; rm -rf /").await;
        assert!(matches!(result, Err(ShellError::Injection(_))));
    }

    #[tokio::test]
    async fn backtick_injection_blocked() {
        let result = execute("ls `whoami`").await;
        assert!(matches!(result, Err(ShellError::Injection(_))));
    }

    #[tokio::test]
    async fn dollar_paren_injection_blocked() {
        let result = execute("ls $(whoami)").await;
        assert!(matches!(result, Err(ShellError::Injection(_))));
    }

    #[tokio::test]
    async fn redirect_injection_blocked() {
        let result = execute("echo bad > /etc/passwd").await;
        assert!(matches!(result, Err(ShellError::Injection(_))));
    }

    #[tokio::test]
    async fn and_chain_injection_blocked() {
        let result = execute("ls && rm -rf /").await;
        assert!(matches!(result, Err(ShellError::Injection(_))));
    }

    #[tokio::test]
    async fn sensitive_path_shadow_blocked() {
        let result = execute("cat /etc/shadow").await;
        assert!(matches!(result, Err(ShellError::SensitivePath(_))));
    }

    #[tokio::test]
    async fn sensitive_path_ssh_blocked() {
        let result = execute("cat /home/user/.ssh/id_rsa").await;
        assert!(matches!(result, Err(ShellError::SensitivePath(_))));
    }

    #[tokio::test]
    async fn empty_command_rejected() {
        let result = execute("").await;
        assert!(matches!(result, Err(ShellError::Empty)));
    }

    #[tokio::test]
    async fn whitespace_only_rejected() {
        let result = execute("   ").await;
        assert!(matches!(result, Err(ShellError::Empty)));
    }

    #[tokio::test]
    async fn systemctl_status_allowed() {
        // May fail if systemctl not present, but should not be rejected by validation
        let result = execute("systemctl status sshd").await;
        // Either succeeds or exec error (not NotAllowed)
        match result {
            Ok(_) => {}
            Err(ShellError::Exec(_)) => {}
            other => panic!("expected Ok or Exec error, got: {other:?}"),
        }
    }

    #[tokio::test]
    async fn systemctl_restart_blocked() {
        let result = execute("systemctl restart sshd").await;
        assert!(matches!(result, Err(ShellError::NotAllowed(_))));
    }

    #[tokio::test]
    async fn df_with_human_readable_succeeds() {
        let result = execute("df -h").await;
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn free_with_human_readable_succeeds() {
        let result = execute("free -h").await;
        assert!(result.is_ok());
    }
}
