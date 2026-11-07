//! When attribute instances expire (clause 4.22).
//!
//! An instance is invalid from its `expiresAt` on: at the instant itself it is
//! already gone. The entity-level `expiresAt` is the entity's own and does not
//! take part in the attribute minimum.

use crate::{entity_state::EntityState, instance::Instance};
use andromeda_ngsi_ld_lexicon::{identifier::Identifier, time::date_time::NgsiLdDateTime};
use chrono::{DateTime, Utc};
use std::borrow::Cow;

impl Instance {
    /// Whether the instance is expired at `at` (clause 4.22: from its
    /// `expiresAt` on).
    #[must_use]
    pub fn expired_at(&self, at: DateTime<Utc>) -> bool {
        self.expires_at().is_some_and(|expires_at| expires_at.get() <= at)
    }
}

impl EntityState {
    /// The entity's own expiry instant.
    #[must_use]
    pub fn entity_expiry(&self) -> Option<DateTime<Utc>> {
        self.expires_at().map(NgsiLdDateTime::get)
    }

    /// The earliest expiry of any attribute instance: what the expiry sweep
    /// is indexed on.
    #[must_use]
    pub fn min_attribute_expiry(&self) -> Option<DateTime<Utc>> {
        self.attributes()
            .iter()
            .flat_map(|(_, instances)| instances.iter())
            .filter_map(|(_, instance)| instance.expires_at().map(NgsiLdDateTime::get))
            .min()
    }

    /// Whether the instance of `name` with `dataset_id` is expired at `at`;
    /// `false` when there is no such instance.
    #[must_use]
    pub fn instance_expired(&self, name: &str, dataset_id: Option<&Identifier>, at: DateTime<Utc>) -> bool {
        self.attributes()
            .get(name)
            .and_then(|instances| instances.get(dataset_id))
            .is_some_and(|instance| instance.expired_at(at))
    }

    /// Whether some instance of `name` is still valid at `at`.
    #[must_use]
    pub fn attribute_stands(&self, name: &str, at: DateTime<Utc>) -> bool {
        self.attributes()
            .get(name)
            .is_some_and(|instances| instances.iter().any(|(_, instance)| !instance.expired_at(at)))
    }

    /// The entity as it stands at `at`: every instance expired by then left
    /// out, and an attribute none of whose instances stands left out whole.
    ///
    /// Borrowed when nothing expired. Otherwise the copy shares every
    /// attribute it does not change.
    #[must_use]
    pub fn standing_at(&self, at: DateTime<Utc>) -> Cow<'_, EntityState> {
        let expired = |instance: &Instance| instance.expired_at(at);
        let any_expired = self
            .attributes()
            .iter()
            .any(|(_, instances)| instances.iter().any(|(_, instance)| expired(instance)));
        if !any_expired {
            return Cow::Borrowed(self);
        }
        let mut standing = self.clone();
        let touched: Vec<_> = standing
            .attributes()
            .iter()
            .filter(|(_, instances)| instances.iter().any(|(_, instance)| expired(instance)))
            .map(|(name, _)| name.clone())
            .collect();
        for name in touched {
            let attributes = standing.attributes_mut();
            let emptied = attributes
                .get_mut(name.as_str())
                .is_some_and(|instances| !instances.retain(|_, instance| !expired(instance)));
            if emptied {
                attributes.remove(name.as_str());
            }
        }
        Cow::Owned(standing)
    }
}

#[cfg(test)]
mod tests {
    use crate::entity_state::EntityState;
    use andromeda_ngsi_ld_lexicon::time::date_time::NgsiLdDateTime;
    use chrono::{DateTime, Utc};
    use serde_json::{Value, json};
    use std::{borrow::Cow, sync::Arc};

    fn instant(raw: &str) -> DateTime<Utc> {
        raw.parse::<NgsiLdDateTime>().expect("instant parses").get()
    }

    fn instance(expires_at: Option<&str>, dataset: Option<&str>) -> Value {
        let mut object = json!({"@type": ["https://uri.etsi.org/ngsi-ld/Property"], "https://uri.etsi.org/ngsi-ld/hasValue": [{"@value": 1}]});
        if let Some(raw) = expires_at {
            object["https://uri.etsi.org/ngsi-ld/expiresAt"] = json!([{"@value": raw}]);
        }
        if let Some(dataset) = dataset {
            object["https://uri.etsi.org/ngsi-ld/datasetId"] = json!([{"@id": dataset}]);
        }
        object
    }

    fn state(members: &Value) -> EntityState {
        let mut node = json!({"@id": "urn:ngsi-ld:A", "@type": ["https://example.org/T"]});
        if let (Some(node), Some(members)) = (node.as_object_mut(), members.as_object()) {
            node.extend(members.iter().map(|(key, value)| (key.clone(), value.clone())));
        }
        EntityState::from_slice(&serde_json::to_vec(&json!([node])).expect("JSON")).expect("a state")
    }

