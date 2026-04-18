//! Shared subscription persistence layer.
//!
//! - [`repo`] — trait definition and the `impl_subscription_store!` macro.
//! - [`rows`] — `SubscriptionRow` persistence struct.
//! - [`params`] — borrowed input structs.
//! - [`mapper`] — PgRow → row → domain conversion.

pub mod mapper;
pub mod params;
pub mod repo;
pub mod rows;
