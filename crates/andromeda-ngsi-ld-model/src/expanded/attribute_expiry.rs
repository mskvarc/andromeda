//! Reading the instance-level `expiresAt` of an expanded payload (clause 4.22).
//!
//! An attribute instance may carry its own `expiresAt`, with the meaning the
//! Entity-level member has: once it passes, the instance is gone. Two readers
//! share this module so they cannot disagree about where the instant lives:
//! the mutation that removes past-due instances, and the write path that
//! records the earliest such instant on the entity row so an expiry sweep can
//! find candidates by index instead of by walking every payload.

use crate::expanded::node::expanded_object_ref;
use andromeda_ngsi_ld_vocabulary::expanded::properties;
use chrono::{DateTime, Utc};
use serde_json::Value;

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

#[cfg(test)]
mod tests {
    use crate::expanded::attribute_expiry::{instance_expires_at, min_attribute_expires_at};
    use chrono::{DateTime, Utc};
    use serde_json::{Value, json};

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
}
