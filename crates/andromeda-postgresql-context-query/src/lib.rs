//! PostgreSQL statements that read current context information.
//!
//! Clause 3.1 calls an entity's current state *context information*; clauses
//! 5.6.3 and 5.9 define how it and the context source registrations that shadow
//! it are queried. This crate assembles those two `SELECT` statements from the
//! fragments `andromeda-postgresql-query` renders, and does nothing else: it
//! opens no connection, holds no pool, and returns a
//! [`BuiltQuery`](andromeda_postgresql_query::built::BuiltQuery) for the store
//! to bind and execute.

#![deny(missing_docs)]
#![deny(clippy::missing_docs_in_private_items)]

pub mod csr;
pub mod entity;
pub mod order_geography;
pub mod snapshot_purge;

#[cfg(test)]
mod boundary {
    /// The layer rule this crate is on the inside of.
    ///
    /// Asserted here rather than by a central test so the failure names the
    /// crate that broke the rule, and so a crate added to the workspace cannot
    /// be forgotten by a table it was never added to. The rule itself is read
    /// once, in `andromeda-layer-guard`, which every crate carries as a
    /// dev-dependency and which declares nothing of its own.
    /// The temporal statement crate is forbidden by name, in both directions:
    /// the three-crate SQL split exists so a temporal backend that is not
    /// PostgreSQL can replace one port group without inheriting the other.
    #[test]
    fn this_crate_declares_only_what_its_layer_permits() {
        /// Third-party crates this layer must never ship.
        const FORBIDDEN: &[&str] = &["sqlx", "axum", "reqwest", "confique", "clap", "tokio", "andromeda-postgresql-temporal-query"];
        /// In-workspace crates this one may name, in any dependency table.
        /// Anything else is an upward or sideways edge.
        const ALLOWED_IN_WORKSPACE: &[&str] = &[
            "andromeda-layer-guard",
            "andromeda-ngsi-ld-geometry",
            "andromeda-ngsi-ld-lexicon",
            "andromeda-ngsi-ld-model",
            "andromeda-ngsi-ld-query",
            "andromeda-ngsi-ld-representation",
            "andromeda-ngsi-ld-vocabulary",
            "andromeda-postgresql-encoding",
            "andromeda-postgresql-query",
        ];

        andromeda_layer_guard::manifest::assert_layer(env!("CARGO_MANIFEST_DIR"), FORBIDDEN, ALLOWED_IN_WORKSPACE);
    }
}
