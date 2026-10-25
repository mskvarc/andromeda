//! The members of an attribute instance a `q` path names after `.` without
//! naming a sub-attribute.

use crate::filter::target_value::TargetValue;
use andromeda_ngsi_ld_lexicon::{attribute_name::AttributeName, time::value::TimeValue};
use andromeda_ngsi_ld_vocabulary::expanded::properties::{
    CREATED_AT,
    DATASET_ID,
    EXPIRES_AT,
    INSTANCE_ID,
    MODIFIED_AT,
    OBJECT_TYPE,
    OBSERVED_AT,
    UNIT_CODE,
    VALUE_TYPE,
};
use iri_rs::Iri;
use std::borrow::Cow;
use strum::{EnumIter, IntoEnumIterator};

/// A member an attribute instance carries as data of its own rather than as a
/// sub-attribute, which clause 4.9 nevertheless lets a path address with `.`
/// exactly as it addresses a sub-attribute (`temperature.observedAt`).
///
/// Its target value is what the instance holds there: the four instants are
/// `DateTime` values (clauses 4.8 and 4.22), every other member a String. A
/// path ends at such a member; nothing is addressable beneath it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, EnumIter)]
pub enum InstanceMetadata {
    /// When the value was observed, `observedAt`.
    ObservedAt,
    /// When the instance was created, `createdAt` (clause 4.8).
    CreatedAt,
    /// When the instance was last modified, `modifiedAt` (clause 4.8).
    ModifiedAt,
    /// When the instance expires, `expiresAt` (clause 4.22).
    ExpiresAt,
    /// The dataset the instance belongs to, `datasetId` (clause 4.5.5).
    DatasetId,
    /// The instance identifier, `instanceId`.
    InstanceId,
    /// The UN/CEFACT unit of the value, `unitCode`.
    UnitCode,
    /// The type of the value, `valueType`.
    ValueType,
    /// The first type a Relationship declares its object to have,
    /// `objectType`.
    ObjectType,
}

impl InstanceMetadata {
    /// The expanded IRI the member is keyed by in an expanded attribute
    /// instance, and which an expanded `q` path names it by.
    #[must_use]
    pub const fn iri(self) -> Iri<&'static str> {
        match self {
            InstanceMetadata::ObservedAt => OBSERVED_AT,
            InstanceMetadata::CreatedAt => CREATED_AT,
            InstanceMetadata::ModifiedAt => MODIFIED_AT,
            InstanceMetadata::ExpiresAt => EXPIRES_AT,
            InstanceMetadata::DatasetId => DATASET_ID,
            InstanceMetadata::InstanceId => INSTANCE_ID,
            InstanceMetadata::UnitCode => UNIT_CODE,
            InstanceMetadata::ValueType => VALUE_TYPE,
            InstanceMetadata::ObjectType => OBJECT_TYPE,
        }
    }

    /// The clause 4.9 target value of the member holding `text`.
    ///
    /// An instant the instance holds was admitted as a `DateTime` or stamped by
    /// the broker, so its text always reads as one.
    #[must_use]
    pub fn target_value(self, text: Cow<'_, str>) -> TargetValue<'_> {
        match self {
            InstanceMetadata::ObservedAt | InstanceMetadata::CreatedAt | InstanceMetadata::ModifiedAt | InstanceMetadata::ExpiresAt => text
                .parse()
                .map_or(TargetValue::Text(text), |instant| TargetValue::Time(TimeValue::DateTime(instant))),
            InstanceMetadata::DatasetId
            | InstanceMetadata::InstanceId
            | InstanceMetadata::UnitCode
            | InstanceMetadata::ValueType
            | InstanceMetadata::ObjectType => TargetValue::Text(text),
        }
    }

    /// The member an expanded path segment names, or `None` when it names a
    /// sub-attribute or a family's value member instead.
    #[must_use]
    pub fn named(name: &AttributeName) -> Option<InstanceMetadata> {
        InstanceMetadata::iter().find(|member| member.iri().as_str() == name.as_str())
    }
}

#[cfg(test)]
#[expect(
    clippy::expect_used,
    reason = "test code unwraps, expects and panics on purpose; migrating it to typed assertions is pending"
)]
mod tests {
    use crate::queried_entity::instance_metadata::InstanceMetadata;
    use strum::IntoEnumIterator;

    #[test]
    fn every_member_is_named_by_its_own_expanded_iri() {
        for member in InstanceMetadata::iter() {
            let name = member.iri().as_str().parse().expect("an expanded IRI is an attribute name");
            pretty_assertions::assert_eq!(InstanceMetadata::named(&name), Some(member));
        }
    }

    #[test]
    fn an_ordinary_attribute_name_names_no_instance_member() {
        let name = "https://example.com/observedAt".parse().expect("an attribute name");
        pretty_assertions::assert_eq!(InstanceMetadata::named(&name), None);
    }

    #[test]
    fn the_value_member_of_a_family_is_not_instance_metadata() {
        let name = "https://uri.etsi.org/ngsi-ld/hasValue".parse().expect("an attribute name");
        pretty_assertions::assert_eq!(InstanceMetadata::named(&name), None);
    }
}
