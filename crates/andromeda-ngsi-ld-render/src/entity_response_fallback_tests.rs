//! The clause 4.3.6.8 version fallback, applied by the typed walk as it
//! writes Entity data: what each row of Tables 4.3.6.8-1, -2 and -3 leaves of
//! an entity, in every representation, including the members a rendered
//! document could not be adapted on -- sub-attributes, nested Linked
//! Entities, `showChanges` members, notified deletions and the `GeoJSON`
//! geometry.

use crate::{
    context::ResponseContext,
    entity_response::{EntityBody, EntityPayload},
    notification_entity::{NotificationEntity, StampedInstants, TargetSpelling},
    notified_changes::{HeldAttributes, NotifiedChanges},
    response::{ResponseHead, ResponseRenderer},
    version::{adaptation::VersionAdaptation, adapted_payload::AdaptedPayload, fallback::VersionFallback},
};
use andromeda_http_headers::{language::LanguageFilter, media_type::ResponseMediaType};
use andromeda_ngsi_ld_lexicon::spec_version::SpecVersion;
use andromeda_ngsi_ld_model::entity::Entity;
use andromeda_ngsi_ld_representation::response_format::ResponseFormat;
use iri_rs::IriBuf;
use pretty_assertions::assert_eq;
use serde_json::{Value, json};

/// An entity from its normalized document.
fn entity(document: Value) -> Entity {
    serde_json::from_value(document).expect("the entity decodes")
}

/// A context the `Link` header advertises, so no body carries one.
fn linked() -> ResponseContext {
    ResponseContext::Link(IriBuf::new("https://example.org/ctx.jsonld".to_owned()).expect("an IRI"))
}

/// The body and head an entity response writes for `entity` in `format`
/// under `media_type`, for the requested `version`, negotiated against
/// `filter`.
fn written(
    entity: &Entity,
    media_type: ResponseMediaType,
    format: ResponseFormat,
    version: Option<SpecVersion>,
    filter: Option<&LanguageFilter>,
) -> (Value, ResponseHead) {
    let mut body = Vec::new();
    let head = ResponseRenderer::new(media_type, linked())
        .with_format(format)
        .with_requested_version(version)
        .with_entities(EntityPayload::Single(EntityBody::new(entity, None)), filter)
        .render_into(&mut body)
        .expect("the entity is written");
    (serde_json::from_slice(&body).expect("the body is JSON"), head)
}

/// The JSON body of `entity` in `format` for `version`.
fn body(entity: &Entity, format: ResponseFormat, version: SpecVersion) -> Value {
    written(entity, ResponseMediaType::Json, format, Some(version), None).0
}

/// One attribute of `document`'s body in `format` for `version`.
fn attribute(document: Value, name: &str, format: ResponseFormat, version: SpecVersion) -> Value {
    body(&entity(document), format, version)[name].clone() // clone: the assertion reads one member of a body it does not keep
}

/// An entity carrying `members` beside its `id` and `type`.
fn carrying(members: Value) -> Value {
    let mut document = json!({"id": "urn:ngsi-ld:Building:1", "type": "Building"});
    if let (Some(document), Value::Object(members)) = (document.as_object_mut(), members) {
        document.extend(members);
    }
    document
}

#[test]
fn a_language_property_is_written_as_a_property_below_1_4() {
    let document = carrying(json!({"name": {"type": "LanguageProperty", "languageMap": {"en": "a", "de": "b"}}}));
    assert_eq!(
        attribute(document.clone(), "name", ResponseFormat::Normalized, SpecVersion::V1_3), // clone: read in three representations
        json!({"type": "Property", "value": {"en": "a", "de": "b"}})
    );
    assert_eq!(
        attribute(document.clone(), "name", ResponseFormat::Concise, SpecVersion::V1_3), // clone: read in three representations
        json!({"en": "a", "de": "b"})
    );
    assert_eq!(
        attribute(document, "name", ResponseFormat::KeyValues, SpecVersion::V1_3),
        json!({"en": "a", "de": "b"})
    );
}

