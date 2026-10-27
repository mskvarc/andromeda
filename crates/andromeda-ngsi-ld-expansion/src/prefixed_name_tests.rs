//! A name the request's context reads as a compact IRI follows the clause
//! 4.6.2 grammar after its colon, whatever member writes it.

use crate::{
    expansion_fixture::{attribute, engine, entity, patch},
    normalization::Normalization,
};
use serde_json::json;

/// The refusal of the attribute name `ex:bad-name`.
const BAD_NAME_REFUSAL: &str =
    "'ex:bad-name' is not an attribute name: the part after the prefix must start with a letter and hold only letters, digits and '_'";

#[tokio::test]
async fn an_entity_attribute_whose_part_after_the_prefix_holds_a_hyphen_is_refused() {
    let refusal = entity(
        &engine(),
        json!({"@context": {"ex": "http://example.org/"}, "id": "urn:ngsi-ld:Vehicle:p", "type": "Vehicle", "ex:bad-name": {"type": "Property", "value": 1}}),
        Normalization::Entity,
    )
    .await
    .expect("the fixture expands")
    .refused_before_expansion();
    pretty_assertions::assert_eq!(refusal, BAD_NAME_REFUSAL);
}

#[tokio::test]
async fn an_entity_attribute_whose_part_after_the_prefix_starts_with_a_digit_is_refused() {
    let refusal = entity(
        &engine(),
        json!({"@context": {"ex": "http://example.org/"}, "id": "urn:ngsi-ld:Vehicle:p", "type": "Vehicle", "ex:42x": {"type": "Property", "value": 1}}),
        Normalization::Entity,
    )
    .await
    .expect("the fixture expands")
    .refused_before_expansion();
    pretty_assertions::assert_eq!(
        refusal,
        "'ex:42x' is not an attribute name: the part after the prefix must start with a letter and hold only letters, digits and '_'"
    );
}

#[tokio::test]
async fn an_entity_type_whose_part_after_the_prefix_holds_a_hyphen_is_refused() {
    let refusal = entity(
        &engine(),
        json!({"@context": {"ex": "http://example.org/"}, "id": "urn:ngsi-ld:Vehicle:p", "type": ["Vehicle", "ex:bad-type"]}),
        Normalization::Entity,
    )
    .await
    .expect("the fixture expands")
    .refused_before_expansion();
    pretty_assertions::assert_eq!(
        refusal,
        "'ex:bad-type' is not an entity type name: the part after the prefix must start with a letter and hold only letters, digits and '_'"
    );
}

#[tokio::test]
async fn a_sub_attribute_whose_part_after_the_prefix_breaks_the_grammar_is_refused() {
    let refusal = entity(
        &engine(),
        json!({
            "@context": {"ex": "http://example.org/"},
            "id": "urn:ngsi-ld:Vehicle:p",
            "type": "Vehicle",
            "speed": {"type": "Property", "value": 1, "ex:bad-name": {"type": "Property", "value": 2}}
        }),
        Normalization::Entity,
    )
    .await
    .expect("the fixture expands")
    .refused_before_expansion();
    pretty_assertions::assert_eq!(refusal, BAD_NAME_REFUSAL);
}

#[tokio::test]
async fn a_merge_fragment_attribute_whose_part_after_the_prefix_breaks_the_grammar_is_refused() {
    let refusal = patch(
        &engine(),
        json!({"@context": {"ex": "http://example.org/"}, "ex:bad-name": {"type": "Property", "value": 1}}),
        Normalization::MemberDeletions,
    )
    .await
    .expect("the fixture expands")
    .refused_before_expansion();
    pretty_assertions::assert_eq!(refusal, BAD_NAME_REFUSAL);
}

#[tokio::test]
async fn an_attribute_path_name_whose_part_after_the_prefix_breaks_the_grammar_is_refused() {
    let refusal = attribute(&engine(), "ex:bad-name", json!({"@context": {"ex": "http://example.org/"}, "value": 1}))
        .await
        .expect("the fixture expands")
        .refused_before_expansion();
    pretty_assertions::assert_eq!(refusal, BAD_NAME_REFUSAL);
}

/// `urn` is no term of the context, so `urn:x-y` is an absolute IRI and
/// clause 4.6.2 restricts only the names written as terms or compact IRIs.
#[tokio::test]
async fn a_well_formed_prefixed_name_and_an_absolute_iri_still_expand() {
    let document = entity(
        &engine(),
        json!({
            "@context": {"ex": "http://example.org/"},
            "id": "urn:ngsi-ld:Vehicle:p",
            "type": "urn:x-y",
            "ex:good_name": {"type": "Property", "value": 1}
        }),
        Normalization::Entity,
    )
    .await
    .expect("the fixture expands")
    .agreed();
    assert!(document.attributes().contains_key("http://example.org/good_name"));
}
