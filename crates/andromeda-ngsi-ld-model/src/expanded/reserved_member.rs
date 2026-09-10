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
/// members that are not attributes, and every family's value carrier in both
/// its singular and its plural (Temporal API) spelling.
#[must_use]
pub fn is_reserved_member(name: &AttributeName) -> bool {
    is_system_attribute(name) || is_entity_member(name) || is_value_carrier(name)
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

    #[test]
    fn a_client_chosen_attribute_iri_is_not_reserved() {
        assert!(!is_reserved_member(&name("https://example.org/speed")));
        assert!(!is_reserved_member(&name("https://uri.etsi.org/ngsi-ld/default-context/temperature")));
    }
}