#[test]
fn vocab_json_and_list_properties_are_written_as_properties_below_1_8() {
    let document = carrying(json!({
        "cat": {"type": "VocabProperty", "vocab": "Shop"},
        "cfg": {"type": "JsonProperty", "json": {"a": 1}},
        "l": {"type": "ListProperty", "valueList": [1, 2]}
    }));
    let normalized = body(&entity(document.clone()), ResponseFormat::Normalized, SpecVersion::V1_4); // clone: read in two representations
    assert_eq!(normalized["cat"], json!({"type": "Property", "value": "Shop"}));
    assert_eq!(normalized["cfg"], json!({"type": "Property", "value": {"a": 1}}));
    assert_eq!(normalized["l"], json!({"type": "Property", "value": [1, 2]}));
    let concise = body(&entity(document), ResponseFormat::Concise, SpecVersion::V1_4);
    assert_eq!(concise["cat"], json!("Shop"));
    assert_eq!(concise["cfg"], json!({"a": 1}));
    assert_eq!(concise["l"], json!([1, 2]));
}

/// Clause 4.5.2.3: a bare object carrying `type` reads back as a normalized
/// attribute, so the reformatted value keeps its `value` member.
#[test]
fn a_json_value_with_a_type_member_keeps_its_value_wrapper_in_concise() {
    let document = carrying(json!({"cfg": {"type": "JsonProperty", "json": {"type": "x"}}}));
    assert_eq!(
        attribute(document, "cfg", ResponseFormat::Concise, SpecVersion::V1_4),
        json!({"value": {"type": "x"}})
    );
}

/// Clause 5.2.5: a value that is a geometry names the Property in `type`, or
/// the typeless object would read back as a `GeoProperty`.
#[test]
fn a_reformatted_value_holding_a_geometry_keeps_the_property_type_in_concise() {
    let document = carrying(json!({"cfg": {"type": "JsonProperty", "json": {"type": "Point", "coordinates": [1, 2]}}}));
    assert_eq!(
        attribute(document, "cfg", ResponseFormat::Concise, SpecVersion::V1_4),
        json!({"type": "Property", "value": {"type": "Point", "coordinates": [1, 2]}})
    );
}

/// The normalized reformat keeps the first target as the Relationship's
/// `object`; the simplified rendering of either family is the same array of
/// URIs, which the reformat leaves as it is.
#[test]
fn a_list_relationship_is_written_as_its_first_target_below_1_8() {
    let document = carrying(json!({
        "rel": {"type": "ListRelationship", "objectList": [{"object": "urn:ngsi-ld:A:1"}, {"object": "urn:ngsi-ld:B:1"}], "objectType": "A"}
    }));
    assert_eq!(
        attribute(document.clone(), "rel", ResponseFormat::Normalized, SpecVersion::V1_4), // clone: read in three representations
        json!({"type": "Relationship", "object": "urn:ngsi-ld:A:1"})
    );
    assert_eq!(
        attribute(document.clone(), "rel", ResponseFormat::Concise, SpecVersion::V1_4), // clone: read in three representations
        json!({"object": "urn:ngsi-ld:A:1"})
    );
    assert_eq!(
        attribute(document, "rel", ResponseFormat::KeyValues, SpecVersion::V1_4),
        json!(["urn:ngsi-ld:A:1", "urn:ngsi-ld:B:1"])
    );
}

#[test]
fn property_members_are_removed_by_version() {
    let document = carrying(json!({
        "speed": {
            "type": "Property",
            "value": 5,
            "datasetId": "urn:ngsi-ld:Dataset:a",
            "observedAt": "2025-01-01T00:00:00Z",
            "unitCode": "KMH",
            "expiresAt": "2030-01-01T00:00:00Z",
            "valueType": "Speed"
        }
    }));
    assert_eq!(
        attribute(document.clone(), "speed", ResponseFormat::Normalized, SpecVersion::V1_0), // clone: read at two versions
        json!({"type": "Property", "value": 5})
    );
    assert_eq!(
        attribute(document, "speed", ResponseFormat::Normalized, SpecVersion::V1_8),
        json!({
            "type": "Property",
            "value": 5,
            "datasetId": "urn:ngsi-ld:Dataset:a",
            "observedAt": "2025-01-01T00:00:00Z",
            "unitCode": "KMH"
        })
    );
}

#[test]
fn relationship_members_are_removed_by_version() {
    let document = carrying(json!({
        "owner": {
            "type": "Relationship",
            "object": "urn:ngsi-ld:Person:1",
            "objectType": "Person",
            "observedAt": "2025-01-01T00:00:00Z",
            "datasetId": "urn:ngsi-ld:Dataset:a",
            "expiresAt": "2030-01-01T00:00:00Z"
        }
    }));
    assert_eq!(
        attribute(document.clone(), "owner", ResponseFormat::Normalized, SpecVersion::V1_0), // clone: read at two versions
        json!({"type": "Relationship", "object": "urn:ngsi-ld:Person:1"})
    );
    assert_eq!(
        attribute(document, "owner", ResponseFormat::Normalized, SpecVersion::V1_4),
        json!({
            "type": "Relationship",
            "object": "urn:ngsi-ld:Person:1",
            "observedAt": "2025-01-01T00:00:00Z",
            "datasetId": "urn:ngsi-ld:Dataset:a"
        })
    );
}

