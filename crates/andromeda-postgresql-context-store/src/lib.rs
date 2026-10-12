//! PostgreSQL-backed context store for the NGSI-LD context broker.
//!
//! This crate owns the persistence layer for entity state, subscriptions,
//! context-source registrations, notification events and deliveries. It does
//! NOT depend on the temporal store; both stores are independent.

#![deny(missing_docs)]
#![deny(clippy::missing_docs_in_private_items)]

/// Write statements whose text depends on the branch alone.
mod branch_statement;
/// Active PostgreSQL-family component discovery.
pub mod component_report;
/// PostgreSQL connection and pool configuration.
pub mod config;
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
/// Persistence layer for NGSI-LD entities.
pub mod entity;
/// Entity Map persistence (clauses 5.2.39 and 5.5.14).
pub mod entity_map;
/// Error types for the context store crate.
pub mod error;
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
pub mod maintenance_config;
/// The three embedded migration sets and the ledger each is recorded in.
pub mod migration_set;
/// Persistence layer for the notification delivery queue.
pub mod notification_delivery;
/// Durable side-effect queue for opt-in async batch processing.
pub mod pending_side_effects;
/// Reading a tenant's database for the earliest instant it has work due.
pub mod pending_work;
/// The schema contract one tenant database carries, and what it is checked against.
pub mod schema_contract;
/// Dialect DDL and session statements SeaQuery cannot express.
pub mod schema_ddl;
/// Persistence layer for snapshot records.
pub mod snapshot;
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
        "andromeda-configuration-scalars",
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
        "andromeda-snapshot",
        "andromeda-spatial-index",
        "andromeda-subscription",
        "andromeda-subscription-forwarding",
        "andromeda-telemetry",
        "andromeda-tenancy",
        "andromeda-unit-catalog",
    ],
    allow_workspace_dev_dependencies: [
        "andromeda-json-literal",
        "andromeda-ngsi-ld-representation",
        "andromeda-test-fixtures",
    ],
    allow_production_dependencies: ["sea-query", "sea-query-sqlx", "sqlx", "tokio"],
}
