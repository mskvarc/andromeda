//! Persistence layer for the NGSI-LD context broker.
//!
//! Each submodule provides a repository struct backed by PostgreSQL (via `sqlx`)
//! or an in-memory store, encapsulating all SQL queries and data mapping logic.

/// Shared `attribute_names` dictionary cache.
pub mod attribute_names;
pub mod copy;
pub mod csource_registration;
pub mod csource_subscription;
pub mod entity;
pub mod entity_map;
pub mod enum_labels;
/// Binding geometries into PostgreSQL statements.
pub mod ewkb_binding;
pub mod jsonld_context;
pub mod outbox;
pub mod pending_side_effects;
pub mod snapshot;
pub mod subscription;
pub mod subscription_store;
pub mod temporal;
pub mod timescale_features;
pub mod uow;
/// Binding typed query values into PostgreSQL statements.
pub mod value_binding;
