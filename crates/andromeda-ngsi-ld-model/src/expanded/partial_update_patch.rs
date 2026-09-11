//! The clause 5.5.8 Partial Update Patch, applied to one attribute instance.
//!
//! ETSI GS CIM 009 v1.9.1 clause 5.5.8 describes the algorithm a Partial
//! Attribute update (clause 5.6.4) and an Update Attributes (clause 5.6.2)
//! perform, "adapted from IETF RFC 7396 [16] [...] in order to observe the name
//! to URI expansion rules and the JSON-LD null processing". Three of its rules
//! decide what this module does:
//!
//! - "If the provided Fragment [...] contains members that do not appear within
//!   the target [...] those members are added to the target."
//! - "For each member of the Fragment contained by the target, the target member
//!   value is replaced by the value given in the Fragment." The replacement is
//!   wholesale — clause 5.5.12 contrasts the merge patch, which alone "merges
//!   the provided information with the existing information up to an arbitrary
//!   depth".
//! - "For each member of the Fragment, whose value is an NGSI-LD Null, contained
//!   by the target, the target member is deleted. [...] A datasetId cannot be
//!   deleted by setting it to the value `urn:ngsi-ld:null`."
//!
//! A member's value is an NGSI-LD Null in either of the two spellings clause
//! 4.5.0 gives it. Written against a non-reified member — an `observedAt`, a
//! `unitCode` — it is the bare URI, which expands to a plain node. Written
//! against a sub-attribute it is that sub-attribute's own deletion sentinel:
//! clause 4.5.2 states an NGSI-LD Null on the right-hand side of `value`
//! indicates "a deletion of the Property", and clauses 4.5.3, 4.5.18, 4.5.21 and
//! 4.5.22 say the same of `object`, `languageMap`, `valueList` and `objectList`.
//! Both spellings delete the member they stand on, so both are recognised here.
//!
//! The patch never rewrites a JSON-LD keyword. Clause 5.6.4.4 closes with "it is
//! not allowed to change the type of an Attribute", and `@type` is the only
//! keyword an expanded attribute instance carries.

use crate::expanded::null_sentinel::{instance_is_null_sentinel, member_is_null_sentinel};
use andromeda_ngsi_ld_vocabulary::expanded::properties;
use serde_json::Value;

/// Apply one expanded Attribute Fragment to one stored attribute instance.
///
/// `target` is the stored instance and `fragment` the patch; both are expanded
/// attribute instance nodes, so their members are the envelope members and
/// sub-attributes of clause 4.5.2.2.
pub fn patch_attribute_instance(target: &mut Value, fragment: &Value) {
    let (Some(target_members), Some(fragment_members)) = (target.as_object_mut(), fragment.as_object()) else {
        // A shape that is not two instance nodes has no member structure to
        // patch, so the fragment replaces the target outright.
        *target = fragment.clone(); // clone: the fragment is borrowed and the target takes ownership.
        return;
    };

    for (member, patched) in fragment_members {
        if member.starts_with('@') {
            continue;
        }
        if member_value_is_null(patched) {
            // Clause 5.5.8 closes its deletion rule with the one member it does
            // not reach: "A datasetId cannot be deleted by setting it to the
            // value urn:ngsi-ld:null." Removing it would move the instance to
            // the default one of clause 4.5.5, rewriting an identity the
            // fragment was never allowed to touch.
            if member != properties::DATASET_ID.as_str() {
                target_members.remove(member);
            }
            continue;
        }
        // clone: both the member name and its value are borrowed from the
        // fragment and the target stores them owned.
        target_members.insert(member.clone(), patched.clone());
    }
}

/// Whether an expanded member of an attribute instance is an NGSI-LD Null.
///
/// The member is array-wrapped by expansion, and clause 4.5.0 admits the
/// sentinel there in two shapes: the bare URI a non-reified member takes, and
/// the value-position sentinel of a reified sub-attribute.
fn member_value_is_null(member: &Value) -> bool {
    let Some(nodes) = member.as_array() else {
        return member_is_null_sentinel(member) || instance_is_null_sentinel(member);
    };
    nodes.len() == 1 && nodes.iter().all(|node| member_is_null_sentinel(node) || instance_is_null_sentinel(node))
}

