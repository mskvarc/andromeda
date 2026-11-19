//! Geospatial filtering: the `georel` / `geometry` / `coordinates` /
//! `geoproperty` parameter group (clause 4.10).
//!
//! A geo-query is a [`GeoConstraint`](crate::geo::geo_constraint::GeoConstraint)
//! in one of two name stages. In the written stage the `geoproperty` is the
//! attribute name the client wrote; `expand_geo_constraint` makes the expanded
//! stage, where it is the absolute `AttributeIri` the request `@context` expands
//! it to, and `GeoConstraint::into_written` reads that IRI back as a written
//! name for a stored resource. Compilation takes the written stage, expands the
//! `geoproperty`, admits the reference geometry and yields the
//! [`CompiledGeoQuery`](crate::geo::compiled_geo_query::CompiledGeoQuery) an
//! execution evaluates.

pub mod admission;
pub mod compilation;
pub mod compiled_geo_query;
pub mod default_location_narrowing;
pub mod expansion;
pub mod geo_constraint;
pub mod geo_query_error;
pub mod near_distance_band;
pub mod reference_budget;
pub mod reference_geometry;
pub mod registration_area;
pub mod required_member;
