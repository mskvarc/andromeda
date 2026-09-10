//! Which expanded member names NGSI-LD reserves for itself rather than leaving to attributes.
//!
//! Clause 4.5.2 makes an attribute name any IRI a client chooses, but the
//! expanded form of an Entity and the expanded form of one attribute instance
//! both carry members the specification defines: `scope` and `expiresAt` on the
//! Entity (clauses 4.18 and 4.22), the system metadata of clause 4.8, and the
//! value carriers each attribute family declares (`hasValue`, `hasObject`,
//! `hasLanguageMap`, …). None of those is an attribute, so a walk looking for
//! attributes has to step over them: their sub-nodes are values and language
//! entries, not attribute instances, and reading them as instances attributes
//! properties to them that clause 4.5.5.1 states about attributes alone.

use crate::expanded::instance_key::is_system_attribute;
use andromeda_ngsi_ld_lexicon::attribute_name::AttributeName;
use andromeda_ngsi_ld_vocabulary::expanded::properties;

/// Whether an expanded IRI names a member NGSI-LD defines rather than an attribute.
///
/// Covers the system metadata of [`is_system_attribute`], the two Entity
/// members that are not attributes, every family's value carrier in both its
/// singular and its plural (Temporal API) spelling, and the two data-integrity
/// seals of clause 4.5.2.2.
#[must_use]
pub fn is_reserved_member(name: &AttributeName) -> bool {
    is_system_attribute(name) || is_entity_member(name) || is_value_carrier(name) || is_proof_seal(name)
}

/// Whether an expanded IRI names one of the two data-integrity seals of clause
/// 4.5.2.2.
///
/// The clause calls `entityIdSealed` and `entityTypeSealed` "non-reified
/// subproperties" of `ngsildproof`: each carries its value directly, so the
/// node below one is a value or an IRI reference and never an attribute
/// instance. Naming them here is what stops a walk from reading one as an
/// instance and holding it to the multi-instance rules of clause 4.5.5.1.
///
/// No name test guards this. The two IRIs are reachable only through the scoped
/// `@context` clause 4.4 attaches to the `ngsildproof` term, and a seal written
/// under any other attribute is refused before expansion by
/// `andromeda_ngsi_ld_representation::proof_seal_admission`, so an expanded
/// payload carrying one carries it where the clause admits it.
fn is_proof_seal(name: &AttributeName) -> bool {
    name.as_str() == properties::ENTITY_ID_SEALED.as_str() || name.as_str() == properties::ENTITY_TYPE_SEALED.as_str()
}

/// Whether an expanded IRI names an Entity member that is not an attribute.
fn is_entity_member(name: &AttributeName) -> bool {
    name.as_str() == properties::SCOPE.as_str() || name.as_str() == properties::EXPIRES_AT.as_str()
}

/// Whether an expanded IRI names the value carrier of an attribute family.
///
/// All three spellings are listed: the singular member of clause 5.2.5, the
/// plural one the Temporal API's `temporalValues` representation carries, and
/// the output-only `previous…` one clause 5.2.14.1 puts in a notification when
/// `showChanges` was requested. The last of those carries a value exactly as
/// the first does — clause 5.2.5 says its "representation is the same as that
/// of `value`" — so a walk that read it as an attribute would attribute the
/// multi-instance rules of clause 4.5.5.1 to a value.
fn is_value_carrier(name: &AttributeName) -> bool {
    [
        properties::JSON,
        properties::JSONS,
        properties::LANGUAGE_MAP,
        properties::LANGUAGE_MAPS,
        properties::OBJECT,
        properties::OBJECTS,
        properties::OBJECT_LIST,
        properties::OBJECT_LISTS,
        properties::PREVIOUS_JSON,
        properties::PREVIOUS_LANGUAGE_MAP,
        properties::PREVIOUS_OBJECT,
        properties::PREVIOUS_OBJECT_LIST,
        properties::PREVIOUS_VALUE,
        properties::PREVIOUS_VALUE_LIST,
        properties::PREVIOUS_VOCAB,
        properties::VALUE,
        properties::VALUES,
        properties::VALUE_LIST,
        properties::VALUE_LISTS,
        properties::VOCAB,
        properties::VOCABS,
    ]
    .iter()
    .any(|carrier| carrier.as_str() == name.as_str())
}

#[cfg(test)]
mod tests {
    use crate::expanded::reserved_member::is_reserved_member;
    use andromeda_ngsi_ld_lexicon::attribute_name::AttributeName;
    use andromeda_ngsi_ld_vocabulary::expanded::properties;

    fn name(iri: &str) -> AttributeName {
        iri.parse().expect("the fixture names a valid attribute name")
    }

    #[test]
    fn the_entity_members_the_specification_defines_are_not_attributes() {
        for reserved in [properties::SCOPE, properties::EXPIRES_AT, properties::CREATED_AT, properties::MODIFIED_AT] {
            assert!(is_reserved_member(&name(reserved.as_str())), "{reserved} is not an attribute");
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
            assert!(is_reserved_member(&name(reserved.as_str())), "{reserved} carries a value, not an attribute");
        }
    }

    /// Clause 5.2.14.1 gives every family an output-only `previous…` member
    /// whose representation is that of the family's value, so it carries a
    /// value rather than an attribute.
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
            assert!(is_reserved_member(&name(reserved.as_str())), "{reserved} carries a value, not an attribute");
        }
    }

    /// Clause 4.5.2.2 calls `entityIdSealed` and `entityTypeSealed` "non-reified
    /// subproperties", so each carries a value rather than an attribute
    /// instance and a walk looking for attributes steps over both.
    #[test]
    fn the_two_data_integrity_seals_carry_a_value_rather_than_an_attribute() {
        for reserved in [properties::ENTITY_ID_SEALED, properties::ENTITY_TYPE_SEALED] {
            assert!(is_reserved_member(&name(reserved.as_str())), "{reserved} is non-reified, not an attribute");
        }
    }

    /// The `ngsildproof` Property that carries the seals is an attribute in its
    /// own right (clause 4.5.2.2 defines it as "a Property"), so it is not
    /// reserved and a walk descends into it.
    #[test]
    fn the_ngsildproof_property_carrying_the_seals_is_itself_an_attribute() {
        assert!(!is_reserved_member(&name(properties::NGSILDPROOF.as_str())));
    }

    #[test]
    fn a_client_chosen_attribute_iri_is_not_reserved() {
        assert!(!is_reserved_member(&name("https://example.org/speed")));
        assert!(!is_reserved_member(&name("https://uri.etsi.org/ngsi-ld/default-context/temperature")));
    }
}
