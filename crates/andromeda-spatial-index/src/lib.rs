//! Deciding which geometries are worth comparing.
//!
//! ETSI GS CIM 009 v1.9.1 clause 4.10 says what `georel=within` means; it says
//! nothing about how a broker avoids evaluating it against every entity it
//! holds. That is this crate: two R-trees partitioned by
//! [`key::SpatialKey`], one over entity geometries and one over subscription
//! reference geometries, each answering the same question from opposite ends.
//!
//! Nothing here decides what a relation *means*. A candidate that survives the
//! bounding-box prefilter is either put through
//! [`andromeda_ngsi_ld_geometry::relation::matches`], the one reading of clause
//! 4.10 the in-memory matchers share, or handed to the store, which applies its
//! own; the tree is only ever an optimisation.
//!
//! The boxes are great-circle bounds and `near` envelopes that cover every
//! point within the distance on the WGS 84 spheroid, wrapping across the
//! antimeridian, because `near` is measured there. Two relations get no
//! prefilter at all, and that is deliberate rather than missing: `disjoint`
//! matches precisely the entries a bounding-box test discards, and `near` with
//! only a `minDistance` matches entries arbitrarily far from the reference. The
//! subscription index scans its partition in full for them; the entity index
//! scans it for `disjoint` and declines to narrow such a `near` at all.

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
