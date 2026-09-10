//! The generic PostgreSQL spelling of a `SeaQuery` statement.
//!
//! Everything here is dialect vocabulary with no NGSI-LD in it: the `PostGIS`,
//! `ltree`, `jsonb` and array operators sea-query has no native constructor for,
//! the casts and quoting a hand-written fragment needs, the typed bind values a
//! store binds, and the render-output struct a rendered statement leaves as.
//!
//! It knows nothing of the query languages ETSI GS CIM 009 v1.9.1 defines —
//! that translation is `andromeda-postgresql-query`, which is built on top of
//! these primitives — and it names no physical relation, which is
//! `andromeda-postgresql-schema`. `sqlx` stays out: a rendered statement leaves
//! as a `sea_query::SelectStatement` the store binds through `sea-query-sqlx`.

#![deny(missing_docs)]
#![deny(clippy::missing_docs_in_private_items)]

pub mod advisory_lock;
pub mod aggregate;
pub mod cast;
pub mod clock;
pub mod collation;
pub mod column_reference;
pub mod datetime;
pub mod interval;
pub mod introspection;
pub mod jsonb;
pub mod literal;
pub mod ltree;
pub mod maintenance;
pub mod operator;
pub mod pg_array;
pub mod pg_operator;
pub mod pg_type;
pub mod placeholder;
pub mod postgis;
pub mod queue;
pub mod spherical_distance;
pub mod timescale;
pub mod transaction;
pub mod unnest;

#[cfg(test)]
andromeda_architecture_checks::dependency_boundary! {
    allow_workspace_dependencies: [
    ],
    allow_workspace_dev_dependencies: [
    ],
    allow_production_dependencies: ["sea-query"],
}
