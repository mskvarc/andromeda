//! Reading the instance-level `expiresAt` of an expanded payload (clause 4.22).
//!
//! An attribute instance may carry its own `expiresAt`, with the meaning the
//! Entity-level member has: once it passes, the instance is gone. Two readers
//! share this module so they cannot disagree about where the instant lives:
//! the mutation that removes past-due instances, and the write path that
//! records the earliest such instant on the entity row so an expiry sweep can
//! find candidates by index instead of by walking every payload.

use crate::expanded::{
    dataset_id::position_of_dataset,
    node::{expanded_object_mut, expanded_object_ref},
};
use andromeda_ngsi_ld_lexicon::uri::Uri;
use andromeda_ngsi_ld_vocabulary::expanded::properties;
use chrono::{DateTime, Utc};
use serde_json::Value;
use std::borrow::Cow;

/// The `expiresAt` one attribute instance carries, when it carries one the
/// broker can read.
///
/// The instant is the `@value` of the first entry under the `expiresAt` term.
/// A value that does not parse as an RFC 3339 timestamp yields `None`: a
/// value the broker cannot read is not evidence of an expiry, so the instance
/// is treated as one that never expires.
pub fn instance_expires_at(instance: &Value) -> Option<DateTime<Utc>> {
    instance
        .get(properties::EXPIRES_AT.as_str())
        .and_then(Value::as_array)
        .and_then(|values| values.first())
        .and_then(|value| value.get("@value"))
        .and_then(Value::as_str)
        .and_then(|raw| DateTime::parse_from_rfc3339(raw).ok())
        .map(|expires_at| expires_at.with_timezone(&Utc))
}

/// The earliest `expiresAt` any attribute instance of the expanded payload
/// carries, or `None` when no instance expires.
///
/// Attributes are the array-valued members whose name is not a JSON-LD
/// keyword; each element of such an array is one instance. The Entity-level
/// `expiresAt` member is not an attribute and does not count: its entries are
/// value nodes, not instances, and carry no `expiresAt` of their own, so the
/// walk passes over them without a special case.
pub fn min_attribute_expires_at(expanded: &Value) -> Option<DateTime<Utc>> {
    let object = expanded_object_ref(expanded)?;
    object
        .iter()
        .filter(|(name, _)| !name.starts_with('@'))
        .filter_map(|(_, value)| value.as_array())
        .flat_map(|instances| instances.iter())
        .filter_map(instance_expires_at)
        .min()
}

/// Whether one attribute instance is past its `expiresAt` at `at`.
///
/// Clause 4.22: an instance "shall become invalid" at its `expiresAt`, so an
/// instance whose instant is at or before `at` is expired. An instance without
/// a readable `expiresAt` never expires.
fn instance_expired_at(instance: &Value, at: DateTime<Utc>) -> bool {
    instance_expires_at(instance).is_some_and(|expires_at| expires_at <= at)
}

/// Whether any attribute instance of the expanded payload is expired at `at`.
fn carries_expired_instance(expanded: &Value, at: DateTime<Utc>) -> bool {
    expanded_object_ref(expanded).is_some_and(|object| {
        object
            .iter()
            .filter(|(name, _)| !name.starts_with('@'))
            .filter_map(|(_, value)| value.as_array())
            .flat_map(|instances| instances.iter())
            .any(|instance| instance_expired_at(instance, at))
    })
}

/// Remove every attribute instance of the expanded payload that is expired at
/// `at`, and every attribute left without an instance.
///
/// Clause 4.22 makes an expired instance invalid while its deletion from
/// storage may lag, so whatever reads a stored payload as the entity it now is
/// -- a subscription's `q` and `geoQ`, a notification's content -- reads it
/// through this: an expired instance is absent from it, exactly as it will be
/// once the expiry sweep has deleted it.
pub fn remove_expired_instances(expanded: &mut Value, at: DateTime<Utc>) {
    let Some(object) = expanded_object_mut(expanded) else {
        return;
    };
    object.retain(|name, value| {
        if name.starts_with('@') {
            return true;
        }
        let Some(instances) = value.as_array_mut() else {
            return true;
        };
        let before = instances.len();
        instances.retain(|instance| !instance_expired_at(instance, at));
        before == 0 || !instances.is_empty()
    });
}

/// The instances one attribute of the expanded payload carries.
fn instances_of<'a>(expanded: &'a Value, attribute: &str) -> &'a [Value] {
    expanded_object_ref(expanded)
        .and_then(|object| object.get(attribute))
        .and_then(Value::as_array)
        .map_or(&[], Vec::as_slice)
}

