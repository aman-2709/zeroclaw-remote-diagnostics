//! MQTT channel for AWS IoT Core communication.
//!
//! Provides a typed MQTT abstraction for the ZeroClaw fleet agent:
//! - `Channel` trait for publish/subscribe (mockable in tests)
//! - `MqttChannel` with TLS (mTLS) for production
//! - `MockChannel` for testing without a broker
//! - `ShadowClient` for device shadow operations
//! - `IncomingMessage` classification for dispatching events

pub mod channel;
pub mod config;
pub mod error;
pub mod handler;
pub mod mock;
pub mod shadows;
pub mod tls;

// Re-exports for convenience.
pub use channel::{Channel, MqttChannel};
pub use config::MqttConfig;
pub use error::{MqttError, MqttResult};
pub use handler::{IncomingMessage, classify};
pub use mock::MockChannel;
pub use shadows::ShadowClient;
