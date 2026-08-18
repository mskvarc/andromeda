//! Removing attribute instances whose `expiresAt` has passed (clause 4.22).

use crate::expanded::{attribute_expiry::instance_expires_at, node::expanded_object_mut};
use chrono::{DateTime, Utc};
use serde_json::Value;

/// Drop every attribute instance whose `expiresAt` is at or before `cutoff`,
/// and drop any attribute left with no instances.
///
/// Clause 4.22 gives an instance-level `expiresAt` the same meaning an
/// entity-level one has: once it passes, the instance is gone. Removing it is a
/// deletion like any other, so the caller pairs this with a tombstone per
/// removed instance.
///
/// JSON-LD keywords and any member whose value is not an instance array are
/// left exactly as they were. An `expiresAt` that does not parse as an RFC 3339
/// timestamp keeps its instance: a value the broker cannot read is not evidence
/// that the instance has expired.
///
/// Returns whether anything was removed.
pub fn remove_expired_attribute_instances(entity: &mut Value, cutoff: DateTime<Utc>) -> bool {
    let Some(obj) = expanded_object_mut(entity) else {
        return false;
    };

    let mut removed_any = false;
    let mut emptied: Vec<String> = Vec::new();

    for (attr_name, attr_value) in obj.iter_mut() {
        if attr_name.starts_with('@') {
            continue;
        }
        let Some(instances) = attr_value.as_array_mut() else {
            continue;
        };
        let before = instances.len();
        instances.retain(|instance| !has_passed(instance, cutoff));
        if instances.len() != before {
            removed_any = true;
        }
        if instances.is_empty() {
            // clone: the key is borrowed from the map being iterated, and the
            // removal cannot happen until that borrow ends.
            emptied.push(attr_name.clone());
        }
    }

    for attr_name in emptied {
        obj.remove(&attr_name);
    }

    removed_any
}

/// Whether one attribute instance carries an `expiresAt` at or before `cutoff`.
fn has_passed(instance: &Value, cutoff: DateTime<Utc>) -> bool {
    instance_expires_at(instance).is_some_and(|expires_at| expires_at <= cutoff)
}

#[cfg(test)]
mod tests {
    use super::remove_expired_attribute_instances;
    use chrono::{DateTime, Utc};
    use serde_json::{Value, json};

    fn cutoff() -> DateTime<Utc> {
        DateTime::parse_from_rfc3339("2026-01-01T00:00:00Z").expect("cutoff parses").with_timezone(&Utc)
    }

    fn instance(expires_at: Option<&str>, dataset_id: Option<&str>) -> Value {
        let mut obj = serde_json::Map::new();
        obj.insert("@type".to_string(), json!(["https://uri.etsi.org/ngsi-ld/Property"]));
        obj.insert("https://uri.etsi.org/ngsi-ld/hasValue".to_string(), json!([{"@value": 1}]));
        if let Some(raw) = expires_at {
            obj.insert(
                "https://uri.etsi.org/ngsi-ld/expiresAt".to_string(),
                json!([{"@type": "https://uri.etsi.org/ngsi-ld/DateTime", "@value": raw}]),
            );
        }
        if let Some(id) = dataset_id {
            obj.insert("https://uri.etsi.org/ngsi-ld/datasetId".to_string(), json!([{"@id": id}]));
        }
        Value::Object(obj)
    }

    fn entity_with(speed: Value) -> Value {
        json!([{"@id": "urn:ngsi-ld:A", "@type": ["https://example.org/Vehicle"], "https://example.org/speed": speed}])
    }

    fn attribute_names(entity: &Value) -> Vec<String> {
        entity[0]
            .as_object()
            .map(|obj| obj.keys().filter(|key| !key.starts_with('@')).cloned().collect()) // clone: owned names for comparison
            .unwrap_or_default()
    }

