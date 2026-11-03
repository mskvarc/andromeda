use crate::{
    attribute_strip::AttributeStrip,
    node_refusal::{NodeReadError, NodeRefusal, RefusedNode},
    request_body::{DecodeBounds, RequestBody},
    temporal::{
        plural_member_refusal::PluralMemberRefusal,
        written_temporal_entity::{WrittenScope, WrittenTemporalEntity},
    },
};
use andromeda_json_literal::piece::JsonPiece;
use andromeda_ngsi_ld_geometry::geometry_census::GeometryPositionLimit;
use andromeda_ngsi_ld_number::json_value::value_census::JsonValueLimit;
use andromeda_ngsi_ld_representation::compact_node::context_reading::ContextReading;
use serde_json::{Value, json, value::RawValue};
use std::num::NonZeroUsize;

/// The body decoded as a client's Temporal Evolution.
fn written(body: &Value) -> Result<WrittenTemporalEntity, serde_json::Error> {
    WrittenTemporalEntity::from_request_text(body.to_string().as_bytes(), ContextReading::keyword_spellings())
}

/// `attribute` as the entity `attribute` of a body, unfolded and written back.
fn unfolded(attribute: &Value) -> Value {
    let body = json!({"id": "urn:ngsi-ld:Vehicle:1", "type": "Vehicle", "attribute": attribute});
    let entity = written(&body).expect("the body decodes");
    serde_json::to_value(&entity).expect("the body writes")["attribute"].clone()
}

/// The refusal `attribute` is refused with.
fn refusal(attribute: &Value) -> PluralMemberRefusal {
    let body = json!({"id": "urn:ngsi-ld:Vehicle:1", "type": "Vehicle", "attribute": attribute});
    let raw = RawValue::from_string(body.to_string()).expect("raw JSON");
    let error = WrittenTemporalEntity::from_node(JsonPiece::new(&raw)).expect_err("the attribute is refused");
    match error {
        NodeReadError::Refused(RefusedNode {
            refusal: NodeRefusal::PluralMember(refusal),
            ..
        }) => *refusal,
        other @ (NodeReadError::Refused(_) | NodeReadError::Unreadable(_)) => panic!("not a plural member refusal: {other:?}"),
    }
}

/// Clause 4.5.9, EXAMPLE 1: each pair is the value and the instant it was
/// observed at.
#[test]
fn each_pair_becomes_the_instance_it_stands_for() {
    let attribute = json!({"type": "Property", "values": [["Joe Bloggs", "2022-08-09T18:25:02Z"], ["Bill Smith", "2022-08-10T18:25:02Z"]]});
    pretty_assertions::assert_eq!(
        unfolded(&attribute),
        json!([
            {"type": "Property", "value": "Joe Bloggs", "observedAt": "2022-08-09T18:25:02Z"},
            {"type": "Property", "value": "Bill Smith", "observedAt": "2022-08-10T18:25:02Z"}
        ])
    );
}

/// Clause 4.5.9 holds every family's pairs in its own member, the value of a
/// `LanguageProperty`, `JsonProperty` and `VocabProperty` wrapped in a
/// one-member object (EXAMPLES 2 to 7); each unfolds to the family's value
/// member holding the value alone.
#[test]
fn every_family_unfolds_into_its_own_value_member() {
    let instant = "2022-08-09T18:25:02Z";
    for (attribute, instance) in [
        (
            json!({"type": "Relationship", "objects": [[["urn:ngsi-ld:Device:1", "urn:ngsi-ld:Device:2"], instant]]}),
            json!({"type": "Relationship", "object": ["urn:ngsi-ld:Device:1", "urn:ngsi-ld:Device:2"], "observedAt": instant}),
        ),
        (
            json!({"type": "LanguageProperty", "languageMaps": [[{"languageMap": {"en": "yes", "fr": "oui"}}, instant]]}),
            json!({"type": "LanguageProperty", "languageMap": {"en": "yes", "fr": "oui"}, "observedAt": instant}),
        ),
        (
            json!({"type": "JsonProperty", "jsons": [[{"json": [{"id": "85a6cc52"}]}, instant]]}),
            json!({"type": "JsonProperty", "json": [{"id": "85a6cc52"}], "observedAt": instant}),
        ),
        (
            json!({"type": "VocabProperty", "vocabs": [[{"vocab": "Male"}, instant]]}),
            json!({"type": "VocabProperty", "vocab": "Male", "observedAt": instant}),
        ),
        (
            json!({"type": "ListProperty", "valueLists": [[["First", "Second"], instant]]}),
            json!({"type": "ListProperty", "valueList": ["First", "Second"], "observedAt": instant}),
        ),
        (
            json!({"type": "ListRelationship", "objectLists": [[["urn:ngsi-ld:Person:Alice"], instant]]}),
            // The model writes the normalized object list of Table 5.2.37-1.
            json!({"type": "ListRelationship", "objectList": [{"object": "urn:ngsi-ld:Person:Alice"}], "observedAt": instant}),
        ),
    ] {
        pretty_assertions::assert_eq!(unfolded(&attribute), json!([instance]));
    }
}

