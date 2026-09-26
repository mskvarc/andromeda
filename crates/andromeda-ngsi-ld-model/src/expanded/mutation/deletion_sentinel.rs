//! Members of an expanded payload that ask for a deletion (clauses 4.5.0 and
//! 4.18), and dropping them from a payload that has nothing to delete.

use crate::expanded::{
    node::expanded_object_mut,
    null_sentinel::{instance_is_null_sentinel, member_is_null_sentinel},
};
use andromeda_ngsi_ld_vocabulary::expanded::properties;
use serde_json::Value;

/// Whether an expanded `scope` member is the NGSI-LD Null of a deleted scope.
///
/// Clause 4.18 admits the sentinel only "in case of deleted scopes", so it is
/// the whole value of the member and never one entry beside a path; a member
/// that mixes the two is refused before a write reaches this point.
pub(crate) fn member_is_scope_deletion(member: &Value) -> bool {
    member.as_array().is_some_and(|values| values.len() == 1 && member_is_null_sentinel(&values[0]))
}

/// Whether an expanded member carries nothing but deletions.
///
/// Clause 4.5.0 spells a deleted member either as the bare NGSI-LD Null the
/// first level takes or as an instance whose value member is the sentinel, and
/// an attribute member is an array of instances. A member every instance of
/// which is a deletion asks for the whole member to go.
pub(crate) fn member_is_deletion(member: &Value) -> bool {
    match member {
        Value::Array(instances) => {
            !instances.is_empty()
                && instances
                    .iter()
                    .all(|instance| member_is_null_sentinel(instance) || instance_is_null_sentinel(instance))
        }
        Value::Null | Value::Bool(_) | Value::Number(_) | Value::String(_) | Value::Object(_) => {
            member_is_null_sentinel(member) || instance_is_null_sentinel(member)
        }
    }
}

/// What [`drop_deletion_sentinels`] found in the payload it was given.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum DroppedSentinels {
    /// At least one member asked for a deletion and was dropped.
    Dropped,
    /// The payload asked for no deletion and is unchanged.
    Unchanged,
}

/// Drop every member of an expanded entity payload that asks for a deletion.
///
/// This is clause 5.5.8 applied to a target that contains nothing: "for each
/// member of the Fragment, whose value is an NGSI-LD Null, contained by the
/// target, the target member is deleted" leaves a deletion the target does not
/// contain with nothing to do. Clause 5.6.8.4 is where that arises -- its
/// update mode runs clause 5.6.2, which supports NGSI-LD Nulls, over a payload
/// that creates the Entity when it does not exist yet -- and the sentinels are
/// dropped rather than stored, because clause 4.5.0 reserves the value for a
/// deletion and a stored one would be served back as data.
///
/// The entity `scope` member of clause 4.18 is dropped on the same terms: the
/// clause admits the sentinel there "only in case of deleted scopes".
pub fn drop_deletion_sentinels(entity: &mut Value) -> DroppedSentinels {
    let Some(entity_obj) = expanded_object_mut(entity) else {
        return DroppedSentinels::Unchanged;
    };
    let deletions: Vec<String> = entity_obj
        .iter()
        .filter(|(key, _)| !key.starts_with('@'))
        .filter(|(key, member)| {
            if key.as_str() == properties::SCOPE.as_str() {
                member_is_scope_deletion(member)
            } else {
                member_is_deletion(member)
            }
        })
        .map(|(key, _)| key.clone()) // clone: the removal outlives the borrow the scan holds
        .collect();
    if deletions.is_empty() {
        return DroppedSentinels::Unchanged;
    }
    for key in deletions {
        entity_obj.remove(&key);
    }
    DroppedSentinels::Dropped
}

#[cfg(test)]
mod tests {
    use crate::expanded::mutation::{
        deletion_sentinel::{DroppedSentinels, drop_deletion_sentinels},
        speed_entity::property_instance,
    };
    use andromeda_ngsi_ld_vocabulary::expanded::properties;
    use serde_json::json;

    /// The expanded IRI of the entity `scope` member (clause 4.18).
    fn scope_iri() -> &'static str {
        properties::SCOPE.as_str()
    }

    /// Clause 5.6.8.4 update mode runs clause 5.6.2 over a payload that also
    /// creates the Entity when it does not exist: on that branch the deletions
    /// have nothing to delete and are dropped, while everything else is stored.
    #[test]
    fn dropping_deletion_sentinels_keeps_the_data_and_removes_every_deletion() {
        let mut payload = json!([{
            "@id": "urn:ngsi-ld:Device:1",
            "@type": ["https://example.com/Device"],
            scope_iri(): [{"@value": "urn:ngsi-ld:null"}],
            "https://example.com/speed": [property_instance(&json!(10), None)],
            "https://example.com/gone": [property_instance(&json!("urn:ngsi-ld:null"), None)],
            "https://example.com/bare": [{"@value": "urn:ngsi-ld:null"}]
        }]);

        pretty_assertions::assert_eq!(drop_deletion_sentinels(&mut payload), DroppedSentinels::Dropped);

        pretty_assertions::assert_eq!(payload[0]["@id"], json!("urn:ngsi-ld:Device:1"));
        pretty_assertions::assert_eq!(
            payload[0]["https://example.com/speed"][0]["https://uri.etsi.org/ngsi-ld/hasValue"][0]["@value"],
            json!(10)
        );
        for dropped in [scope_iri(), "https://example.com/gone", "https://example.com/bare"] {
            assert!(payload[0].get(dropped).is_none(), "{dropped} is dropped: {payload}");
        }
    }

    /// A payload that asks for no deletion is left exactly as it was, so the
    /// create branch keeps using the preparation pass's own projection.
    #[test]
    fn dropping_deletion_sentinels_leaves_a_payload_without_one_unchanged() {
        let original = json!([{
            "@id": "urn:ngsi-ld:Device:1",
            scope_iri(): [{"@value": "/Madrid"}],
            "https://example.com/speed": [property_instance(&json!(10), None)]
        }]);
        let mut payload = original.clone();

        pretty_assertions::assert_eq!(drop_deletion_sentinels(&mut payload), DroppedSentinels::Unchanged);

        pretty_assertions::assert_eq!(payload, original);
    }

    /// An attribute whose instances mix a deletion with live data is not a
    /// deletion of the member: the surviving instances are what the create
    /// branch stores, so the member stays and the patch decides per instance.
    #[test]
    fn an_attribute_mixing_a_deletion_with_data_is_not_dropped_as_a_deletion() {
        let mut payload = json!([{
            "@id": "urn:ngsi-ld:Device:1",
            "https://example.com/speed": [
                property_instance(&json!("urn:ngsi-ld:null"), Some("urn:ngsi-ld:Dataset:A")),
                property_instance(&json!(20), Some("urn:ngsi-ld:Dataset:B"))
            ]
        }]);

        pretty_assertions::assert_eq!(drop_deletion_sentinels(&mut payload), DroppedSentinels::Unchanged);

        assert!(payload[0].get("https://example.com/speed").is_some(), "{payload}");
    }
}
