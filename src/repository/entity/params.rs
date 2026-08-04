//! Repository input parameter types for entity persistence.

use crate::{
    core::error::AndromedaError,
    domain::geometry::Geometry,
    repository::geometry::{Ewkb, encode_optional},
};
use chrono::{DateTime, Utc};
use serde_json::Value;

/// Pre-extracted columns for an entity `save` request. The service layer
/// performs all `Value` inspection and hands typed columns to the repository.
#[derive(Debug)]
pub struct EntitySaveParams<'a> {
    /// Entity identifier (URN).
    pub id: &'a str,
    /// Full expanded JSON-LD payload (persisted as JSONB).
    pub payload: &'a Value,
    /// Extracted type IRIs.
    pub types: Vec<String>,
    /// Extracted scope paths in ltree dot notation.
    pub scope_paths: Vec<String>,
    /// Hoisted `location` GeoProperty geometry, if present.
    pub location: Option<Geometry>,
    /// Hoisted `observationSpace` GeoProperty geometry, if present.
    pub observation_space: Option<Geometry>,
    /// Hoisted `operationSpace` GeoProperty geometry, if present.
    pub operation_space: Option<Geometry>,
    /// Extracted `expiresAt` timestamp, if present.
    pub expires_at: Option<DateTime<Utc>>,
}

/// Pre-extracted columns for an entity `create` request.
#[derive(Debug)]
pub struct EntityCreateParams<'a> {
    /// Entity identifier (URN).
    pub id: &'a str,
    /// Type IRI list (borrowed from the caller).
    pub types: &'a [&'a str],
    /// Scope paths in ltree dot notation (pre-converted).
    pub scope_paths: Vec<String>,
    /// Full expanded JSON-LD payload.
    pub payload: &'a Value,
    /// Hoisted `location` geometry.
    pub location: Option<Geometry>,
    /// Hoisted `observationSpace` geometry.
    pub observation_space: Option<Geometry>,
    /// Hoisted `operationSpace` geometry.
    pub operation_space: Option<Geometry>,
    /// Explicit expiry, if any.
    pub expires_at: Option<DateTime<Utc>>,
}

/// Row data prepared for bulk entity SQL operations.
#[derive(Debug)]
pub(crate) struct PreparedEntityRow {
    /// Entity identifier (URN).
    pub id: String,
    /// JSON array of type IRIs.
    pub types_json: Value,
    /// JSON array of ltree-formatted scope paths.
    pub scope_paths_json: Value,
    /// Full expanded JSON-LD payload.
    pub payload: Value,
    /// The `location` GeoProperty geometry, if present.
    pub location: Option<Geometry>,
    /// The `observationSpace` GeoProperty geometry, if present.
    pub observation_space: Option<Geometry>,
    /// The `operationSpace` GeoProperty geometry, if present.
    pub operation_space: Option<Geometry>,
    /// Entity expiration timestamp.
    pub expires_at: Option<DateTime<Utc>>,
}

/// Parallel column vectors extracted from [`PreparedEntityRow`] slices for
/// sqlx `UNNEST` binding.
#[derive(Debug)]
pub struct PreparedEntityColumns {
    /// Entity identifiers.
    pub ids: Vec<String>,
    /// Type arrays as JSON.
    pub types_json: Vec<Value>,
    /// Scope arrays as JSON.
    pub scope_paths_json: Vec<Value>,
    /// Expanded payloads retained for in-process side-effect consumers
    /// (temporal recorder, change events) that still require a DOM view.
    pub payloads: Vec<Value>,
    /// Pre-serialised JSON text for each payload, produced once via
    /// `sonic_rs::to_string` so that the SQL bind path can pass the wire
    /// bytes directly to Postgres (bound as `text[]` and cast to `jsonb`)
    /// instead of walking the `serde_json::Value` DOM per row inside sqlx.
    pub payload_json: Vec<String>,
    /// Location EWKB buffers.
    pub(crate) locations: Vec<Option<Ewkb>>,
    /// Observation-space EWKB buffers.
    pub(crate) obs_spaces: Vec<Option<Ewkb>>,
    /// Operation-space EWKB buffers.
    pub(crate) op_spaces: Vec<Option<Ewkb>>,
    /// Expiration timestamps.
    pub expires_ats: Vec<Option<DateTime<Utc>>>,
}

impl PreparedEntityColumns {
    /// Create an empty set of parallel SQL bind vectors sized for `len` rows.
    pub fn with_capacity(len: usize) -> PreparedEntityColumns {
        PreparedEntityColumns {
            ids: Vec::with_capacity(len),
            types_json: Vec::with_capacity(len),
            scope_paths_json: Vec::with_capacity(len),
            payloads: Vec::with_capacity(len),
            payload_json: Vec::with_capacity(len),
            locations: Vec::with_capacity(len),
            obs_spaces: Vec::with_capacity(len),
            op_spaces: Vec::with_capacity(len),
            expires_ats: Vec::with_capacity(len),
        }
    }

    /// Pre-serialise an expanded payload with `sonic_rs` and append it to both
    /// `payloads` (DOM view) and `payload_json` (wire bytes). Always use this
    /// rather than pushing `payloads` directly so the two parallel vectors
    /// stay aligned.
    ///
    /// # Errors
    ///
    /// Returns [`AndromedaError::internal`] if the payload cannot be serialised.
    pub fn push_payload(&mut self, payload: Value) -> Result<(), AndromedaError> {
        let json = sonic_rs::to_string(&payload).map_err(|err| AndromedaError::internal(format!("serialise entity payload: {err}")))?;
        self.payload_json.push(json);
        self.payloads.push(payload);
        Ok(())
    }

    /// Encode one row's three hoisted geometries into the parallel EWKB bind
    /// vectors. This is the only way to append them, so the vectors cannot
    /// fall out of step with each other and EWKB stays inside the store.
    ///
    /// # Errors
    ///
    /// Returns [`AndromedaError::internal`] if any geometry cannot be encoded.
    pub fn push_geometries(
        &mut self,
        location: Option<&Geometry>,
        observation_space: Option<&Geometry>,
        operation_space: Option<&Geometry>,
    ) -> Result<(), AndromedaError> {
        self.locations.push(encode_optional(location)?);
        self.obs_spaces.push(encode_optional(observation_space)?);
        self.op_spaces.push(encode_optional(operation_space)?);
        Ok(())
    }
}

pub(crate) fn decompose_rows(entities: Vec<PreparedEntityRow>) -> Result<PreparedEntityColumns, AndromedaError> {
    let len = entities.len();
    let mut columns = PreparedEntityColumns::with_capacity(len);

    for e in entities {
        columns.ids.push(e.id);
        columns.types_json.push(e.types_json);
        columns.scope_paths_json.push(e.scope_paths_json);
        columns.push_payload(e.payload)?;
        columns.push_geometries(e.location.as_ref(), e.observation_space.as_ref(), e.operation_space.as_ref())?;
        columns.expires_ats.push(e.expires_at);
    }

    Ok(columns)
}
