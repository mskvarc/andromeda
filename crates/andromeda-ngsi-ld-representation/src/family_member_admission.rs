//! Which members one attribute family may never carry.
//!
//! ETSI GS CIM 009 v1.9.1 closes each family's representation clause with a
//! "Prohibited" list: clauses 4.5.2.2 and 4.5.2.3 for a Property, 4.5.3.2 and
//! 4.5.3.3 for a Relationship, 4.5.18.2 and 4.5.18.3 for a
//! `LanguageProperty`, 4.5.20.2 and 4.5.20.3 for a `VocabProperty`, 4.5.21.2
//! and 4.5.21.3 for a `ListProperty`, 4.5.22.2 and 4.5.22.3 for a
//! `ListRelationship`, and 4.5.24.2 and 4.5.24.3 for a `JsonProperty`. A
//! Property "shall never include" `object`, `objectList`, `languageMap`,
//! `vocab`, `json`, `valueList`, `entity` or `entityList`; a Relationship
//! shall never include `value`, `valueList`, `languageMap`, `vocab` or
//! `json`; a `LanguageProperty` shall never include `value`; and so on for
//! the six specialized families, each list naming the output-only
//! `previous…` spelling beside the current one.
//!
//! Every entry on every one of those fourteen lists names the value member of
//! *another* family. That is what makes them one rule rather than fourteen: a
//! family admits the value member it declares for itself and forbids all the
//! others, because a payload carrying two families' value members names no
//! single family and clause 5.5.4 makes data that is not syntactically correct
//! per the API data type definitions a `BadRequestData`.
//!
//! Nothing here restates the lists. The membership table already exists as
//! [`AttributeType::reserved_fields`] — the members each family defines for
//! itself — and the prohibition is its complement over the value members of
//! the eight families. A second, hand-written copy of the same fourteen lists
//! could only ever drift from it.
//!
//! Two members outside the value carriers join the complement because the
//! clauses treat them the same way: `entity` (clause 4.5.23.2, the inline
//! linked entity of a Relationship's `object`) and `entityList` (the inline
//! linked entities of a `ListRelationship`'s `objectList`) each belong to one
//! family and are prohibited on the other seven.
//!
//! `unitCode` is not part of the complement, even though four of the clauses
//! prohibit it. Whether a family is unitless is a statement about quantities
//! rather than about another family's value — clauses 4.5.21.2 and 4.5.21.3
//! leave a `ListProperty`'s inherited `unitCode` in place while removing its
//! `value` — and `andromeda_ngsi_ld_lexicon::unit_code_admission` already owns
//! that question.
//!
//! [`MemberAdmission`] is the two-state answer every admission rule of the
//! representation gives for a member it judges.

use andromeda_ngsi_ld_lexicon::attribute_type::AttributeType;
use std::str::FromStr;
use strum::{Display, EnumIter, EnumString, IntoStaticStr};

/// Whether one attribute family admits a member.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum MemberAdmission {
    /// The family declares the member, or the member is not one any family
    /// claims — a sub-attribute name, a system attribute, `datasetId`.
    Admitted,
    /// The member names another family's value, so the clause defining this
    /// family states it shall never be present.
    Forbidden,
}

/// A compacted member that names some family's value.
///
/// Both spellings of each of the eight families' value members — the current
/// one of [`AttributeType::compact_value_key`] and the output-only one of
/// [`AttributeType::previous_key`] — and the two linked-entity members of
/// clause 4.5.23.2. The spellings are the ones the NGSI-LD Core `@context`
/// gives these terms.
#[derive(Clone, Copy, Debug, Display, EnumIter, EnumString, Eq, IntoStaticStr, PartialEq)]
#[strum(serialize_all = "camelCase")]
pub enum ValueMember {
    /// `entity`, the inline linked entity of a Relationship's `object`.
    Entity,
    /// `entityList`, the inline linked entities of a `ListRelationship`.
    EntityList,
    /// `json`, the value of a `JsonProperty`.
    Json,
    /// `languageMap`, the value of a `LanguageProperty`.
    LanguageMap,
    /// `object`, the value of a Relationship.
    Object,
    /// `objectList`, the value of a `ListRelationship`.
    ObjectList,
    /// `previousJson`, the previous value of a `JsonProperty`.
    PreviousJson,
    /// `previousLanguageMap`, the previous value of a `LanguageProperty`.
    PreviousLanguageMap,
    /// `previousObject`, the previous value of a Relationship.
    PreviousObject,
    /// `previousObjectList`, the previous value of a `ListRelationship`.
    PreviousObjectList,
    /// `previousValue`, the previous value of a Property or a `GeoProperty`.
    PreviousValue,
    /// `previousValueList`, the previous value of a `ListProperty`.
    PreviousValueList,
    /// `previousVocab`, the previous value of a `VocabProperty`.
    PreviousVocab,
    /// `value`, the value of a Property or a `GeoProperty`.
    Value,
    /// `valueList`, the value of a `ListProperty`.
    ValueList,
    /// `vocab`, the value of a `VocabProperty`.
    Vocab,
}

/// The value member `member` spells, when `attribute_type` may never carry it.
///
/// A member no family claims is no prohibited member: it is a sub-attribute
/// name, or one of the members every family shares. A member some family claims
/// is prohibited on every family whose own table does not declare it.
#[must_use]
pub fn prohibited_member(attribute_type: AttributeType, member: &str) -> Option<ValueMember> {
    ValueMember::from_str(member)
        .ok()
        .filter(|claimed| !attribute_type.reserved_fields().contains(&<&'static str>::from(*claimed)))
}

#[cfg(test)]
mod tests {
    use crate::family_member_admission::{ValueMember, prohibited_member};
    use andromeda_ngsi_ld_lexicon::attribute_type::AttributeType;
    use andromeda_ngsi_ld_vocabulary::compact::properties as compact_properties;
    use std::collections::BTreeSet;
    use strum::IntoEnumIterator;

    /// The members are the two spellings of every family's value member and the
    /// two linked-entity members, so the enum cannot drift from the tables of
    /// the lexicon.
    #[test]
    fn the_value_members_are_every_familys_value_spellings_and_the_linked_entity_members() {
        let claimed: BTreeSet<&str> = AttributeType::ALL
            .into_iter()
            .flat_map(|family| [family.compact_value_key(), family.previous_key()])
            .chain([compact_properties::ENTITY, compact_properties::ENTITY_LIST])
            .collect();
        let members: BTreeSet<&str> = ValueMember::iter().map(<&'static str>::from).collect();
        pretty_assertions::assert_eq!(members, claimed);
    }

    /// Clause 4.5.2.2, "Prohibited": a Property never includes `object`.
    #[test]
    fn a_property_prohibits_the_object_member_of_a_relationship() {
        pretty_assertions::assert_eq!(prohibited_member(AttributeType::Property, "object"), Some(ValueMember::Object));
    }

    /// Clause 4.5.3.2: a Relationship declares `object` for itself.
    #[test]
    fn a_relationship_does_not_prohibit_its_own_object_member() {
        pretty_assertions::assert_eq!(prohibited_member(AttributeType::Relationship, "object"), None);
    }

    /// A sub-attribute name and the members every family shares are claimed by
    /// no family.
    #[test]
    fn a_member_no_family_claims_is_prohibited_on_none() {
        for attribute_type in AttributeType::ALL {
            for member in ["speed", "observedAt", "datasetId", "instanceId", "createdAt", "expiresAt"] {
                pretty_assertions::assert_eq!(prohibited_member(attribute_type, member), None, "{attribute_type}/{member}");
            }
        }
    }
}
