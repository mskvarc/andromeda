//! Which attribute may carry the two data-integrity seal members.
//!
//! ETSI GS CIM 009 v1.9.1 clause 4.5.2.2 defines `ngsildproof` as "a Property
//! with the non-reified subproperties `entityIdSealed` and
//! `entityTypeSealed`", and in the same clause's "Prohibited" list states that
//! those two "shall never be present, unless the Property name is
//! `ngsildproof`". Clause 4.5.2.3 repeats it for the concise representation,
//! and clauses 4.5.3.2 and 4.5.3.3 prohibit them on a Relationship outright.
//! Together they say the seals belong to the one Property clause 4.5.2.2
//! defines for them and nowhere else, whatever the family of the attribute
//! carrying them: a seal on any other attribute claims an integrity proof the
//! payload does not carry, which clause 5.5.4 makes a `BadRequestData`.
//!
//! The rule is decided on the member name a client wrote, before JSON-LD
//! expansion, because the core `@context` of clause 4.4 declares the two terms
//! only inside `ngsildproof`'s own scoped context. A seal written anywhere else
//! resolves against the default `@vocab` instead and, once expanded, is
//! indistinguishable from an ordinary sub-attribute.

use crate::family_member_admission::MemberAdmission;
use andromeda_ngsi_ld_lexicon::attribute_type::AttributeType;
use andromeda_ngsi_ld_vocabulary::compact::properties as compact_properties;

/// Whether a compacted member name is one of the two seals of clause 4.5.2.2.
#[must_use]
pub fn is_proof_seal(member: &str) -> bool {
    member == compact_properties::ENTITY_ID_SEALED || member == compact_properties::ENTITY_TYPE_SEALED
}

/// Whether the attribute named `attribute_name`, of family `family`, may carry
/// `member`.
///
/// Only the `ngsildproof` Property of clause 4.5.2.2 admits a seal. The family
/// is part of the rule and not only the name: clause 4.5.2.2 defines
/// `ngsildproof` as "a Property", while clauses 4.5.3.2 and 4.5.3.3 prohibit
/// both seals on a Relationship with no exception at all -- so an attribute
/// that calls itself `ngsildproof` and declares a Relationship carries no
/// admissible seal. A family this cannot determine is treated as the Property
/// clause 4.5.2.3 infers for a bare value.
#[must_use]
pub fn proof_seal_admission(attribute_name: &str, family: AttributeType, member: &str) -> MemberAdmission {
    if is_proof_seal(member) && !(attribute_name == compact_properties::NGSILDPROOF && family == AttributeType::Property) {
        MemberAdmission::Forbidden
    } else {
        MemberAdmission::Admitted
    }
}

#[cfg(test)]
mod tests {
    use crate::{
        family_member_admission::MemberAdmission,
        proof_seal_admission::{is_proof_seal, proof_seal_admission},
    };
    use andromeda_ngsi_ld_lexicon::attribute_type::AttributeType;

    #[test]
    fn the_two_seals_of_clause_4_5_2_2_are_recognised_and_nothing_else_is() {
        assert!(is_proof_seal("entityIdSealed"));
        assert!(is_proof_seal("entityTypeSealed"));
        assert!(!is_proof_seal("entity"));
        assert!(!is_proof_seal("ngsildproof"));
        assert!(!is_proof_seal("speed"));
    }

    /// Clauses 4.5.3.2 and 4.5.3.3 prohibit both seals on a Relationship with
    /// no exception, so the `ngsildproof` name does not admit them there; clause
    /// 4.5.2.2 defines `ngsildproof` as a Property.
    #[test]
    fn a_seal_on_an_ngsildproof_that_declares_a_relationship_is_forbidden() {
        for member in ["entityIdSealed", "entityTypeSealed"] {
            pretty_assertions::assert_eq!(
                proof_seal_admission("ngsildproof", AttributeType::Relationship, member),
                MemberAdmission::Forbidden,
                "{member}"
            );
        }
    }

    #[test]
    fn a_seal_outside_the_ngsildproof_property_is_forbidden() {
        for member in ["entityIdSealed", "entityTypeSealed"] {
            pretty_assertions::assert_eq!(
                proof_seal_admission("temperature", AttributeType::Property, member),
                MemberAdmission::Forbidden,
                "{member}"
            );
            pretty_assertions::assert_eq!(
                proof_seal_admission("ngsildproof", AttributeType::Property, member),
                MemberAdmission::Admitted,
                "{member}"
            );
        }
    }

    #[test]
    fn a_member_that_is_not_a_seal_is_admitted_under_any_attribute_name() {
        pretty_assertions::assert_eq!(
            proof_seal_admission("temperature", AttributeType::Property, "unitCode"),
            MemberAdmission::Admitted
        );
        pretty_assertions::assert_eq!(proof_seal_admission("ngsildproof", AttributeType::Property, "value"), MemberAdmission::Admitted);
    }
}
