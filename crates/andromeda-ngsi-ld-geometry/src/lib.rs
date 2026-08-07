//! Geospatial values and relations for NGSI-LD.
//!
//! ETSI GS CIM 009 v1.9.1 clause 4.7 models a GeoProperty value as a GeoJSON
//! geometry (IETF RFC 7946) in WGS84 / CRS84, and clause 4.10 defines the
//! `georel` spatial relationships a query may assert over one. This crate owns
//! both, and nothing else: it is a specification crate, so the one encoding it
//! carries is the one the specification names.
//!
//! PostGIS's EWKB is deliberately absent. It is a backend wire format, and it
//! lives in `andromeda-postgresql-encoding` alongside the other formats that are
//! properties of PostgreSQL rather than of NGSI-LD.
//!
//! It is a leaf: it names no other crate in this workspace, and it deliberately
//! knows nothing about persistence, transport or broker policy.
//!
//! Spatial *indexing* is not here either. An R-tree partitioned by tenant, type
//! and GeoProperty is a broker decision about which entities are worth
//! comparing; what a comparison means is the part that belongs to geometry, and
//! that is [`relation::matches`].

#![deny(missing_docs)]
#![deny(clippy::missing_docs_in_private_items)]

pub mod distance;
pub mod geo_rel;
pub mod geometry;
pub mod relation;

#[cfg(test)]
mod boundary {
    /// The layer rule this crate is on the inside of.
    ///
    /// Asserted here rather than by a central test so the failure names the
    /// crate that broke the rule, and so a crate added to the workspace cannot
    /// be forgotten by a table it was never added to. The rule itself is read
    /// once, in `andromeda-layer-guard`, which every crate carries as a
    /// dev-dependency and which declares nothing of its own.
    /// This crate is a leaf: it names no other crate in the workspace, because
    /// every geo-aware layer depends on it and an upward edge from here reaches
    /// all of them.
    #[test]
    fn this_crate_declares_only_what_its_layer_permits() {
        /// In-workspace crates this one may name, in any dependency table.
        /// Anything else is an upward or sideways edge.
        const ALLOWED_IN_WORKSPACE: &[&str] = &["andromeda-layer-guard"];

        andromeda_layer_guard::manifest::assert_layer(
            env!("CARGO_MANIFEST_DIR"),
            andromeda_layer_guard::manifest::BASELINE_FORBIDDEN,
            ALLOWED_IN_WORKSPACE,
        );
    }
}

#[cfg(test)]
mod dialect {
    /// No storage dialect may be written here, in any form.
    ///
    /// The manifest guard above reads dependency names, which is exactly what a
    /// dialect emitted as a string constant slips past: a specification crate
    /// can spell an entire SQL predicate without declaring `sea-query`.
    #[test]
    fn no_source_file_in_this_crate_emits_a_storage_dialect() {
        andromeda_layer_guard::dialect::assert_no_dialect(env!("CARGO_MANIFEST_DIR"));
    }
}
