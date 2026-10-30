//! What a relayed notification makes of the deletions its source reports
//! (clause 5.8.6), in every representation a local subscription may ask for,
//! and through the completion of a split entity.
//!
//! Clause 5.8.6 reports a deleted attribute with "only the name of the
//! attribute as key and the URI "urn:ngsi-ld:null" as value", unless a
//! `datasetId`, the system attributes or a previous value must be reported
//! with it; then the attribute is an object whose value member holds its
//! family's deletion encoding (clause 4.5.0). The first is the deletion of the
//! whole member, the second of the instance of its `datasetId`.

use crate::{
    received_entity::{NotifiedDeletion, NotifiedPart},
    relay_fixture::{offline_engine, received, relay_outcome, relayed, relayed_with_changes, written},
};
use andromeda_json_literal::JsonLiteral;
use andromeda_jsonld::context_spec::ContextSpec;
use andromeda_ngsi_ld_entity::document::attribute_member::AttributeMember;
use andromeda_ngsi_ld_representation::response_format::ResponseFormat;
use serde_json::{Value, json};

/// The NGSI-LD Null.
const NULL: &str = "urn:ngsi-ld:null";

/// The `datasetId` of the non-default instances.
const DATASET: &str = "urn:ngsi-ld:Dataset:gps";

/// The three representations a notification is written in (clause 5.8.6).
const FORMATS: [ResponseFormat; 3] = [ResponseFormat::Normalized, ResponseFormat::Concise, ResponseFormat::Simplified];

/// A `Vehicle` whose `attr` is `attr`, beside a `speed` it still holds.
fn vehicle(attr: &Value) -> Value {
    json!({"id": "urn:ngsi-ld:Vehicle:1", "type": "Vehicle", "attr": attr, "speed": {"type": "Property", "value": 5}})
}

/// The deletions `part` sets apart, by name and kind.
fn deletion_kinds(part: &NotifiedPart) -> Vec<String> {
    part.deletions
        .iter()
        .map(|deletion| match deletion {
            NotifiedDeletion::Member(name) => format!("member {name}"),
            NotifiedDeletion::Instance(name, _) => format!("instance {name}"),
        })
        .collect()
}

/// The default-context IRI of `attr`.
const ATTR: &str = "https://uri.etsi.org/ngsi-ld/default-context/attr";

/// Assert that `attr`, notified as the bare Null in some spelling, is relayed
/// as the bare Null in every representation, is set apart as the deletion of
/// the whole member, and is put back on the completed entity as that.
async fn assert_relayed_bare(attr: &Value) {
    for format in FORMATS {
        let relayed = relayed(&vehicle(attr), format).await;
        pretty_assertions::assert_eq!(relayed["attr"], json!(NULL), "{attr} in {format:?}: {relayed}");
    }
    let engine = offline_engine();
    let part = NotifiedPart::of(received(&engine, &vehicle(attr)).await).expect("the part is a version of the entity");
    pretty_assertions::assert_eq!(deletion_kinds(&part), [format!("member {ATTR}")], "{attr}");
    let completed = NotifiedPart::completed(part.version, part.deletions, part.previous);
    pretty_assertions::assert_eq!(completed.document.attributes().get(ATTR), Some(&AttributeMember::Remove), "{attr}");
    let normalized = written(&engine, &completed, ResponseFormat::Normalized, None)
        .await
        .expect("the completed entity writes");
    pretty_assertions::assert_eq!(normalized["attr"], json!(NULL), "{attr}: {normalized}");
}

/// Clause 5.8.6: a deleted attribute reported by the bare Null is relayed as
/// the bare Null in every representation, never reified into a Property whose
/// value is the Null, which would report only the default instance deleted.
#[tokio::test]
async fn a_bare_null_deletion_is_relayed_bare_in_every_format() {
    assert_relayed_bare(&json!(NULL)).await;
}

/// Clause 4.5.0 with JSON-LD 1.1 expansion: an array of the one Null and a
/// node reference to it expand to the same Null as the bare string, so each
/// reports the whole attribute deleted and is relayed as the bare Null.
#[tokio::test]
async fn every_spelling_expanding_to_the_null_is_relayed_as_the_bare_null() {
    for spelling in [json!([NULL]), json!({"@id": NULL}), json!({"id": NULL}), json!([{"@id": NULL}])] {
        assert_relayed_bare(&spelling).await;
    }
}

