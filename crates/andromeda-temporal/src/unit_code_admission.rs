//! `unitCode` admission for temporal writes.
//!
//! The Core API refuses a `unitCode` on an attribute family clause 4.5.2
//! defines without one when it admits a written entity.
//! The Temporal API builds its own atomic writes and instance updates from its
//! own written documents, so it holds their instances to the same rule. A
//! `unitCode` that names no UN/CEFACT common code (Table 5.2.5-1) is refused by
//! the typed decoding already; the NGSI-LD Null in its place deletes no unit an
//! observation could have, so it is refused as the unusable code it is.

use crate::{error::TemporalError, temporal_validation_reason::TemporalValidationReason};
use andromeda_ngsi_ld_entity::document::{EntityDocument, attribute_member::AttributeMember, instance_document::InstanceDocument, slot::Slot};
use andromeda_ngsi_ld_lexicon::{
    attribute_name::AttributeName,
    unit_code_admission::{UnitCodeAdmission, unit_code_admission},
    unit_code_resolution::UnitCodeRefusal,
};

/// Refuse a written Temporal Evolution document carrying an attribute instance
/// whose family forbids the `unitCode` it carries.
///
/// # Errors
///
/// Returns [`TemporalError::Validation`] for the first such instance.
pub(crate) fn admit_document_unit_codes(document: &EntityDocument) -> Result<(), TemporalError> {
    document.attributes().iter().try_for_each(|(attribute, member)| match member {
        AttributeMember::Instances(instances) => instances.iter().try_for_each(|instance| admit_instance_unit_code(attribute, instance)),
        AttributeMember::Remove => Ok(()),
    })
}

/// Refuse one written instance whose family forbids the `unitCode` it carries,
/// or which sets its `unitCode` to the NGSI-LD Null.
///
/// # Errors
///
/// Returns [`TemporalError::Validation`] naming the instance's attribute.
pub(crate) fn admit_instance_unit_code(attribute: &AttributeName, instance: &InstanceDocument) -> Result<(), TemporalError> {
    match instance.unit_code() {
        None => Ok(()),
        Some(Slot::Remove) => Err(TemporalValidationReason::InstanceUnitCodeUnusable {
            attribute: attribute.clone(), // clone: the refusal owns the attribute it names
            source: UnitCodeRefusal::NotACommonCode,
        }
        .into()),
        Some(Slot::Set(_)) => match instance.implied_family() {
            Some(family) if unit_code_admission(family) == UnitCodeAdmission::Forbidden => Err(TemporalValidationReason::InstanceUnitCodeForbidden {
                family,
                attribute: attribute.clone(), // clone: the refusal owns the attribute it names
            }
            .into()),
            Some(_) | None => Ok(()),
        },
    }
}

#[cfg(test)]
#[expect(
    clippy::expect_used,
    reason = "test code unwraps, expects and panics on purpose; migrating it to typed assertions is pending"
)]
mod tests {
    use crate::unit_code_admission::{admit_document_unit_codes, admit_instance_unit_code};
    use andromeda_ngsi_ld_entity::document::{EntityDocument, instance_document::InstanceDocument};
    use andromeda_ngsi_ld_lexicon::attribute_name::AttributeName;
    use serde_json::{Value, json};

    fn document(json: &Value) -> EntityDocument {
        EntityDocument::from_slice(&serde_json::to_vec(json).expect("JSON")).expect("a written document decodes")
    }

    #[test]
    fn a_relationship_carrying_a_unit_code_is_refused_in_an_entity_payload() {
        let written = document(&json!([{
            "@id": "urn:ngsi-ld:Vehicle:1",
            "https://example.com/ref": [{
                "@type": ["https://uri.etsi.org/ngsi-ld/Relationship"],
                "https://uri.etsi.org/ngsi-ld/hasObject": [{"@id": "urn:ngsi-ld:Building:1"}],
                "https://uri.etsi.org/ngsi-ld/unitCode": [{"@value": "KGM"}]
            }]
        }]));
        let error = admit_document_unit_codes(&written).expect_err("a Relationship admits no unitCode");
        assert!(error.to_string().contains("unitCode"), "{error}");
    }

    #[test]
    fn a_property_carrying_a_unit_code_is_accepted_in_an_entity_payload() {
        let written = document(&json!([{
            "@id": "urn:ngsi-ld:Vehicle:1",
            "https://example.com/speed": [{
                "@type": ["https://uri.etsi.org/ngsi-ld/Property"],
                "https://uri.etsi.org/ngsi-ld/hasValue": [{"@value": 55}],
                "https://uri.etsi.org/ngsi-ld/unitCode": [{"@value": "KMH"}]
            }]
        }]));
        admit_document_unit_codes(&written).expect("a Property defines a unitCode member");
    }

    #[test]
    fn a_replacement_instance_carrying_a_forbidden_unit_code_is_refused() {
        let label: AttributeName = "https://example.com/label".parse().expect("a valid attribute name");
        let instance = InstanceDocument::from_slice(
            &label,
            &serde_json::to_vec(&json!({
                "@type": ["https://uri.etsi.org/ngsi-ld/LanguageProperty"],
                "https://uri.etsi.org/ngsi-ld/hasLanguageMap": [{"@value": "hello", "@language": "en"}],
                "https://uri.etsi.org/ngsi-ld/unitCode": [{"@value": "KGM"}]
            }))
            .expect("JSON"),
        )
        .expect("an expanded instance");
        let error = admit_instance_unit_code(&label, &instance).expect_err("a LanguageProperty admits no unitCode");
        assert!(error.to_string().contains("unitCode"), "{error}");
    }

    /// The NGSI-LD Null names no unit an observation could carry.
    #[test]
    fn a_unit_code_set_to_the_null_is_refused() {
        let speed: AttributeName = "https://example.com/speed".parse().expect("a valid attribute name");
        let instance = InstanceDocument::from_slice(
            &speed,
            &serde_json::to_vec(&json!({
                "@type": ["https://uri.etsi.org/ngsi-ld/Property"],
                "https://uri.etsi.org/ngsi-ld/hasValue": [{"@value": 55}],
                "https://uri.etsi.org/ngsi-ld/unitCode": [{"@value": "urn:ngsi-ld:null"}]
            }))
            .expect("JSON"),
        )
        .expect("an expanded instance");
        let error = admit_instance_unit_code(&speed, &instance).expect_err("the Null is no unit");
        assert!(error.to_string().contains("unusable 'unitCode'"), "{error}");
    }
}
