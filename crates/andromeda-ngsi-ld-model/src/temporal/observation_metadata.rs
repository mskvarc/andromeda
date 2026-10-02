//! The members an observed attribute instance carries beside its value.
//!
//! A history row keeps an instance's value, `datasetId`, `instanceId`,
//! `observedAt` and its clause 4.8 times in columns of their own. What remains
//! is the instance's metadata: its sub-attributes (clause 4.5.2) and the
//! companion members of its family — `unitCode`, `valueType`, `objectType` —
//! together with `expiresAt` (clause 4.22) and, on an instance recording a
//! deletion, `deletedAt` (clause 4.5.7). The row stores them as the JSON object
//! of those members in their expanded form, which the temporal `q` merges into
//! the expanded instance it rebuilds around the stored value.

use andromeda_ngsi_ld_entity::{
    date_time_text::DateTimeText,
    document::{
        instance_document::{InstanceDocument, InstanceDocumentParts},
        slot::Slot,
    },
    refusal::EntityRefusal,
    unit_code::UnitCode,
};
use andromeda_ngsi_ld_lexicon::{attribute_name::AttributeName, attribute_type::AttributeType};
use chrono::{DateTime, Utc};
use serde::Serialize;
use serde_json::value::{RawValue, to_raw_value};

/// The metadata of one observed instance: never empty, and never holding the
/// value, the identity or the time axes the row keeps in columns.
///
/// Boxed because most observations carry none, and the row every observation
/// is held in should not pay for the members of those that do.
#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(transparent)]
pub struct ObservationMetadata(Box<InstanceDocument>);

impl ObservationMetadata {
    /// The metadata among the members of one instance, `None` when the
    /// instance carries none.
    ///
    /// The value, the family, `datasetId`, `instanceId`, `observedAt`,
    /// `createdAt` and `modifiedAt` are not metadata: each has a column of its
    /// own, and keeping `createdAt` or `modifiedAt` here would echo a
    /// current-state system time back as a sub-Property of a past observation.
    #[must_use]
    pub fn from_parts(parts: InstanceDocumentParts) -> Option<ObservationMetadata> {
        let carries_nothing = parts.expires_at.is_none()
            && parts.unit_code.is_none()
            && parts.value_type.is_none()
            && parts.object_types.is_none()
            && parts.deleted_at.is_none()
            && parts.proof_seals.is_empty()
            && parts.sub_attributes.is_empty();
        (!carries_nothing).then(|| ObservationMetadata(Box::new(metadata_document(parts))))
    }

    /// Read the metadata a row of `attribute`, recorded in `family`, stores as
    /// `json`, `None` when the stored object carries no metadata member.
    ///
    /// Which members are companions of the value and which are sub-attributes
    /// is the family's to say (`objectType` belongs to the two Relationship
    /// families alone), so the stored members are read as those of an instance
    /// declaring the family.
    ///
    /// # Errors
    ///
    /// Returns the [`EntityRefusal`] of a text that is not the expanded members
    /// of an instance of `family`.
    pub fn from_stored(family: AttributeType, attribute: &AttributeName, json: &str) -> Result<Option<ObservationMetadata>, EntityRefusal> {
        let members = json.trim_start().strip_prefix('{').unwrap_or(json);
        let separator = if members.trim_start().starts_with('}') { "" } else { "," };
        let family_iri = family.iri_str();
        let mut instance = String::with_capacity(json.len() + family_iri.len() + 16);
        instance.push_str("{\"@type\":[\"");
        instance.push_str(family_iri);
        instance.push_str("\"]");
        instance.push_str(separator);
        instance.push_str(members);
        Ok(ObservationMetadata::from_parts(
            InstanceDocument::from_slice(attribute, instance.as_bytes())?.into_parts(),
        ))
    }

    /// The JSON object a history row stores the metadata as.
    ///
    /// # Errors
    ///
    /// Returns the serializer's failure, which metadata of the model never
    /// produces.
    pub fn to_stored_json(&self) -> Result<Box<RawValue>, serde_json::Error> {
        to_raw_value(self)
    }

    /// The `unitCode` member, or its deletion.
    #[must_use]
    pub const fn unit_code(&self) -> Option<&Slot<UnitCode>> {
        self.0.unit_code()
    }

    /// The instant the instance expires at (clause 4.22).
    #[must_use]
    pub fn expires_at(&self) -> Option<DateTime<Utc>> {
        self.0.expires_at().and_then(Slot::as_set).map(DateTimeText::instant)
    }

    /// Give the `unitCode` member up, with what metadata remains without it.
    ///
    /// A history row keeps the unit in a catalog-identity column of its own
    /// rather than repeating the expanded member in every metadata document.
    #[must_use]
    pub fn take_unit_code(self) -> (Option<ObservationMetadata>, Option<Slot<UnitCode>>) {
        let mut parts = self.0.into_parts();
        let unit_code = parts.unit_code.take();
        (ObservationMetadata::from_parts(parts), unit_code)
    }

    /// Put `unit_code` back into `metadata`, as the instance carried it.
    #[must_use]
    pub fn with_unit_code(metadata: Option<ObservationMetadata>, unit_code: UnitCode) -> ObservationMetadata {
        let unit_code = Some(Slot::Set(unit_code));
        match metadata {
            Some(ObservationMetadata(document)) => {
                let mut parts = document.into_parts();
                parts.unit_code = unit_code;
                ObservationMetadata(Box::new(metadata_document(parts)))
            }
            None => ObservationMetadata(Box::new(InstanceDocument::builder().unit_code(unit_code).build())),
        }
    }

    /// Give the metadata up as the members of an instance.
    #[must_use]
    pub fn into_parts(self) -> InstanceDocumentParts {
        self.0.into_parts()
    }
}

/// The instance holding the metadata members of `parts` and nothing else.
fn metadata_document(parts: InstanceDocumentParts) -> InstanceDocument {
    InstanceDocument::builder()
        .expires_at(parts.expires_at)
        .unit_code(parts.unit_code)
        .value_type(parts.value_type)
        .object_types(parts.object_types)
        .deleted_at(parts.deleted_at)
        .proof_seals(parts.proof_seals)
        .sub_attributes(parts.sub_attributes)
        .build()
}