#[test]
fn multiple_instances_collapse_to_the_default_one_below_1_3() {
    let document = carrying(json!({
        "speed": [
            {"type": "Property", "value": 1, "datasetId": "urn:ngsi-ld:Dataset:a"},
            {"type": "Property", "value": 2},
            {"type": "Property", "value": 3, "datasetId": "urn:ngsi-ld:Dataset:b"}
        ]
    }));
    assert_eq!(
        attribute(document.clone(), "speed", ResponseFormat::Normalized, SpecVersion::V1_0), // clone: read in three representations
        json!({"type": "Property", "value": 2})
    );
    assert_eq!(
        attribute(document.clone(), "speed", ResponseFormat::Concise, SpecVersion::V1_0), // clone: read in three representations
        json!(2)
    );
    assert_eq!(attribute(document, "speed", ResponseFormat::KeyValues, SpecVersion::V1_0), json!(2));
}

/// Table 4.3.6.8-1 notes 2 and 3 leave the choice open without a default
/// instance; every representation keeps the first held one, whatever its
/// `datasetId` sorts as.
#[test]
fn without_a_default_instance_the_first_held_instance_is_kept() {
    let document = carrying(json!({
        "speed": [
            {"type": "Property", "value": 26, "datasetId": "urn:ngsi-ld:Dataset:z"},
            {"type": "Property", "value": 1, "datasetId": "urn:ngsi-ld:Dataset:a"}
        ]
    }));
    assert_eq!(
        attribute(document.clone(), "speed", ResponseFormat::Normalized, SpecVersion::V1_0), // clone: read in two representations
        json!({"type": "Property", "value": 26})
    );
    assert_eq!(attribute(document, "speed", ResponseFormat::KeyValues, SpecVersion::V1_0), json!(26));
}

/// Two instances without a `datasetId` both claim to be the default one; every
/// representation keeps the first held.
#[test]
fn of_two_default_instances_every_representation_keeps_the_first() {
    let document = carrying(json!({
        "speed": [
            {"type": "Property", "value": 1},
            {"type": "Property", "value": 2}
        ]
    }));
    assert_eq!(
        attribute(document.clone(), "speed", ResponseFormat::Normalized, SpecVersion::V1_0), // clone: read in two representations
        json!({"type": "Property", "value": 1})
    );
    assert_eq!(attribute(document, "speed", ResponseFormat::KeyValues, SpecVersion::V1_0), json!(1));
}

#[test]
fn instances_are_kept_from_1_3() {
    let document = carrying(json!({
        "speed": [
            {"type": "Property", "value": 26, "datasetId": "urn:ngsi-ld:Dataset:z"},
            {"type": "Property", "value": 1, "datasetId": "urn:ngsi-ld:Dataset:a"}
        ]
    }));
    assert_eq!(
        attribute(document.clone(), "speed", ResponseFormat::Normalized, SpecVersion::V1_3), // clone: read in two representations
        json!([
            {"type": "Property", "value": 26, "datasetId": "urn:ngsi-ld:Dataset:z"},
            {"type": "Property", "value": 1, "datasetId": "urn:ngsi-ld:Dataset:a"}
        ])
    );
    assert_eq!(
        attribute(document, "speed", ResponseFormat::KeyValues, SpecVersion::V1_3),
        json!({"dataset": {"urn:ngsi-ld:Dataset:z": 26, "urn:ngsi-ld:Dataset:a": 1}})
    );
}

#[test]
fn entity_level_members_follow_table_4_3_6_8_1() {
    let building = entity(json!({
        "id": "urn:ngsi-ld:Building:1",
        "type": ["Building", "Landmark"],
        "scope": "/Madrid",
        "expiresAt": "2030-01-01T00:00:00Z"
    }));
    assert_eq!(
        body(&building, ResponseFormat::Normalized, SpecVersion::V1_0),
        json!({"id": "urn:ngsi-ld:Building:1", "type": "Building"})
    );
    assert_eq!(
        body(&building, ResponseFormat::Normalized, SpecVersion::V1_3),
        json!({"id": "urn:ngsi-ld:Building:1", "type": ["Building", "Landmark"]})
    );
    assert_eq!(
        body(&building, ResponseFormat::Normalized, SpecVersion::V1_8),
        json!({"id": "urn:ngsi-ld:Building:1", "type": ["Building", "Landmark"], "scope": "/Madrid"})
    );
}

