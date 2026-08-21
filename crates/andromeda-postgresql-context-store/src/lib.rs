//! PostgreSQL-backed context store for the NGSI-LD context broker.
//!
//! This crate owns the persistence layer for entity state, subscriptions,
//! context-source registrations, notification events and deliveries, and the
//! `current_attribute_values` projection. It does NOT depend on the temporal
//! store; both stores are independent.

#![deny(missing_docs)]
#![deny(clippy::missing_docs_in_private_items)]

/// Active PostgreSQL-family component discovery.
pub mod component_report;
/// PostgreSQL connection and pool configuration.
pub mod config;
/// Persistence layer for context source registrations.
pub mod csource_registration;
/// Repository for NGSI-LD context source subscriptions.
pub mod csource_subscription;
/// The current-state projection of the attribute instances entities hold.
pub mod current_attribute_values;
/// Persistence layer for NGSI-LD entities.
pub mod entity;
/// Entity Map persistence (clauses 5.2.39 and 5.5.14).
pub mod entity_map;
/// The contract between a domain enum and the PostgreSQL enum it is stored as.
pub mod enum_labels;
/// Error types for the context store crate.
pub mod error;
/// The platform route from minted subscription identifiers to tenants (clause 5.8.1.4).
pub mod forwarded_subscription_routes;
/// What the database pool reports to `/health`.
pub mod health;
/// Persistence layer for JSON-LD context documents.
pub mod jsonld_context;
/// PostgreSQL maintenance configuration owned beside the database connection settings.
pub mod maintenance_config;
/// Persistence layer for the notification delivery queue.
pub mod notification_delivery;
/// Durable side-effect queue for opt-in async batch processing.
pub mod pending_side_effects;
/// The schema contract one tenant database carries, and what it is checked against.
pub mod schema_contract;
/// Dialect DDL and session statements SeaQuery cannot express.
pub mod schema_ddl;
/// Persistence layer for snapshot records.
pub mod snapshot;
/// Filling the entity spatial index from the store.
/// Repository for NGSI-LD entity subscriptions.
pub mod subscription;
/// Subscription forwarding mappings (clause 5.8.1.4).
pub mod subscription_forwarding;
/// Shared subscription persistence layer.
pub mod subscription_store;
/// Reaching, creating and migrating a tenant's database.
pub mod tenant_database;
/// The `tenants` table, behind the tenancy crate's registry port.
pub mod tenant_registry;

#[cfg(test)]
andromeda_architecture_checks::dependency_boundary! {
    allow_workspace_dependencies: [
        "andromeda-component-report",
        "andromeda-context-commit",
        "andromeda-context-registry",
        "andromeda-entity",
        "andromeda-entity-map",
        "andromeda-health",
        "andromeda-jsonld",
        "andromeda-ngsi-ld-geometry",
        "andromeda-ngsi-ld-lexicon",
        "andromeda-ngsi-ld-model",
        "andromeda-ngsi-ld-number",
        "andromeda-ngsi-ld-query",
        "andromeda-ngsi-ld-vocabulary",
        "andromeda-notification",
        "andromeda-postgresql-name-dictionary",
        "andromeda-postgresql-codes",
        "andromeda-postgresql-context-query",
        "andromeda-postgresql-encoding",
        "andromeda-postgresql-query",
        "andromeda-postgresql-schema",
        "andromeda-postgresql-seaquery",
        "andromeda-registration",
        "andromeda-snapshot",
        "andromeda-subscription-forwarding",
        "andromeda-spatial-index",
        "andromeda-subscription",
        "andromeda-telemetry",
        "andromeda-tenancy",
        "andromeda-unit-catalog",
    ],
    allow_workspace_dev_dependencies: [
    ],
    allow_production_dependencies: ["confique", "sea-query", "sea-query-sqlx", "sqlx", "tokio"],
}
