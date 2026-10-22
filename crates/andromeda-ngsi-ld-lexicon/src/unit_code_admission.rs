//! Which attribute families the specification defines a `unitCode` member for.

use crate::attribute_type::AttributeType;
use andromeda_ngsi_ld_vocabulary::compact::properties as compact_properties;

/// Whether one attribute family may carry a `unitCode` member.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum UnitCodeAdmission {
    /// The family defines a `unitCode` member (clause 4.5.2).
    Admitted,
    /// The family is defined without one, so a `unitCode` on it is invalid input.
    Forbidden,
}

/// The `unitCode` admission of one attribute family.
///
/// Clauses 4.5.3 (Relationship), 4.5.18 (`LanguageProperty`), 4.5.20
/// (`VocabProperty`) and 4.5.24 (`JsonProperty`) each state that `unitCode` shall
/// never be present, and a `ListRelationship` is a list of relationship targets
/// under the same rule. Their values are not quantities: a unit on them names
/// nothing the broker could store or render back, so the write is refused
/// rather than silently stripped.
///
/// The family's reserved members are the one list of what each family defines.
#[must_use]
pub fn unit_code_admission(attribute_type: AttributeType) -> UnitCodeAdmission {
    if attribute_type.defines_member(compact_properties::UNIT_CODE) {
        UnitCodeAdmission::Admitted
    } else {
        UnitCodeAdmission::Forbidden
    }
}

#[cfg(test)]
mod tests {
    use crate::{
        attribute_type::AttributeType,
        unit_code_admission::{UnitCodeAdmission, unit_code_admission},
    };

    #[test]
    fn the_families_the_specification_leaves_unitless_admit_no_unit_code() {
        let forbidden = [
            AttributeType::Relationship,
            AttributeType::LanguageProperty,
            AttributeType::VocabProperty,
            AttributeType::JsonProperty,
            AttributeType::ListRelationship,
        ];
        for attribute_type in forbidden {
            pretty_assertions::assert_eq!(unit_code_admission(attribute_type), UnitCodeAdmission::Forbidden, "{attribute_type}");
        }
    }

    #[test]
    fn the_quantity_bearing_families_admit_a_unit_code() {
        for attribute_type in [AttributeType::Property, AttributeType::GeoProperty, AttributeType::ListProperty] {
            pretty_assertions::assert_eq!(unit_code_admission(attribute_type), UnitCodeAdmission::Admitted, "{attribute_type}");
        }
    }
}