/// Whether the payload's instance of `attribute` under `dataset` is expired at
/// `at`; `false` when the payload carries no such instance.
#[must_use]
pub fn instance_expired(expanded: &Value, attribute: &str, dataset: Option<&Uri>, at: DateTime<Utc>) -> bool {
    let instances = instances_of(expanded, attribute);
    match position_of_dataset(instances, dataset) {
        Ok(Some(position)) => instances.get(position).is_some_and(|instance| instance_expired_at(instance, at)),
        // An instance the payload does not carry, or whose `datasetId` cannot
        // be read, has no expiry to be past.
        Ok(None) | Err(_) => false,
    }
}

/// Whether `attribute` still has an instance that is not expired at `at`.
#[must_use]
pub fn attribute_stands(expanded: &Value, attribute: &str, at: DateTime<Utc>) -> bool {
    instances_of(expanded, attribute).iter().any(|instance| !instance_expired_at(instance, at))
}

/// The expanded payload as it stands at `at`: borrowed unchanged when no
/// instance is expired, otherwise a copy without the expired instances (see
/// [`remove_expired_instances`]).
#[must_use]
pub fn standing_at(expanded: &Value, at: DateTime<Utc>) -> Cow<'_, Value> {
    if !carries_expired_instance(expanded, at) {
        return Cow::Borrowed(expanded);
    }
    let mut standing = expanded.clone(); // clone: the stored payload is shared; only the standing copy loses its expired instances
    remove_expired_instances(&mut standing, at);
    Cow::Owned(standing)
}

#[cfg(test)]
mod tests {
    use crate::expanded::attribute_expiry::{attribute_stands, instance_expired, instance_expires_at, min_attribute_expires_at, standing_at};
    use chrono::{DateTime, Utc};
    use pretty_assertions::assert_eq;
    use serde_json::{Value, json};
    use std::borrow::Cow;

    fn instant(raw: &str) -> DateTime<Utc> {
        DateTime::parse_from_rfc3339(raw).expect("instant parses").with_timezone(&Utc)
    }

    fn instance(expires_at: Option<&str>) -> Value {
        let mut object = serde_json::Map::new();
        object.insert("@type".to_string(), json!(["https://uri.etsi.org/ngsi-ld/Property"]));
        object.insert("https://uri.etsi.org/ngsi-ld/hasValue".to_string(), json!([{"@value": 1}]));
        if let Some(raw) = expires_at {
            object.insert("https://uri.etsi.org/ngsi-ld/expiresAt".to_string(), json!([{"@value": raw}]));
        }
        Value::Object(object)
    }

    #[test]
    fn an_instance_without_an_expiry_yields_none() {
        assert_eq!(instance_expires_at(&instance(None)), None);
    }

    #[test]
    fn an_instance_expiry_is_read_from_the_value_node() {
        assert_eq!(
            instance_expires_at(&instance(Some("2030-01-01T00:00:00Z"))),
            Some(instant("2030-01-01T00:00:00Z"))
        );
    }

    #[test]
    fn an_unparsable_instance_expiry_yields_none() {
        assert_eq!(instance_expires_at(&instance(Some("not a date"))), None);
    }

    #[test]
    fn a_payload_whose_instances_never_expire_has_no_minimum() {
        let entity = json!([{"@id": "urn:ngsi-ld:A", "https://example.org/speed": [instance(None)]}]);
        assert_eq!(min_attribute_expires_at(&entity), None);
    }

    #[test]
    fn a_single_expiring_instance_is_the_minimum() {
        let entity = json!([{"@id": "urn:ngsi-ld:A", "https://example.org/speed": [instance(Some("2030-01-01T00:00:00Z"))]}]);
        assert_eq!(min_attribute_expires_at(&entity), Some(instant("2030-01-01T00:00:00Z")));
    }

    #[test]
    fn the_earliest_of_two_expiring_instances_across_attributes_wins() {
        let entity = json!([{
            "@id": "urn:ngsi-ld:A",
            "https://example.org/speed": [instance(Some("2031-01-01T00:00:00Z")), instance(None)],
            "https://example.org/heading": [instance(Some("2030-06-01T00:00:00Z"))]
        }]);
        assert_eq!(min_attribute_expires_at(&entity), Some(instant("2030-06-01T00:00:00Z")));
    }

