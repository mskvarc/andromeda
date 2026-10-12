//! The members an observed attribute instance carries beside its value.
//!
//! A history row keeps an instance's value, `datasetId`, `instanceId`,
//! `observedAt` and its clause 4.8 times in columns of their own. What remains
//! is the instance's metadata: its sub-attributes (clause 4.5.2) and the
//! companion members of its family — `unitCode`, `valueType`, `objectType` —
//! together with `expiresAt` (clause 4.22) and the data-integrity seals. A
//! `deletedAt` is no metadata: the broker generates it (Table 5.2.5-2), and a
//! history row recording a deletion holds it in a column of its own.

use andromeda_ngsi_ld_entity::{
    date_time_text::DateTimeText,
    document::{
        attribute_member::AttributeMember,
        instance_document::{InstanceDocument, InstanceDocumentParts},
        slot::Slot,
    },
    unit_code::UnitCode,
};
use chrono::{DateTime, Utc};
use serde::Serialize;

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
    /// `createdAt`, `modifiedAt` and `deletedAt` are not metadata: each has a
    /// column of its own, and keeping `createdAt` or `modifiedAt` here would
    /// echo a current-state system time back as a sub-Property of a past
    /// observation.
    #[must_use]
    pub fn from_parts(parts: InstanceDocumentParts) -> Option<ObservationMetadata> {
        let carries_nothing = parts.expires_at.is_none()
            && parts.unit_code.is_none()
            && parts.value_type.is_none()
            && parts.object_types.is_none()
            && parts.proof_seals.is_empty()
            && parts.sub_attributes.is_empty();
        (!carries_nothing).then(|| ObservationMetadata(Box::new(metadata_document(parts))))
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
///
/// The sub-attributes keep no output-only member: history records the system
/// times of the instance alone, and a member a client wrote in their place is
/// never kept (Table 5.2.5-2).
fn metadata_document(mut parts: InstanceDocumentParts) -> InstanceDocument {
    parts
        .sub_attributes
        .values_mut()
        .for_each(AttributeMember::forget_output_only_members_throughout);
    InstanceDocument::builder()
        .expires_at(parts.expires_at)
        .unit_code(parts.unit_code)
        .value_type(parts.value_type)
        .object_types(parts.object_types)
        .proof_seals(parts.proof_seals)
        .sub_attributes(parts.sub_attributes)
        .build()
}

#[cfg(test)]
mod tests {
    use crate::temporal::observation_metadata::ObservationMetadata;
    use andromeda_ngsi_ld_entity::{
        document::{attribute_member::AttributeMember, instance_document::InstanceDocument},
        system_instant::SystemInstant,
    };
    use andromeda_ngsi_ld_lexicon::attribute_name::AttributeName;
    use indexmap::IndexMap;

    /// A client-written instant.
    fn at() -> SystemInstant {
        "1900-01-01T00:00:00Z".parse().expect("RFC 3339")
    }

    /// One sub-attribute named `name` carrying both system times and `nested`.
    fn stamped(name: &str, nested: IndexMap<AttributeName, AttributeMember>) -> IndexMap<AttributeName, AttributeMember> {
        let instance = InstanceDocument::builder()
            .created_at(Some(at()))
            .modified_at(Some(at()))
            .sub_attributes(nested)
            .build();
        IndexMap::from([(name.parse().expect("a valid name"), AttributeMember::Instances(vec![instance]))])
    }

    /// The system times of every sub-attribute instance, depth first.
    fn times(sub_attributes: &IndexMap<AttributeName, AttributeMember>) -> Vec<(Option<SystemInstant>, Option<SystemInstant>)> {
        sub_attributes
            .values()
            .flat_map(AttributeMember::instances)
            .flat_map(|instance| {
                let mut found = vec![(instance.created_at(), instance.modified_at())];
                found.extend(times(instance.sub_attributes()));
                found
            })
            .collect()
    }

    /// Clause 5.2.5: a client's `createdAt` and `modifiedAt` are ignored, so
    /// the metadata a history row stores keeps none on any sub-attribute.
    #[test]
    fn sub_attribute_system_times_are_not_kept_at_any_depth() {
        let written = InstanceDocument::builder()
            .sub_attributes(stamped("https://example.org/accuracy", stamped("https://example.org/method", IndexMap::new())))
            .build();

        let metadata = ObservationMetadata::from_parts(written.into_parts()).expect("the sub-attributes are metadata");

        pretty_assertions::assert_eq!(times(&metadata.into_parts().sub_attributes), vec![(None, None), (None, None)]);
    }
}
