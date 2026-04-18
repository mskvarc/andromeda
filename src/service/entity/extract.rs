//! Service-side extraction of persistence-shaped columns from expanded
//! JSON-LD entity payloads.
//!
//! The repository contract forbids `Value` inspection; the service layer
//! materialises every typed column (types, scopes, WKB geometry, expiry)
//! before a persistence call is made.

use crate::{
    core::error::AndromedaError,
    domain::{expanded, geometry::Wkb, outbox::NewOutboxEntry},
    query::sql::scope_to_ltree,
    repository::entity::{
        params::{EntityCreateParams, EntitySaveParams},
        repo::EntityRepository,
    },
};
use chrono::{DateTime, Utc};
use serde_json::Value;
use urn::Urn;

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

/// Extract create-time columns shared by `save()` but with a caller-supplied
/// identifier, type list, scope list, and expiry.
///
/// Unlike `build_save_params`, the caller already owns the id/types/scopes
/// (they come from URL parameters or earlier JSON-LD processing); only the
/// hoisted geometry columns are derived from the payload.
pub fn extract_geo_wkb(entity: &Value) -> (Option<Wkb>, Option<Wkb>, Option<Wkb>) {
    (
        expanded::extract_location_wkb(entity),
        expanded::extract_observation_space_wkb(entity),
        expanded::extract_operation_space_wkb(entity),
    )
}

/// Save a prepared payload via the repository. Returns rows affected.
pub async fn save_entity(repo: &EntityRepository, id: &str, entity: &Value) -> Result<u64, AndromedaError> {
    repo.save(build_save_params(id, entity)).await
}

/// Save with outbox dual-write. Returns rows affected.
pub async fn save_entity_with_outbox(repo: &EntityRepository, id: &str, entity: &Value, outbox_entries: &[NewOutboxEntry]) -> Result<u64, AndromedaError> {
    repo.save_with_outbox(build_save_params(id, entity), outbox_entries).await
}

/// Create-path wrapper used when the types/scopes are already known.
pub async fn create_entity(
    repo: &EntityRepository,
    id: &str,
    type_: &[&str],
    scopes: &[String],
    entity: &Value,
    expires_at: Option<DateTime<Utc>>,
) -> Result<Urn, AndromedaError> {
    let (location_wkb, observation_space_wkb, operation_space_wkb) = extract_geo_wkb(entity);
    let scope_paths: Vec<String> = scopes.iter().map(|s| scope_to_ltree(s)).collect();
    repo.create(EntityCreateParams {
        id,
        types: type_,
        scope_paths,
        payload: entity,
        location_wkb,
        observation_space_wkb,
        operation_space_wkb,
        expires_at,
    })
    .await
}

/// Create with outbox dual-write.
pub async fn create_entity_with_outbox(
    repo: &EntityRepository,
    id: &str,
    type_: &[&str],
    scopes: &[String],
    entity: &Value,
    expires_at: Option<DateTime<Utc>>,
    outbox_entries: &[NewOutboxEntry],
) -> Result<Urn, AndromedaError> {
    let (location_wkb, observation_space_wkb, operation_space_wkb) = extract_geo_wkb(entity);
    let scope_paths: Vec<String> = scopes.iter().map(|s| scope_to_ltree(s)).collect();
    repo.create_with_outbox(
        EntityCreateParams {
            id,
            types: type_,
            scope_paths,
            payload: entity,
            location_wkb,
            observation_space_wkb,
            operation_space_wkb,
            expires_at,
        },
        outbox_entries,
    )
    .await
}