/// A geometry pair unfolds into the `GeoProperty` instance it stands for.
#[test]
fn a_geometry_pair_unfolds_into_a_geo_property_instance() {
    let attribute = json!({"type": "GeoProperty", "values": [[{"type": "Point", "coordinates": [1, 2]}, "2022-08-09T18:25:02Z"]]});
    let unfolded = unfolded(&attribute);
    pretty_assertions::assert_eq!(unfolded[0]["type"], json!("GeoProperty"));
    pretty_assertions::assert_eq!(unfolded[0]["value"]["type"], json!("Point"));
    pretty_assertions::assert_eq!(unfolded[0]["observedAt"], json!("2022-08-09T18:25:02Z"));
}

/// Clause 4.5.9 has the object "only contain" its plural member: an instance
/// member beside the pairs, which no element could carry, is refused rather
/// than copied into every instance.
#[test]
fn a_member_beside_the_pairs_is_refused() {
    for attribute in [
        json!({"type": "Property", "unitCode": "KMH", "values": [[60, "2020-08-01T12:00:00Z"]]}),
        json!({"type": "Property", "datasetId": "urn:ngsi-ld:Dataset:a", "values": [[60, "2020-08-01T12:00:00Z"]]}),
        json!({"type": "Property", "accuracy": {"type": "Property", "value": 1}, "values": [[60, "2020-08-01T12:00:00Z"]]}),
    ] {
        assert!(matches!(refusal(&attribute), PluralMemberRefusal::MemberBesidePairs { .. }), "{attribute}");
    }
}

/// Every element of the plural member is a pair: an instance object, a bare
/// value and a pair missing or adding an element are refused.
#[test]
fn an_element_that_is_no_pair_is_refused() {
    for element in [
        json!({"value": 60, "observedAt": "2020-08-01T12:00:00Z"}),
        json!(60),
        json!([60]),
        json!([60, "2020-08-01T12:00:00Z", "2020-08-01T13:00:00Z"]),
    ] {
        let attribute = json!({"type": "Property", "values": [element]});
        assert!(matches!(refusal(&attribute), PluralMemberRefusal::NotAPair { .. }), "{element}");
    }
    let attribute = json!({"type": "Property", "values": {"value": 60}});
    assert!(matches!(refusal(&attribute), PluralMemberRefusal::NotAnArray { .. }));
}

/// A wrapped family's value is refused unwrapped, wrapped under another
/// member, or wrapped beside a second member.
#[test]
fn a_wrapped_value_outside_its_one_member_object_is_refused() {
    for element in [
        json!({"en": "yes"}),
        json!({"value": {"en": "yes"}}),
        json!({"languageMap": {"en": "yes"}, "observedAt": "2020-08-01T12:00:00Z"}),
    ] {
        let attribute = json!({"type": "LanguageProperty", "languageMaps": [[element, "2020-08-01T12:00:00Z"]]});
        assert!(matches!(refusal(&attribute), PluralMemberRefusal::UnwrappedValue { .. }), "{element}");
    }
}

/// On the `observedAt` axis the instant is the instance's `observedAt`, which
/// is a date-time string.
#[test]
fn an_instant_that_is_no_string_is_refused() {
    let attribute = json!({"type": "Property", "values": [[60, null]]});
    assert!(matches!(refusal(&attribute), PluralMemberRefusal::InstantNotAString { .. }));
}

/// Clause 4.6.3 admits at most six fractional-second digits in the entity
/// `expiresAt` a write carries.
#[test]
fn an_entity_expires_at_beyond_six_fractional_digits_is_refused() {
    let body = |expires_at: &str| json!({"id": "urn:ngsi-ld:Vehicle:1", "type": "Vehicle", "expiresAt": expires_at});
    assert!(written(&body("2099-03-04T05:06:07.123456789Z")).is_err());
    assert!(written(&body("2099-03-04T05:06:07.1234567Z")).is_err());
    let kept = written(&body("2099-03-04T05:06:07.123456Z")).expect("six digits");
    pretty_assertions::assert_eq!(kept.expires_at.map(|instant| instant.timestamp_subsec_micros()), Some(123_456));
}

/// An object naming no family, or not holding its family's member, is an
/// instance and is read as one.
#[test]
fn an_object_without_its_family_plural_member_is_an_instance() {
    let attribute = json!({"type": "Property", "value": 60, "observedAt": "2020-08-01T12:00:00Z"});
    pretty_assertions::assert_eq!(unfolded(&attribute), json!([attribute]));
}