    #[test]
    fn an_instance_past_its_expiry_is_removed() {
        let mut entity = entity_with(json!([instance(Some("2025-12-31T23:59:59Z"), None)]));
        assert!(remove_expired_attribute_instances(&mut entity, cutoff()));
        assert!(attribute_names(&entity).is_empty());
    }

    #[test]
    fn an_instance_expiring_exactly_at_the_cutoff_is_removed() {
        let mut entity = entity_with(json!([instance(Some("2026-01-01T00:00:00Z"), None)]));
        assert!(remove_expired_attribute_instances(&mut entity, cutoff()));
        assert!(attribute_names(&entity).is_empty());
    }

    #[test]
    fn an_instance_expiring_after_the_cutoff_survives() {
        let mut entity = entity_with(json!([instance(Some("2026-01-01T00:00:01Z"), None)]));
        assert!(!remove_expired_attribute_instances(&mut entity, cutoff()));
        pretty_assertions::assert_eq!(attribute_names(&entity), vec!["https://example.org/speed".to_string()]);
    }

    #[test]
    fn an_instance_with_no_expiry_survives() {
        let mut entity = entity_with(json!([instance(None, None)]));
        assert!(!remove_expired_attribute_instances(&mut entity, cutoff()));
        pretty_assertions::assert_eq!(attribute_names(&entity), vec!["https://example.org/speed".to_string()]);
    }

    #[test]
    fn only_the_expired_instance_of_a_multi_attribute_goes() {
        let mut entity = entity_with(json!([
            instance(Some("2025-06-01T00:00:00Z"), Some("urn:d1")),
            instance(Some("2026-06-01T00:00:00Z"), Some("urn:d2")),
        ]));
        assert!(remove_expired_attribute_instances(&mut entity, cutoff()));

        let remaining = entity[0]["https://example.org/speed"].as_array().expect("instances");
        pretty_assertions::assert_eq!(remaining.len(), 1);
        pretty_assertions::assert_eq!(remaining[0]["https://uri.etsi.org/ngsi-ld/datasetId"][0]["@id"], json!("urn:d2"));
    }

    #[test]
    fn an_attribute_whose_last_instance_expires_loses_its_key() {
        let mut entity = entity_with(json!([instance(Some("2025-01-01T00:00:00Z"), None)]));
        assert!(remove_expired_attribute_instances(&mut entity, cutoff()));
        assert!(entity[0].as_object().is_some_and(|obj| !obj.contains_key("https://example.org/speed")));
    }

    #[test]
    fn json_ld_keywords_are_preserved() {
        let mut entity = entity_with(json!([instance(Some("2025-01-01T00:00:00Z"), None)]));
        assert!(remove_expired_attribute_instances(&mut entity, cutoff()));
        pretty_assertions::assert_eq!(entity[0]["@id"], json!("urn:ngsi-ld:A"));
        pretty_assertions::assert_eq!(entity[0]["@type"], json!(["https://example.org/Vehicle"]));
    }

    #[test]
    fn a_non_array_member_is_preserved() {
        let mut entity = json!([{
            "@id": "urn:ngsi-ld:A",
            "https://example.org/note": "kept verbatim",
            "https://example.org/speed": [instance(Some("2025-01-01T00:00:00Z"), None)]
        }]);
        assert!(remove_expired_attribute_instances(&mut entity, cutoff()));
        pretty_assertions::assert_eq!(entity[0]["https://example.org/note"], json!("kept verbatim"));
    }

    #[test]
    fn an_unparseable_expiry_keeps_the_instance() {
        let mut entity = entity_with(json!([instance(Some("not a date"), None)]));
        assert!(!remove_expired_attribute_instances(&mut entity, cutoff()));
        pretty_assertions::assert_eq!(attribute_names(&entity), vec!["https://example.org/speed".to_string()]);
    }

    #[test]
    fn an_empty_payload_array_is_left_alone() {
        let mut entity = json!([]);
        assert!(!remove_expired_attribute_instances(&mut entity, cutoff()));
        pretty_assertions::assert_eq!(entity, json!([]));
    }
}
