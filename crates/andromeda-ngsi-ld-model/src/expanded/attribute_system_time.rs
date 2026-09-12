//! Clause 4.8 attribute-instance system times in the expanded payload.
//!
//! Clause 4.8 defines `createdAt` and `modifiedAt` for "the Entity, Property or
//! Relationship", and Table 5.2.14-1 includes them in a representation the
//! request asked for with `sysAttrs`. The Entity-level pair lives in store
//! columns; the attribute-instance pair lives in the stored expanded payload,
//! at the grain of one instance per `datasetId` (clause 4.5.5). This module
//! writes them on a mutation ([`stamp_attribute_instants`]) and removes them
//! from a representation that did not ask for `sysAttrs`
//! ([`strip_attribute_instants`]).
//!
//! A member is an attribute exactly when it is neither a JSON-LD keyword nor a
//! member NGSI-LD reserves for itself ([`is_reserved_member`]); the two
//! operations share that test so what a write stamps is exactly what a
//! non-`sysAttrs` read strips.

use crate::expanded::{
    dataset_id::get_instance_dataset_id,
    node::{expanded_object_mut, expanded_object_ref},
    reserved_member::is_reserved_member,
};
use andromeda_ngsi_ld_lexicon::attribute_name::AttributeName;
use andromeda_ngsi_ld_vocabulary::expanded::{classes, properties};
use chrono::{DateTime, SecondsFormat, Utc};
use serde_json::{Value, json};

/// The attribute name an expanded member key carries, when the key names an
/// attribute rather than a keyword or a reserved member.
fn attribute_name(key: &str) -> Option<AttributeName> {
    if key.starts_with('@') {
        return None;
    }
    let name = key.parse::<AttributeName>().ok()?;
    (!is_reserved_member(&name)).then_some(name)
}

/// The expanded representation of a `DateTime` system member (clause 4.8).
///
/// Rendered to microsecond precision, not the whole-second precision the
/// Entity-level rendering uses: clause 4.8 makes `createdAt` the instant an
/// attribute was entered, and `orderBy=name.createdAt` (clause 4.23.3 EXAMPLE 5)
/// can only reflect the order two attributes were created in when their stamps
/// distinguish creations less than a second apart. Sub-second RFC 3339 is a
/// valid NGSI-LD `DateTime`.
fn datetime_member(instant: DateTime<Utc>) -> Value {
    json!([{"@type": classes::DATE_TIME.as_str(), "@value": instant.to_rfc3339_opts(SecondsFormat::Micros, true)}])
}

/// A copy of an instance with its system times removed, for a content
/// comparison that decides whether a write actually changed the instance.
fn without_system_times(instance: &Value) -> Value {
    let mut copy = instance.clone(); // clone: the comparison needs an owned instance shorn of its system times
    if let Some(object) = copy.as_object_mut() {
        object.remove(properties::CREATED_AT.as_str());
        object.remove(properties::MODIFIED_AT.as_str());
    }
    copy
}

/// Stamp clause 4.8 `createdAt`/`modifiedAt` onto every attribute instance of
/// an expanded entity, given the entity's `previous` stored payload (or
/// [`Value::Null`] when the write creates the entity) and the write instant.
///
/// The rule is per instance, matched to its predecessor by `datasetId` (clause
/// 4.5.5):
///
/// - an instance with no predecessor is new: `createdAt` and `modifiedAt` both
///   take `now`;
/// - an instance whose content (its members other than the system times) is
///   unchanged from its predecessor keeps both of the predecessor's times, so a
///   restatement that leaves an instance untouched — an Append with
///   `noOverwrite`, say — does not move its `modifiedAt`;
/// - an instance whose content changed keeps the predecessor's `createdAt` and
///   takes `now` for `modifiedAt`.
///
/// Keeping the predecessor's `createdAt` is what lets it survive a
/// read-modify-write even when the operation replaces the whole instance
/// (clauses 5.6.2, 5.6.19), which drops the member the previous write stamped.
pub fn stamp_attribute_instants(entity: &mut Value, previous: &Value, now: DateTime<Utc>) {
    // The predecessor is read before the target is borrowed mutably, so it is
    // captured as an owned map rather than held as a borrow across the write.
    let previous_obj = expanded_object_ref(previous).cloned();
    let Some(object) = expanded_object_mut(entity) else {
        return;
    };

    let keys: Vec<String> = object.keys().filter(|key| attribute_name(key).is_some()).cloned().collect(); // clone: the write below mutates the map the keys index
    for key in keys {
        let previous_instances = previous_obj.as_ref().and_then(|previous| previous.get(&key)).and_then(Value::as_array);
        let Some(instances) = object.get_mut(&key).and_then(Value::as_array_mut) else {
            continue;
        };

        for instance in instances.iter_mut() {
            let dataset = get_instance_dataset_id(instance);
            let predecessor = previous_instances.and_then(|previous| previous.iter().find(|candidate| get_instance_dataset_id(candidate) == dataset));

            // The predecessor's stamps are copied verbatim rather than parsed and
            // re-rendered, so a `createdAt` stays byte-for-byte stable across every
            // later write and never drifts in format.
            let created = predecessor
                .and_then(|candidate| candidate.get(properties::CREATED_AT.as_str()).cloned()) // clone: the surviving stamp is owned by the rewritten instance
                .unwrap_or_else(|| datetime_member(now));
            let unchanged = predecessor.is_some_and(|predecessor| without_system_times(instance) == without_system_times(predecessor));
            let modified = if unchanged {
                predecessor
                    .and_then(|predecessor| predecessor.get(properties::MODIFIED_AT.as_str()).cloned()) // clone: the surviving stamp is owned by the rewritten instance
                    .unwrap_or_else(|| datetime_member(now))
            } else {
                datetime_member(now)
            };

            let Some(instance) = instance.as_object_mut() else {
                continue;
            };
            instance.insert(properties::CREATED_AT.as_str().to_string(), created);
            instance.insert(properties::MODIFIED_AT.as_str().to_string(), modified);
        }
    }
}

