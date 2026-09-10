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

use andromeda_ngsi_ld_lexicon::attribute_type::AttributeType;
use andromeda_ngsi_ld_vocabulary::compact::properties as compact_properties;

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

/// Every compacted member that names some family's value.
///
/// Both spellings of each of the eight families' value members — the current
/// one of [`AttributeType::compact_value_key`] and the output-only one of
/// [`AttributeType::previous_key`] — followed by the two linked-entity members
/// of clause 4.5.23.2. Families sharing a value member (a Property and a
/// `GeoProperty` share `value`) yield it more than once, which membership
/// testing does not care about.
pub fn cross_family_members() -> impl Iterator<Item = &'static str> {
    AttributeType::ALL
        .into_iter()
        .flat_map(|family| [family.compact_value_key(), family.previous_key()])
        .chain([compact_properties::ENTITY, compact_properties::ENTITY_LIST])
}

/// Whether `attribute_type` admits the compacted member `member`.
///
/// A member no family claims is admitted: it is a sub-attribute name, or one
/// of the members every family shares. A member some family claims is admitted
/// only by the families whose own table declares it.
#[must_use]
pub fn member_admission(attribute_type: AttributeType, member: &str) -> MemberAdmission {
    let claimed_by_a_family = cross_family_members().any(|carrier| carrier == member);
    if claimed_by_a_family && !attribute_type.reserved_fields().contains(&member) {
        MemberAdmission::Forbidden
    } else {
        MemberAdmission::Admitted
    }
}

#[cfg(test)]
mod tests {
    use crate::family_member_admission::{MemberAdmission, cross_family_members, member_admission};
    use andromeda_ngsi_ld_lexicon::attribute_type::AttributeType;
    use std::collections::BTreeSet;

