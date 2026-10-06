//! A value member written as the empty array, expanded by the typed walk and
//! by the processor alike.
//!
//! JSON-LD 1.1 expansion keeps a member whose value is `[]` with no item
//! (step 13.14) and drops the `null` items of an array (step 5), so `[]` and
//! `[null]` reach the assembly as one present, empty member. Clause 3.1 makes
//! the empty array a Property value (Table 5.2.5-1: "Any JSON value"); every
//! other value member names at least one target, term, string or geometry.

use crate::{
    expansion_fixture::{FixtureFailure, attribute, engine, entity, patch},
    normalization::Normalization,
};
use andromeda_ngsi_ld_entity::document::{
    EntityDocument,
    instance_document::InstanceDocument,
    written_value::{ValueSlot, WrittenValue},
};
use andromeda_ngsi_ld_lexicon::attribute_type::AttributeType;
use serde_json::{Value, json};

/// The core context's default vocabulary, which every undefined term expands
/// against (clause 5.5.7).
const DEFAULT: &str = "https://uri.etsi.org/ngsi-ld/default-context/";

/// The one instance written for the undefined term `name`.
fn instance<'a>(document: &'a EntityDocument, name: &str) -> &'a InstanceDocument {
    let attribute = format!("{DEFAULT}{name}");
    let member = document
        .attributes()
        .get(attribute.as_str())
        .unwrap_or_else(|| panic!("{attribute} is written"));
    let [instance] = member.instances() else {
        panic!("{attribute} carries one instance");
    };
    instance
}

/// An entity carrying `tags` as written.
fn tagged(tags: &Value) -> Value {
    json!({"id": "urn:ngsi-ld:Probe:1", "type": "Probe", "tags": tags})
}

/// Clause 3.1: `"value": []` is a Property holding no value, read the same by
/// the typed walk and by the processor.
#[tokio::test]
async fn an_empty_array_value_is_a_property_holding_no_value() {
    let document = entity(&engine(), tagged(&json!({"type": "Property", "value": []})), Normalization::Entity)
        .await
        .expect("the fixture expands")
        .agreed();

    let tags = instance(&document, "tags");
    pretty_assertions::assert_eq!(tags.family(), Some(AttributeType::Property));
    pretty_assertions::assert_eq!(tags.value(), &ValueSlot::Set(WrittenValue::Value(Vec::new())));
}

/// Clauses 4.5.0 and 5.2.1: expansion removes a `null`, so `[null]` is the
/// same written value as `[]`.
#[tokio::test]
async fn an_array_of_null_is_the_empty_array() {
    let document = entity(&engine(), tagged(&json!({"type": "Property", "value": [null]})), Normalization::Entity)
        .await
        .expect("the fixture expands")
        .agreed();

    pretty_assertions::assert_eq!(instance(&document, "tags").value(), &ValueSlot::Set(WrittenValue::Value(Vec::new())));
}

/// Clause 4.5.2.3: the concise `"tags": []` is the Property value `[]`, in an
/// entity, a merge patch and an update fragment alike.
#[tokio::test]
async fn the_concise_empty_array_is_a_property_value() {
    let created = entity(&engine(), tagged(&json!([])), Normalization::Entity)
        .await
        .expect("the fixture expands")
        .agreed();
    let merged = patch(&engine(), json!({"tags": []}), Normalization::MemberDeletions)
        .await
        .expect("the fixture expands")
        .agreed();
    let updated = patch(&engine(), json!({"tags": []}), Normalization::Entity)
        .await
        .expect("the fixture expands")
        .agreed();

    for document in [&created, &merged, &updated] {
        let tags = instance(document, "tags");
        pretty_assertions::assert_eq!(tags.family(), Some(AttributeType::Property));
        pretty_assertions::assert_eq!(tags.value(), &ValueSlot::Set(WrittenValue::Value(Vec::new())));
    }
}

/// Clauses 5.6.4 and 5.6.19: an attribute fragment declaring no family sets
/// the empty array rather than leaving the value untouched.
#[tokio::test]
async fn an_attribute_fragment_setting_the_empty_array_sets_it() {
    let (_, written) = attribute(&engine(), &format!("{DEFAULT}tags"), json!({"value": []}))
        .await
        .expect("the fixture expands")
        .agreed()
        .into_parts();

    pretty_assertions::assert_eq!(written.value(), &ValueSlot::Set(WrittenValue::Value(Vec::new())));
}

/// Clause 5.2.5: a Property with no `value` member at all is no Property an
/// entity holds, a different statement from the empty array; the compact
/// model refuses it where it stands.
#[tokio::test]
async fn a_property_without_a_value_member_is_refused_where_it_stands() {
    let Err(FixtureFailure::NotTyped(refusal)) = entity(&engine(), tagged(&json!({"type": "Property", "unitCode": "KMH"})), Normalization::Entity).await else {
        panic!("the compact model refuses the Property");
    };
    let refusal = refusal.to_string();
    assert!(refusal.contains("/tags") && refusal.contains("missing field `value`"), "{refusal}");
}

/// The other value members name at least one target (clause 4.5.3.2), term
/// (clause 4.5.20.2), string (clause 4.5.18.2) or geometry (clause 4.7.1), so
/// an empty one is malformed rather than absent.
#[tokio::test]
async fn an_empty_member_of_any_other_family_is_malformed() {
    let cases = [
        (json!({"type": "Relationship", "object": []}), "object"),
        (json!({"type": "VocabProperty", "vocab": []}), "vocab"),
        (json!({"type": "LanguageProperty", "languageMap": {}}), "languageMap"),
        (json!({"type": "LanguageProperty", "languageMap": {"en": []}}), "languageMap"),
    ];
    for (tags, member) in cases {
        let refusal = entity(&engine(), tagged(&tags), Normalization::Entity)
            .await
            .expect("the fixture expands")
            .refused();
        assert!(refusal.contains(&format!("the member '{member}' is not shaped")), "{tags}: {refusal}");
    }
    // Clause 4.7.1: a geometry is a GeoJSON object, which the compact model
    // refuses to hold before anything is expanded.
    let Err(FixtureFailure::NotTyped(refusal)) = entity(&engine(), tagged(&json!({"type": "GeoProperty", "value": []})), Normalization::Entity).await else {
        panic!("the compact model refuses the geometry");
    };
    assert!(refusal.to_string().contains("/tags/value"), "{refusal}");
}
