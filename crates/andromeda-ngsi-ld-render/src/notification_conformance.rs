//! The clause 5.8.6 amendment of a notification's Entities to the NGSI-LD
//! version its Subscription declared.
//!
//! Table 5.2.12-1 gives a Subscription an `ngsildConformance` member, "a
//! semantically versioned string in the form major.minor, which conforms to a
//! version of the NGSI-LD specification", and clause 5.8.6 has the notification
//! "undergo a backwards compatibility operation as defined by clause 4.3.6.8
//! and be amended to conform to the supplied version". This module owns where
//! in the notification that operation runs; the operation itself is
//! [`crate::version`].

use crate::version::VersionRenderer;
use andromeda_ngsi_ld_lexicon::spec_version::SpecVersion;
use andromeda_ngsi_ld_representation::response_format::ResponseFormat;
use serde_json::Value;

/// Amend the Entities one notification carries so they conform to `conformance`
/// (clause 5.8.6, `ngsildConformance`); an absent member leaves them as they
/// are.
///
/// The amendment runs over the Entities and never over the notification that
/// carries them. Clause 5.3.1 shapes a Notification as `id`, `type`,
/// `subscriptionId`, `notifiedAt` and `data`, while Tables 4.3.6.8-1 to -3
/// describe NGSI-LD Entity, Property and Relationship data alone: applied to
/// the whole message, Table 4.3.6.8-1 note 2 would read `data` as one
/// multi-instance attribute and reduce the Entities to a single one.
///
/// Clause 5.8.6 orders this after the `notification.format` rendering -- it is
/// the last step before "A Notification shall be sent" -- so the representation
/// the Entities are already in is the one the fallback tables are applied to,
/// and `format` is what says which that is.
#[must_use]
pub fn amend_notification_entities(entities: Vec<Value>, conformance: Option<SpecVersion>, format: ResponseFormat) -> Vec<Value> {
    let Some(target) = conformance else {
        return entities;
    };
    entities
        .into_iter()
        .map(|entity| VersionRenderer::new(target).with_format(format).with_payload(entity).render().0)
        .collect()
}

#[cfg(test)]
mod tests {
    use crate::notification_conformance::amend_notification_entities;
    use andromeda_ngsi_ld_lexicon::spec_version::SpecVersion;
    use andromeda_ngsi_ld_representation::response_format::ResponseFormat;
    use serde_json::{Value, json};

    fn notified_entity() -> Value {
        json!({
            "id": "urn:ngsi-ld:Vehicle:1",
            "type": ["Vehicle", "Car"],
            "scope": ["/A"],
            "expiresAt": "2099-01-01T00:00:00Z",
            "speed": {"type": "Property", "value": 20, "observedAt": "2025-01-01T00:00:00Z"}
        })
    }

    /// A Subscription that declares no `ngsildConformance` gets the Entities
    /// exactly as the format rendering left them.
    #[test]
    fn an_absent_conformance_leaves_the_entities_untouched() {
        let amended = amend_notification_entities(vec![notified_entity()], None, ResponseFormat::Normalized);
        pretty_assertions::assert_eq!(amended, vec![notified_entity()]);
    }

    /// Every Entity of the `data` member is amended, not only the first.
    #[test]
    fn every_entity_of_the_data_member_is_amended() {
        let amended = amend_notification_entities(vec![notified_entity(), notified_entity()], Some(SpecVersion::V1_0), ResponseFormat::Normalized);
        pretty_assertions::assert_eq!(amended.len(), 2);
        for entity in &amended {
            pretty_assertions::assert_eq!(entity["type"], json!("Vehicle"));
            assert!(entity.get("scope").is_none(), "{entity}");
            assert!(entity.get("expiresAt").is_none(), "{entity}");
            assert!(entity["speed"].get("observedAt").is_none(), "{entity}");
        }
    }

    /// The Entities are amended in the representation the
    /// `notification.format` rendering left them in.
    #[test]
    fn the_amendment_reads_the_representation_the_format_rendering_produced() {
        let concise = json!({"id": "urn:ngsi-ld:Vehicle:1", "type": "Vehicle", "name": {"languageMap": {"en": "Tower"}}});
        let amended = amend_notification_entities(vec![concise], Some(SpecVersion::V1_3), ResponseFormat::Concise);
        pretty_assertions::assert_eq!(amended[0]["name"], json!({"en": "Tower"}));
    }

    /// A conformance at or above the version this broker conforms to asks for
    /// nothing, so the Entities are delivered as they were rendered.
    #[test]
    fn a_conformance_at_the_conformant_version_amends_nothing() {
        let amended = amend_notification_entities(vec![notified_entity()], Some(SpecVersion::CONFORMANT), ResponseFormat::Normalized);
        pretty_assertions::assert_eq!(amended, vec![notified_entity()]);
    }
}
