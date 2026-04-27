//! Service-side extraction of persistence-shaped columns from expanded
//! JSON-LD entity payloads.
//!
//! The repository contract forbids `Value` inspection; the service layer
//! materialises every typed column (types, scopes, WKB geometry, expiry)
//! before a persistence call is made.

use crate::{
    domain::{expanded, geometry::Wkb},
    query::sql::scope_to_ltree,
    repository::entity::params::EntitySaveParams,
};
use serde_json::Value;

/// Build [`EntitySaveParams`] for `save()` from an expanded JSON-LD payload.
///
/// Performs one-shot `Value` inspection: extracts types, scopes, hoisted
/// geometry WKB, and `expiresAt` so the repository can bind typed columns
/// without touching the payload.
pub fn build_save_params<'a>(id: &'a str, entity: &'a Value) -> EntitySaveParams<'a> {
    let types = expanded::extract_types(entity);
    let scope_paths: Vec<String> = expanded::extract_scopes(entity).iter().map(|s| scope_to_ltree(s)).collect();
    let location_wkb = expanded::extract_location_wkb(entity);
    let observation_space_wkb = expanded::extract_observation_space_wkb(entity);
    let operation_space_wkb = expanded::extract_operation_space_wkb(entity);
    let expires_at = expanded::extract_expires_at(entity);
    EntitySaveParams {
        id,
        payload: entity,
        types,
        scope_paths,
        location_wkb,
        observation_space_wkb,
        operation_space_wkb,
        expires_at,
    }
}

/// Extract hoisted geometry WKBs (`location`, `observationSpace`,
/// `operationSpace`) from an expanded JSON-LD payload. Used by callers
/// that already own the id/types/scopes (e.g. `EntityCreateParams`).
pub fn extract_geo_wkb(entity: &Value) -> (Option<Wkb>, Option<Wkb>, Option<Wkb>) {
    (
        expanded::extract_location_wkb(entity),
        expanded::extract_observation_space_wkb(entity),
        expanded::extract_operation_space_wkb(entity),
    )
}