/// Clause 5.5.7: a context may alias `@id`, and the node reference to the
/// Null written under the alias is the bare Null as well.
#[tokio::test]
async fn a_null_reference_under_an_aliasing_context_is_relayed_as_the_bare_null() {
    let inbound = ContextSpec::from_body(JsonLiteral::admit(r#"{"ident": "@id"}"#).expect("the aliasing context is JSON"));
    for format in FORMATS {
        let relayed = relay_outcome(&vehicle(&json!({"ident": NULL})), &inbound, format, None)
            .await
            .expect("the notified entity is relayed");
        pretty_assertions::assert_eq!(relayed["attr"], json!(NULL), "{format:?}: {relayed}");
    }
}

/// A Relationship reported deleted by the bare Null, beside the
/// Relationships the entity holds, stays the bare Null: the Null names no
/// family (clause 5.8.6), so nothing reads it as a Property.
#[tokio::test]
async fn a_bare_null_deletion_of_a_relationship_is_never_relayed_as_a_property() {
    let notified = json!({
        "id": "urn:ngsi-ld:Vehicle:1",
        "type": "Vehicle",
        "owner": NULL,
        "driver": {"type": "Relationship", "object": "urn:ngsi-ld:Person:a"}
    });
    for format in FORMATS {
        let relayed = relayed(&notified, format).await;
        pretty_assertions::assert_eq!(relayed["owner"], json!(NULL), "{format:?}: {relayed}");
    }
}

/// Assert that `attr`, an instance notified deleted in the object form, is
/// relayed as `normalized`, `concise` and `simplified`, is set apart as the
/// deletion of one instance, and is relayed alike once the completion puts it
/// back.
async fn assert_relayed_object_form(attr: &Value, normalized: &Value, concise: &Value, simplified: &Value) {
    for (format, expected) in [
        (ResponseFormat::Normalized, normalized),
        (ResponseFormat::Concise, concise),
        (ResponseFormat::Simplified, simplified),
    ] {
        let relayed = relayed(&vehicle(attr), format).await;
        pretty_assertions::assert_eq!(&relayed["attr"], expected, "{attr} in {format:?}: {relayed}");
    }
    let engine = offline_engine();
    let part = NotifiedPart::of(received(&engine, &vehicle(attr)).await).expect("the part is a version of the entity");
    pretty_assertions::assert_eq!(deletion_kinds(&part), [format!("instance {ATTR}")], "{attr}");
    let completed = NotifiedPart::completed(part.version, part.deletions, part.previous);
    let completed = written(&engine, &completed, ResponseFormat::Normalized, None)
        .await
        .expect("the completed entity writes");
    pretty_assertions::assert_eq!(&completed["attr"], normalized, "{attr}: {completed}");
}

/// One test per family and instance, named after it: the instance a source
/// notifies deleted in the object form, then what each representation
/// relays of it.
macro_rules! object_form_deletions {
    ($($(#[$doc:meta])* $test:ident: $attr:expr => normalized $normalized:expr, concise $concise:expr, simplified $simplified:expr;)+) => {
        $(
            $(#[$doc])*
            #[tokio::test]
            async fn $test() {
                assert_relayed_object_form(&$attr, &$normalized, &$concise, &$simplified).await;
            }
        )+
    };
}

object_form_deletions! {
    /// Clause 4.5.2.2: a deleted Property holds the Null in `value`; alone it
    /// is the bare Null in the concise representation (clause 5.5.4).
    a_deleted_property:
        json!({"type": "Property", "value": NULL})
        => normalized json!({"type": "Property", "value": NULL}), concise json!(NULL), simplified json!(NULL);
    /// Clause 4.7: a deleted `GeoProperty` holds the Null in `value`.
    a_deleted_geo_property:
        json!({"type": "GeoProperty", "value": NULL})
        => normalized json!({"type": "GeoProperty", "value": NULL}), concise json!(NULL), simplified json!(NULL);
    /// Clause 4.5.3.2: a deleted Relationship holds the Null in `object`.
    a_deleted_relationship:
        json!({"type": "Relationship", "object": NULL})
        => normalized json!({"type": "Relationship", "object": NULL}), concise json!({"object": NULL}), simplified json!(NULL);
    /// Clause 4.5.18.2: a deleted `LanguageProperty` holds the language map
    /// holding the Null under `@none`.
    a_deleted_language_property:
        json!({"type": "LanguageProperty", "languageMap": {"@none": NULL}})
        => normalized json!({"type": "LanguageProperty", "languageMap": {"@none": NULL}}),
           concise json!({"languageMap": {"@none": NULL}}),
           simplified json!({"languageMap": {"@none": NULL}});
    /// Clause 4.5.0: `vocab` holds a URI, so a deleted `VocabProperty` holds
    /// the Null in it.
    a_deleted_vocab_property:
        json!({"type": "VocabProperty", "vocab": NULL})
        => normalized json!({"type": "VocabProperty", "vocab": NULL}), concise json!({"vocab": NULL}), simplified json!({"vocab": NULL});
    /// Clause 4.5.24.2 makes `json` a JSON object or an array of objects, so
    /// the Null string in it is never data and is the deletion.
    a_deleted_json_property:
        json!({"type": "JsonProperty", "json": NULL})
        => normalized json!({"type": "JsonProperty", "json": NULL}), concise json!({"json": NULL}), simplified json!({"json": NULL});
    /// Clause 4.5.21.2: a deleted `ListProperty` holds the array of the one
    /// Null in `valueList`.
    a_deleted_list_property:
        json!({"type": "ListProperty", "valueList": [NULL]})
        => normalized json!({"type": "ListProperty", "valueList": [NULL]}), concise json!({"valueList": [NULL]}), simplified json!([NULL]);
    /// Clause 4.5.22.2: a deleted `ListRelationship` holds the array of the
    /// one Null in `objectList`.
    a_deleted_list_relationship:
        json!({"type": "ListRelationship", "objectList": [NULL]})
        => normalized json!({"type": "ListRelationship", "objectList": [NULL]}),
           concise json!({"objectList": [NULL]}),
           simplified json!([NULL]);
    /// Clause 5.8.6: a deleted instance of a `datasetId` is notified as an
    /// object carrying it, and clause 4.5.4 writes the `datasetId` of the
    /// simplified representation in the `dataset` map.
    a_deleted_property_of_a_dataset:
        json!({"type": "Property", "value": NULL, "datasetId": DATASET})
        => normalized json!({"type": "Property", "value": NULL, "datasetId": DATASET}),
           concise json!({"value": NULL, "datasetId": DATASET}),
           simplified json!({"dataset": {DATASET: NULL}});
    /// Clause 4.5.2.3: the typeless object of a deleted `GeoProperty` would
    /// read back as a Property, so the concise one names its type.
    a_deleted_geo_property_of_a_dataset:
        json!({"type": "GeoProperty", "value": NULL, "datasetId": DATASET})
        => normalized json!({"type": "GeoProperty", "value": NULL, "datasetId": DATASET}),
           concise json!({"type": "GeoProperty", "value": NULL, "datasetId": DATASET}),
           simplified json!({"dataset": {DATASET: NULL}});
    /// Clause 5.8.6 with clause 4.5.3.2.
    a_deleted_relationship_of_a_dataset:
        json!({"type": "Relationship", "object": NULL, "datasetId": DATASET})
        => normalized json!({"type": "Relationship", "object": NULL, "datasetId": DATASET}),
           concise json!({"object": NULL, "datasetId": DATASET}),
           simplified json!({"dataset": {DATASET: NULL}});
    /// Clause 5.8.6 with clause 4.5.18.2.
    a_deleted_language_property_of_a_dataset:
        json!({"type": "LanguageProperty", "languageMap": {"@none": NULL}, "datasetId": DATASET})
        => normalized json!({"type": "LanguageProperty", "languageMap": {"@none": NULL}, "datasetId": DATASET}),
           concise json!({"languageMap": {"@none": NULL}, "datasetId": DATASET}),
           simplified json!({"dataset": {DATASET: {"languageMap": {"@none": NULL}}}});
    /// Clause 5.8.6 with clause 4.5.20.2.
    a_deleted_vocab_property_of_a_dataset:
        json!({"type": "VocabProperty", "vocab": NULL, "datasetId": DATASET})
        => normalized json!({"type": "VocabProperty", "vocab": NULL, "datasetId": DATASET}),
           concise json!({"vocab": NULL, "datasetId": DATASET}),
           simplified json!({"dataset": {DATASET: {"vocab": NULL}}});
    /// Clause 5.8.6 with clause 4.5.24.2.
    a_deleted_json_property_of_a_dataset:
        json!({"type": "JsonProperty", "json": NULL, "datasetId": DATASET})
        => normalized json!({"type": "JsonProperty", "json": NULL, "datasetId": DATASET}),
           concise json!({"json": NULL, "datasetId": DATASET}),
           simplified json!({"dataset": {DATASET: {"json": NULL}}});
    /// Clause 5.8.6 with clause 4.5.21.2.
    a_deleted_list_property_of_a_dataset:
        json!({"type": "ListProperty", "valueList": [NULL], "datasetId": DATASET})
        => normalized json!({"type": "ListProperty", "valueList": [NULL], "datasetId": DATASET}),
           concise json!({"valueList": [NULL], "datasetId": DATASET}),
           simplified json!({"dataset": {DATASET: [NULL]}});
    /// Clause 5.8.6 with clause 4.5.22.2.
    a_deleted_list_relationship_of_a_dataset:
        json!({"type": "ListRelationship", "objectList": [NULL], "datasetId": DATASET})
        => normalized json!({"type": "ListRelationship", "objectList": [NULL], "datasetId": DATASET}),
           concise json!({"objectList": [NULL], "datasetId": DATASET}),
           simplified json!({"dataset": {DATASET: [NULL]}});
}

/// Clause 5.8.6, second case: under `sysAttrs` a deleted attribute is an
/// object carrying its system attributes (clause 4.8), which the relay keeps
/// in the representations that carry them.
#[tokio::test]
async fn a_deletion_reported_with_its_system_attributes_keeps_them() {
    let attr = json!({
        "type": "Property",
        "value": NULL,
        "createdAt": "2026-01-01T00:00:00Z",
        "modifiedAt": "2026-01-02T00:00:00Z",
        "deletedAt": "2026-01-03T00:00:00Z"
    });
    let times = json!({"createdAt": "2026-01-01T00:00:00Z", "modifiedAt": "2026-01-02T00:00:00Z", "deletedAt": "2026-01-03T00:00:00Z"});
    let normalized = relayed(&vehicle(&attr), ResponseFormat::Normalized).await;
    let concise = relayed(&vehicle(&attr), ResponseFormat::Concise).await;
    for (written, format) in [(&normalized, "normalized"), (&concise, "concise")] {
        for member in ["createdAt", "modifiedAt", "deletedAt"] {
            pretty_assertions::assert_eq!(written["attr"][member], times[member], "{format}: {written}");
        }
        pretty_assertions::assert_eq!(written["attr"]["value"], json!(NULL), "{format}: {written}");
    }
}

/// Clause 5.8.6, third case: under `showChanges` a deleted attribute of each
/// family is an object carrying its family's previous member, which survives
/// the completion of a split part beside the deletion.
#[tokio::test]
async fn every_family_deletion_keeps_its_previous_value_through_the_completion() {
    let engine = offline_engine();
    for attr in [
        json!({"type": "Property", "value": NULL, "previousValue": 4}),
        json!({"type": "GeoProperty", "value": NULL, "previousValue": {"type": "Point", "coordinates": [1, 2]}}),
        json!({"type": "Relationship", "object": NULL, "previousObject": "urn:ngsi-ld:Person:a"}),
        json!({"type": "LanguageProperty", "languageMap": {"@none": NULL}, "previousLanguageMap": {"en": "a"}}),
        json!({"type": "VocabProperty", "vocab": NULL, "previousVocab": "Tower"}),
        json!({"type": "JsonProperty", "json": NULL, "previousJson": {"a": 1}}),
        json!({"type": "ListProperty", "valueList": [NULL], "previousValueList": [1, 2]}),
        json!({"type": "ListRelationship", "objectList": [NULL], "previousObjectList": [{"object": "urn:ngsi-ld:Person:a"}]}),
        json!({"type": "Property", "value": NULL, "previousValue": 4, "datasetId": DATASET}),
    ] {
        let notified = received(&engine, &vehicle(&attr)).await;
        let relayed = relayed_with_changes(&engine, &notified, ResponseFormat::Normalized).await;
        pretty_assertions::assert_eq!(relayed["attr"], attr, "{relayed}");
        let part = NotifiedPart::of(notified).expect("the part is a version of the entity");
        let completed = NotifiedPart::completed(part.version, part.deletions, part.previous);
        let relayed = relayed_with_changes(&engine, &completed, ResponseFormat::Normalized).await;
        pretty_assertions::assert_eq!(relayed["attr"], attr, "{relayed}");
        pretty_assertions::assert_eq!(relayed["speed"], json!({"type": "Property", "value": 5}));
    }
}

/// Clause 4.5.0 gives each family one deletion encoding, in its own value
/// member: the Null under another family's value member, or a language map
/// written as the bare Null, encodes nothing and refuses its entity.
#[tokio::test]
async fn a_deletion_spelled_in_another_familys_value_member_is_refused() {
    for attr in [
        json!({"type": "VocabProperty", "value": NULL}),
        json!({"type": "JsonProperty", "value": NULL}),
        json!({"type": "LanguageProperty", "value": NULL}),
        json!({"type": "LanguageProperty", "languageMap": NULL}),
        json!({"type": "Relationship", "value": NULL}),
    ] {
        for format in FORMATS {
            let outcome = relay_outcome(&vehicle(&attr), &ContextSpec::core_only(), format, None).await;
            assert!(outcome.is_err(), "{attr} in {format:?}: {outcome:?}");
        }
    }
}
