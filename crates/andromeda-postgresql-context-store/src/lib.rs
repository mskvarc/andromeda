//! PostgreSQL-backed context store for the NGSI-LD context broker.
//!
//! This crate owns the persistence layer for entity state, subscriptions,
//! context-source registrations, outbox events, and the `attribute_current`
//! projection. It does NOT depend on the temporal store; both stores are
//! independent.

#![deny(missing_docs)]
#![deny(clippy::missing_docs_in_private_items)]

/// The `attribute_current` projection and the worker that maintains it.
pub mod attribute_current;
/// Shared `attribute_names` dictionary cache.
pub mod attribute_names;
/// Persistence layer for context source registrations.
pub mod csource_registration;
/// Repository for NGSI-LD context source subscriptions.
pub mod csource_subscription;
/// Persistence layer for NGSI-LD entities.
pub mod entity;
/// The contract between a domain enum and the PostgreSQL enum it is stored as.
pub mod enum_labels;
/// Error types for the context store crate.
pub mod error;
/// What the database pool reports to `/health`.
pub mod health;
/// Persistence layer for JSON-LD context documents.
pub mod jsonld_context;
/// Persistence layer for the notification outbox.
pub mod outbox;
/// Durable side-effect queue for opt-in async batch processing.
pub mod pending_side_effects;
/// Rebuilding one TEA's `attribute_current` row from `attribute_history`.
pub mod repair_attribute_current;
/// Persistence layer for snapshot records.
pub mod snapshot;
/// Filling the entity spatial index from the store.
pub mod spatial_warmup;
/// Repository for NGSI-LD entity subscriptions.
pub mod subscription;
/// Shared subscription persistence layer.
pub mod subscription_store;
/// Reaching, creating and migrating a tenant's database.
pub mod tenant_database;
/// The `tenants` table, behind the tenancy crate's registry port.
pub mod tenant_registry;

#[cfg(test)]
mod boundary {
    /// The layer rule this crate is on the inside of.
    #[test]
    fn this_crate_declares_only_what_its_layer_permits() {
        /// In-workspace crates this one may name, in any dependency table.
        const ALLOWED_IN_WORKSPACE: &[&str] = &[
            "andromeda-context-commit",
            "andromeda-context-registry",
            "andromeda-health",
            "andromeda-jsonld",
            "andromeda-layer-guard",
            "andromeda-ngsi-ld-geometry",
            "andromeda-ngsi-ld-lexicon",
            "andromeda-ngsi-ld-model",
            "andromeda-ngsi-ld-query",
            "andromeda-ngsi-ld-vocabulary",
            "andromeda-postgresql-context-query",
            "andromeda-postgresql-encoding",
            "andromeda-postgresql-query",
            "andromeda-registration",
            "andromeda-spatial-index",
            "andromeda-subscription",
            "andromeda-tenancy",
        ];
        let forbidden = andromeda_layer_guard::manifest::baseline_forbidden_except(&["sea-query", "sea-query-binder", "sonic-rs", "sqlx", "tokio"]);
        andromeda_layer_guard::manifest::assert_layer(env!("CARGO_MANIFEST_DIR"), &forbidden, ALLOWED_IN_WORKSPACE);
    }
}
