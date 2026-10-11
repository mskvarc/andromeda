use crate::forwarded_fragment::ForwardedFragment;
use andromeda_json_literal::JsonLiteral;
use andromeda_ngsi_ld_lexicon::spec_version::SpecVersion;
use andromeda_ngsi_ld_model::attribute::{member_reading::MemberReading, merge_member::MergeMember};
use serde_json::{Value, json};

/// The fragment `text` of the attribute `speed`, as a source conforming to
/// `conformance` receives it.
fn forwarded(text: &str, conformance: Option<SpecVersion>) -> Value {
    let written = JsonLiteral::admit(text).expect("a JSON text");
    let member = MergeMember::from_fragment_node("speed", written.piece()).expect("a fragment");
    serde_json::to_value(ForwardedFragment::new(&member, conformance)).expect("the fragment writes")
}

/// Clause 5.6.4: a fragment naming neither a `type` nor a value member is the
/// instance it updates, so its members are forwarded as the client wrote them
/// and never nested under a `value`.
#[test]
fn a_fragment_naming_no_family_is_forwarded_as_written() {
    let text = r#"{"speed": {"type": "Property", "value": 99, "source": {"type": "Property", "value": "Speedometer"}}}"#;
    pretty_assertions::assert_eq!(
        forwarded(text, None),
        json!({"speed": {"type": "Property", "value": 99, "source": {"type": "Property", "value": "Speedometer"}}})
    );
}

/// Clause 4.5.1: an instance whose value member implies its family is
/// written with the family in `type`.
#[test]
fn a_fragment_implying_its_family_is_written_with_its_type() {
    pretty_assertions::assert_eq!(
        forwarded(r#"{"value": 5, "unitCode": "KMH"}"#, None),
        json!({"type": "Property", "value": 5, "unitCode": "KMH"})
    );
}

/// Clause 5.6.4 leaves the value out of a fragment that changes only a
/// sub-attribute or a qualifier.
#[test]
fn a_fragment_without_its_value_is_forwarded_without_one() {
    pretty_assertions::assert_eq!(
        forwarded(r#"{"type": "Property", "unitCode": "CEL"}"#, None),
        json!({"type": "Property", "unitCode": "CEL"})
    );
}

/// Clause 5.5.12: a sub-attribute written as the NGSI-LD Null removes it, and
/// the Null travels as the Property whose value it is.
#[test]
fn a_removed_sub_attribute_is_forwarded_as_the_null_property() {
    pretty_assertions::assert_eq!(
        forwarded(r#"{"source": "urn:ngsi-ld:null"}"#, None),
        json!({"source": {"type": "Property", "value": "urn:ngsi-ld:null"}})
    );
}

/// Table 4.3.6.8-2: a 1.0 source reads no `observedAt`, `unitCode` or
/// `datasetId`, and none of them is written.
#[test]
fn a_1_0_source_receives_no_member_it_never_defined() {
    let text = r#"{"type": "Property", "value": 5, "observedAt": "2025-01-01T00:00:00Z", "unitCode": "KMH", "valueType": "Speed"}"#;
    pretty_assertions::assert_eq!(forwarded(text, Some(SpecVersion::V1_0)), json!({"type": "Property", "value": 5}));
}

/// Table 4.3.6.8-1: a `LanguageProperty` reaches a source below 1.4 as the
/// Property it specializes, its language map as the value.
#[test]
fn a_language_property_is_forwarded_as_a_property_below_1_4() {
    pretty_assertions::assert_eq!(
        forwarded(r#"{"type": "LanguageProperty", "languageMap": {"en": "fast"}}"#, Some(SpecVersion::V1_3)),
        json!({"type": "Property", "value": {"en": "fast"}})
    );
}

/// Table 4.3.6.8-1: a `ListRelationship` reaches a source below 1.8 as a
/// Relationship naming the first target of its list.
#[test]
fn a_list_relationship_keeps_its_first_target_below_1_8() {
    let text = r#"{"type": "ListRelationship", "objectList": [{"object": "urn:ngsi-ld:T:a"}, {"object": "urn:ngsi-ld:T:b"}]}"#;
    pretty_assertions::assert_eq!(
        forwarded(text, Some(SpecVersion::V1_4)),
        json!({"type": "Relationship", "object": "urn:ngsi-ld:T:a"})
    );
}

/// Table 4.3.6.8-1 notes 2 and 3: a source below 1.3 receives one instance of
/// a multi-attribute, the default one by preference.
#[test]
fn the_default_instance_alone_reaches_a_source_below_1_3() {
    let text = r#"[{"value": 1, "datasetId": "urn:ngsi-ld:Dataset:a"}, {"value": 2}]"#;
    pretty_assertions::assert_eq!(forwarded(text, Some(SpecVersion::V1_0)), json!({"type": "Property", "value": 2}));
    pretty_assertions::assert_eq!(
        forwarded(text, None),
        json!([{"type": "Property", "value": 1, "datasetId": "urn:ngsi-ld:Dataset:a"}, {"type": "Property", "value": 2}])
    );
}
