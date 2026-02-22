//! ZeroClaw Cloud API — library crate for fleet management REST server.
//!
//! Re-exports all modules so the binary (`main.rs`) and external crates
//! (e.g. `zc-e2e-tests`) can access internal types like `AppState`,
//! `build_router`, and `InferenceEngine`.

pub mod config;
pub mod db;
pub mod error;
pub mod events;
pub mod inference;
pub mod mqtt_bridge;
pub mod routes;
pub mod state;
