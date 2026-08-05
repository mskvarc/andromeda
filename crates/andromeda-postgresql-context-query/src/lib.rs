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
    /// A crate asserts its own manifest rather than a central test asserting
    /// everyone's: the failure then names the crate that broke the rule, and a
    /// crate added to the workspace cannot be forgotten by a table it was never
    /// added to.
    ///
    /// The temporal statement crate is forbidden by name, in both directions:
    /// the three-crate SQL split exists so a temporal backend that is not
    /// PostgreSQL can replace one group without inheriting the other.
    #[test]
    fn this_crate_declares_only_what_its_layer_permits() {
        /// Third-party crates this layer must never declare.
        const FORBIDDEN: &[&str] = &["sqlx", "axum", "reqwest", "confique", "clap", "tokio", "andromeda-postgresql-temporal-query"];
        /// In-workspace crates this one may name. Anything else is an upward or
        /// sideways edge.
        const ALLOWED_IN_WORKSPACE: &[&str] = &[
            "andromeda-ngsi-ld-geometry",
            "andromeda-ngsi-ld-lexicon",
            "andromeda-ngsi-ld-model",
            "andromeda-ngsi-ld-query",
            "andromeda-ngsi-ld-representation",
            "andromeda-ngsi-ld-vocabulary",
            "andromeda-postgresql-encoding",
            "andromeda-postgresql-query",
        ];

        let manifest = include_str!("../Cargo.toml");
        let dependencies: Vec<&str> = manifest
            .split("[dependencies]")
            .nth(1)
            .unwrap_or_else(|| unreachable!("manifest has a [dependencies] table"))
            .split("\n[")
            .next()
            .unwrap_or_else(|| unreachable!("dependency table is terminated"))
            .lines()
            .map(str::trim)
            .filter(|line| !line.is_empty() && !line.starts_with('#'))
            .collect();

        for line in &dependencies {
            let name = line.split(['=', '.', ' ']).next().unwrap_or_default();
            assert!(!FORBIDDEN.contains(&name), "this crate must not depend on {name}");
            if name.starts_with("andromeda-") {
                assert!(ALLOWED_IN_WORKSPACE.contains(&name), "{name} is not an edge this crate's layer permits");
            }
        }
    }
}
