//! Deciding which geometries are worth comparing.
//!
//! ETSI GS CIM 009 v1.9.1 clause 4.10 says what `georel=within` means; it says
//! nothing about how a broker avoids evaluating it against every entity it
//! holds. That is this crate: two R-trees partitioned by
//! [`key::SpatialKey`], one over entity geometries and one over subscription
//! reference geometries, each answering the same question from opposite ends.
//!
//! Nothing here decides what a relation *means*. Every candidate that survives
//! the bounding-box prefilter is put through
//! [`andromeda_ngsi_ld_geometry::relation::matches_geo`], the one reading of
//! clause 4.10 the in-memory matchers share, so an index hit and an index miss
//! answer identically and the tree is only ever an optimisation.
//!
//! Two relations get no prefilter at all, and that is deliberate rather than
//! missing: `disjoint` matches precisely the entries a bounding-box test
//! discards, and `near` with a `minDistance` matches entries arbitrarily far
//! from the reference. Both scan their partition in full.

#![deny(missing_docs)]
#![deny(clippy::missing_docs_in_private_items)]

pub mod entity;
pub mod key;
pub mod subscription;

#[cfg(test)]
mod boundary {
    /// The layer rule this crate is on the inside of.
    ///
    /// Asserted here rather than by a central test so the failure names the
    /// crate that broke the rule, and so a crate added to the workspace cannot
    /// be forgotten by a table it was never added to. The rule itself is read
    /// once, in `andromeda-layer-guard`, which every crate carries as a
    /// dev-dependency and which declares nothing of its own.
    ///
    /// An index that could read a store would be a query planner, and this one
    /// is deliberately not: it is filled from outside and answers only from
    /// what it holds.
    #[test]
    fn this_crate_declares_only_what_its_layer_permits() {
        /// Third-party crates this layer must never ship.
        const FORBIDDEN: &[&str] = &["sqlx", "sea-query", "geozero", "bytes", "axum", "reqwest", "clap", "confique"];
        /// In-workspace crates this one may name, in any dependency table.
        /// Anything else is an upward or sideways edge.
        const ALLOWED_IN_WORKSPACE: &[&str] = &[
            "andromeda-layer-guard",
            "andromeda-ngsi-ld-geometry",
            "andromeda-ngsi-ld-lexicon",
            "andromeda-ngsi-ld-query",
            "andromeda-tenancy",
        ];

        andromeda_layer_guard::manifest::assert_layer(env!("CARGO_MANIFEST_DIR"), FORBIDDEN, ALLOWED_IN_WORKSPACE);
    }
}

#[cfg(test)]
mod dialect {
    /// No storage dialect may be written here, in any form.
    ///
    /// The manifest guard above reads dependency names, which is exactly what a
    /// dialect emitted as a string constant slips past: a crate can spell an
    /// entire SQL predicate without declaring `sea-query`.
    #[test]
    fn no_source_file_in_this_crate_emits_a_storage_dialect() {
        andromeda_layer_guard::dialect::assert_no_dialect(env!("CARGO_MANIFEST_DIR"));
    }
}
