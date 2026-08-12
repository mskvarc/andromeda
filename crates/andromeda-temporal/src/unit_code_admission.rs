//! Post-expansion `unitCode` admission for temporal writes.
//!
//! The Core Entity envelope validator refuses a `unitCode` on an attribute
//! family clause 4.5.2 defines without one before an `AtomicWrite` is built
//! (`andromeda-entity`). The Temporal API constructs its own `AtomicWrite`s
//! and instance updates from separately expanded JSON-LD, so it needs the
//! same admission check applied to its own expanded payloads rather than
//! inheriting the Core Entity path's coverage.

use crate::error::TemporalError;
use andromeda_ngsi_ld_model::expanded::unit_code_admission::forbidden_unit_code_family;
use serde_json::Value;

/// Reject an expanded entity payload carrying an attribute instance whose
/// declared family forbids the `unitCode` it carries.
///
/// Applies to the Temporal API create and append paths, which hand this an
/// entire expanded entity (or array of one) rather than a single instance.
pub(crate) fn validate_entity_unit_code_admission(expanded: &Value) -> Result<(), TemporalError> {
    let entities = match expanded {
        Value::Array(values) => values.as_slice(),
        other => std::slice::from_ref(other),
    };
    for entity in entities {
        let Some(object) = entity.as_object() else {
            continue;
        };
        for (attribute_name, value) in object {
            if attribute_name.starts_with('@') {
                continue;
            }
            let Some(instances) = value.as_array() else {
                continue;
            };
            for instance in instances {
                validate_instance_unit_code_admission(attribute_name, instance)?;
            }
        }
    }
    Ok(())
}

/// Reject one expanded attribute instance whose declared family forbids the `unitCode` it carries.
///
/// Applies to the Temporal API instance PATCH path (clause 5.6.14), which
/// hands this the single replacement instance rather than an entity payload.
pub(crate) fn validate_instance_unit_code_admission(attribute_name: &str, instance: &Value) -> Result<(), TemporalError> {
    if let Some(unitless) = forbidden_unit_code_family(instance) {
        return Err(TemporalError::validation(format!(
            "{unitless} '{attribute_name}' cannot carry 'unitCode': the specification defines this attribute family without one"
        )));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use crate::unit_code_admission::{validate_entity_unit_code_admission, validate_instance_unit_code_admission};
    use serde_json::json;

    #[test]
    fn a_relationship_carrying_a_unit_code_is_refused_in_an_entity_payload() {
        let expanded = json!([{
            "@id": "urn:ngsi-ld:Vehicle:1",
            "https://example.com/ref": [{
                "@type": ["https://uri.etsi.org/ngsi-ld/Relationship"],
                "https://uri.etsi.org/ngsi-ld/hasObject": [{"@id": "urn:ngsi-ld:Building:1"}],
                "https://uri.etsi.org/ngsi-ld/unitCode": [{"@value": "KGM"}]
            }]
        }]);
        let error = validate_entity_unit_code_admission(&expanded).expect_err("a Relationship admits no unitCode");
        assert!(error.to_string().contains("unitCode"), "{error}");
    }

    #[test]
    fn a_property_carrying_a_unit_code_is_accepted_in_an_entity_payload() {
        let expanded = json!([{
            "@id": "urn:ngsi-ld:Vehicle:1",
            "https://example.com/speed": [{
                "@type": ["https://uri.etsi.org/ngsi-ld/Property"],
                "https://uri.etsi.org/ngsi-ld/hasValue": [{"@value": 55}],
                "https://uri.etsi.org/ngsi-ld/unitCode": [{"@value": "KMH"}]
            }]
        }]);
        validate_entity_unit_code_admission(&expanded).expect("a Property defines a unitCode member");
    }

    #[test]
    fn a_replacement_instance_carrying_a_forbidden_unit_code_is_refused() {
        let instance = json!({
            "@type": ["https://uri.etsi.org/ngsi-ld/LanguageProperty"],
            "https://uri.etsi.org/ngsi-ld/languageMap": [{"@value": {"en": "hello"}}],
            "https://uri.etsi.org/ngsi-ld/unitCode": [{"@value": "KGM"}]
        });
        let error = validate_instance_unit_code_admission("https://example.com/label", &instance).expect_err("a LanguageProperty admits no unitCode");
        assert!(error.to_string().contains("unitCode"), "{error}");
    }
}