/// Clause 4.5.6: a reified Scope dates the paths it names and is read apart; a
/// Scope written as paths is carried by the entity.
#[test]
fn only_a_reified_scope_is_a_history() {
    let reified =
        written(&json!({"id": "urn:ngsi-ld:V:1", "scope": {"type": "Property", "value": "/Madrid", "observedAt": "2020-01-01T00:00:00Z"}})).expect("a body");
    assert!(matches!(reified.scope, Some(WrittenScope::Reified(ref instances)) if instances.len() == 1));
    let several = written(&json!({"id": "urn:ngsi-ld:V:1", "scope": [
        {"type": "Property", "value": "/Madrid", "observedAt": "2020-01-01T00:00:00Z"},
        {"type": "Property", "value": "/Company", "observedAt": "2020-01-02T00:00:00Z"}
    ]}))
    .expect("a body");
    assert!(matches!(several.scope, Some(WrittenScope::Reified(ref instances)) if instances.len() == 2));
    let paths = written(&json!({"id": "urn:ngsi-ld:V:1", "scope": ["/Madrid", "/Company"]})).expect("a body");
    assert!(matches!(paths.scope, Some(WrittenScope::Paths(_))));
}

/// Clause 5.2.2: the entity's own system times, which a producer may not
/// supply, are not read; a fragment names no `id` or `type` where the path
/// names the evolution.
#[test]
fn a_fragment_may_omit_the_identity_and_drops_system_times() {
    let entity = written(&json!({"createdAt": "2020-01-01T00:00:00Z", "speed": [{"type": "Property", "value": 1}]})).expect("a body");
    assert!(entity.id.is_none() && entity.type_.is_none());
    pretty_assertions::assert_eq!(entity.attrs.len(), 1);
    assert!(entity.has_attributes());
}

/// Clause 5.6.11.4: the attributes a proxied registration covers leave the
/// body; the rest is what remains.
#[test]
fn stripping_attributes_reports_what_remains() {
    let mut entity = written(&json!({"speed": [{"type": "Property", "value": 1}], "fuel": [{"type": "Property", "value": 2}]})).expect("a body");
    let speed = "speed".parse().expect("an attribute name");
    pretty_assertions::assert_eq!(entity.strip_attributes(&[speed]), AttributeStrip::Narrowed);
    pretty_assertions::assert_eq!(entity.attrs.len(), 1);
}

/// Clause 5.2.1: no member of a Temporal Evolution admits a JSON `null`, an
/// optional one included.
#[test]
fn a_json_null_on_an_optional_member_is_refused() {
    for member in ["id", "type", "expiresAt", "scope"] {
        let mut body =
            json!({"id": "urn:ngsi-ld:Vehicle:1", "type": "Vehicle", "speed": [{"type": "Property", "value": 1, "observedAt": "2024-01-01T00:00:00Z"}]});
        body[member] = Value::Null;
        assert!(written(&body).is_err(), "{member}");
    }
    let observed_null = json!({"id": "urn:ngsi-ld:Vehicle:1", "type": "Vehicle", "speed": [{"type": "Property", "value": 1, "observedAt": null}]});
    assert!(written(&observed_null).is_err());
}

/// The body decoded as a client's Temporal Evolution within an entity value
/// limit of `entity_values`.
fn written_within(body: &Value, entity_values: usize) -> WrittenTemporalEntity {
    let bounds = DecodeBounds::new(
        GeometryPositionLimit::new(NonZeroUsize::new(1_000_000).expect("a positive limit")),
        JsonValueLimit::new(NonZeroUsize::new(entity_values).expect("a positive limit")),
    );
    WrittenTemporalEntity::from_bounded_request_text(body.to_string().as_bytes(), ContextReading::keyword_spellings(), bounds).expect("the body decodes")
}

fn instances(count: usize) -> Value {
    Value::Array(vec![json!({"type": "Property", "value": 1, "observedAt": "2030-01-01T00:00:00Z"}); count])
}

/// Clause 6.3.2: an evolution past the entity value limit is cut one value
/// past it, which the count of the evolution then refuses.
#[test]
fn the_instances_of_an_evolution_past_the_limit_are_cut() {
    let body = json!({"id": "urn:ngsi-ld:Vehicle:1", "type": "Vehicle", "speed": instances(80)});
    assert!(written_within(&body, 10).attrs["speed"].len() < 80);
    pretty_assertions::assert_eq!(written_within(&body, 1_000).attrs["speed"].len(), 80);
    pretty_assertions::assert_eq!(written(&body).expect("the body decodes").attrs["speed"].len(), 80);
}

#[test]
fn the_pairs_of_a_simplified_attribute_past_the_limit_are_cut() {
    let pairs = Value::Array(vec![json!([1, "2030-01-01T00:00:00Z"]); 80]);
    let body = json!({"id": "urn:ngsi-ld:Vehicle:1", "type": "Vehicle", "speed": {"type": "Property", "values": pairs}});
    assert!(written_within(&body, 10).attrs["speed"].len() < 80);
    pretty_assertions::assert_eq!(written_within(&body, 1_000).attrs["speed"].len(), 80);
}

#[test]
fn the_attributes_of_an_evolution_past_the_limit_are_cut() {
    let mut body = json!({"id": "urn:ngsi-ld:Vehicle:1", "type": "Vehicle"});
    for index in 0..80 {
        body[format!("a{index}")] = instances(1);
    }
    assert!(written_within(&body, 10).attrs.len() < 80);
    pretty_assertions::assert_eq!(written_within(&body, 1_000).attrs.len(), 80);
}
