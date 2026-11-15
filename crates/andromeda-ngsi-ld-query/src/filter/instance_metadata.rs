//! The members of an attribute instance a `q` path names after `.` without
//! naming a sub-attribute.

use andromeda_ngsi_ld_lexicon::attribute_name::AttributeName;
use andromeda_ngsi_ld_vocabulary::{
    compact::properties as compact_properties,
    expanded::properties::{CREATED_AT, DATASET_ID, EXPIRES_AT, INSTANCE_ID, MODIFIED_AT, OBJECT_TYPE, OBSERVED_AT, UNIT_CODE, VALUE_TYPE},
};
use iri_rs::Iri;
use strum::{EnumIter, IntoEnumIterator};

/// A member an attribute instance carries as data of its own rather than as a
/// sub-attribute, which clause 4.9 nevertheless lets a path address with `.`
/// exactly as it addresses a sub-attribute (`temperature.observedAt`).
///
/// Its target value is what the instance holds there: the four instants are
/// `DateTime` values (clauses 4.8 and 4.22), every other member a String, and
/// an `objectType` naming several types the array of them. A path ends at such
/// a member; nothing is addressable beneath it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, EnumIter, serde::Serialize)]
#[serde(rename_all = "kebab-case")]
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
    /// The types a Relationship declares its object to have, `objectType`.
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

    /// The core `@context` term for the member, the name clauses 4.5.2.2 and
    /// 4.5.3.2 write it under in a compacted instance.
    #[must_use]
    pub const fn term(self) -> &'static str {
        match self {
            InstanceMetadata::ObservedAt => compact_properties::OBSERVED_AT,
            InstanceMetadata::CreatedAt => compact_properties::CREATED_AT,
            InstanceMetadata::ModifiedAt => compact_properties::MODIFIED_AT,
            InstanceMetadata::ExpiresAt => compact_properties::EXPIRES_AT,
            InstanceMetadata::DatasetId => compact_properties::DATASET_ID,
            InstanceMetadata::InstanceId => compact_properties::INSTANCE_ID,
            InstanceMetadata::UnitCode => compact_properties::UNIT_CODE,
            InstanceMetadata::ValueType => compact_properties::VALUE_TYPE,
            InstanceMetadata::ObjectType => compact_properties::OBJECT_TYPE,
        }
    }

    /// Whether the member holds a vocabulary term, which the core `@context`
    /// coerces with `@vocab`: `valueType` (clause 4.5.2) and `objectType`
    /// (clause 4.5.3). The instance holds such a member expanded, so a Query
    /// Term operand compared with it is a term to expand under the same
    /// `@context`.
    #[must_use]
    pub const fn holds_vocabulary_term(self) -> bool {
        match self {
            InstanceMetadata::ValueType | InstanceMetadata::ObjectType => true,
            InstanceMetadata::ObservedAt
            | InstanceMetadata::CreatedAt
            | InstanceMetadata::ModifiedAt
            | InstanceMetadata::ExpiresAt
            | InstanceMetadata::DatasetId
            | InstanceMetadata::InstanceId
            | InstanceMetadata::UnitCode => false,
        }
    }

    /// Whether the member holds an instant: `observedAt` and the clause 4.8
    /// timestamps are `DateTime` strings, as `expiresAt` is (clause 4.22), so
    /// a time operand compares with the instant their text spells.
    #[must_use]
    pub const fn holds_instant(self) -> bool {
        match self {
            InstanceMetadata::ObservedAt | InstanceMetadata::CreatedAt | InstanceMetadata::ModifiedAt | InstanceMetadata::ExpiresAt => true,
            InstanceMetadata::DatasetId
            | InstanceMetadata::InstanceId
            | InstanceMetadata::UnitCode
            | InstanceMetadata::ValueType
            | InstanceMetadata::ObjectType => false,
        }
    }

    /// The member an expanded path segment names, or `None` when it names a
    /// sub-attribute or a family's value member instead.
    #[must_use]
    pub fn named(name: &AttributeName) -> Option<InstanceMetadata> {
        InstanceMetadata::iter().find(|member| member.iri().as_str() == name.as_str())
    }

    /// The member a compacted name spells by its core `@context` term, or
    /// `None` for any other name.
    #[must_use]
    pub fn termed(name: &AttributeName) -> Option<InstanceMetadata> {
        InstanceMetadata::iter().find(|member| member.term() == name.as_str())
    }
}

#[cfg(test)]
mod tests {
    use crate::filter::instance_metadata::InstanceMetadata;
    use andromeda_ngsi_ld_vocabulary::core_context::NGSI_LD_CORE_CONTEXT_V19;
    use serde_json::Value;
    use strum::IntoEnumIterator;

    #[test]
    fn every_member_is_named_by_its_own_expanded_iri() {
        for member in InstanceMetadata::iter() {
            let name = member.iri().as_str().parse().expect("an expanded IRI is an attribute name");
            pretty_assertions::assert_eq!(InstanceMetadata::named(&name), Some(member));
        }
    }

    /// A compacted instance names each member by the term the v1.9 core
    /// `@context` expands to its IRI, and the expanded IRI is no term.
    #[test]
    fn every_member_is_spelled_by_the_term_it_expands_from() {
        let context: Value = serde_json::from_str(NGSI_LD_CORE_CONTEXT_V19).expect("the core context is JSON");
        for member in InstanceMetadata::iter() {
            let definition = context.get("@context").and_then(|terms| terms.get(member.term())).expect("a core context term");
            let compact_iri = definition
                .as_str()
                .or_else(|| definition.get("@id").and_then(Value::as_str))
                .expect("a term definition naming an IRI");
            pretty_assertions::assert_eq!(compact_iri.replacen("ngsi-ld:", "https://uri.etsi.org/ngsi-ld/", 1), member.iri().as_str());
            let term = member.term().parse().expect("a term is an attribute name");
            pretty_assertions::assert_eq!(InstanceMetadata::termed(&term), Some(member));
            let iri = member.iri().as_str().parse().expect("an expanded IRI is an attribute name");
            pretty_assertions::assert_eq!(InstanceMetadata::termed(&iri), None);
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
