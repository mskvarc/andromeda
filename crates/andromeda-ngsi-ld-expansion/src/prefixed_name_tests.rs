//! A name the request's context reads as a compact IRI follows the clause
//! 4.6.2 grammar after its colon, whatever member writes it.

use crate::{
    error::ExpansionError,
    expansion_fixture::{attribute, engine, entity, patch},
    normalization::Normalization,
};
use andromeda_ngsi_ld_entity::{name_refusal::NameRefusal, refusal::EntityRefusal};
use serde_json::json;

/// Attribute names whose expansion under `{"ex": "http://example.org/"}` is no
/// absolute IRI: an IRI admits no `{`, `"` or `%` that two hexadecimal digits
/// do not follow (IETF RFC 3987), and `urn` and `https` are no terms.
const UNEXPANDABLE_NAMES: [&str; 6] = ["ex:a{b", "ex:{", "ex:%", "ex:\"", "urn:x:{", "https://example.org/{"];

/// Whether `refusal` refuses `name` as an attribute name that expands to no
/// IRI.
fn refuses_unexpandable(refusal: &ExpansionError, name: &str) -> bool {
    matches!(refusal, ExpansionError::Refused(EntityRefusal::Name(NameRefusal::UnexpandedAttributeName { attribute })) if attribute.as_str() == name)
}

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

/// Expansion would drop an attribute whose name expands to no IRI, so the
/// write is refused instead (clause 4.6.2).
#[tokio::test]
async fn an_entity_attribute_whose_name_expands_to_no_iri_is_refused() {
    for name in UNEXPANDABLE_NAMES {
        let refusal = entity(
            &engine(),
            json!({"@context": {"ex": "http://example.org/"}, "id": "urn:ngsi-ld:Vehicle:p", "type": "Vehicle", name: {"type": "Property", "value": 1}}),
            Normalization::Entity,
        )
        .await
        .expect("the fixture expands")
        .refusal_before_expansion();
        assert!(refuses_unexpandable(&refusal, name), "{name}: {refusal:?}");
    }
}

#[tokio::test]
async fn a_sub_attribute_whose_name_expands_to_no_iri_is_refused() {
    for name in UNEXPANDABLE_NAMES {
        let refusal = entity(
            &engine(),
            json!({
                "@context": {"ex": "http://example.org/"},
                "id": "urn:ngsi-ld:Vehicle:p",
                "type": "Vehicle",
                "speed": {"type": "Property", "value": 1, name: {"type": "Property", "value": 2}}
            }),
            Normalization::Entity,
        )
        .await
        .expect("the fixture expands")
        .refusal_before_expansion();
        assert!(refuses_unexpandable(&refusal, name), "{name}: {refusal:?}");
    }
}

#[tokio::test]
async fn a_merge_fragment_attribute_whose_name_expands_to_no_iri_is_refused() {
    for name in UNEXPANDABLE_NAMES {
        let refusal = patch(
            &engine(),
            json!({"@context": {"ex": "http://example.org/"}, name: {"type": "Property", "value": 1}}),
            Normalization::MemberDeletions,
        )
        .await
        .expect("the fixture expands")
        .refusal_before_expansion();
        assert!(refuses_unexpandable(&refusal, name), "{name}: {refusal:?}");
    }
}

#[tokio::test]
async fn an_attribute_path_name_that_expands_to_no_iri_is_refused() {
    for name in UNEXPANDABLE_NAMES {
        let refusal = attribute(&engine(), name, json!({"@context": {"ex": "http://example.org/"}, "value": 1}))
            .await
            .expect("the fixture expands")
            .refusal_before_expansion();
        assert!(refuses_unexpandable(&refusal, name), "{name}: {refusal:?}");
    }
}

/// Whether `refusal` refuses the `objectType` `object_type` of the attribute
/// `attribute` as an entity type name that breaks the grammar after its prefix.
fn refuses_prefixed_object_type(refusal: &ExpansionError, attribute: &str, object_type: &str) -> bool {
    matches!(
        refusal,
        ExpansionError::Refused(EntityRefusal::Name(NameRefusal::PrefixedObjectType { attribute: refused, object_type: written }))
            if refused.as_str() == attribute && written.as_str() == object_type
    )
}

/// An `objectType` names entity types (clause 4.5.3.2), so it is held to the
/// grammar an entity type is, on a Relationship, a `ListRelationship` and a
/// Relationship written as a sub-attribute.
#[tokio::test]
async fn an_object_type_whose_part_after_the_prefix_breaks_the_grammar_is_refused() {
    let cases = [
        (
            "r",
            json!({"r": {"type": "Relationship", "object": "urn:ngsi-ld:A:1", "objectType": ["Building", "ex:bad-type"]}}),
        ),
        (
            "l",
            json!({"l": {"type": "ListRelationship", "objectList": ["urn:ngsi-ld:A:1"], "objectType": "ex:42x"}}),
        ),
        (
            "s",
            json!({"p": {"type": "Property", "value": 1, "s": {"type": "Relationship", "object": "urn:ngsi-ld:A:1", "objectType": "ex:bad-type"}}}),
        ),
    ];
    for (attribute, members) in cases {
        let mut body = json!({"@context": {"ex": "http://example.org/"}, "id": "urn:ngsi-ld:Vehicle:p", "type": "Vehicle"});
        body.as_object_mut().expect("an object").extend(members.as_object().expect("an object").clone());
        let refusal = entity(&engine(), body, Normalization::Entity)
            .await
            .expect("the fixture expands")
            .refusal_before_expansion();
        let object_type = if attribute == "l" { "ex:42x" } else { "ex:bad-type" };
        assert!(refuses_prefixed_object_type(&refusal, attribute, object_type), "{attribute}: {refusal:?}");
    }
}

#[tokio::test]
async fn a_merge_fragment_object_type_whose_part_after_the_prefix_breaks_the_grammar_is_refused() {
    let refusal = patch(
        &engine(),
        json!({"@context": {"ex": "http://example.org/"}, "r": {"type": "Relationship", "object": "urn:ngsi-ld:A:1", "objectType": "ex:bad-type"}}),
        Normalization::MemberDeletions,
    )
    .await
    .expect("the fixture expands")
    .refusal_before_expansion();
    assert!(refuses_prefixed_object_type(&refusal, "r", "ex:bad-type"), "{refusal:?}");
}

#[tokio::test]
async fn a_well_formed_prefixed_object_type_still_expands() {
    let document = entity(
        &engine(),
        json!({
            "@context": {"ex": "http://example.org/"},
            "id": "urn:ngsi-ld:Vehicle:p",
            "type": "Vehicle",
            "r": {"type": "Relationship", "object": "urn:ngsi-ld:A:1", "objectType": ["ex:Building", "urn:x-y"]}
        }),
        Normalization::Entity,
    )
    .await
    .expect("the fixture expands")
    .agreed();
    assert!(document.attributes().contains_key("https://uri.etsi.org/ngsi-ld/default-context/r"));
}
