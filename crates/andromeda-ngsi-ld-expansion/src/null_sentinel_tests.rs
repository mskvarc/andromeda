//! The NGSI-LD Null (clause 4.5.0) in both its spellings, in every place a
//! write may carry it, through every write kind.

use crate::{
    expansion_fixture::{attribute, decoded_both_ways, engine, entity, fragment, patch},
    normalization::Normalization,
};
use andromeda_ngsi_ld_entity::{
    document::{
        EntityDocument,
        attribute_member::AttributeMember,
        slot::Slot,
        value_carrier::ValueCarrier,
        written_value::{ValueSlot, WrittenValue},
    },
    expanded_value::ExpandedValue,
    refusal::EntityRefusal,
    value_mode::FragmentMember,
};
use serde_json::{Value, json};

/// The two spellings of the NGSI-LD Null.
const SPELLINGS: [&str; 2] = ["urn:ngsi-ld:null", "https://uri.etsi.org/ngsi-ld/null"];

/// The expanded IRI of a term the core context does not define.
fn term(name: &str) -> String {
    format!("https://uri.etsi.org/ngsi-ld/default-context/{name}")
}

/// One instance of each family set to its deletion encoding (clauses 4.5.2.2,
/// 4.5.3.2, 4.5.18, 4.5.20.2, 4.5.21.2, 4.5.22.2, 4.5.24.2, 4.7), with the value
/// member each must read as removed.
fn deletions(sentinel: &str) -> Vec<(&'static str, Value, ValueCarrier)> {
    vec![
        ("deletedProperty", json!({"type": "Property", "value": sentinel}), ValueCarrier::Value),
        ("deletedGeoProperty", json!({"type": "GeoProperty", "value": sentinel}), ValueCarrier::Value),
        ("deletedRelationship", json!({"type": "Relationship", "object": sentinel}), ValueCarrier::Object),
        (
            "deletedLanguageProperty",
            json!({"type": "LanguageProperty", "languageMap": {"@none": sentinel}}),
            ValueCarrier::LanguageMap,
        ),
        ("deletedVocabProperty", json!({"type": "VocabProperty", "vocab": sentinel}), ValueCarrier::Vocab),
        ("deletedJsonProperty", json!({"type": "JsonProperty", "json": sentinel}), ValueCarrier::Json),
        (
            "deletedListProperty",
            json!({"type": "ListProperty", "valueList": [sentinel]}),
            ValueCarrier::ValueList,
        ),
        (
            "deletedListRelationship",
            json!({"type": "ListRelationship", "objectList": [sentinel]}),
            ValueCarrier::ObjectList,
        ),
        (
            "deletedWrappedListRelationship",
            json!({"type": "ListRelationship", "objectList": [{"object": sentinel}]}),
            ValueCarrier::ObjectList,
        ),
    ]
}

