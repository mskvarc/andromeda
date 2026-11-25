//! A `valueType`, `objectType` or `vocab` names a type or a vocabulary term
//! by the absolute IRI it expands to, so one that expands to anything else is
//! refused on both expansion paths rather than stored as a name that reads back
//! as another (clause 4.6.2).

use crate::{
    expansion_fixture::{engine, entity},
    normalization::Normalization,
};
use andromeda_ngsi_ld_entity::{member_shape::MemberShape, refusal::EntityRefusal};
use andromeda_ngsi_ld_lexicon::reference::NgsiLdReferenceError;
use serde_json::{Value, json};

/// Values whose expansion under `{"ex": "http://example.org/"}` is no absolute
/// IRI: an IRI admits no `{` and no `%` that two hexadecimal digits do not
/// follow (IETF RFC 3987).
const UNEXPANDABLE_VALUES: [&str; 4] = ["ex:a{b", "ex:%", "urn:x:{", "https://example.org/{"];

/// The refusal of an entity whose attribute `name` is `attribute`.
async fn refusal_of(name: &str, attribute: Value) -> EntityRefusal {
    entity(
        &engine(),
        json!({"@context": {"ex": "http://example.org/"}, "id": "urn:ngsi-ld:Thing:r", "type": "Thing", name: attribute}),
        Normalization::Entity,
    )
    .await
    .expect("the fixture expands")
    .refusal()
}

/// Whether `refusal` refuses the `member` of an attribute as an expansion that
/// is no absolute IRI.
fn refuses_as_not_absolute(refusal: &EntityRefusal, expected: MemberShape) -> bool {
    matches!(refusal, EntityRefusal::Reference { member, source: NgsiLdReferenceError::NotAbsolute, .. } if *member == expected)
}

#[tokio::test]
async fn an_object_type_that_expands_to_no_iri_is_refused() {
    for value in UNEXPANDABLE_VALUES {
        let refusal = refusal_of("r", json!({"type": "Relationship", "object": "urn:ngsi-ld:Thing:o", "objectType": value})).await;
        assert!(refuses_as_not_absolute(&refusal, MemberShape::ObjectType), "{value}: {refusal:?}");
    }
}

#[tokio::test]
async fn a_vocab_that_expands_to_no_iri_is_refused() {
    for value in UNEXPANDABLE_VALUES {
        let refusal = refusal_of("v", json!({"type": "VocabProperty", "vocab": value})).await;
        assert!(refuses_as_not_absolute(&refusal, MemberShape::Vocab), "{value}: {refusal:?}");
    }
}

#[tokio::test]
async fn a_value_type_that_expands_to_no_iri_is_refused() {
    for value in UNEXPANDABLE_VALUES {
        let refusal = refusal_of("p", json!({"type": "Property", "value": 1, "valueType": value})).await;
        assert!(refuses_as_not_absolute(&refusal, MemberShape::ValueType), "{value}: {refusal:?}");
    }
}

#[tokio::test]
async fn an_object_type_and_a_vocab_naming_a_term_or_an_iri_still_expand() {
    let document = entity(
        &engine(),
        json!({
            "@context": {"ex": "http://example.org/"},
            "id": "urn:ngsi-ld:Thing:r",
            "type": "Thing",
            "r": {"type": "Relationship", "object": "urn:ngsi-ld:Thing:o", "objectType": ["ex:Building", "Building", "urn:x:y"]},
            "v": {"type": "VocabProperty", "vocab": ["ex:commercial", "https://example.org/a"]}
        }),
        Normalization::Entity,
    )
    .await
    .expect("the fixture expands")
    .agreed();
    assert_eq!(document.attributes().len(), 2);
}
