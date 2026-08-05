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
