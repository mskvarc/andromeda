//! What an expanded member name stands for: an attribute, or a member NGSI-LD
//! reserves for itself.
//!
//! Clause 4.5.2 makes an attribute name any IRI a client chooses, but the
//! expanded form of an entity and of an attribute instance both carry members
//! the specification defines: `scope` and `expiresAt` on the entity (clauses
//! 4.18 and 4.22), the system metadata of clause 4.8, the identity members of
//! clause 4.5.5 and 4.5.7, the value carriers of each family (clause 4.5) in
//! their singular, Temporal API plural and output-only `previous…` spellings,
//! the linked-entity members of clause 4.5.23.2, and the data-integrity seals
//! of clause 4.5.2.2. None of them is an attribute.

use crate::document::value_carrier::ValueCarrier;
use andromeda_ngsi_ld_vocabulary::{compact::properties as compact_properties, expanded::properties};
use derive_more::Display;
use iri_rs::Iri;

/// A seal member of clause 4.5.2.2, displayed as its core `@context` term.
#[derive(Clone, Copy, Debug, Display, Eq, Hash, PartialEq)]
pub enum ProofSeal {
    /// `entityIdSealed`.
    #[display("{}", compact_properties::ENTITY_ID_SEALED)]
    EntityId,
    /// `entityTypeSealed`.
    #[display("{}", compact_properties::ENTITY_TYPE_SEALED)]
    EntityType,
}

/// What one expanded member name stands for.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum MemberRole {
    /// An attribute, or a sub-attribute of an instance.
    Attribute,
    /// The entity `scope` (clause 4.18).
    Scope,
    /// `expiresAt` (clause 4.22), of the entity or of an instance.
    ExpiresAt,
    /// A family's value carrier.
    Carrier(ValueCarrier),
    /// `datasetId` (clause 4.5.5).
    DatasetId,
    /// `instanceId` (clause 4.5.7).
    InstanceId,
    /// `observedAt`.
    ObservedAt,
    /// `createdAt` (clause 4.8).
    CreatedAt,
    /// `modifiedAt` (clause 4.8).
    ModifiedAt,
    /// `deletedAt` (clause 4.8).
    DeletedAt,
    /// `unitCode`.
    UnitCode,
    /// `valueType`.
    ValueType,
    /// `objectType`.
    ObjectType,
    /// A data-integrity seal.
    Seal(ProofSeal),
    /// A member only a response or the Temporal API carries: the plural and
    /// `previous…` value carriers and the linked-entity members.
    OutputOnly,
}

/// The output-only members: Temporal API plurals (clause 4.5.9), the
/// `previous…` carriers of `showChanges` (clause 5.2.14.1) and the inline
/// linked entities of clause 4.5.23.2.
const OUTPUT_ONLY: [Iri<&'static str>; 16] = [
    properties::VALUES,
    properties::OBJECTS,
    properties::LANGUAGE_MAPS,
    properties::VOCABS,
    properties::JSONS,
    properties::VALUE_LISTS,
    properties::OBJECT_LISTS,
    properties::PREVIOUS_VALUE,
    properties::PREVIOUS_OBJECT,
    properties::PREVIOUS_LANGUAGE_MAP,
    properties::PREVIOUS_VOCAB,
    properties::PREVIOUS_JSON,
    properties::PREVIOUS_VALUE_LIST,
    properties::PREVIOUS_OBJECT_LIST,
    properties::ENTITY,
    properties::ENTITY_LIST,
];

impl MemberRole {
    /// The role of the expanded member `name`.
    #[must_use]
    pub fn of(name: &str) -> MemberRole {
        if let Some(carrier) = ValueCarrier::from_iri(name) {
            return MemberRole::Carrier(carrier);
        }
        let fixed = [
            (properties::SCOPE, MemberRole::Scope),
            (properties::EXPIRES_AT, MemberRole::ExpiresAt),
            (properties::DATASET_ID, MemberRole::DatasetId),
            (properties::INSTANCE_ID, MemberRole::InstanceId),
            (properties::OBSERVED_AT, MemberRole::ObservedAt),
            (properties::CREATED_AT, MemberRole::CreatedAt),
            (properties::MODIFIED_AT, MemberRole::ModifiedAt),
            (properties::DELETED_AT, MemberRole::DeletedAt),
            (properties::UNIT_CODE, MemberRole::UnitCode),
            (properties::VALUE_TYPE, MemberRole::ValueType),
            (properties::OBJECT_TYPE, MemberRole::ObjectType),
            (properties::ENTITY_ID_SEALED, MemberRole::Seal(ProofSeal::EntityId)),
            (properties::ENTITY_TYPE_SEALED, MemberRole::Seal(ProofSeal::EntityType)),
        ];
        if let Some((_, role)) = fixed.into_iter().find(|(iri, _)| iri.as_str() == name) {
            return role;
        }
        if OUTPUT_ONLY.iter().any(|iri| iri.as_str() == name) {
            return MemberRole::OutputOnly;
        }
        MemberRole::Attribute
    }