    #[test]
    fn a_payload_whose_instances_never_expire_has_no_minimum() {
        pretty_assertions::assert_eq!(
            state(&json!({"https://example.org/speed": [instance(None, None)]})).min_attribute_expiry(),
            None
        );
    }

    #[test]
    fn the_earliest_of_two_expiring_instances_across_attributes_wins() {
        let entity = state(&json!({
            "https://example.org/speed": [instance(Some("2031-01-01T00:00:00Z"), None), instance(None, Some("urn:ngsi-ld:Dataset:a"))],
            "https://example.org/heading": [instance(Some("2030-06-01T00:00:00Z"), None)]
        }));
        pretty_assertions::assert_eq!(entity.min_attribute_expiry(), Some(instant("2030-06-01T00:00:00Z")));
    }

    #[test]
    fn the_entity_level_expiry_is_not_counted() {
        let entity = state(&json!({
            "https://uri.etsi.org/ngsi-ld/expiresAt": [{"@type": "https://uri.etsi.org/ngsi-ld/DateTime", "@value": "2029-01-01T00:00:00Z"}],
            "https://example.org/speed": [instance(None, None)]
        }));
        pretty_assertions::assert_eq!(entity.min_attribute_expiry(), None);
        pretty_assertions::assert_eq!(entity.entity_expiry(), Some(instant("2029-01-01T00:00:00Z")));
    }

    #[test]
    fn an_entity_without_an_expired_instance_stands_unchanged_and_uncopied() {
        let entity =
            state(&json!({"https://example.org/speed": [instance(Some("2030-01-01T00:00:00Z"), None), instance(None, Some("urn:ngsi-ld:Dataset:a"))]}));
        assert!(matches!(entity.standing_at(instant("2029-01-01T00:00:00Z")), Cow::Borrowed(_)));
    }

    /// Clause 4.22: an instance expiring exactly at the evaluation instant is
    /// already gone; its siblings and the other attributes stay, and an
    /// untouched attribute is shared rather than copied.
    #[test]
    fn an_instance_at_or_past_its_expiry_is_absent_and_its_siblings_stay() {
        let entity = state(&json!({
            "https://example.org/speed": [instance(Some("2030-01-01T00:00:00Z"), None), instance(None, Some("urn:ngsi-ld:Dataset:a"))],
            "https://example.org/heading": [instance(Some("2031-01-01T00:00:00Z"), None)]
        }));
        let standing = entity.standing_at(instant("2030-01-01T00:00:00Z")).into_owned();
        let expected = state(&json!({
            "https://example.org/speed": [instance(None, Some("urn:ngsi-ld:Dataset:a"))],
            "https://example.org/heading": [instance(Some("2031-01-01T00:00:00Z"), None)]
        }));
        pretty_assertions::assert_eq!(standing, expected);
        let shared = |state: &EntityState| state.attributes().get_shared("https://example.org/heading").map(Arc::clone);
        assert!(matches!((shared(&entity), shared(&standing)), (Some(before), Some(after)) if Arc::ptr_eq(&before, &after)));
    }

    #[test]
    fn an_attribute_whose_every_instance_expired_is_absent() {
        let entity = state(&json!({"https://example.org/speed": [instance(Some("2000-01-01T00:00:00Z"), None)]}));
        let standing = entity.standing_at(instant("2030-01-01T00:00:00Z"));
        assert!(standing.attributes().is_empty());
    }

    #[test]
    fn one_instance_is_expired_while_its_sibling_stands() {
        let entity =
            state(&json!({"https://example.org/speed": [instance(Some("2000-01-01T00:00:00Z"), None), instance(None, Some("urn:ngsi-ld:Dataset:a"))]}));
        let at = instant("2030-01-01T00:00:00Z");
        let dataset = "urn:ngsi-ld:Dataset:a".parse().expect("a valid URI");
        assert!(entity.instance_expired("https://example.org/speed", None, at));
        assert!(!entity.instance_expired("https://example.org/speed", Some(&dataset), at));
        assert!(entity.attribute_stands("https://example.org/speed", at));
        assert!(
            !entity.instance_expired("https://example.org/heading", None, at),
            "an absent instance is not expired"
        );
    }

    #[test]
    fn an_attribute_whose_every_instance_expired_does_not_stand() {
        let entity = state(&json!({"https://example.org/speed": [instance(Some("2000-01-01T00:00:00Z"), None)]}));
        assert!(!entity.attribute_stands("https://example.org/speed", instant("2030-01-01T00:00:00Z")));
        assert!(!entity.attribute_stands("https://example.org/heading", instant("2030-01-01T00:00:00Z")));
    }

    #[test]
    fn an_unparsable_instance_expiry_is_refused_when_decoding() {
        let document = json!([{"@id": "urn:ngsi-ld:A", "@type": ["https://example.org/T"], "https://example.org/speed": [instance(Some("not a date"), None)]}]);
        assert!(EntityState::from_slice(&serde_json::to_vec(&document).expect("JSON")).is_err());
    }
}
