//! PostgreSQL-backed context store for the NGSI-LD context broker.
//!
//! This crate owns the persistence layer for entity state, subscriptions,
//! context-source registrations, notification events and deliveries. It does
//! NOT depend on the temporal store; both stores are independent.

#![deny(missing_docs)]
#![deny(clippy::missing_docs_in_private_items)]

/// Active PostgreSQL-family component discovery.
pub mod component_report;
/// PostgreSQL connection and pool configuration.
/// The tenant-wide order context source registration and subscription writes
/// commit in.
mod context_source_change_clock;
/// Resolution of persisted live and Snapshot context branches.
pub mod context_space;
/// Persistence layer for context source registrations.
pub mod csource_registration;
/// Repository for NGSI-LD context source subscriptions.
pub mod csource_subscription;
/// The `current_attributes` rows an Entity's attributes are stored as.
pub mod current_attribute;
/// The session defaults installed on every database the broker serves from.
pub mod database_defaults;
/// Persistence layer for NGSI-LD entities.
pub mod entity;
/// Entity Map persistence (clauses 5.2.39 and 5.5.14).
pub mod entity_map;
/// One Entity's state as the `entity_state_record` a queue stores it in.
pub mod entity_state_record;
/// Error types for the context store crate.
pub mod error;
/// Updating a tenant database's versioned extensions to the versions the
/// server ships, and installing the optional extensions it ships.
pub mod extension_update;
/// The platform route from minted subscription identifiers to tenants (clause 5.8.1.4).
pub mod forwarded_subscription_routes;
/// What the database pool reports to `/health`.
pub mod health;
/// The JSON text of a `json` or `jsonb` column, borrowed straight from the
/// row, and the text a value is bound as to reach a `json` column.
pub mod json_text;
/// Persistence layer for JSON-LD context documents.
pub mod jsonld_context;
/// PostgreSQL maintenance configuration owned beside the database connection settings.
/// The three embedded migration sets and the ledger each is recorded in.
pub mod migration_set;
/// Persistence layer for the notification delivery queue.
pub mod notification_delivery;
/// The extensions installed when the server ships them, though the schema
/// does not depend on them.
pub mod optional_extension;
/// Durable side-effect queue for opt-in async batch processing.
pub mod pending_side_effects;
/// Reading a tenant's database for the earliest instant it has work due.
pub mod pending_work;
/// The schema contract one tenant database carries, and what it is checked against.
pub mod schema_contract;
/// Persistence layer for snapshot records.
pub mod snapshot;
/// The text of a statement as the driver receives it.
#[cfg(test)]
mod statement_text;
/// How the context store's own failures reach the ports it implements.
mod store_fault;
/// Conditions the context store's own callers guarantee.
pub mod store_invariant;
/// Repository for NGSI-LD entity subscriptions.
pub mod subscription;
/// Subscription forwarding mappings (clause 5.8.1.4).
pub mod subscription_forwarding;
/// Shared subscription persistence layer.
pub mod subscription_store;
/// Reaching and creating a tenant's database.
pub mod tenant_database;
/// The versioned extensions each tenant database held after its last open,
/// behind the tenancy crate's catch-up port.
pub mod tenant_extension_versions;
/// Migrating a tenant's database to the schema this binary embeds.
pub mod tenant_migration;
/// The `tenants` table, behind the tenancy crate's registry port.
pub mod tenant_registry;
/// Per-tenant operational usage, behind the tenancy crate's usage-probe port.
pub mod tenant_usage;
/// The `tenant_wake_schedule` table, behind the tenancy crate's wake-schedule port.
pub mod tenant_wake_schedule;

#[cfg(test)]
andromeda_architecture_checks::dependency_boundary! {
    allow_workspace_dependencies: [
        "andromeda-component-report",
        "andromeda-configuration",
        "andromeda-context-commit",
        "andromeda-context-registry",
        "andromeda-context-space",
        "andromeda-entity",
        "andromeda-entity-map",
        "andromeda-health",
        "andromeda-json-literal",
        "andromeda-jsonld",
        "andromeda-ngsi-ld-entity",
        "andromeda-ngsi-ld-geometry",
        "andromeda-ngsi-ld-lexicon",
        "andromeda-ngsi-ld-model",
        "andromeda-ngsi-ld-number",
        "andromeda-ngsi-ld-query",
        "andromeda-ngsi-ld-vocabulary",
        "andromeda-notification",
        "andromeda-persistence-failure",
        "andromeda-postgresql-attribute-instance",
        "andromeda-postgresql-branch",
        "andromeda-postgresql-codes",
        "andromeda-postgresql-collation",
        "andromeda-postgresql-context-query",
        "andromeda-postgresql-encoding",
        "andromeda-postgresql-name-dictionary",
        "andromeda-postgresql-pool",
        "andromeda-postgresql-query",
        "andromeda-postgresql-schema",
        "andromeda-postgresql-seaquery",
        "andromeda-registration",
        "andromeda-retry",
        "andromeda-snapshot",
        "andromeda-spatial-index",
        "andromeda-subscription",
        "andromeda-subscription-forwarding",
        "andromeda-telemetry",
        "andromeda-tenancy",
    ],
    allow_workspace_dev_dependencies: [
        "andromeda-json-literal",
        "andromeda-ngsi-ld-representation",
        "andromeda-postgresql-pool",
    ],
    allow_production_dependencies: ["sea-query", "sqlx", "tokio"],
}
