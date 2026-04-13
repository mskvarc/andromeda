//! Application services implementing NGSI-LD business logic.
//!
//! Services orchestrate domain operations, coordinate repositories, and enforce
//! business rules. They sit between the API layer and persistence, providing
//! the core functionality for entity management, subscriptions, and context
//! source registration.

pub mod context_source_forwarding;
pub mod context_source_registration;
pub mod csource_subscription;
pub mod entity;
pub mod entity_map;
pub mod jsonld_context;
pub mod snapshot;
pub mod snapshot_executor;
pub mod subscription;
pub mod subscription_base;
/// Shared subscription cache for fast in-memory event matching.
pub mod subscription_cache;
pub mod subscription_core;
/// Pure domain logic for subscription matching and entity-level filtering.
pub mod subscription_matching;
pub mod temporal;
pub mod temporal_recorder;
