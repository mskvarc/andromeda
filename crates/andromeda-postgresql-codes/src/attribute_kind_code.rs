//! Translation between the NGSI-LD attribute family and the code stored on a row.
//!
//! [`AttributeType`] is the specification's vocabulary and [`AttributeKind`] is
//! the migration-owned number a stored row keeps it in: a history row, a current
//! attribute value, the attribute instances an entity's search row records and
//! the discovery census. The two are separate on purpose — one may gain a member
//! from a future revision of the specification while the other may only ever be
//! appended to — so the translation is written out rather than derived, and it
//! lives beside the code so every store reads and writes the one mapping.

use crate::attribute_kind::AttributeKind;
use andromeda_ngsi_ld_lexicon::attribute_type::AttributeType;

/// The code a stored row records for an attribute of this family.
#[must_use]
pub const fn stored_attribute_kind(attribute_type: AttributeType) -> AttributeKind {
    match attribute_type {
        AttributeType::Property => AttributeKind::Property,
        AttributeType::Relationship => AttributeKind::Relationship,
        AttributeType::GeoProperty => AttributeKind::GeoProperty,
        AttributeType::VocabProperty => AttributeKind::VocabProperty,
        AttributeType::LanguageProperty => AttributeKind::LanguageProperty,
        AttributeType::JsonProperty => AttributeKind::JsonProperty,
        AttributeType::ListProperty => AttributeKind::ListProperty,
        AttributeType::ListRelationship => AttributeKind::ListRelationship,
    }
}

/// The attribute family a stored code names.
#[must_use]
pub const fn attribute_type_of(kind: AttributeKind) -> AttributeType {
    match kind {
        AttributeKind::Property => AttributeType::Property,
        AttributeKind::Relationship => AttributeType::Relationship,
        AttributeKind::GeoProperty => AttributeType::GeoProperty,
        AttributeKind::VocabProperty => AttributeType::VocabProperty,
        AttributeKind::LanguageProperty => AttributeType::LanguageProperty,
        AttributeKind::JsonProperty => AttributeType::JsonProperty,
        AttributeKind::ListProperty => AttributeType::ListProperty,
        AttributeKind::ListRelationship => AttributeType::ListRelationship,
    }
}

#[cfg(test)]
mod tests {
    use crate::{
        attribute_kind::AttributeKind,
        attribute_kind_code::{attribute_type_of, stored_attribute_kind},
    };

    #[test]
    fn every_stored_kind_round_trips_through_the_attribute_family() {
        for kind in AttributeKind::ALL {
            pretty_assertions::assert_eq!(stored_attribute_kind(attribute_type_of(*kind)), *kind);
        }
    }
}
