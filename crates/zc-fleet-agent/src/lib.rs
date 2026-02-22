//! ZeroClaw Fleet Agent — library crate for edge runtime diagnostics.
//!
//! Re-exports all modules so external crates (e.g. `zc-e2e-tests`) can
//! access internal types like `CommandExecutor`, `ToolRegistry`, and
//! `OllamaClient`.

pub mod config;
pub mod executor;
pub mod heartbeat;
pub mod inference;
pub mod mqtt_loop;
pub mod registry;
