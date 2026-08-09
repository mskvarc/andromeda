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
pub mod partitioned_rtree;
pub mod subscription;

#[cfg(test)]
andromeda_architecture_checks::dependency_boundary! {
    allow_workspace_dependencies: [
        "andromeda-ngsi-ld-geometry",
        "andromeda-ngsi-ld-lexicon",
        "andromeda-ngsi-ld-query",
        "andromeda-tenancy",
    ],
    allow_workspace_dev_dependencies: [
    ],
    allow_production_dependencies: [],
}

#[cfg(test)]
andromeda_architecture_checks::forbid_sql!();