/// Tables 4.3.6.8-2 and -3 apply to every Property and Relationship, and a
/// sub-attribute is one (clauses 4.5.2 and 4.5.3).
#[test]
fn sub_attributes_are_adapted() {
    let document = carrying(json!({
        "speed": {
            "type": "Property",
            "value": 5,
            "caption": {"type": "LanguageProperty", "languageMap": {"en": "speed"}, "observedAt": "2025-01-01T00:00:00Z"},
            "readings": [
                {"type": "Property", "value": 1, "datasetId": "urn:ngsi-ld:Dataset:x"},
                {"type": "Property", "value": 2}
            ]
        }
    }));
    assert_eq!(
        attribute(document, "speed", ResponseFormat::Normalized, SpecVersion::V1_0),
        json!({
            "type": "Property",
            "value": 5,
            "caption": {"type": "Property", "value": {"en": "speed"}},
            "readings": {"type": "Property", "value": 2}
        })
    );
}

/// A Linked Entity nested into a Relationship is Entity data of the same
/// payload (Table 4.3.6.8-1).
#[test]
fn nested_linked_entities_are_adapted() {
    let document = carrying(json!({
        "owner": {
            "type": "Relationship",
            "object": "urn:ngsi-ld:Shop:1",
            "entity": {
                "id": "urn:ngsi-ld:Shop:1",
                "type": "Shop",
                "expiresAt": "2030-01-01T00:00:00Z",
                "category": {"type": "VocabProperty", "vocab": "Retail"}
            }
        }
    }));
    assert_eq!(
        attribute(document, "owner", ResponseFormat::Normalized, SpecVersion::V1_4)["entity"],
        json!({"id": "urn:ngsi-ld:Shop:1", "type": "Shop", "category": {"type": "Property", "value": "Retail"}})
    );
}

/// Clause 4.5.2: a Property's previous member is `previousValue`, which is
/// where the previous value of a family reformatted as one is written.
#[test]
fn the_previous_member_follows_the_written_family() {
    let document = carrying(json!({
        "street": {"type": "LanguageProperty", "languageMap": {"en": "new"}, "previousLanguageMap": {"en": "old"}},
        "stops": {"type": "ListRelationship", "objectList": ["urn:ngsi-ld:Stop:2"], "previousObjectList": ["urn:ngsi-ld:Stop:1"]}
    }));
    let normalized = body(&entity(document), ResponseFormat::Normalized, SpecVersion::V1_3);
    assert_eq!(
        normalized["street"],
        json!({"type": "Property", "value": {"en": "new"}, "previousValue": {"en": "old"}})
    );
    assert_eq!(
        normalized["stops"],
        json!({"type": "Relationship", "object": "urn:ngsi-ld:Stop:2", "previousObject": "urn:ngsi-ld:Stop:1"})
    );
}

/// The clause 4.15 negotiated Property is a Property, so Table 4.3.6.8-2
/// removes its `observedAt` below 1.3.
#[test]
fn a_negotiated_lang_property_loses_observed_at_below_1_3() {
    let building = entity(carrying(json!({
        "street": {"type": "LanguageProperty", "languageMap": {"en": "Main Street"}, "observedAt": "2025-01-01T00:00:00Z"}
    })));
    let filter = LanguageFilter::parse("en").expect("a filter");
    let (written, _) = written(
        &building,
        ResponseMediaType::Json,
        ResponseFormat::Normalized,
        Some(SpecVersion::V1_0),
        Some(&filter),
    );
    assert_eq!(written["street"], json!({"type": "Property", "value": "Main Street", "lang": "en"}));
}

/// Clause 4.5.2.3: once the fallback removed every member beside the value,
/// the concise Property is its bare value.
#[test]
fn concise_collapses_once_suppressed_members_are_gone() {
    let document = carrying(json!({"speed": {"type": "Property", "value": 5, "observedAt": "2025-01-01T00:00:00Z"}}));
    assert_eq!(attribute(document, "speed", ResponseFormat::Concise, SpecVersion::V1_0), json!(5));
}

