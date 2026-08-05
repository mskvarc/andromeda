//! PostgreSQL statements that read the temporal evolution of an entity.
//!
//! Clauses 5.6.11 onward define the temporal representation and the queries
//! over it. Answering one takes two stages — select the attribute instances a
//! request addresses, then read their history — plus a separate `time_bucket`
//! aggregation path for clause 4.5.19. This crate assembles all three from the
//! fragments `andromeda-postgresql-query` renders.
//!
//! Keeping it apart from the context statements is what makes a temporal
//! backend that is not PostgreSQL a wiring choice: a ClickHouse profile
//! replaces this crate and reuses the context statements unchanged.

#![deny(missing_docs)]
#![deny(clippy::missing_docs_in_private_items)]

pub mod aggregation;
pub mod history;
pub mod schema;
pub mod stage1;
pub mod time_column;

#[cfg(test)]
mod boundary {
    /// The layer rule this crate is on the inside of.
    ///
    /// A crate asserts its own manifest rather than a central test asserting
    /// everyone's: the failure then names the crate that broke the rule, and a
    /// crate added to the workspace cannot be forgotten by a table it was never
    /// added to.
    ///
    /// The context statement crate is forbidden by name, for the reason given
    /// in its counterpart: neither port group may name the other.
    #[test]
    fn this_crate_declares_only_what_its_layer_permits() {
        /// Third-party crates this layer must never declare.
        const FORBIDDEN: &[&str] = &["sqlx", "axum", "reqwest", "confique", "clap", "tokio", "andromeda-postgresql-context-query"];
        /// In-workspace crates this one may name. Anything else is an upward or
        /// sideways edge.
        const ALLOWED_IN_WORKSPACE: &[&str] = &[
            "andromeda-ngsi-ld-geometry",
            "andromeda-ngsi-ld-lexicon",
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
