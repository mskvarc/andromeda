//! Reading a [`TemporalHistoryRow`] back out of the durable intent it travelled
//! in.
//!
//! The row is written with its structured value and its metadata in the JSON a
//! history row stores them as, so an intent and a stored row share one form.
//! That JSON says what it is only together with the row's attribute family and
//! name, which serde does not hand a field's decoder; the row is therefore read
//! into this wire form first and the two stored documents decoded once the
//! whole row is known. They are held as JSON literals rather than as raw text
//! because the row travels inside an internally tagged intent entry, which
//! serde buffers before it dispatches on the tag, and a raw-text holder cannot
//! be read back out of that buffer; a literal is transcribed out of the buffer
//! into text and decoded by the stored-JSON codec, never walked.

use crate::temporal::{
    TemporalHistoryRow,
    TemporalValue,
    geometry_observation::GeometryWire,
    observation_metadata::ObservationMetadata,
    structured_value::StructuredValue,
    structured_value_codec::StoredValueError,
};
use andromeda_json_literal::JsonLiteral;
use andromeda_ngsi_ld_entity::{document::instance_document::InstanceDocument, refusal::EntityRefusal};
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
use thiserror::Error;

/// A durable history row whose stored documents do not read as its family's.
#[derive(Debug, Error)]
pub enum StoredObservationError {
    /// The structured value is not one of the row's family.
    #[error("the structured value of '{attribute}' is unreadable: {source}")]
    Structured {
        /// The attribute the row records.
        attribute: AttributeName,
        /// Why the value is unreadable.
        #[source]
        source: StoredValueError,
    },
    /// The members of a geometry are not the stored members of one.
    #[error("the geometry members of '{attribute}' are unreadable: {source}")]
    GeometryMembers {
        /// The attribute the row records.
        attribute: AttributeName,
        /// Why the members are unreadable.
        #[source]
        source: EntityRefusal,
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
    /// A `GeoJSON` geometry, its members still raw.
    Geometry(GeometryWire),
    /// The stored JSON of a structured value.
    Object(JsonLiteral),
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
    metadata: Option<JsonLiteral>,
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
            ValueWire::Geometry(geometry) => {
                TemporalValue::Geometry(
                    geometry
                        .into_geometry(&attribute_name)
                        .map_err(|source| StoredObservationError::GeometryMembers {
                            attribute: attribute_name.clone(), // clone: the error owns the attribute it names
                            source,
                        })?,
                )
            }
            ValueWire::Object(stored) => TemporalValue::Structured(
                StructuredValue::from_stored(attribute_type, &attribute_name, &stored.to_string()).map_err(|source| StoredObservationError::Structured {
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
            Some(stored) => metadata_of(attribute_type, &attribute_name, &stored.to_string()).map_err(|source| StoredObservationError::Metadata {
                attribute: attribute_name.clone(), // clone: the error owns the attribute it names
                source,
            })?,
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

/// Read the metadata the row of `attribute`, recorded in `family`, carries as
/// the expanded members of an instance, `None` when they hold no metadata
/// member.
///
/// Which members are companions of the value and which are sub-attributes is
/// the family's to say (`objectType` belongs to the two Relationship families
/// alone), so the members are read as those of an instance declaring the
/// family.
fn metadata_of(family: AttributeType, attribute: &AttributeName, json: &str) -> Result<Option<ObservationMetadata>, EntityRefusal> {
    let members = json.trim_start().strip_prefix('{').unwrap_or(json);
    let separator = if members.trim_start().starts_with('}') { "" } else { "," };
    let family_iri = family.iri_str();
    let mut instance = String::with_capacity(json.len() + family_iri.len() + 16);
    instance.push_str("{\"@type\":[\"");
    instance.push_str(family_iri);
    instance.push_str("\"]");
    instance.push_str(separator);
    instance.push_str(members);
    ObservationMetadata::admit(InstanceDocument::from_slice(attribute, instance.as_bytes())?.into_parts())
}