/// Assert every deletion of `deletions` reads as the removal of its value.
fn assert_removed(document: &EntityDocument, cases: &[(&'static str, Value, ValueCarrier)]) {
    for (name, _, carrier) in cases {
        let member = &document.attributes()[term(name).as_str()];
        pretty_assertions::assert_eq!(member.instances()[0].value(), &ValueSlot::Remove(*carrier), "{name}");
    }
}

/// The entity carrying every deletion of `cases`.
fn deleting_entity(cases: &[(&'static str, Value, ValueCarrier)]) -> Value {
    let mut body = json!({"id": "urn:ngsi-ld:Device:null", "type": "Device"});
    for (name, instance, _) in cases {
        body[*name] = instance.clone(); // clone: the cases are read again to check the result
    }
    body
}

/// Every family's deletion encoding, in both spellings, through a typed
/// entity, a typed merge fragment and an entity fragment held as JSON.
#[tokio::test]
async fn every_deletion_encoding_reads_as_the_removal_of_its_value() {
    let engine = engine();
    for sentinel in SPELLINGS {
        let cases = deletions(sentinel);
        let body = deleting_entity(&cases);
        let typed = entity(&engine, body.clone(), Normalization::Entity).await.expect("expands").agreed(); // clone: the body is expanded three ways
        assert_removed(&typed, &cases);
        let merged = patch(&engine, body.clone(), Normalization::MergeFragment).await.expect("expands").agreed(); // clone: the body is expanded three ways
        assert_removed(&merged, &cases);
        let appended = fragment(&engine, body, Normalization::Entity).await.expect("expands").agreed();
        assert_removed(&appended, &cases);
    }
}

/// Clause 5.5.12: a merge member that is `urn:ngsi-ld:null` removes the whole
/// member; any other write, and the IRI spelling the merge rewrite does not
/// keep, read it as a concise Property value deleting its value.
#[tokio::test]
async fn a_first_level_null_removes_the_member_of_a_merge_only() {
    let engine = engine();
    let [urn, iri] = SPELLINGS;
    let merged = patch(&engine, json!({"speed": urn}), Normalization::MergeFragment)
        .await
        .expect("expands")
        .agreed();
    pretty_assertions::assert_eq!(merged.attributes()[term("speed").as_str()], AttributeMember::Remove);
    let merged = patch(&engine, json!({"speed": iri}), Normalization::MergeFragment)
        .await
        .expect("expands")
        .agreed();
    pretty_assertions::assert_eq!(
        merged.attributes()[term("speed").as_str()].instances()[0].value(),
        &ValueSlot::Remove(ValueCarrier::Value)
    );
    for sentinel in SPELLINGS {
        let updated = fragment(&engine, json!({"speed": sentinel}), Normalization::Entity)
            .await
            .expect("expands")
            .agreed();
        pretty_assertions::assert_eq!(
            updated.attributes()[term("speed").as_str()].instances()[0].value(),
            &ValueSlot::Remove(ValueCarrier::Value)
        );
    }
}

/// Clauses 4.18, 4.22, 5.5.8: the entity's scope and expiry, and an
/// instance's observation time and unit, deleted by the Null.
#[tokio::test]
async fn members_beside_the_value_are_deleted_by_the_null() {
    let engine = engine();
    for sentinel in SPELLINGS {
        let document = fragment(
            &engine,
            json!({"scope": sentinel, "expiresAt": sentinel, "speed": {"value": 1, "observedAt": sentinel, "unitCode": sentinel, "valueType": sentinel}}),
            Normalization::MergeFragment,
        )
        .await
        .expect("expands")
        .agreed();
        pretty_assertions::assert_eq!(document.scope(), Some(&Slot::Remove));
        pretty_assertions::assert_eq!(document.expires_at(), Some(&Slot::Remove));
        let speed = &document.attributes()[term("speed").as_str()].instances()[0];
        pretty_assertions::assert_eq!(speed.observed_at(), Some(&Slot::Remove));
        pretty_assertions::assert_eq!(speed.unit_code(), Some(&Slot::Remove));
        pretty_assertions::assert_eq!(speed.value_type(), Some(&Slot::Remove));
    }
}

/// Clause 5.5.12: inside a merged structured value, a member set to the Null
/// is deleted.
#[tokio::test]
async fn a_nested_null_in_a_merged_structured_value_deletes_that_member() {
    let engine = engine();
    for sentinel in SPELLINGS {
        let document = patch(
            &engine,
            json!({"address": {"type": "Property", "value": {"street": sentinel, "city": "x"}}}),
            Normalization::MergeFragment,
        )
        .await
        .expect("expands")
        .agreed();
        let ValueSlot::Set(WrittenValue::Value(values)) = document.attributes()[term("address").as_str()].instances()[0].value() else {
            panic!("a value");
        };
        let [ExpandedValue::Node(node)] = values.as_slice() else {
            panic!("a node");
        };
        pretty_assertions::assert_eq!(node.members().get(term("street").as_str()), Some(&FragmentMember::Remove));
    }
}

/// Clauses 5.5.8 and 5.5.12: a `datasetId` is never deleted by the Null; both
/// expansions refuse it alike.
#[tokio::test]
async fn a_null_dataset_id_is_refused_both_ways() {
    let engine = engine();
    for sentinel in SPELLINGS {
        let refusal = fragment(
            &engine,
            json!({"speed": {"type": "Property", "value": 1, "datasetId": sentinel}}),
            Normalization::Entity,
        )
        .await
        .expect("expands")
        .refused();
        assert!(refusal.contains("datasetId"), "{refusal}");
    }
}

/// One instance of each value member holding the Null beside data, at the
/// top of the member and nested inside a structured value.
fn nulls_beside_data(sentinel: &str) -> Vec<(&'static str, Value)> {
    vec![
        ("property", json!({"type": "Property", "value": ["a", sentinel]})),
        ("relationship", json!({"type": "Relationship", "object": ["urn:ngsi-ld:Device:a", sentinel]})),
        ("vocabProperty", json!({"type": "VocabProperty", "vocab": ["Thing", sentinel]})),
        ("listProperty", json!({"type": "ListProperty", "valueList": ["a", sentinel]})),
        (
            "listRelationship",
            json!({"type": "ListRelationship", "objectList": ["urn:ngsi-ld:Device:a", sentinel]}),
        ),
        (
            "wrappedListRelationship",
            json!({"type": "ListRelationship", "objectList": [{"object": "urn:ngsi-ld:Device:a"}, {"object": sentinel}]}),
        ),
        ("nestedArray", json!({"type": "Property", "value": {"tags": ["x", sentinel]}})),
        ("nestedNode", json!({"type": "Property", "value": {"id": sentinel, "level": 1}})),
    ]
}

/// Clause 4.5.0: the Null deletes only alone, and "null cannot be used as a
/// value in NGSI-LD", so beside other values every write refuses it rather
/// than storing a document the stored-document decoder refuses to read.
#[tokio::test]
async fn the_null_beside_data_is_refused_by_every_write_kind() {
    let engine = engine();
    for sentinel in SPELLINGS {
        for (name, instance) in nulls_beside_data(sentinel) {
            let body = json!({"id": "urn:ngsi-ld:Device:null", "type": "Device", name: instance});
            let refusals = [
                entity(&engine, body.clone(), Normalization::Entity).await.expect("expands").refused(), // clone: the body is expanded four ways
                patch(&engine, body.clone(), Normalization::MergeFragment).await.expect("expands").refused(), // clone: the body is expanded four ways
                fragment(&engine, body, Normalization::Entity).await.expect("expands").refused(),
                attribute(&engine, name, instance).await.expect("expands").refused(),
            ];
            for refusal in refusals {
                assert!(refusal.contains("the NGSI-LD Null stands beside other values"), "{name} {sentinel}: {refusal}");
            }
        }
    }
}

/// What the write path refuses as the Null beside data, the stored-document
/// decoder of the entity crate refuses too: no written document holds a Null
/// it cannot read back.
#[tokio::test]
async fn the_stored_document_decoder_refuses_what_the_write_refuses() {
    let engine = engine();
    for sentinel in SPELLINGS {
        for (name, instance) in nulls_beside_data(sentinel) {
            let body = json!({"id": "urn:ngsi-ld:Device:null", "type": "Device", name: instance});
            let (decoded, canonical) = decoded_both_ways(&engine, body, Normalization::Entity).await.expect("expands");
            assert!(matches!(decoded, Err(EntityRefusal::NullBesideData { .. })), "{name} {sentinel}: {decoded:?}");
            assert!(canonical.is_err(), "{name} {sentinel}: {canonical:?}");
        }
    }
}

/// Clause 5.6.4: a partial update deletes an instance's value by its family's
/// deletion encoding.
#[tokio::test]
async fn an_attribute_fragment_deletes_its_value_with_the_null() {
    let engine = engine();
    for sentinel in SPELLINGS {
        let fragment = attribute(&engine, "speed", json!({"value": sentinel})).await.expect("expands").agreed();
        pretty_assertions::assert_eq!(fragment.instance().value(), &ValueSlot::Remove(ValueCarrier::Value));
    }
}
