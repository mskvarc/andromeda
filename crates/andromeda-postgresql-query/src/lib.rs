//! The PostgreSQL spelling of the NGSI-LD query languages.
//!
//! `andromeda-ngsi-ld-query` compiles a request into the languages ETSI GS CIM
//! 009 v1.9.1 defines; this crate says how each of them is written in
//! PostgreSQL. A `q` filter becomes a probe against the `attribute_current`
//! projection, a `georel` becomes a PostGIS predicate, a `scopeQ` becomes an
//! `lquery` match, a type selection becomes a predicate over the `types` array
//! — plus the primitives those renderings are built from: typed bind values,
//! schema identifiers, jsonpath and literal escaping, and the two operators
//! sea-query has no native constructor for.
//!
//! Everything here is a fragment. Assembling fragments into a statement is the
//! job of the crate that owns the statement:
//! `andromeda-postgresql-context-query` for current entity state and context
//! source registrations, `andromeda-postgresql-temporal-query` for an entity's
//! temporal evolution. Both port groups render the same fragments, which is why
//! the fragments are here and not in either of them.
//!
//! `sqlx` stays out: a rendered statement leaves as a [`built::BuiltQuery`],
//! and the store binds its [`bind::BindValue`]s.

#![deny(missing_docs)]
#![deny(clippy::missing_docs_in_private_items)]

pub mod attribute_name_id;
pub mod attribute_name_lookup;
pub mod bind;
pub mod built;
pub mod column_reference;
pub mod error;
pub mod geo_predicate;
/// Input for a single attribute history observation in a bulk insert.
pub mod history_batch_entry;
pub mod jsonb;
pub mod literal;
pub mod ltree;
pub mod operator;
pub mod postgis;
pub mod projection_barrier;
pub mod q_predicate;
pub mod schema;
pub mod scope_predicate;
pub mod type_predicate;
pub mod value_binding;

#[cfg(test)]
andromeda_architecture_checks::dependency_boundary! {
    allow_workspace_dependencies: [
        "andromeda-ngsi-ld-geometry",
        "andromeda-ngsi-ld-lexicon",
        "andromeda-ngsi-ld-query",
        "andromeda-ngsi-ld-vocabulary",
        "andromeda-postgresql-encoding",
    ],
    allow_workspace_dev_dependencies: [
    ],
    allow_production_dependencies: ["sea-query"],
}
