//! Application services implementing NGSI-LD business logic.
//!
//! Services orchestrate domain operations, coordinate repositories, and enforce
//! business rules. They sit between the API layer and persistence, providing
//! the core functionality for entity management, subscriptions, and context
//! source registration.

pub mod compiled_subscription;
pub mod csource_extract;
pub mod csource_forwarding;
pub mod csource_merge;
pub mod csource_registration;
pub mod csource_subscription;
pub mod entity;
pub mod entity_map;
/// Evaluating a clause 4.10 geo-query against an expanded entity payload.
pub mod geo_filter;
pub mod jsonld_context;
/// The transactional notification outbox.
pub mod outbox;
pub mod snapshot;
pub mod snapshot_executor;
pub mod snapshot_prepare;
pub mod snapshot_status;
pub mod subscription;
pub mod subscription_base;
/// Shared subscription cache for fast in-memory event matching.
pub mod subscription_cache;
pub mod subscription_core;
/// Pure domain logic for subscription matching and entity-level filtering.
pub mod subscription_matching;
pub mod subscription_status;
pub mod subscription_throttle;
pub mod temporal;