#[cfg(test)]
mod tests {
    use crate::expanded::partial_update_patch::patch_attribute_instance;
    use serde_json::{Value, json};

    /// One stored Property instance carrying every member the cases patch.
    fn stored_property() -> Value {
        json!({
            "@type": ["https://uri.etsi.org/ngsi-ld/Property"],
            "https://uri.etsi.org/ngsi-ld/hasValue": [{"@value": 4}],
            "https://uri.etsi.org/ngsi-ld/unitCode": [{"@value": "C62"}],
            "https://uri.etsi.org/ngsi-ld/observedAt": [{"@value": "2020-09-09T16:40:00Z", "@type": "https://uri.etsi.org/ngsi-ld/DateTime"}],
            "https://example.org/hasWeatherObserved": [{
                "@type": ["https://uri.etsi.org/ngsi-ld/Relationship"],
                "https://uri.etsi.org/ngsi-ld/hasObject": [{"@id": "urn:ngsi-ld:Sensor:1"}]
            }]
        })
    }

    /// Clause 5.5.8 EXAMPLE 1: a member the Fragment names is replaced and a
    /// member it omits is left untouched.
    #[test]
    fn a_member_the_fragment_names_is_replaced_and_the_others_survive() {
        let mut target = stored_property();
        patch_attribute_instance(
            &mut target,
            &json!({
                "@type": ["https://uri.etsi.org/ngsi-ld/Property"],
                "https://uri.etsi.org/ngsi-ld/hasValue": [{"@value": 100}]
            }),
        );
        pretty_assertions::assert_eq!(target["https://uri.etsi.org/ngsi-ld/hasValue"], json!([{"@value": 100}]));
        pretty_assertions::assert_eq!(target["https://uri.etsi.org/ngsi-ld/unitCode"], json!([{"@value": "C62"}]));
        assert!(target.get("https://example.org/hasWeatherObserved").is_some());
    }

    /// Clause 5.5.8: a member the target does not carry is added.
    #[test]
    fn a_member_absent_from_the_target_is_added() {
        let mut target = json!({"@type": ["https://uri.etsi.org/ngsi-ld/Property"], "https://uri.etsi.org/ngsi-ld/hasValue": [{"@value": 4}]});
        patch_attribute_instance(&mut target, &json!({"https://uri.etsi.org/ngsi-ld/unitCode": [{"@value": "CEL"}]}));
        pretty_assertions::assert_eq!(target["https://uri.etsi.org/ngsi-ld/unitCode"], json!([{"@value": "CEL"}]));
    }

    /// Clause 5.5.8: a non-reified member set to the bare NGSI-LD Null of
    /// clause 4.5.0 is deleted, and nothing else is.
    #[test]
    fn a_non_reified_member_set_to_the_null_sentinel_is_deleted() {
        for member in ["https://uri.etsi.org/ngsi-ld/unitCode", "https://uri.etsi.org/ngsi-ld/observedAt"] {
            let mut target = stored_property();
            patch_attribute_instance(
                &mut target,
                &json!({
                    "@type": ["https://uri.etsi.org/ngsi-ld/Property"],
                    "https://uri.etsi.org/ngsi-ld/hasValue": [{"@value": 4}],
                    member: [{"@value": "urn:ngsi-ld:null"}]
                }),
            );
            assert!(target.get(member).is_none(), "{member} survived: {target}");
            pretty_assertions::assert_eq!(target["https://uri.etsi.org/ngsi-ld/hasValue"], json!([{"@value": 4}]));
            assert!(target.get("https://example.org/hasWeatherObserved").is_some(), "{target}");
        }
    }

