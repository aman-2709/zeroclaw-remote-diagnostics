//! Mock MQTT channel for testing without a real broker.
//!
//! Records all published messages and subscription filters for
//! assertion in tests.

use async_trait::async_trait;
use rumqttc::QoS;
use std::sync::Mutex;

use crate::channel::Channel;
use crate::error::MqttResult;

/// A recorded publish call.
#[derive(Debug, Clone)]
pub struct PublishedMessage {
    pub topic: String,
    pub payload: Vec<u8>,
    pub qos: QoS,
}

/// Mock implementation of the `Channel` trait.
///
/// Stores all publishes and subscriptions in memory for test verification.
/// Thread-safe via `Mutex` (fine for test contexts).
pub struct MockChannel {
    published: Mutex<Vec<PublishedMessage>>,
    subscriptions: Mutex<Vec<(String, QoS)>>,
}

impl MockChannel {
    pub fn new() -> Self {
        Self {
            published: Mutex::new(Vec::new()),
            subscriptions: Mutex::new(Vec::new()),
        }
    }

    /// Get all published messages.
    pub fn published(&self) -> Vec<PublishedMessage> {
        self.published.lock().unwrap().clone()
    }

    /// Get all subscription filters.
    pub fn subscriptions(&self) -> Vec<(String, QoS)> {
        self.subscriptions.lock().unwrap().clone()
    }

    /// Get the last published message.
    pub fn last_published(&self) -> Option<PublishedMessage> {
        self.published.lock().unwrap().last().cloned()
    }

    /// Get published messages for a specific topic.
    pub fn published_to(&self, topic: &str) -> Vec<PublishedMessage> {
        self.published
            .lock()
            .unwrap()
            .iter()
            .filter(|m| m.topic == topic)
            .cloned()
            .collect()
    }

    /// Check whether a subscription was made to the given filter.
    pub fn is_subscribed_to(&self, filter: &str) -> bool {
        self.subscriptions
            .lock()
            .unwrap()
            .iter()
            .any(|(f, _)| f == filter)
    }

    /// Clear all recorded state.
    pub fn reset(&self) {
        self.published.lock().unwrap().clear();
        self.subscriptions.lock().unwrap().clear();
    }
}

impl Default for MockChannel {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl Channel for MockChannel {
    async fn publish(&self, topic: &str, payload: &[u8], qos: QoS) -> MqttResult<()> {
        self.published.lock().unwrap().push(PublishedMessage {
            topic: topic.to_string(),
            payload: payload.to_vec(),
            qos,
        });
        Ok(())
    }

    async fn subscribe(&self, filter: &str, qos: QoS) -> MqttResult<()> {
        self.subscriptions
            .lock()
            .unwrap()
            .push((filter.to_string(), qos));
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn publish_records_messages() {
        let mock = MockChannel::new();
        mock.publish("test/topic", b"hello", QoS::AtLeastOnce)
            .await
            .unwrap();
        mock.publish("test/other", b"world", QoS::AtMostOnce)
            .await
            .unwrap();

        let msgs = mock.published();
        assert_eq!(msgs.len(), 2);
        assert_eq!(msgs[0].topic, "test/topic");
        assert_eq!(msgs[0].payload, b"hello");
        assert_eq!(msgs[1].topic, "test/other");
    }

    #[tokio::test]
    async fn subscribe_records_filters() {
        let mock = MockChannel::new();
        mock.subscribe("fleet/+/command/#", QoS::AtLeastOnce)
            .await
            .unwrap();

        assert!(mock.is_subscribed_to("fleet/+/command/#"));
        assert!(!mock.is_subscribed_to("fleet/+/telemetry/#"));
    }

    #[tokio::test]
    async fn last_published() {
        let mock = MockChannel::new();
        assert!(mock.last_published().is_none());

        mock.publish("a", b"1", QoS::AtMostOnce).await.unwrap();
        mock.publish("b", b"2", QoS::AtLeastOnce).await.unwrap();

        let last = mock.last_published().unwrap();
        assert_eq!(last.topic, "b");
    }

    #[tokio::test]
    async fn published_to_filter() {
        let mock = MockChannel::new();
        mock.publish("topic/a", b"1", QoS::AtMostOnce)
            .await
            .unwrap();
        mock.publish("topic/b", b"2", QoS::AtMostOnce)
            .await
            .unwrap();
        mock.publish("topic/a", b"3", QoS::AtMostOnce)
            .await
            .unwrap();

        let filtered = mock.published_to("topic/a");
        assert_eq!(filtered.len(), 2);
    }

    #[tokio::test]
    async fn reset_clears_state() {
        let mock = MockChannel::new();
        mock.publish("t", b"d", QoS::AtMostOnce).await.unwrap();
        mock.subscribe("f", QoS::AtLeastOnce).await.unwrap();

        mock.reset();
        assert!(mock.published().is_empty());
        assert!(mock.subscriptions().is_empty());
    }
}
