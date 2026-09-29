//! The parallel bind vectors every entity statement is driven from.
//!
//! One entity or a thousand, the statements bind the same arrays, so this is
//! the only shape the write path builds. Everything that turns a
//! specification value into a column — the `ltree` rendering of a scope, the
//! exact JSON pre-serialisation of a payload, the EWKB encoding of a geometry
//! — happens on the way in here.

use crate::error::ContextStoreError;
use andromeda_ngsi_ld_geometry::geometry::Geometry;
use andromeda_ngsi_ld_lexicon::identifier::Identifier;
use andromeda_postgresql_codes::attribute_kind::AttributeKind;
use andromeda_postgresql_encoding::{array_literal::IntegerArrayLiteral, ewkb_binding::geometry_bytes};
use andromeda_postgresql_query::{attribute_name_key::AttributeNameKey, entity_type_name_key::EntityTypeNameKey};
use chrono::{DateTime, Utc};
use serde_json::Value;

/// Parallel column vectors for one fused entity statement.
///
/// Every integer array is carried as the PostgreSQL array literal a statement
/// casts to its column type ([`IntegerArrayLiteral`]), one per entity.
///
/// The payload is carried once, as the JSON text the statements bind (as
/// `text[]` cast to `jsonb`, or as the bytes of a binary `COPY` field): the
/// DOM it was serialised from is not kept, because no consumer of the column
/// set reads it and one serialisation per entity is the whole cost.
#[derive(Debug)]
pub struct PreparedEntityColumns {
    /// Entity identifiers.
    pub ids: Vec<Identifier>,
    /// Type dictionary-key arrays.
    pub entity_type_name_keys: Vec<IntegerArrayLiteral>,
    /// Attribute-name dictionary-key arrays.
    pub attribute_name_keys: Vec<IntegerArrayLiteral>,
    /// The attribute name of every attribute instance, as dictionary keys
    /// aligned with `attribute_instance_kinds`.
    pub attribute_instance_name_keys: Vec<IntegerArrayLiteral>,
    /// The stored family code of every attribute instance, aligned with
    /// `attribute_instance_name_keys`.
    pub attribute_instance_kinds: Vec<IntegerArrayLiteral>,
    /// Scope arrays as JSON, already in `ltree` dot notation.
    pub scope_paths_json: Vec<Value>,
    /// Pre-serialised JSON text for each payload.
    pub payload_json: Vec<String>,
    /// Location EWKB buffers.
    pub locations: Vec<Option<Vec<u8>>>,
    /// Observation-space EWKB buffers.
    pub obs_spaces: Vec<Option<Vec<u8>>>,
    /// Operation-space EWKB buffers.
    pub op_spaces: Vec<Option<Vec<u8>>>,
    /// The instant after which every `location` instance has expired, when
    /// every instance carries one.
    pub location_expires_ats: Vec<Option<DateTime<Utc>>>,
    /// Entity expiration timestamps.
    pub expires_ats: Vec<Option<DateTime<Utc>>>,
    /// Earliest attribute-instance expiration timestamps.
    pub min_attribute_expires_ats: Vec<Option<DateTime<Utc>>>,
}

impl PreparedEntityColumns {
    /// Create an empty set of parallel SQL bind vectors sized for `len` rows.
    #[must_use]
    pub fn with_capacity(len: usize) -> PreparedEntityColumns {
        PreparedEntityColumns {
            ids: Vec::with_capacity(len),
            entity_type_name_keys: Vec::with_capacity(len),
            attribute_name_keys: Vec::with_capacity(len),
            attribute_instance_name_keys: Vec::with_capacity(len),
            attribute_instance_kinds: Vec::with_capacity(len),
            scope_paths_json: Vec::with_capacity(len),
            payload_json: Vec::with_capacity(len),
            locations: Vec::with_capacity(len),
            obs_spaces: Vec::with_capacity(len),
            op_spaces: Vec::with_capacity(len),
            location_expires_ats: Vec::with_capacity(len),
            expires_ats: Vec::with_capacity(len),
            min_attribute_expires_ats: Vec::with_capacity(len),
        }
    }

    /// Serialise an expanded payload once and append its JSON text.
    ///
    /// # Errors
    ///
    /// Returns [`ContextStoreError::Internal`] when the payload cannot be
    /// serialised to JSON text.
    pub fn push_payload(&mut self, payload: &Value) -> Result<(), ContextStoreError> {
        let json = serde_json::to_string(payload).map_err(|error| ContextStoreError::Internal(format!("serialise entity payload: {error}")))?;
        self.payload_json.push(json);
        Ok(())
    }

    /// Append one row's type keys, in declaration order.
    pub fn push_entity_type_name_keys(&mut self, keys: impl IntoIterator<Item = EntityTypeNameKey>) {
        self.entity_type_name_keys
            .push(IntegerArrayLiteral::of(keys.into_iter().map(|key| i64::from(key.get()))));
    }

    /// Append one row's attribute-name keys, in payload member order.
    pub fn push_attribute_name_keys(&mut self, keys: impl IntoIterator<Item = AttributeNameKey>) {
        self.attribute_name_keys
            .push(IntegerArrayLiteral::of(keys.into_iter().map(|key| i64::from(key.get()))));
    }

    /// Append one row's attribute instances as the two aligned arrays the
    /// search row stores them in. This is the only way to append them, so the
    /// arrays cannot fall out of step with each other.
    ///
    /// The instances are sorted by name key and family code first: the stored
    /// arrays are compared whole to decide whether a write rewrites the search
    /// row, so a payload that lists the same instances in another member order
    /// must encode to the same arrays.
    pub fn push_attribute_instances(&mut self, mut instances: Vec<(AttributeNameKey, AttributeKind)>) {
        instances.sort_unstable_by_key(|(name, kind)| (name.get(), kind.code()));
        let instances = instances;
        self.attribute_instance_name_keys
            .push(IntegerArrayLiteral::of(instances.iter().map(|(name, _)| i64::from(name.get()))));
        self.attribute_instance_kinds
            .push(IntegerArrayLiteral::of(instances.iter().map(|(_, kind)| i64::from(kind.code()))));
    }

    /// Encode one row's three hoisted geometries into the parallel EWKB bind
    /// vectors. This is the only way to append them, so the vectors cannot
    /// fall out of step with each other and EWKB stays inside the store.
    ///
    /// # Errors
    ///
    /// Returns [`ContextStoreError::Encoding`] when one of the three geometries
    /// cannot be encoded as EWKB.
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

/// The texts of one literal column, in row order, as a statement binds them
/// into a `text[]` parameter.
pub(crate) fn literal_texts(column: &[IntegerArrayLiteral]) -> Vec<&str> {
    column.iter().map(IntegerArrayLiteral::as_str).collect()
}
