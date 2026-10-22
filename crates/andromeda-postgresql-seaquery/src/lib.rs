//! The PostgreSQL adapter of sea-query: the one crate that spells SQL and
//! hands it to the driver.
//!
//! Everything here is dialect vocabulary with no NGSI-LD in it: the `PostGIS`,
//! `ltree`, `jsonb` and array operators sea-query has no native constructor
//! for, the casts a fragment needs, the utility commands (schema and database
//! DDL, settings, maintenance, `TimescaleDB` procedures) sea-query has no form
//! for, and the execution of every statement: [`statement::Statement`] for a
//! statement with bound values, [`command::Command`] for a utility command.
//! Both are sealed and render privately, so statement text never leaves this
//! crate as a string and no other crate builds SQL.
//!
//! It knows nothing of the query languages ETSI GS CIM 009 v1.9.1 defines
//! (that translation is `andromeda-postgresql-query`, built on these
//! primitives) and it names no broker relation, which is
//! `andromeda-postgresql-schema`.

#![deny(missing_docs)]
#![deny(clippy::missing_docs_in_private_items)]

pub mod advisory_lock;
pub mod aggregate;
pub mod branch;
pub mod calendar_period;
pub mod cast;
pub mod catalog_function;
pub mod check_constraint;
pub mod clock;
pub mod collation;
pub mod column_reference;
pub mod columnstore_chunk;
pub mod columnstore_conversion;
pub mod columnstore_enablement;
pub mod command;
pub mod conflict_image;
pub mod database_definition;
pub mod date_field;
pub mod datetime;
pub mod ddl_statement;
pub mod explain;
pub mod extension_update;
pub mod external_arguments;
pub mod gin_index;
mod identifier;
pub mod interval;
pub mod introspection;
pub mod jsonb;
pub mod literal;
pub mod ltree;
pub mod maintenance;
pub mod open_instant;
pub mod operator;
pub mod pg_array;
pub mod pg_operator;
pub mod pg_type;
pub mod pooler_console;
pub mod postgis;
pub mod queue;
pub mod relation_copy;
pub mod relation_lock;
pub mod relation_truncation;
pub mod returning_image;
pub mod row_rejection;
pub mod scalar_subquery;
pub mod schema_definition;
pub mod schema_migration;
pub mod sequence;
pub mod setting;
pub mod spherical_distance;
pub mod statement;
mod statement_arguments;
pub mod statement_planning;
pub mod statistics_object;
pub mod system_catalog;
pub mod temporary_table;
pub mod timescale;
pub mod timescale_job;
pub mod transaction;
pub mod transaction_id;
pub mod trigger_switch;
pub mod unnest;

#[cfg(test)]
andromeda_architecture_checks::dependency_boundary! {
    allow_workspace_dependencies: [
    ],
    allow_workspace_dev_dependencies: [
    ],
    allow_production_dependencies: ["sea-query", "sqlx", "sqlx-core"],
}
