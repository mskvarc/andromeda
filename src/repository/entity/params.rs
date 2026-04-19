//! Repository input parameter types for entity persistence.

use crate::domain::geometry::Wkb;
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
    /// Hoisted `location` GeoProperty WKB, if present.
    pub location_wkb: Option<Wkb>,
    /// Hoisted `observationSpace` GeoProperty WKB, if present.
    pub observation_space_wkb: Option<Wkb>,
    /// Hoisted `operationSpace` GeoProperty WKB, if present.
    pub operation_space_wkb: Option<Wkb>,
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
    /// Hoisted `location` WKB.
    pub location_wkb: Option<Wkb>,
    /// Hoisted `observationSpace` WKB.
    pub observation_space_wkb: Option<Wkb>,
    /// Hoisted `operationSpace` WKB.
    pub operation_space_wkb: Option<Wkb>,
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
    /// Canonical WKB for the `location` GeoProperty, if present.
    pub location: Option<Wkb>,
    /// Canonical WKB for the `observationSpace` GeoProperty, if present.
    pub observation_space: Option<Wkb>,
    /// Canonical WKB for the `operationSpace` GeoProperty, if present.
    pub operation_space: Option<Wkb>,
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
    /// Expanded payloads.
    pub payloads: Vec<Value>,
    /// Location WKB buffers.
    pub locations: Vec<Option<Wkb>>,
    /// Observation-space WKB buffers.
    pub obs_spaces: Vec<Option<Wkb>>,
    /// Operation-space WKB buffers.
    pub op_spaces: Vec<Option<Wkb>>,
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
            locations: Vec::with_capacity(len),
            obs_spaces: Vec::with_capacity(len),
            op_spaces: Vec::with_capacity(len),
            expires_ats: Vec::with_capacity(len),
        }
    }
}

pub(crate) fn decompose_rows(entities: Vec<PreparedEntityRow>) -> PreparedEntityColumns {
    let len = entities.len();
    let mut columns = PreparedEntityColumns::with_capacity(len);

    for e in entities {
        columns.ids.push(e.id);
        columns.types_json.push(e.types_json);
        columns.scope_paths_json.push(e.scope_paths_json);
        columns.payloads.push(e.payload);
        columns.locations.push(e.location);
        columns.obs_spaces.push(e.observation_space);
        columns.op_spaces.push(e.operation_space);
        columns.expires_ats.push(e.expires_at);
    }

    columns
}