/// Remove clause 4.8 `createdAt`/`modifiedAt` from every attribute instance of
/// an expanded entity, for a representation the request did not ask to carry
/// `sysAttrs` (Table 5.2.14-1). The stored payload carries the members on every
/// instance, so a read that leaves them in place would serve them unasked.
pub fn strip_attribute_instants(entity: &mut Value) {
    let Some(object) = expanded_object_mut(entity) else {
        return;
    };
    let keys: Vec<String> = object.keys().filter(|key| attribute_name(key).is_some()).cloned().collect(); // clone: the removal below mutates the map the keys index
    for key in keys {
        let Some(instances) = object.get_mut(&key).and_then(Value::as_array_mut) else {
            continue;
        };
        for instance in instances.iter_mut() {
            if let Some(object) = instance.as_object_mut() {
                object.remove(properties::CREATED_AT.as_str());
                object.remove(properties::MODIFIED_AT.as_str());
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::expanded::attribute_system_time::{stamp_attribute_instants, strip_attribute_instants};
    use andromeda_ngsi_ld_vocabulary::expanded::properties;
    use chrono::{DateTime, Utc};
    use serde_json::{Value, json};

    /// An instant parsed from an RFC3339 string.
    fn at(value: &str) -> DateTime<Utc> {
        value.parse::<DateTime<Utc>>().expect("a valid instant")
    }

    /// One expanded Property instance carrying `value`, optionally tagged with a
    /// `datasetId`.
    fn property(value: i64, dataset: Option<&str>) -> Value {
        let mut instance = json!({
            "@type": ["https://uri.etsi.org/ngsi-ld/Property"],
            "https://uri.etsi.org/ngsi-ld/hasValue": [{"@value": value}]
        });
        if let Some(dataset) = dataset {
            instance["https://uri.etsi.org/ngsi-ld/datasetId"] = json!([{"@id": dataset}]);
        }
        instance
    }

    /// An expanded entity carrying `speed` with the given instances.
    fn entity(instances: &Value) -> Value {
        json!([{"@id": "urn:ngsi-ld:Device:1", "https://example.com/speed": instances}])
    }

    /// The `@value` of an instance's system member.
    fn member(entity: &Value, index: usize, member: &str) -> Option<String> {
        entity[0]["https://example.com/speed"][index][member][0]["@value"].as_str().map(str::to_string)
    }

    #[test]
    fn a_created_instance_takes_now_for_both_times() {
        let now = at("2024-02-03T04:05:06Z");
        let mut entity = entity(&json!([property(10, None)]));

        stamp_attribute_instants(&mut entity, &Value::Null, now);

        pretty_assertions::assert_eq!(
            member(&entity, 0, properties::CREATED_AT.as_str()).as_deref(),
            Some("2024-02-03T04:05:06.000000Z")
        );
        pretty_assertions::assert_eq!(
            member(&entity, 0, properties::MODIFIED_AT.as_str()).as_deref(),
            Some("2024-02-03T04:05:06.000000Z")
        );
    }

    #[test]
    fn a_changed_instance_keeps_created_and_moves_modified() {
        let previous = entity(&json!([{
            "@type": ["https://uri.etsi.org/ngsi-ld/Property"],
            "https://uri.etsi.org/ngsi-ld/hasValue": [{"@value": 10}],
            properties::CREATED_AT.as_str(): [{"@type": "https://uri.etsi.org/ngsi-ld/DateTime", "@value": "2020-01-01T00:00:00Z"}],
            properties::MODIFIED_AT.as_str(): [{"@type": "https://uri.etsi.org/ngsi-ld/DateTime", "@value": "2020-01-01T00:00:00Z"}]
        }]));
        let mut entity = entity(&json!([property(20, None)]));
        let now = at("2024-02-03T04:05:06Z");

        stamp_attribute_instants(&mut entity, &previous, now);

        pretty_assertions::assert_eq!(member(&entity, 0, properties::CREATED_AT.as_str()).as_deref(), Some("2020-01-01T00:00:00Z"));
        pretty_assertions::assert_eq!(
            member(&entity, 0, properties::MODIFIED_AT.as_str()).as_deref(),
            Some("2024-02-03T04:05:06.000000Z")
        );
    }

    #[test]
    fn an_unchanged_instance_keeps_both_times() {
        let previous = entity(&json!([{
            "@type": ["https://uri.etsi.org/ngsi-ld/Property"],
            "https://uri.etsi.org/ngsi-ld/hasValue": [{"@value": 10}],
            properties::CREATED_AT.as_str(): [{"@type": "https://uri.etsi.org/ngsi-ld/DateTime", "@value": "2020-01-01T00:00:00Z"}],
            properties::MODIFIED_AT.as_str(): [{"@type": "https://uri.etsi.org/ngsi-ld/DateTime", "@value": "2021-06-01T00:00:00Z"}]
        }]));
        let mut entity = entity(&json!([property(10, None)]));
        let now = at("2024-02-03T04:05:06Z");

        stamp_attribute_instants(&mut entity, &previous, now);

        pretty_assertions::assert_eq!(member(&entity, 0, properties::CREATED_AT.as_str()).as_deref(), Some("2020-01-01T00:00:00Z"));
        pretty_assertions::assert_eq!(
            member(&entity, 0, properties::MODIFIED_AT.as_str()).as_deref(),
            Some("2021-06-01T00:00:00Z"),
            "an untouched instance does not move its modifiedAt"
        );
    }

    #[test]
    fn each_dataset_instance_keeps_its_own_times() {
        let previous = entity(&json!([
            {
                "@type": ["https://uri.etsi.org/ngsi-ld/Property"],
                "https://uri.etsi.org/ngsi-ld/hasValue": [{"@value": 10}],
                "https://uri.etsi.org/ngsi-ld/datasetId": [{"@id": "urn:ngsi-ld:Dataset:A"}],
                properties::CREATED_AT.as_str(): [{"@type": "https://uri.etsi.org/ngsi-ld/DateTime", "@value": "2020-01-01T00:00:00Z"}],
                properties::MODIFIED_AT.as_str(): [{"@type": "https://uri.etsi.org/ngsi-ld/DateTime", "@value": "2020-01-01T00:00:00Z"}]
            }
        ]));
        // Dataset A is restated unchanged; dataset B is new.
        let mut entity = entity(&json!([
            property(10, Some("urn:ngsi-ld:Dataset:A")),
            property(30, Some("urn:ngsi-ld:Dataset:B"))
        ]));
        let now = at("2024-02-03T04:05:06Z");

        stamp_attribute_instants(&mut entity, &previous, now);

        pretty_assertions::assert_eq!(member(&entity, 0, properties::CREATED_AT.as_str()).as_deref(), Some("2020-01-01T00:00:00Z"));
        pretty_assertions::assert_eq!(
            member(&entity, 1, properties::CREATED_AT.as_str()).as_deref(),
            Some("2024-02-03T04:05:06.000000Z")
        );
    }

    #[test]
    fn a_strip_removes_every_instance_system_time() {
        let now = at("2024-02-03T04:05:06Z");
        let mut entity = entity(&json!([property(10, None)]));
        stamp_attribute_instants(&mut entity, &Value::Null, now);

        strip_attribute_instants(&mut entity);

        assert!(member(&entity, 0, properties::CREATED_AT.as_str()).is_none());
        assert!(member(&entity, 0, properties::MODIFIED_AT.as_str()).is_none());
    }
}