    /// Whether `name` names a member NGSI-LD defines rather than an attribute.
    #[must_use]
    pub fn is_reserved(name: &str) -> bool {
        MemberRole::of(name) != MemberRole::Attribute
    }
}

#[cfg(test)]
mod tests {
    use crate::{
        document::value_carrier::ValueCarrier,
        member_role::{MemberRole, ProofSeal},
    };
    use andromeda_ngsi_ld_vocabulary::expanded::properties;

    #[test]
    fn the_entity_members_the_specification_defines_are_not_attributes() {
        for reserved in [properties::SCOPE, properties::EXPIRES_AT, properties::CREATED_AT, properties::MODIFIED_AT] {
            assert!(MemberRole::is_reserved(reserved.as_str()), "{reserved} is not an attribute");
        }
    }

    #[test]
    fn every_familys_value_carrier_is_reserved_in_both_its_spellings() {
        for reserved in [
            properties::VALUE,
            properties::VALUES,
            properties::OBJECT,
            properties::OBJECTS,
            properties::OBJECT_LIST,
            properties::LANGUAGE_MAP,
            properties::LANGUAGE_MAPS,
            properties::JSON,
            properties::JSONS,
            properties::VOCAB,
            properties::VOCABS,
            properties::VALUE_LIST,
        ] {
            assert!(MemberRole::is_reserved(reserved.as_str()), "{reserved} carries a value, not an attribute");
        }
    }

    /// Clause 5.2.14.1 gives every family an output-only `previous…` member
    /// whose representation is that of the family's value.
    #[test]
    fn every_familys_output_only_spelling_is_reserved_too() {
        for reserved in [
            properties::PREVIOUS_VALUE,
            properties::PREVIOUS_OBJECT,
            properties::PREVIOUS_OBJECT_LIST,
            properties::PREVIOUS_LANGUAGE_MAP,
            properties::PREVIOUS_JSON,
            properties::PREVIOUS_VOCAB,
            properties::PREVIOUS_VALUE_LIST,
        ] {
            pretty_assertions::assert_eq!(MemberRole::of(reserved.as_str()), MemberRole::OutputOnly, "{reserved}");
        }
    }

    /// Clause 4.5.2.2 calls `entityIdSealed` and `entityTypeSealed` "non-reified
    /// subproperties", so each carries a value rather than an attribute.
    #[test]
    fn the_two_data_integrity_seals_carry_a_value_rather_than_an_attribute() {
        pretty_assertions::assert_eq!(MemberRole::of(properties::ENTITY_ID_SEALED.as_str()), MemberRole::Seal(ProofSeal::EntityId));
        pretty_assertions::assert_eq!(MemberRole::of(properties::ENTITY_TYPE_SEALED.as_str()), MemberRole::Seal(ProofSeal::EntityType));
    }

    /// The `ngsildproof` Property that carries the seals is an attribute in its
    /// own right (clause 4.5.2.2 defines it as "a Property").
    #[test]
    fn the_ngsildproof_property_carrying_the_seals_is_itself_an_attribute() {
        assert!(!MemberRole::is_reserved(properties::NGSILDPROOF.as_str()));
    }

    #[test]
    fn a_client_chosen_attribute_iri_is_not_reserved() {
        assert!(!MemberRole::is_reserved("https://example.org/speed"));
        assert!(!MemberRole::is_reserved("https://uri.etsi.org/ngsi-ld/default-context/temperature"));
    }

    #[test]
    fn each_carrier_names_its_own_role() {
        pretty_assertions::assert_eq!(
            MemberRole::of(properties::LANGUAGE_MAP.as_str()),
            MemberRole::Carrier(ValueCarrier::LanguageMap)
        );
    }
}
