//! Reading a [`TemporalHistoryRow`] back out of the durable intent it travelled
//! in.
//!
//! The row is written with its structured value and its metadata in the JSON a
//! history row stores them as, so an intent and a stored row share one form.
//! That JSON says what it is only together with the row's attribute family and
//! name, which serde does not hand a field's decoder; the row is therefore read
//! into this wire form first and the two stored documents decoded once the
//! whole row is known. They are buffered as generic JSON rather than as raw
//! text because the row travels inside an internally tagged intent entry, which
//! serde buffers before it dispatches on the tag, and a raw-text holder cannot
//! be read back out of that buffer; the buffered JSON is written straight back
//! to text and decoded by the stored-JSON codec, never walked.

use crate::temporal::{
    TemporalHistoryRow,
    TemporalValue,
    observation_metadata::ObservationMetadata,
    structured_value::StructuredValue,
    structured_value_codec::StoredValueError,
};
use andromeda_ngsi_ld_entity::refusal::EntityRefusal;
use andromeda_ngsi_ld_geometry::geometry::Geometry;
use andromeda_ngsi_ld_lexicon::{
    attribute_name::AttributeName,
    attribute_type::AttributeType,
    identifier::Identifier,
    instance_identity::InstanceIdentity,
    scope::path::ScopePath,
    uri::Uri,
};
use andromeda_ngsi_ld_number::JsonNumber;
use chrono::{DateTime, Utc};
use serde::Deserialize;
use serde_json::Value;
use thiserror::Error;

/// A durable history row whose stored documents do not read as its family's.
#[derive(Debug, Error)]
pub enum StoredObservationError {
    /// The structured value is not one of the row's family.
    #[error("the structured value of '{attribute}' is unreadable: {source}")]
    Value {
        /// The attribute the row records.
        attribute: AttributeName,
        /// Why the value is unreadable.
        #[source]
        source: StoredValueError,
    },
    /// The metadata is not the expanded members of an instance.
    #[error("the metadata of '{attribute}' is unreadable: {source}")]
    Metadata {
        /// The attribute the row records.
        attribute: AttributeName,
        /// Why the metadata is unreadable.
        #[source]
        source: EntityRefusal,
    },
}

/// The value of a durable row, its structured value still raw.
#[derive(Deserialize)]
#[serde(rename_all = "kebab-case")]
enum ValueWire {
    /// An exact numeric attribute value.
    Number(JsonNumber),
    /// A string attribute value.
    String(String),
    /// A boolean attribute value.
    Boolean(bool),
    /// A `GeoJSON` geometry.
    Geometry(Geometry),
    /// The stored JSON of a structured value.
    Object(Value),
    /// A datetime attribute value.
    DateTime(DateTime<Utc>),
    /// A URI attribute value.
    Uri(Uri),
    /// The Scopes of one reified scope instance.
    Scopes(Vec<ScopePath>),
    /// A deletion marker.
    Tombstone {
        /// The instant the instance was deleted at.
        deleted_at: DateTime<Utc>,
    },
}

/// A durable history row, its stored documents still raw.
#[derive(Deserialize)]
pub(crate) struct HistoryRowWire {
    /// Expanded attribute IRI.
    attribute_name: AttributeName,
    /// NGSI-LD attribute family.
    attribute_type: AttributeType,
    /// Multi-attribute discriminator.
    dataset_id: Option<Identifier>,
    /// The observation value.
    value: ValueWire,
    /// Client-declared observation instant.
    observed_at: Option<DateTime<Utc>>,
    /// The broker's write clock.
    recorded_at: DateTime<Utc>,
    /// Creation instant of the instance.
    created_at: Option<DateTime<Utc>>,
    /// Last modification instant of the instance.
    modified_at: Option<DateTime<Utc>>,
    /// Stable instance identity.
    instance: InstanceIdentity,
    /// The stored JSON of the metadata.
    metadata: Option<Value>,
    /// Partition total, when requested.
    partition_total: Option<i64>,
}

impl TryFrom<HistoryRowWire> for TemporalHistoryRow {
    type Error = StoredObservationError;

    fn try_from(wire: HistoryRowWire) -> Result<TemporalHistoryRow, StoredObservationError> {
        let HistoryRowWire {
            attribute_name,
            attribute_type,
            dataset_id,
            value,
            observed_at,
            recorded_at,
            created_at,
            modified_at,
            instance,
            metadata,
            partition_total,
        } = wire;
        let value = match value {
            ValueWire::Number(number) => TemporalValue::Number(number),
            ValueWire::String(text) => TemporalValue::String(text),
            ValueWire::Boolean(flag) => TemporalValue::Boolean(flag),
            ValueWire::Geometry(geometry) => TemporalValue::Geometry(geometry),
            ValueWire::Object(stored) => TemporalValue::Structured(
                StructuredValue::from_stored(attribute_type, &attribute_name, &stored.to_string()).map_err(|source| StoredObservationError::Value {
                    attribute: attribute_name.clone(), // clone: the error owns the attribute it names
                    source,
                })?,
            ),
            ValueWire::DateTime(instant) => TemporalValue::DateTime(instant),
            ValueWire::Uri(uri) => TemporalValue::Uri(uri),
            ValueWire::Scopes(paths) => TemporalValue::Scopes(paths),
            ValueWire::Tombstone { deleted_at } => TemporalValue::Tombstone { deleted_at },
        };
        let metadata = match metadata {
            Some(stored) => {
                ObservationMetadata::from_stored(attribute_type, &attribute_name, &stored.to_string()).map_err(|source| StoredObservationError::Metadata {
                    attribute: attribute_name.clone(), // clone: the error owns the attribute it names
                    source,
                })?
            }
            None => None,
        };
        Ok(TemporalHistoryRow {
            attribute_name,
            attribute_type,
            dataset_id,
            value,
            observed_at,
            recorded_at,
            created_at,
            modified_at,
            instance,
            metadata,
            partition_total,
        })
    }
}
