//! Geospatial values and relations for NGSI-LD.
//!
//! ETSI GS CIM 009 v1.9.1 clause 4.7 models a `GeoProperty` value as a `GeoJSON`
//! geometry (IETF RFC 7946) in WGS84 / CRS84, and clause 4.10 defines the
//! `georel` spatial relationships a query may assert over one. This crate owns
//! both, and nothing else: it is a specification crate, so the one encoding it
//! carries is the one the specification names.
//!
//! `PostGIS`'s EWKB is deliberately absent. It is a backend wire format, and it
//! lives in `andromeda-postgresql-encoding` alongside the other formats that are
//! properties of PostgreSQL rather than of NGSI-LD.
//!
//! It names one other crate of this workspace, `andromeda-ngsi-ld-number`,
//! which owns how a JSON text is read and its values counted, so that the
//! census of a geometry counts the values beside its positions exactly as a
//! payload's are counted. It deliberately knows nothing about persistence,
//! transport or broker policy.
//!
//! Spatial *indexing* is not here either. An R-tree partitioned by tenant, type
//! and `GeoProperty` is a broker decision about which entities are worth
//! comparing; what a comparison means is the part that belongs to geometry, and
//! that is [`relation::matches`].
//!
//! Coordinates are up to three-dimensional, because RFC 7946 section 3.1.1
//! makes altitude an optional third ordinate of every position and NGSI-LD
//! inherits it. The model that carries them — [`position::Position`],
//! [`form::GeometryForm`] — is this crate's own, and `geo` is used as the
//! planar computation engine underneath it: no maintained Rust crate offers
//! geographic predicates over three-dimensional coordinates, and none is
//! needed, because clause 4.10 evaluates its predicates on the horizontal
//! plane regardless. [`planar`] holds that projection and the reasoning. `near`
//! alone is measured on the earth rather than on the plane, along great-circle
//! edges and on the WGS 84 spheroid through `geo`'s geodesic primitives:
//! [`proximity`] holds that measure, [`great_circle`] the extent of a geometry
//! read that way and [`near_envelope`] the region a distance can reach.

#![deny(missing_docs)]
#![deny(clippy::missing_docs_in_private_items)]

pub mod bounding_box;
pub mod compact_geojson;
pub mod coordinate_path;
mod coordinate_tape;
pub mod dimension;
pub mod foreign_members;
pub mod form;
pub mod geo_rel;
pub mod geojson;
pub mod geojson_members;
#[cfg(test)]
mod geojson_oracle;
pub mod geometry;
pub mod geometry_census;
pub mod geometry_members;
pub mod geometry_type;
pub mod great_circle;
pub mod line_string_coordinates;
pub mod linear_ring;
pub mod near_envelope;
mod nested_coordinates;
pub mod planar;
pub mod position;
pub mod positive_distance;
pub mod proximity;
pub mod relation;
pub mod written_geometry;

#[cfg(test)]
andromeda_architecture_checks::dependency_boundary! {
    allow_workspace_dependencies: [
        "andromeda-ngsi-ld-number",
    ],
    allow_workspace_dev_dependencies: [
    ],
    allow_production_dependencies: [],
}

#[cfg(test)]
andromeda_architecture_checks::forbid_sql!();
