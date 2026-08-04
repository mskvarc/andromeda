//! Service-side extraction of persistence-shaped columns from expanded
//! JSON-LD entity payloads.
//!
//! The repository contract forbids `Value` inspection; the service layer
//! materialises every typed column (types, scopes, geometry, expiry) before a
//! persistence call is made. Geometries leave here in the domain's GeoJSON
//! form; the store encodes them to EWKB at the bind.

use crate::{
    domain::{expanded, geometry::Geometry},
    query::sql::scope_to_ltree,
    repository::entity::params::EntitySaveParams,
};
use serde_json::Value;

/// Build [`EntitySaveParams`] for `save()` from an expanded JSON-LD payload.
///
/// Performs one-shot `Value` inspection: extracts types, scopes, hoisted
/// geometries, and `expiresAt` so the repository can bind typed columns
/// without touching the payload.
pub fn build_save_params<'a>(id: &'a str, entity: &'a Value) -> EntitySaveParams<'a> {
    let types = expanded::extract_types(entity);
    let scope_paths: Vec<String> = expanded::extract_scopes(entity).iter().map(|s| scope_to_ltree(s)).collect();
    let location = expanded::extract_location_geometry(entity);
    let observation_space = expanded::extract_observation_space_geometry(entity);
    let operation_space = expanded::extract_operation_space_geometry(entity);
    let expires_at = expanded::extract_expires_at(entity);
    EntitySaveParams {
        id,
        payload: entity,
        types,
        scope_paths,
        location,
        observation_space,
        operation_space,
        expires_at,
    }
}

/// Extract the hoisted geometries (`location`, `observationSpace`,
/// `operationSpace`) from an expanded JSON-LD payload. Used by callers
/// that already own the id/types/scopes (e.g. `EntityCreateParams`).
pub fn extract_geo_geometries(entity: &Value) -> (Option<Geometry>, Option<Geometry>, Option<Geometry>) {
    (
        expanded::extract_location_geometry(entity),
        expanded::extract_observation_space_geometry(entity),
        expanded::extract_operation_space_geometry(entity),
    )
}
