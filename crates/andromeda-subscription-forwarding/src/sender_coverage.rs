//! What this broker takes of an entity a source notifies under the copy it
//! holds: the Attributes the source's registration covers for that entity.
//!
//! Clause 4.3.6.1 has every constraint a registration specifies respected,
//! attribute names among them, and the copy cannot carry them to the source.
//! Clause 5.8.1.4 removes `attributes`, `pick` and `omit` from the copy's
//! `notification` member, so the source notifies every Attribute it holds of
//! an entity, and a periodic copy has no `watchedAttributes` either (Table
//! 5.2.12-1). The registration is respected on receipt instead: an Attribute
//! it does not cover is no part of what the source sent, and an entity it
//! covers nothing of is no part of the notification.

use andromeda_federation::attribute_coverage::AttributeCoverage;
use andromeda_ngsi_ld_expansion::received_state::ReceivedDocument;
use andromeda_ngsi_ld_model::registration::ContextSourceRegistration;

/// `part` reduced to the Attributes `registration` covers for its entity, the
/// previous values its source reported under `showChanges` with it; `None`
/// when the registration covers none of them.
pub(crate) fn covered_part(mut part: ReceivedDocument, registration: &ContextSourceRegistration) -> Option<ReceivedDocument> {
    let coverage = AttributeCoverage::of(registration, part.document.types(), part.document.id()?);
    match coverage {
        AttributeCoverage::NotMatching => None,
        AttributeCoverage::Unrestricted => Some(part),
        AttributeCoverage::Names(_) => {
            part.document.attributes_mut().retain(|name, _| coverage.covers(name));
            if part.document.attributes().is_empty() {
                return None;
            }
            part.previous = part
                .previous
                .map(|mut previous| {
                    previous.attributes_mut().retain(|name, _| coverage.covers(name));
                    previous
                })
                .filter(|previous| !previous.attributes().is_empty());
            Some(part)
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::{
        relay_fixture::{offline_engine, received},
        sender_coverage::covered_part,
    };
    use andromeda_ngsi_ld_entity::document::EntityDocument;
    use andromeda_ngsi_ld_model::registration::ContextSourceRegistration;
    use serde_json::{Value, json};

    /// The IRI the core context gives a term it does not define (clause 5.5.7).
    fn default_iri(name: &str) -> String {
        format!("https://uri.etsi.org/ngsi-ld/default-context/{name}")
    }

    fn registration(information: &Value) -> ContextSourceRegistration {
        serde_json::from_value(json!({
            "id": "urn:ngsi-ld:ContextSourceRegistration:sender",
            "type": "ContextSourceRegistration",
            "endpoint": "http://broker-b:1056",
            "information": information,
            "status": "ok",
            "createdAt": "2024-01-01T00:00:00Z",
            "modifiedAt": "2024-01-01T00:00:00Z"
        }))
        .expect("valid registration")
    }

    /// A registration of vehicles naming the Property `speed`.
    fn naming_speed() -> ContextSourceRegistration {
        registration(&json!([{"entities": [{"type": default_iri("Vehicle")}], "propertyNames": [default_iri("speed")]}]))
    }

    /// A Vehicle as a source notifies it, holding `speed` and `fuel`, each
    /// with the value it held before the change.
    fn notified() -> Value {
        json!({
            "id": "urn:ngsi-ld:Vehicle:1",
            "type": "Vehicle",
            "speed": {"type": "Property", "value": 50, "previousValue": 40},
            "fuel": {"type": "Property", "value": 12, "previousValue": 13}
        })
    }

    /// The attribute names `document` holds, in order.
    fn names(document: &EntityDocument) -> Vec<String> {
        document.attributes().keys().map(ToString::to_string).collect()
    }

    /// Clause 4.3.6.1: what a source notifies beyond the Attributes its
    /// registration names is not taken, and neither are the previous values
    /// it reported for them.
    #[tokio::test]
    async fn a_notified_part_keeps_only_the_attributes_its_registration_names() {
        let engine = offline_engine();
        let part = covered_part(received(&engine, &notified()).await, &naming_speed()).expect("the registration covers speed");
        pretty_assertions::assert_eq!(names(&part.document), vec![default_iri("speed")]);
        pretty_assertions::assert_eq!(
            names(part.previous.as_ref().expect("speed reported its previous value")),
            vec![default_iri("speed")]
        );
    }

    /// A registration covering none of the entity's Attributes, or not the
    /// entity at all, takes nothing of it.
    #[tokio::test]
    async fn a_notified_part_its_registration_covers_nothing_of_is_not_taken() {
        let engine = offline_engine();
        let fuel_only = json!({"id": "urn:ngsi-ld:Vehicle:1", "type": "Vehicle", "fuel": {"type": "Property", "value": 12}});
        assert!(covered_part(received(&engine, &fuel_only).await, &naming_speed()).is_none());
        let buildings = registration(&json!([{"entities": [{"type": default_iri("Building")}]}]));
        assert!(covered_part(received(&engine, &notified()).await, &buildings).is_none());
    }

    /// A registration naming no Attribute covers every Attribute of the
    /// entities it registers.
    #[tokio::test]
    async fn a_registration_naming_no_attribute_takes_the_whole_part() {
        let engine = offline_engine();
        let vehicles = registration(&json!([{"entities": [{"type": default_iri("Vehicle")}]}]));
        let part = covered_part(received(&engine, &notified()).await, &vehicles).expect("the registration covers the vehicle");
        pretty_assertions::assert_eq!(names(&part.document), vec![default_iri("fuel"), default_iri("speed")]);
        pretty_assertions::assert_eq!(
            names(part.previous.as_ref().expect("previous values were reported")),
            vec![default_iri("fuel"), default_iri("speed")]
        );
    }
}
