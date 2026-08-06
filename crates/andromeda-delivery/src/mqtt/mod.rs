//! Delivery over MQTT: a publish on a pooled broker connection.
//!
//! Unlike HTTP, MQTT delivery is not one exchange per notification. A broker
//! connection is expensive and long-lived, several subscriptions may name the
//! same broker, and the connection's health is a thing in its own right — which
//! is why this module is a pool, a per-endpoint handle onto it, and a health
//! indicator, rather than a single transport.

pub mod ack;
pub mod config;
pub mod connection;
pub mod connection_key;
pub mod dial;
pub mod envelope;
pub mod handle;
pub mod health;
pub mod indicator;
pub mod metadata;
pub mod pool;
pub mod sys_topics;
