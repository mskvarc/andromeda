//! Post-expansion `unitCode` admission for temporal writes.
//!
//! The Core Entity envelope validator refuses a `unitCode` on an attribute
//! family clause 4.5.2 defines without one, and a `unitCode` that names no
//! UN/CEFACT common code (Table 5.2.5-1), before an `AtomicWrite` is built
//! (`andromeda-entity`). The Temporal API constructs its own `AtomicWrite`s
//! and instance updates from separately expanded JSON-LD, so it needs the
//! same admission check applied to its own expanded payloads rather than
//! inheriting the Core Entity path's coverage.

use crate::{error::TemporalError, temporal_invariant::TemporalInvariant, temporal_validation_reason::TemporalValidationReason};
use andromeda_context_commit::submission_refusal::SubmissionRefusal;
use andromeda_ngsi_ld_lexicon::attribute_name::AttributeName;
use andromeda_ngsi_ld_model::expanded::unit_code_admission::{forbidden_unit_code_family, usable_unit_code};
use andromeda_unit_catalog::expanded_unit_code::ExpandedUnitCodeError;
use serde_json::Value;
use std::slice::from_ref;

/// Reject an expanded entity payload carrying an attribute instance whose
/// declared family forbids the `unitCode` it carries, or whose `unitCode` names
/// no unit.
///
/// Applies to the Temporal API create and append paths, which hand this an
/// entire expanded entity (or array of one) rather than a single instance.
pub(crate) fn validate_entity_unit_code_admission(expanded: &Value) -> Result<(), TemporalError> {
    let entities = match expanded {
        Value::Array(values) => values.as_slice(),
        other @ (Value::Null | Value::Bool(_) | Value::Number(_) | Value::String(_) | Value::Object(_)) => from_ref(other),
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

/// Reject one expanded attribute instance whose declared family forbids the
/// `unitCode` it carries, or whose `unitCode` names no unit.
///
/// Applies to the Temporal API instance PATCH path (clause 5.6.14), which
/// hands this the single replacement instance rather than an entity payload.
pub(crate) fn validate_instance_unit_code_admission(attribute_name: &str, instance: &Value) -> Result<(), TemporalError> {
    if let Some(unitless) = forbidden_unit_code_family(instance) {
        return Err(TemporalError::Validation(TemporalValidationReason::InstanceUnitCodeForbidden {
            family: unitless,
            attribute: refused_attribute(attribute_name)?,
        }));
    }
    match usable_unit_code(instance) {
        Ok(()) => Ok(()),
        Err(ExpandedUnitCodeError::Refused(source)) => Err(TemporalError::Validation(TemporalValidationReason::InstanceUnitCodeUnusable {
            attribute: refused_attribute(attribute_name)?,
            source,
        })),
        Err(ExpandedUnitCodeError::Catalog(source)) => Err(TemporalError::Internal(TemporalInvariant::UnitCatalog(source))),
    }
}

/// The attribute a refused instance belongs to, by its expanded name.
fn refused_attribute(attribute_name: &str) -> Result<AttributeName, TemporalError> {
    attribute_name
        .parse::<AttributeName>()
        .map_err(|source| TemporalError::from(SubmissionRefusal::ExpandedAttributeName(source)))
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

    /// Table 5.2.5-1: `unitCode` is a UN/CEFACT common code, which `km/h` is not.
    #[test]
    fn a_property_carrying_a_unit_that_is_no_common_code_is_refused_in_an_entity_payload() {
        let expanded = json!([{
            "@id": "urn:ngsi-ld:Vehicle:1",
            "https://example.com/speed": [{
                "@type": ["https://uri.etsi.org/ngsi-ld/Property"],
                "https://uri.etsi.org/ngsi-ld/hasValue": [{"@value": 55}],
                "https://uri.etsi.org/ngsi-ld/unitCode": [{"@value": "km/h"}]
            }]
        }]);
        let error = validate_entity_unit_code_admission(&expanded).expect_err("km/h is no common code");
        pretty_assertions::assert_eq!(
            error.to_string(),
            "invalid temporal operation: 'https://example.com/speed' carries an unusable 'unitCode': 'unitCode' is not a UN/CEFACT common code"
        );
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