    /// Clause 4.5.3: an NGSI-LD Null on the right-hand side of a sub-attribute's
    /// `object` deletes that Relationship rather than leaving it valueless.
    #[test]
    fn a_sub_attribute_whose_value_is_the_null_sentinel_is_deleted_whole() {
        let mut target = stored_property();
        patch_attribute_instance(
            &mut target,
            &json!({
                "@type": ["https://uri.etsi.org/ngsi-ld/Property"],
                "https://uri.etsi.org/ngsi-ld/hasValue": [{"@value": 4}],
                "https://example.org/hasWeatherObserved": [{
                    "@type": ["https://uri.etsi.org/ngsi-ld/Relationship"],
                    "https://uri.etsi.org/ngsi-ld/hasObject": [{"@id": "urn:ngsi-ld:null"}]
                }]
            }),
        );
        assert!(target.get("https://example.org/hasWeatherObserved").is_none(), "{target}");
        pretty_assertions::assert_eq!(target["https://uri.etsi.org/ngsi-ld/unitCode"], json!([{"@value": "C62"}]));
    }

    /// Clause 5.5.8: "A datasetId cannot be deleted by setting it to the value
    /// `urn:ngsi-ld:null`", so the sentinel leaves the instance identity alone.
    #[test]
    fn a_dataset_id_set_to_the_null_sentinel_is_not_deleted() {
        let mut target = json!({
            "@type": ["https://uri.etsi.org/ngsi-ld/Property"],
            "https://uri.etsi.org/ngsi-ld/hasValue": [{"@value": 4}],
            "https://uri.etsi.org/ngsi-ld/datasetId": [{"@id": "urn:ngsi-ld:Dataset:01"}]
        });
        patch_attribute_instance(&mut target, &json!({"https://uri.etsi.org/ngsi-ld/datasetId": [{"@id": "urn:ngsi-ld:null"}]}));
        pretty_assertions::assert_eq!(target["https://uri.etsi.org/ngsi-ld/datasetId"], json!([{"@id": "urn:ngsi-ld:Dataset:01"}]));
    }

    /// Clause 5.6.4.4: "it is not allowed to change the type of an Attribute",
    /// so the fragment's `@type` never overwrites the stored one.
    #[test]
    fn the_fragment_type_keyword_never_overwrites_the_stored_one() {
        let mut target = stored_property();
        patch_attribute_instance(&mut target, &json!({"@type": ["https://uri.etsi.org/ngsi-ld/GeoProperty"]}));
        pretty_assertions::assert_eq!(target["@type"], json!(["https://uri.etsi.org/ngsi-ld/Property"]));
    }

    /// Clause 5.5.8 replaces a member wholesale rather than merging into it, so
    /// a Property value object the Fragment names loses the keys it omits.
    #[test]
    fn a_value_object_is_replaced_rather_than_merged_into() {
        let mut target = json!({
            "@type": ["https://uri.etsi.org/ngsi-ld/Property"],
            "https://uri.etsi.org/ngsi-ld/hasValue": [{"https://example.org/street": [{"@value": "a"}], "https://example.org/city": [{"@value": "b"}]}]
        });
        patch_attribute_instance(
            &mut target,
            &json!({"https://uri.etsi.org/ngsi-ld/hasValue": [{"https://example.org/street": [{"@value": "c"}]}]}),
        );
        pretty_assertions::assert_eq!(
            target["https://uri.etsi.org/ngsi-ld/hasValue"],
            json!([{"https://example.org/street": [{"@value": "c"}]}])
        );
    }

    /// A list of more than one node is data rather than the "array with one
    /// single NGSI-LD Null" clause 4.5.0 reserves, so it replaces the member.
    #[test]
    fn a_member_holding_more_than_one_node_is_data_even_when_one_is_the_sentinel() {
        let mut target = stored_property();
        let patched = json!([{"@value": "urn:ngsi-ld:null"}, {"@value": 7}]);
        patch_attribute_instance(&mut target, &json!({"https://uri.etsi.org/ngsi-ld/hasValue": patched}));
        pretty_assertions::assert_eq!(
            target["https://uri.etsi.org/ngsi-ld/hasValue"],
            json!([{"@value": "urn:ngsi-ld:null"}, {"@value": 7}])
        );
    }
}