/// Clause 4.5.16.1 with Table 4.3.6.8-1 note 2: the Feature's geometry is the
/// instance its `properties` keep.
#[test]
fn geojson_takes_the_geometry_of_the_kept_instance() {
    let building = entity(carrying(json!({
        "location": [
            {"type": "GeoProperty", "value": {"type": "Point", "coordinates": [1.0, 1.0]}, "datasetId": "urn:ngsi-ld:Dataset:a"},
            {"type": "GeoProperty", "value": {"type": "Point", "coordinates": [2.0, 2.0]}}
        ]
    })));
    let (feature, _) = written(&building, ResponseMediaType::GeoJson, ResponseFormat::Normalized, Some(SpecVersion::V1_0), None);
    assert_eq!(feature["geometry"], json!({"type": "Point", "coordinates": [2.0, 2.0]}));
    assert_eq!(
        feature["properties"]["location"],
        json!({"type": "GeoProperty", "value": {"type": "Point", "coordinates": [2.0, 2.0]}})
    );
}

/// Clause 4.3.6.8: the conformant version asks for nothing, so the body is
/// the one written without a preference, and the outcome says so.
#[test]
fn the_conformant_target_writes_the_unchanged_body() {
    let building = entity(carrying(json!({
        "scope": "/Madrid",
        "speed": {"type": "Property", "value": 5, "observedAt": "2025-01-01T00:00:00Z", "valueType": "Speed"}
    })));
    let (unprefixed, plain_head) = written(&building, ResponseMediaType::Json, ResponseFormat::Normalized, None, None);
    let (conformant, head) = written(
        &building,
        ResponseMediaType::Json,
        ResponseFormat::Normalized,
        Some(SpecVersion::CONFORMANT),
        None,
    );
    assert_eq!(conformant, unprefixed);
    assert_eq!(head.version_adaptation, VersionAdaptation::Unchanged);
    assert_eq!(plain_head.version_adaptation, VersionAdaptation::None);
    let (_, amended) = written(&building, ResponseMediaType::Json, ResponseFormat::Normalized, Some(SpecVersion::V1_8), None);
    assert_eq!(amended.version_adaptation, VersionAdaptation::Applied { version: SpecVersion::V1_8 });
}

/// The notified entity of a Subscription with `ngsildConformance`, written
/// with `changes`.
fn notified(entity: &Entity, changes: NotifiedChanges<'_>, format: ResponseFormat, target: SpecVersion) -> Value {
    let stamped = StampedInstants::default();
    let notified = NotificationEntity::new(entity, &stamped, format, TargetSpelling::AsStored)
        .with_changes(changes)
        .with_fallback(VersionFallback::new(target, AdaptedPayload::Entity));
    serde_json::to_value(notified).expect("the notified entity is written")
}

/// Clause 5.8.6 amends a notified deletion like any other member: a deleted
/// `LanguageProperty` sent to a 1.3 subscriber is a deleted Property, whose
/// NGSI-LD Null is the bare one (clause 4.5.0).
#[test]
fn a_notified_deletion_is_spelled_by_the_written_family() {
    let building = entity(carrying(json!({"street": {"type": "LanguageProperty", "languageMap": {"en": "Main Street"}}})));
    let deleted = NotifiedChanges {
        held: HeldAttributes::Deleted,
        deleted: None,
        previous: None,
    };
    assert_eq!(
        notified(&building, deleted, ResponseFormat::Normalized, SpecVersion::V1_3)["street"],
        json!({"type": "Property", "value": "urn:ngsi-ld:null"})
    );
    assert_eq!(
        notified(&building, deleted, ResponseFormat::KeyValues, SpecVersion::V1_3)["street"],
        json!("urn:ngsi-ld:null")
    );
}

/// Clause 5.8.6 with clause 4.5.2: the `showChanges` previous value a
/// notification reports follows the family the instance is written as.
#[test]
fn a_reported_previous_value_follows_the_written_family() {
    let building = entity(carrying(json!({"street": {"type": "LanguageProperty", "languageMap": {"en": "new"}}})));
    let before = entity(carrying(json!({"street": {"type": "LanguageProperty", "languageMap": {"en": "old"}}})));
    let changes = NotifiedChanges {
        held: HeldAttributes::Current,
        deleted: None,
        previous: Some(&before),
    };
    assert_eq!(
        notified(&building, changes, ResponseFormat::Normalized, SpecVersion::V1_3)["street"],
        json!({"type": "Property", "value": {"en": "new"}, "previousValue": {"en": "old"}})
    );
}
