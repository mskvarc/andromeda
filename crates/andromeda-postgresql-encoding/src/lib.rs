//! PostgreSQL's own wire formats, for the values Andromeda binds and reads back.
//!
//! Everything here is a property of the backend rather than of NGSI-LD. EWKB is
//! PostGIS's extended well-known binary; ETSI GS CIM 009 v1.9.1 never mentions
//! it, and says only that a GeoProperty value is GeoJSON per RFC 7946. Encoding
//! therefore belongs on this side of the boundary and not in
//! `andromeda-ngsi-ld-geometry`, which models the specification's geometry and
//! nothing else.
//!
//! `sqlx` stays out. The impls that would put it here forward to `&[u8]` anyway,
//! so the store binds [`ewkb::Ewkb::as_bytes`] directly and this crate stays
//! usable by the query renderers, which must not see a database driver.

#![deny(missing_docs)]
#![deny(clippy::missing_docs_in_private_items)]

pub mod copy;
pub mod ewkb;
pub mod ewkb_binding;
pub mod ewkb_build;
pub mod ewkb_walk;

#[cfg(test)]
andromeda_architecture_checks::dependency_boundary! {
    allow_workspace_dependencies: [
        "andromeda-ngsi-ld-geometry",
    ],
    allow_workspace_dev_dependencies: [
    ],
    allow_production_dependencies: ["bytes", "geozero"],
}