    #[test]
    fn an_unparsable_expiry_does_not_take_part_in_the_minimum() {
        let entity = json!([{
            "@id": "urn:ngsi-ld:A",
            "https://example.org/speed": [instance(Some("garbage")), instance(Some("2032-01-01T00:00:00Z"))]
        }]);
        assert_eq!(min_attribute_expires_at(&entity), Some(instant("2032-01-01T00:00:00Z")));
    }

    #[test]
    fn keyword_members_and_the_entity_level_expiry_are_not_counted() {
        let entity = json!([{
            "@id": "urn:ngsi-ld:A",
            "@type": ["https://example.org/Vehicle"],
            "https://uri.etsi.org/ngsi-ld/expiresAt": [{"@type": "https://uri.etsi.org/ngsi-ld/DateTime", "@value": "2029-01-01T00:00:00Z"}],
            "https://example.org/speed": [instance(None)]
        }]);
        assert_eq!(min_attribute_expires_at(&entity), None);
    }

    #[test]
    fn a_payload_that_is_not_an_entity_node_has_no_minimum() {
        assert_eq!(min_attribute_expires_at(&json!([])), None);
        assert_eq!(min_attribute_expires_at(&json!("text")), None);
    }

    #[test]
    fn an_entity_without_an_expired_instance_stands_unchanged_and_uncopied() {
        let entity = json!([{"@id": "urn:ngsi-ld:A", "https://example.org/speed": [instance(Some("2030-01-01T00:00:00Z")), instance(None)]}]);
        let standing = standing_at(&entity, instant("2029-01-01T00:00:00Z"));
        assert!(matches!(standing, Cow::Borrowed(_)), "nothing expired, nothing copied");
    }

    /// Clause 4.22: an instance is invalid from its `expiresAt` on, so one
    /// expiring exactly at the evaluation instant is already gone; its sibling
    /// instances and the other attributes stay.
    #[test]
    fn an_instance_at_or_past_its_expiry_is_absent_and_its_siblings_stay() {
        let entity = json!([{
            "@id": "urn:ngsi-ld:A",
            "https://example.org/speed": [instance(Some("2030-01-01T00:00:00Z")), instance(None)],
            "https://example.org/heading": [instance(Some("2031-01-01T00:00:00Z"))]
        }]);
        let standing = standing_at(&entity, instant("2030-01-01T00:00:00Z"));
        assert_eq!(
            standing.into_owned(),
            json!([{
                "@id": "urn:ngsi-ld:A",
                "https://example.org/speed": [instance(None)],
                "https://example.org/heading": [instance(Some("2031-01-01T00:00:00Z"))]
            }])
        );
    }

    #[test]
    fn an_attribute_whose_every_instance_expired_is_absent() {
        let entity = json!({"@id": "urn:ngsi-ld:A", "@type": ["https://example.org/T"], "https://example.org/speed": [instance(Some("2000-01-01T00:00:00Z"))]});
        let standing = standing_at(&entity, instant("2030-01-01T00:00:00Z"));
        assert_eq!(standing.into_owned(), json!({"@id": "urn:ngsi-ld:A", "@type": ["https://example.org/T"]}));
    }

    #[test]
    fn one_instance_is_expired_while_its_sibling_stands() {
        let mut tagged = instance(None);
        tagged["https://uri.etsi.org/ngsi-ld/datasetId"] = json!([{"@id": "urn:ngsi-ld:Dataset:a"}]);
        let entity = json!([{"@id": "urn:ngsi-ld:A", "https://example.org/speed": [instance(Some("2000-01-01T00:00:00Z")), tagged]}]);
        let at = instant("2030-01-01T00:00:00Z");
        let dataset = "urn:ngsi-ld:Dataset:a".parse().expect("a valid URI");
        assert!(instance_expired(&entity, "https://example.org/speed", None, at));
        assert!(!instance_expired(&entity, "https://example.org/speed", Some(&dataset), at));
        assert!(attribute_stands(&entity, "https://example.org/speed", at));
        assert!(
            !instance_expired(&entity, "https://example.org/heading", None, at),
            "an absent instance is not expired"
        );
    }

    #[test]
    fn an_attribute_whose_every_instance_expired_does_not_stand() {
        let entity = json!([{"@id": "urn:ngsi-ld:A", "https://example.org/speed": [instance(Some("2000-01-01T00:00:00Z"))]}]);
        assert!(!attribute_stands(&entity, "https://example.org/speed", instant("2030-01-01T00:00:00Z")));
        assert!(!attribute_stands(&entity, "https://example.org/heading", instant("2030-01-01T00:00:00Z")));
    }
}