    /// The distinct members some family claims, which is the set every
    /// family's prohibition is drawn from.
    fn claimed_members() -> BTreeSet<&'static str> {
        cross_family_members().collect()
    }

    /// The members `attribute_type` may never carry.
    fn forbidden_members(attribute_type: AttributeType) -> BTreeSet<&'static str> {
        claimed_members()
            .into_iter()
            .filter(|member| member_admission(attribute_type, member) == MemberAdmission::Forbidden)
            .collect()
    }

    #[test]
    fn the_claimed_members_are_the_seven_value_members_their_output_only_spellings_and_the_two_linked_entity_members() {
        pretty_assertions::assert_eq!(
            claimed_members(),
            BTreeSet::from([
                "entity",
                "entityList",
                "json",
                "languageMap",
                "object",
                "objectList",
                "previousJson",
                "previousLanguageMap",
                "previousObject",
                "previousObjectList",
                "previousValue",
                "previousValueList",
                "previousVocab",
                "value",
                "valueList",
                "vocab",
            ])
        );
    }

    /// Clause 4.5.2.2 and clause 4.5.2.3, "Prohibited", verbatim.
    #[test]
    fn a_property_forbids_exactly_the_members_clause_4_5_2_2_lists() {
        pretty_assertions::assert_eq!(
            forbidden_members(AttributeType::Property),
            BTreeSet::from([
                "entity",
                "entityList",
                "json",
                "languageMap",
                "object",
                "objectList",
                "previousJson",
                "previousLanguageMap",
                "previousObject",
                "previousObjectList",
                "previousValueList",
                "previousVocab",
                "valueList",
                "vocab",
            ])
        );
    }

    /// Clause 4.5.3.2 and clause 4.5.3.3, "Prohibited", less the `unitCode`
    /// entry `andromeda_ngsi_ld_lexicon::unit_code_admission` owns.
    #[test]
    fn a_relationship_forbids_exactly_the_members_clause_4_5_3_2_lists() {
        pretty_assertions::assert_eq!(
            forbidden_members(AttributeType::Relationship),
            BTreeSet::from([
                "entityList",
                "json",
                "languageMap",
                "objectList",
                "previousJson",
                "previousLanguageMap",
                "previousObjectList",
                "previousValue",
                "previousValueList",
                "previousVocab",
                "value",
                "valueList",
                "vocab",
            ])
        );
    }

    /// Clause 4.5.18.2: a `LanguageProperty` differs from the Property of
    /// clause 4.5.2.2 by carrying `languageMap` instead of `value`, so it
    /// forbids `value` and inherits the rest of the Property prohibition.
    #[test]
    fn a_language_property_forbids_value_and_every_other_familys_value_member() {
        let forbidden = forbidden_members(AttributeType::LanguageProperty);
        assert!(forbidden.contains("value"), "clause 4.5.18.2: {forbidden:?}");
        assert!(forbidden.contains("previousValue"), "clause 4.5.18.2: {forbidden:?}");
        assert!(
            !forbidden.contains("languageMap"),
            "a LanguageProperty declares its own value member: {forbidden:?}"
        );
        assert!(!forbidden.contains("previousLanguageMap"), "{forbidden:?}");
    }

    /// Clause 4.5.20.2 for a `VocabProperty` and clause 4.5.24.2 for a
    /// `JsonProperty`: each forbids `value` and keeps its own specialization.
    #[test]
    fn the_vocab_and_json_families_forbid_value_and_keep_their_own_specialized_member() {
        for (attribute_type, own, own_previous) in [
            (AttributeType::VocabProperty, "vocab", "previousVocab"),
            (AttributeType::JsonProperty, "json", "previousJson"),
        ] {
            let forbidden = forbidden_members(attribute_type);
            assert!(forbidden.contains("value"), "{attribute_type}: {forbidden:?}");
            assert!(forbidden.contains("previousValue"), "{attribute_type}: {forbidden:?}");
            assert!(!forbidden.contains(own), "{attribute_type}: {forbidden:?}");
            assert!(!forbidden.contains(own_previous), "{attribute_type}: {forbidden:?}");
        }
    }

    /// Clause 4.5.21.2: `value` is a generalization of `valueList`, so a
    /// `ListProperty` forbids it while keeping the `unitCode` clause 4.5.2.2
    /// gives every Property.
    #[test]
    fn a_list_property_forbids_the_generalized_value_but_not_its_inherited_unit_code() {
        let forbidden = forbidden_members(AttributeType::ListProperty);
        assert!(forbidden.contains("value"), "clause 4.5.21.2: {forbidden:?}");
        assert!(forbidden.contains("previousValue"), "clause 4.5.21.2: {forbidden:?}");
        assert!(!forbidden.contains("valueList"), "{forbidden:?}");
        pretty_assertions::assert_eq!(member_admission(AttributeType::ListProperty, "unitCode"), MemberAdmission::Admitted);
    }

    /// Clause 4.5.22.2: `object` generalizes `objectList` and `entity`
    /// generalizes `entityList`, so a `ListRelationship` forbids both
    /// generalizations and keeps both specializations.
    #[test]
    fn a_list_relationship_forbids_the_generalized_object_and_entity_members() {
        let forbidden = forbidden_members(AttributeType::ListRelationship);
        assert!(forbidden.contains("object"), "clause 4.5.22.2: {forbidden:?}");
        assert!(forbidden.contains("previousObject"), "clause 4.5.22.2: {forbidden:?}");
        assert!(forbidden.contains("entity"), "clause 4.5.22.2: {forbidden:?}");
        assert!(!forbidden.contains("objectList"), "{forbidden:?}");
        assert!(!forbidden.contains("entityList"), "{forbidden:?}");
    }

    #[test]
    fn every_family_admits_its_own_value_member_and_its_own_output_only_spelling() {
        for attribute_type in AttributeType::ALL {
            pretty_assertions::assert_eq!(
                member_admission(attribute_type, attribute_type.compact_value_key()),
                MemberAdmission::Admitted,
                "{attribute_type}"
            );
            pretty_assertions::assert_eq!(
                member_admission(attribute_type, attribute_type.previous_key()),
                MemberAdmission::Admitted,
                "{attribute_type}"
            );
        }
    }

    #[test]
    fn a_sub_attribute_name_and_the_shared_members_are_admitted_by_every_family() {
        for attribute_type in AttributeType::ALL {
            for member in ["speed", "observedAt", "datasetId", "instanceId", "createdAt", "expiresAt"] {
                pretty_assertions::assert_eq!(member_admission(attribute_type, member), MemberAdmission::Admitted, "{attribute_type}/{member}");
            }
        }
    }
}
