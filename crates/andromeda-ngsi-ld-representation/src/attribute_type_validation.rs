//! Why a compact attribute node is not admitted.
//!
//! ETSI GS CIM 009 v1.9.1 clause 5.5.4: data that is not syntactically correct
//! according to the API data type definitions is an error of type
//! `BadRequestData`. Every node standing where an attribute or a sub-attribute
//! stands must be one [`crate::concise_reading`] reads: clause 5.2.4 fixes the
//! attribute types to the eight of [`AttributeType`], clause 4.5.2.3 treats an
//! object carrying a `type` as a normalized attribute, and clause 4.5.5 writes
//! the instances of a multi-attribute as an array of instances. So a node that
//! declares any other token -- `"Propertyy"`, `"Feature"`, `7` -- names a
//! geometry without its coordinates, carries the value members of two families,
//! or mixes instances with values in one array is not an attribute and must not
//! reach the store as one. A sub-attribute is an attribute (clause 4.5.2.2), so
//! the rule holds at every depth; a value is data and is never walked into.
//!
//! The members an instance carries are held to the same clauses: the seals of
//! clause 4.5.2.2 stand only on the `ngsildproof` Property
//! ([`crate::proof_seal_admission`]), and the "Prohibited" lists of the family
//! clauses forbid the value members of another family
//! ([`crate::family_member_admission`]).

use crate::{concise_reading::ObjectRefusal, family_member_admission::ValueMember, proof_seal_admission::ProofSealMember};
use andromeda_ngsi_ld_lexicon::attribute_type::AttributeType;
use thiserror::Error;

/// Why a compact attribute node is not admitted.
#[derive(Debug, Error, Clone, PartialEq, Eq)]
pub enum AttributeNodeFault {
    /// The node declared a `type` token that is neither an attribute type
    /// (clause 5.2.4) nor a geometry type.
    #[error("declares a type that is not an NGSI-LD attribute type")]
    NotAnAttributeType(#[source] strum::ParseError),
    /// The node declared a `type` that is not a string.
    #[error("declares a type that is not a string")]
    TypeNotAString,
    /// The node named a `GeoJSON` geometry type without the member holding the
    /// geometry's coordinates (clause 4.7.1).
    #[error("names a GeoJSON geometry type without the member holding its coordinates")]
    GeometryWithoutCoordinates,
    /// The node declared no `type` and carried the value members of several
    /// attribute types.
    #[error("declares no type and carries the value members of several attribute types")]
    SeveralValueMembers,
    /// The node is an array holding both attribute instances and values
    /// (clause 4.5.5).
    #[error("mixes attribute instances with values in one array")]
    MixedInstancesAndValues,
    /// The node carried a data-integrity seal outside `ngsildproof` (clause
    /// 4.5.2.2).
    #[error("carries the member {0}, which is admitted only on the ngsildproof Property")]
    ProofSealOutsideNgsildproof(ProofSealMember),
    /// The node carried the value member of another attribute family, which
    /// the "Prohibited" list of the family's own clause forbids (clauses
    /// 4.5.2.2 to 4.5.24.3).
    #[error("carries the member {member}, which defines the value of another attribute family and is never present on a {family}")]
    ValueMemberOfAnotherFamily {
        /// The family the instance belongs to.
        family: AttributeType,
        /// The member of another family's value.
        member: ValueMember,
    },
}

impl AttributeNodeFault {
    /// The fault an object the concise reading refuses is reported with.
    #[must_use]
    pub const fn of_object(refusal: ObjectRefusal) -> AttributeNodeFault {
        match refusal {
            ObjectRefusal::NotAnAttributeType(source) => AttributeNodeFault::NotAnAttributeType(source),
            ObjectRefusal::TypeNotAString => AttributeNodeFault::TypeNotAString,
            ObjectRefusal::GeometryWithoutCoordinates => AttributeNodeFault::GeometryWithoutCoordinates,
            ObjectRefusal::SeveralValueMembers => AttributeNodeFault::SeveralValueMembers,
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::{attribute_type_validation::AttributeNodeFault, concise_reading::ObjectRefusal};

    #[test]
    fn every_refusal_of_the_concise_reading_has_its_own_fault() {
        pretty_assertions::assert_eq!(
            AttributeNodeFault::of_object(ObjectRefusal::NotAnAttributeType(strum::ParseError::VariantNotFound)),
            AttributeNodeFault::NotAnAttributeType(strum::ParseError::VariantNotFound)
        );
        pretty_assertions::assert_eq!(AttributeNodeFault::of_object(ObjectRefusal::TypeNotAString), AttributeNodeFault::TypeNotAString);
        pretty_assertions::assert_eq!(
            AttributeNodeFault::of_object(ObjectRefusal::GeometryWithoutCoordinates),
            AttributeNodeFault::GeometryWithoutCoordinates
        );
        pretty_assertions::assert_eq!(
            AttributeNodeFault::of_object(ObjectRefusal::SeveralValueMembers),
            AttributeNodeFault::SeveralValueMembers
        );
    }
}
