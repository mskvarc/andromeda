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
    /// Asserted here rather than by a central test so the failure names the
    /// crate that broke the rule, and so a crate added to the workspace cannot
    /// be forgotten by a table it was never added to. The rule itself is read
    /// once, in `andromeda-layer-guard`, which every crate carries as a
    /// dev-dependency and which declares nothing of its own.
    /// The context statement crate is forbidden by name, for the reason given in
    /// its counterpart: neither port group may name the other.
    #[test]
    fn this_crate_declares_only_what_its_layer_permits() {
        /// In-workspace crates this one may name, in any dependency table.
        /// Anything else is an upward or sideways edge.
        const ALLOWED_IN_WORKSPACE: &[&str] = &[
            "andromeda-layer-guard",
            "andromeda-ngsi-ld-geometry",
            "andromeda-ngsi-ld-lexicon",
            "andromeda-ngsi-ld-query",
            "andromeda-ngsi-ld-representation",
            "andromeda-postgresql-query",
        ];

        // A SQL statement layer, so it ships `sea-query`; and it must not name
        // its sibling context adapter, a sideways edge the allow-list already
        // excludes and this spells out.
        let mut forbidden = andromeda_layer_guard::manifest::baseline_forbidden_except(&["sea-query"]);
        forbidden.push("andromeda-postgresql-context-query");
        andromeda_layer_guard::manifest::assert_layer(env!("CARGO_MANIFEST_DIR"), &forbidden, ALLOWED_IN_WORKSPACE);
    }
}
