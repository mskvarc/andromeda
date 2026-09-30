//! Dropping an entity a context source answered with that a write to this
//! broker would refuse (ETSI GS CIM 009 v1.9.1 clause 6.3.17).
//!
//! A `unitCode` that names no UN/CEFACT common code, or a geometry off the
//! WGS 84 axes, is refused when a client writes it; relaying one from a source
//! would hand the client exactly what the broker refuses to hold. The entity is
//! left out of the merge and the answer carries a `RevalidationFailed` warning
//! naming the source, the entity and the refused attribute, so the rest of the
//! source's answer still reaches the client. Current and temporal answers are
//! held to the same reading.

use andromeda_http_headers::ngsild_warning::NgsiLdWarning;
use andromeda_ngsi_ld_lexicon::identifier::Identifier;
use andromeda_ngsi_ld_model::attribute::instance_revalidation::first_refused_attribute;
use serde_json::Value;
use tracing::warn;

/// The warning an entity `csr_id` answered with earns when one of its
/// attributes is refused, or `None` when it is admissible.
pub(crate) fn refusal_warning(csr_id: &Identifier, entity: &Value) -> Option<NgsiLdWarning> {
    let refused = first_refused_attribute(entity)?;
    let id = entity.get("id").and_then(Value::as_str).unwrap_or_default();
    warn!("Context source {} returned entity {} with an unusable {}", csr_id, id, refused);
    Some(NgsiLdWarning::RevalidationFailed(format!(
        "Context source {csr_id} returned entity {id} with an unusable {refused}"
    )))
}

/// Keep only the entities of `entities` that `csr_id` answered admissibly,
/// adding a warning to `warnings` for each one dropped.
pub(crate) fn retain_admissible(csr_id: &Identifier, entities: &mut Vec<Value>, warnings: &mut Vec<NgsiLdWarning>) {
    entities.retain(|entity| match refusal_warning(csr_id, entity) {
        Some(warning) => {
            warnings.push(warning);
            false
        }
        None => true,
    });
}

#[cfg(test)]
mod tests {
    use crate::entity_revalidation::retain_admissible;
    use andromeda_http_headers::ngsild_warning::NgsiLdWarning;
    use andromeda_ngsi_ld_lexicon::identifier::Identifier;
    use serde_json::json;

    fn registration_id() -> Identifier {
        "urn:ngsi-ld:Registration:1".parse().expect("registration URI")
    }

    /// Clause 6.3.17: a temporal entity whose history holds an instance a
    /// write would refuse is dropped with a warning naming it, and the rest of
    /// the answer is kept.
    #[test]
    fn a_temporal_entity_with_a_refused_instance_is_dropped_with_a_warning() {
        let mut entities = vec![
            json!({"id": "urn:ngsi-ld:Vehicle:valid", "type": "Vehicle", "speed": [
                {"type": "Property", "value": 50, "unitCode": "KMH", "observedAt": "2020-01-01T00:00:00Z"}
            ]}),
            json!({"id": "urn:ngsi-ld:Vehicle:invalid", "type": "Vehicle", "speed": [
                {"type": "Property", "value": 50, "unitCode": "km/h", "observedAt": "2020-01-01T00:00:00Z"}
            ]}),
        ];
        let mut warnings = Vec::new();
        retain_admissible(&registration_id(), &mut entities, &mut warnings);
        pretty_assertions::assert_eq!(entities.len(), 1);
        pretty_assertions::assert_eq!(entities[0]["id"], json!("urn:ngsi-ld:Vehicle:valid"));
        let [NgsiLdWarning::RevalidationFailed(detail)] = warnings.as_slice() else {
            panic!("one revalidation warning is expected: {warnings:?}");
        };
        pretty_assertions::assert_eq!(
            detail,
            "Context source urn:ngsi-ld:Registration:1 returned entity urn:ngsi-ld:Vehicle:invalid with an unusable attribute 'speed': 'unitCode' is not a UN/CEFACT common code"
        );
    }
}
