//! Whether an expanded attribute instance's declared family admits the `unitCode` member it carries.
//!
//! Clauses 4.5.3 (Relationship), 4.5.18 (LanguageProperty), 4.5.20
//! (VocabProperty) and 4.5.24 (JsonProperty) each state that `unitCode` shall
//! never be present, and a ListRelationship is a list of relationship targets
//! under the same rule. Every write path that constructs a durable
//! consequence from expanded JSON-LD — the Core Entity envelope, the Temporal
//! API create and append paths, and the temporal instance PATCH path — must
//! refuse a `unitCode` on one of those families rather than store it, so the
//! check lives once here rather than once per path.

use crate::expanded::instance_key::declared_attribute_type;
use andromeda_ngsi_ld_lexicon::{
    attribute_type::AttributeType,
    unit_code_admission::{UnitCodeAdmission, unit_code_admission},
};
use andromeda_ngsi_ld_vocabulary::expanded::properties;
use serde_json::Value;

/// The attribute family that forbids the `unitCode` an expanded instance carries.
///
/// `None` means the instance is well formed with respect to `unitCode`
/// admission — either it carries no `unitCode` member at all, or its declared
/// family is one clause 4.5.2 defines a `unitCode` member for.
pub fn forbidden_unit_code_family(instance: &Value) -> Option<AttributeType> {
    let carries_unit_code = instance.as_object().is_some_and(|object| object.contains_key(properties::UNIT_CODE.as_str()));
    if !carries_unit_code {
        return None;
    }
    let attribute_type = declared_attribute_type(instance);
    (unit_code_admission(attribute_type) == UnitCodeAdmission::Forbidden).then_some(attribute_type)
}

#[cfg(test)]
mod tests {
    use crate::expanded::unit_code_admission::forbidden_unit_code_family;
    use andromeda_ngsi_ld_lexicon::attribute_type::AttributeType;
    use serde_json::json;

    #[test]
    fn a_relationship_carrying_a_unit_code_names_its_own_family_as_forbidding_it() {
        let instance = json!({
            "@type": ["https://uri.etsi.org/ngsi-ld/Relationship"],
            "https://uri.etsi.org/ngsi-ld/hasObject": [{"@id": "urn:ngsi-ld:Device:target"}],
            "https://uri.etsi.org/ngsi-ld/unitCode": [{"@value": "KGM"}],
        });
        pretty_assertions::assert_eq!(forbidden_unit_code_family(&instance), Some(AttributeType::Relationship));
    }

    #[test]
    fn a_property_carrying_a_unit_code_is_well_formed() {
        let instance = json!({
            "@type": ["https://uri.etsi.org/ngsi-ld/Property"],
            "https://uri.etsi.org/ngsi-ld/hasValue": [{"@value": 1}],
            "https://uri.etsi.org/ngsi-ld/unitCode": [{"@value": "KGM"}],
        });
        pretty_assertions::assert_eq!(forbidden_unit_code_family(&instance), None);
    }

    #[test]
    fn a_relationship_without_a_unit_code_is_well_formed() {
        let instance = json!({
            "@type": ["https://uri.etsi.org/ngsi-ld/Relationship"],
            "https://uri.etsi.org/ngsi-ld/hasObject": [{"@id": "urn:ngsi-ld:Device:target"}],
        });
        pretty_assertions::assert_eq!(forbidden_unit_code_family(&instance), None);
    }
}
