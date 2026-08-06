//! What one atomic write does to the entity row it names.
//!
//! There is no upsert variant. Both clause 5.6.8 upsert paths read the batch
//! before writing it — one to split the chunk into creations and updates, the
//! other to compute the removal diff a replacement implies — so "insert or
//! update, whichever applies" is a question the caller has already answered by
//! the time it builds the write. Modelling it as the answer rather than the
//! question also removes the `xmax = 0` system column the upsert statement used
//! to report that answer back.

use andromeda_ngsi_ld_geometry::geometry::Geometry;
use andromeda_ngsi_ld_lexicon::{entity_type_name::EntityTypeName, scope::grammar::ScopeBuf};
use andromeda_ngsi_ld_model::expanded::{
    entity_member::{extract_expires_at, extract_scopes, extract_types},
    geo_property::{extract_location_geometry, extract_observation_space_geometry, extract_operation_space_geometry},
};
use chrono::{DateTime, Utc};
use derive_more::IsVariant;
use serde_json::Value;
use std::sync::Arc;

/// The entity row an atomic write puts in the store, in NGSI-LD terms.
///
/// Scopes are clause 4.18 scope values, not the label-tree paths a particular
/// store indexes them as; rendering them is the adapter's business.
#[derive(Debug)]
pub struct EntityRecord {
    /// The expanded JSON-LD payload, shared with the change-event path so it is
    /// serialised once and never deep-copied.
    pub payload: Arc<Value>,
    /// The entity's types (clause 4.5.1).
    pub types: Vec<EntityTypeName>,
    /// The entity's scopes (clause 4.18).
    pub scopes: Vec<ScopeBuf>,
    /// The hoisted `location` GeoProperty geometry, if the payload carries one.
    pub location: Option<Geometry>,
    /// The hoisted `observationSpace` GeoProperty geometry, if any.
    pub observation_space: Option<Geometry>,
    /// The hoisted `operationSpace` GeoProperty geometry, if any.
    pub operation_space: Option<Geometry>,
    /// The entity's `expiresAt` (clause 4.22), if any.
    pub expires_at: Option<DateTime<Utc>>,
}

impl EntityRecord {
    /// Read every column out of the expanded payload that is about to be
    /// stored.
    ///
    /// Used by the single-entity paths, which hold one payload and nothing
    /// else. The batch paths extract the same members during preparation,
    /// possibly on a blocking worker, and build the record from what they
    /// already have.
    pub fn from_expanded(payload: Arc<Value>) -> EntityRecord {
        EntityRecord {
            types: extract_types(&payload),
            scopes: extract_scopes(&payload),
            location: extract_location_geometry(&payload),
            observation_space: extract_observation_space_geometry(&payload),
            operation_space: extract_operation_space_geometry(&payload),
            expires_at: extract_expires_at(&payload),
            payload,
        }
    }
}

/// The change one atomic write makes to the `entities` row.
#[derive(Debug, IsVariant)]
pub enum EntityMutation {
    /// Add a row that is not there.
    Insert(EntityRecord),
    /// Overwrite a row wholesale.
    Update(EntityRecord),
    /// Remove the row, reporting what it held.
    Delete,
    /// Leave the row alone. The write carries only history, outbox entries or
    /// side effects — a temporal append against an entity the Entity API
    /// already knows, or a replayed side-effect job.
    None,
}

impl EntityMutation {
    /// The row this mutation writes, if it writes one.
    pub const fn record(&self) -> Option<&EntityRecord> {
        match self {
            EntityMutation::Insert(record) | EntityMutation::Update(record) => Some(record),
            EntityMutation::Delete | EntityMutation::None => None,
        }
    }
}
