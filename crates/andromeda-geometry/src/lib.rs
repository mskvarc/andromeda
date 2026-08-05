//! Geospatial values, encodings and relations for NGSI-LD.
//!
//! ETSI GS CIM 009 v1.9.1 clause 4.7 models a GeoProperty value as a GeoJSON
//! geometry (IETF RFC 7946) in WGS84 / CRS84, and clause 4.10 defines the
//! `georel` spatial relationships a query may assert over one. This crate owns
//! both, together with the two encodings a broker needs them in: GeoJSON on the
//! wire and PostGIS EWKB at the store boundary.
//!
//! It is a leaf: it names no other crate in this workspace, and it deliberately
//! knows nothing about persistence, transport or broker policy. `sqlx` in
//! particular stays out — the store binds [`ewkb::Ewkb::as_bytes`] as `bytea`.
//!
//! Spatial *indexing* is not here either. An R-tree partitioned by tenant, type
//! and GeoProperty is a broker decision about which entities are worth
//! comparing; what a comparison means is the part that belongs to geometry, and
//! that is [`relation::matches`].

#![deny(missing_docs)]
#![deny(clippy::missing_docs_in_private_items)]

pub mod distance;
pub mod ewkb;
pub mod geo_rel;
pub mod geometry;
pub mod relation;
