//! The parallel bind vectors every entity statement is driven from.
//!
//! One entity or a thousand, the statements bind the same nine arrays, so this
//! is the only shape the write path builds. Everything that turns a
//! specification value into a column — the `ltree` rendering of a scope, the
//! `sonic_rs` pre-serialisation of a payload, the EWKB encoding of a geometry —
//! happens on the way in here.

use crate::error::ContextStoreError;
use andromeda_ngsi_ld_geometry::geometry::Geometry;
use andromeda_ngsi_ld_lexicon::uri::Uri;
use andromeda_postgresql_encoding::ewkb_binding::geometry_bytes;
use chrono::{DateTime, Utc};
use serde_json::Value;

/// Parallel column vectors for one fused entity statement.
#[derive(Debug)]
pub struct PreparedEntityColumns {
    /// Entity identifiers.
    pub ids: Vec<Uri>,
    /// Type arrays as JSON.
    pub types_json: Vec<Value>,
    /// Scope arrays as JSON, already in `ltree` dot notation.
    pub scope_paths_json: Vec<Value>,
    /// Expanded payloads retained for the in-process side-effect consumers
    /// that still require a DOM view.
    pub payloads: Vec<Value>,
    /// Pre-serialised JSON text for each payload, produced once via
    /// `sonic_rs::to_string` so that the SQL bind path can pass the wire
    /// bytes directly to Postgres (bound as `text[]` and cast to `jsonb`)
    /// instead of walking the `serde_json::Value` DOM per row inside sqlx.
    pub payload_json: Vec<String>,
    /// Location EWKB buffers.
    pub locations: Vec<Option<Vec<u8>>>,
    /// Observation-space EWKB buffers.
    pub obs_spaces: Vec<Option<Vec<u8>>>,
    /// Operation-space EWKB buffers.
    pub op_spaces: Vec<Option<Vec<u8>>>,
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
    pub fn push_payload(&mut self, payload: Value) -> Result<(), ContextStoreError> {
        let json = sonic_rs::to_string(&payload).map_err(|err| ContextStoreError::Internal(format!("serialise entity payload: {err}")))?;
        self.payload_json.push(json);
        self.payloads.push(payload);
        Ok(())
    }

    /// Encode one row's three hoisted geometries into the parallel EWKB bind
    /// vectors. This is the only way to append them, so the vectors cannot
    /// fall out of step with each other and EWKB stays inside the store.
    pub fn push_geometries(
        &mut self,
        location: Option<&Geometry>,
        observation_space: Option<&Geometry>,
        operation_space: Option<&Geometry>,
    ) -> Result<(), ContextStoreError> {
        self.locations.push(geometry_bytes(location)?);
        self.obs_spaces.push(geometry_bytes(observation_space)?);
        self.op_spaces.push(geometry_bytes(operation_space)?);
        Ok(())
    }
}
