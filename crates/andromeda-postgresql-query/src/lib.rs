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
mod boundary {
    /// The layer rule this crate is on the inside of.
    ///
    /// Asserted here rather than by a central test so the failure names the
    /// crate that broke the rule, and so a crate added to the workspace cannot
    /// be forgotten by a table it was never added to. The rule itself is read
    /// once, in `andromeda-layer-guard`, which every crate carries as a
    /// dev-dependency and which declares nothing of its own.
    /// The data model is absent by design: this crate renders a compiled query,
    /// and a compiled query is a query-crate type. Assembling a resource here
    /// would mean a statement fragment had taken a job that belongs above it.
    #[test]
    fn this_crate_declares_only_what_its_layer_permits() {
        /// In-workspace crates this one may name, in any dependency table.
        /// Anything else is an upward or sideways edge.
        const ALLOWED_IN_WORKSPACE: &[&str] = &[
            "andromeda-layer-guard",
            "andromeda-ngsi-ld-geometry",
            "andromeda-ngsi-ld-lexicon",
            "andromeda-ngsi-ld-query",
            "andromeda-ngsi-ld-vocabulary",
            "andromeda-postgresql-encoding",
        ];

        // This is the SQL statement layer, so it ships `sea-query`; every other
        // driver, transport and runtime on the baseline stays forbidden.
        let forbidden = andromeda_layer_guard::manifest::baseline_forbidden_except(&["sea-query"]);
        andromeda_layer_guard::manifest::assert_layer(env!("CARGO_MANIFEST_DIR"), &forbidden, ALLOWED_IN_WORKSPACE);
    }
}
