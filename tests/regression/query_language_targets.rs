//! Clause 4.9 Query Terms whose target element is not a plain Property.
//!
//! Clause 4.9 defines a target value for every attribute family, not only for a
//! Property's `value` and a Relationship's `object`: a `LanguageProperty`'s
//! `languageMap` with or without a target language, a `ListProperty`'s
//! `valueList`, a `ListRelationship`'s `objectList`, a `VocabProperty`'s
//! `vocab`. The PostgreSQL pushdown that answers an ordinary (non-local) entity
//! query matched none of them, while the in-memory evaluator did — so the same
//! `q` gave two different answers depending on which path ran.
//!
//! Every query below is an ordinary `GET /entities` with `q`, which is the
//! request the pushdown answers, and every assertion states what ETSI GS CIM
//! 009 v1.9.1 requires rather than what the broker happened to do.
//!
//! Naming convention: `regression_<finding>_<scenario>`.

#![allow(clippy::expect_used, clippy::unwrap_used, reason = "integration test setup uses explicit assertions")]

use andromeda_test_fixtures::context::TEST_SUITE_CONTEXT;
use andromeda_test_harness::environment::TestEnvironment;
use pretty_assertions::assert_eq;
use reqwest::StatusCode;
use serde_json::json;

/// One entity carrying every attribute family whose clause 4.9 target value is
/// not a single scalar, plus the two plain Properties whose target value is one.
///
/// The language maps deliberately hold the queried value under a key that is
/// not the first one, and one of them holds arrays rather than single strings,
/// so a reader that only ever saw the first entry — or only ever saw a string —
/// cannot pass by accident.
///
/// `roomTemp` and `roomName` are the scalar targets: clause 4.9's datatype rules
/// are stated over the target value's type, and a suite holding only structured
/// targets never exercises them. `roomExtras` is the array a client wrote as a
/// plain Property value, which clause 4.9 makes an array target exactly as it
/// makes a `valueList` one.
fn structured_entity(id: &str) -> serde_json::Value {
    json!({
        "id": id,
        "type": "Building",
        "roomLabel": {"type": "LanguageProperty", "languageMap": {"en": "Door", "de": "Tuer"}},
        "roomAliases": {"type": "LanguageProperty", "languageMap": {"fr": ["chat", "rouge"], "en": ["red", "cat"]}},
        "roomTags": {"type": "ListProperty", "valueList": ["residential", "commercial"]},
        "roomSpeeds": {"type": "ListProperty", "valueList": [1, 2, 3]},
        "roomLinks": {"type": "ListRelationship", "objectList": ["urn:ngsi-ld:Room:1", "urn:ngsi-ld:Room:2"]},
        "roomVocab": {"type": "VocabProperty", "vocab": ["http://example.org/Male", "http://example.org/Other"]},
        "roomTemp": {"type": "Property", "value": 20},
        "roomName": {"type": "Property", "value": "atrium"},
        "roomExtras": {"type": "Property", "value": ["a", "b", "c"]},
        "@context": [TEST_SUITE_CONTEXT]
    })
}

/// An isolated environment holding the structured entity, and its identifier.
async fn seeded(local: &str) -> (TestEnvironment, String) {
    let scope = TestEnvironment::from_suite("regression").await.expect("isolated test environment");
    let id = scope.id(local);
    let created = scope.client().post_entity(&structured_entity(&id)).send().await.expect("create entity failed");
    assert_eq!(created.status, StatusCode::CREATED, "create: {:?}", created.body);
    (scope, id)
}

/// Whether the seeded entity satisfies one `q` expression.
///
/// The query names the entity by `id` as well, so the answer is about this
/// entity alone and no concurrently running suite can contribute a row.
async fn matches(scope: &TestEnvironment, id: &str, q: &str) -> bool {
    let query = format!("id={}&q={}", urlencoding::encode(id), urlencoding::encode(q));
    let response = scope
        .client()
        .query_entities(&query, "application/json", Some(TEST_SUITE_CONTEXT.as_str()))
        .await
        .expect("entity query failed");
    assert_eq!(response.status, StatusCode::OK, "q={q}: {:?}", response.body);
    let body = response.json_value().expect("query response must be JSON");
    let entities = body.as_array().expect("query response must be an array");
    assert!(entities.len() <= 1, "q={q} selected more than the one seeded entity: {body}");
    entities.len() == 1
}

/// Assert each `q` expression selects the seeded entity, or does not.
async fn assert_selection(scope: &TestEnvironment, id: &str, cases: &[(&str, bool)]) {
    for (q, expected) in cases {
        assert_eq!(matches(scope, id, q).await, *expected, "q={q}");
    }
}

/// Clause 4.9, `equal` operator: "The Query Term value target element
/// corresponds to a `LanguageProperty` and a natural language is specified
/// e.g. `color[en]=="red"`: a match is found as the value of the key-value pair
/// corresponding to the specified natural language of the languageMap
/// (e.g. matches `{"fr": "rouge", "en": "red","de": "rot"}` but not
/// `{"fr": "red", "en": "black","de": "blue"}`)."
#[tokio::test]
async fn regression_query_targets_language_property_with_a_target_language_matches_that_key_alone() {
    let (scope, id) = seeded("Building:q-target-language").await;

    assert_selection(
        &scope,
        &id,
        &[
            ("roomLabel[en]==\"Door\"", true),
            ("roomLabel[de]==\"Tuer\"", true),
            // The clause's own counter-example: the value belongs to another
            // key, so the target language's value does not match it.
            ("roomLabel[de]==\"Door\"", false),
            ("roomLabel[en]==\"Tuer\"", false),
        ],
    )
    .await;

    scope.finish().await.expect("cleanup failed");
}

/// Clause 4.9, `equal` operator: "The Query Term value target element
/// corresponds to a `LanguageProperty` and no natural language is specified
/// e.g. `color[*]=="red"`: any match is found in the values of the key-value
/// pairs of the languageMap", which clause 4.9 p. 87 restates as the target
/// value definition — "a value from any of the key-value pairs held within the
/// languageMap associated to such `LanguageProperty`".
#[tokio::test]
async fn regression_query_targets_language_property_without_a_language_matches_any_key() {
    let (scope, id) = seeded("Building:q-any-language").await;

    assert_selection(
        &scope,
        &id,
        &[
            // "Tuer" is not the first entry of the map: a reader that stopped at
            // the first language would miss it.
            ("roomLabel==\"Tuer\"", true),
            ("roomLabel==\"Door\"", true),
            ("roomLabel[*]==\"Tuer\"", true),
            ("roomLabel==\"rot\"", false),
        ],
    )
    .await;

    scope.finish().await.expect("cleanup failed");
}

/// Clause 4.9, `equal` operator: "a match is found as a single element from the
/// array of values of the key-value pair corresponding to the specified natural
/// language of the languageMap (e.g. matches `{"fr": ["chat", "rouge"],
/// "en": ["red", "cat], "de": ["rote", "Katze"]}`)", and, with no language
/// specified, "a match is found as a single element of the array of values of
/// the key-value pairs of the languageMap".
#[tokio::test]
async fn regression_query_targets_language_property_array_values_match_a_single_element() {
    let (scope, id) = seeded("Building:q-language-array").await;

    assert_selection(
        &scope,
        &id,
        &[
            ("roomAliases[en]==\"cat\"", true),
            ("roomAliases[fr]==\"rouge\"", true),
            ("roomAliases==\"chat\"", true),
            // An element of another key's array is not the target value of this
            // key.
            ("roomAliases[en]==\"chat\"", false),
        ],
    )
    .await;

    scope.finish().await.expect("cleanup failed");
}

/// Clause 4.9 p. 87: "If the target element is a `ListProperty`, the target value
/// is defined as the valueList array associated to such a `ListProperty`", and
/// the `equal` operator matches when the Query Term value "is included in the
/// target value, if the latter is an array".
#[tokio::test]
async fn regression_query_targets_list_property_matches_a_member_of_its_value_list() {
    let (scope, id) = seeded("Building:q-value-list").await;

    assert_selection(
        &scope,
        &id,
        &[
            ("roomTags==\"commercial\"", true),
            ("roomTags==\"residential\"", true),
            ("roomSpeeds==2", true),
            ("roomTags==\"industrial\"", false),
            ("roomSpeeds==9", false),
        ],
    )
    .await;

    scope.finish().await.expect("cleanup failed");
}

/// Clause 4.9 p. 87: "If the target element is a `ListRelationship`, the target
/// object is defined as the array of objects associated (represented as URIs)
/// to such `ListRelationship`", and the `equal` operator: "The Query Term value
/// is a URI and the target element corresponds to a Relationship, a
/// `ListRelationship` or a `VocabProperty` … Is included in the target value, if
/// the latter is an array".
#[tokio::test]
async fn regression_query_targets_list_relationship_matches_a_uri_in_its_object_list() {
    let (scope, id) = seeded("Building:q-object-list").await;

    assert_selection(
        &scope,
        &id,
        &[
            ("roomLinks==\"urn:ngsi-ld:Room:2\"", true),
            ("roomLinks==\"urn:ngsi-ld:Room:1\"", true),
            ("roomLinks==\"urn:ngsi-ld:Room:9\"", false),
        ],
    )
    .await;

    scope.finish().await.expect("cleanup failed");
}

/// Clause 4.9 p. 87: "If the target element is a `VocabProperty`, the target value
/// shall be expanded according to the @context", and the `equal` operator lists
/// a `VocabProperty` beside a Relationship and a `ListRelationship` as a target
/// a URI Query Term is compared against, matching when it "is included in the
/// target value, if the latter is an array". The Query Terms here are already
/// absolute IRIs, so expansion leaves them unchanged.
#[tokio::test]
async fn regression_query_targets_vocab_property_matches_a_uri_in_its_vocab() {
    let (scope, id) = seeded("Building:q-vocab").await;

    assert_selection(
        &scope,
        &id,
        &[
            ("roomVocab==\"http://example.org/Other\"", true),
            ("roomVocab==\"http://example.org/Male\"", true),
            ("roomVocab==\"http://example.org/Female\"", false),
        ],
    )
    .await;

    scope.finish().await.expect("cleanup failed");
}

/// Clause 4.9, `unequal` operator, over every structured target it names:
/// "No matching value is found as the value of the specified language key of a
/// languageMap where a language filter is specified"; "No matching value is
/// found in any of the values of the key-value pairs of a languageMap";
/// "Is not included in the target value, if the latter is an array (e.g. matches
/// `["blue","black","green"]`, but not `["blue","red","green"]`)".
///
/// The operator also carries clause 4.9's standing requirement that "a matching
/// entity shall contain the target element", which for a language-selected path
/// is the key-value pair itself: a language the map does not hold has no target
/// value to differ from, so `roomLabel[xx]!=…` matches nothing.
#[tokio::test]
async fn regression_query_targets_unequal_is_the_complement_over_every_structured_target() {
    let (scope, id) = seeded("Building:q-unequal").await;

    assert_selection(
        &scope,
        &id,
        &[
            ("roomLabel[en]!=\"Tuer\"", true),
            ("roomLabel[en]!=\"Door\"", false),
            ("roomLabel!=\"rot\"", true),
            ("roomLabel!=\"Tuer\"", false),
            ("roomAliases[en]!=\"chat\"", true),
            ("roomAliases[en]!=\"cat\"", false),
            ("roomTags!=\"industrial\"", true),
            ("roomTags!=\"commercial\"", false),
            ("roomLinks!=\"urn:ngsi-ld:Room:9\"", true),
            ("roomLinks!=\"urn:ngsi-ld:Room:2\"", false),
            ("roomVocab!=\"http://example.org/Female\"", true),
            ("roomVocab!=\"http://example.org/Male\"", false),
            ("roomLabel[xx]!=\"Door\"", false),
        ],
    )
    .await;

    scope.finish().await.expect("cleanup failed");
}

/// Clause 4.9, `equal` operator: "If the Query Term value is a list of values
/// (production rule named `ValueList`), e.g. `color=="black", "red"`: The target
/// value is identical or equivalent to any of the list values … The target value
/// includes any of the Query Term values, if the target value is an array".
#[tokio::test]
async fn regression_query_targets_a_value_list_query_term_reaches_every_structured_target() {
    let (scope, id) = seeded("Building:q-value-list-term").await;

    assert_selection(
        &scope,
        &id,
        &[
            ("roomLabel==\"nothing\",\"Tuer\"", true),
            ("roomLabel[en]==\"nothing\",\"Door\"", true),
            ("roomAliases==\"nothing\",\"chat\"", true),
            ("roomTags==\"industrial\",\"commercial\"", true),
            ("roomSpeeds==8,2", true),
            ("roomLinks==\"urn:ngsi-ld:Room:9\",\"urn:ngsi-ld:Room:2\"", true),
            ("roomVocab==\"http://example.org/Female\",\"http://example.org/Male\"", true),
            ("roomTags==\"industrial\",\"agricultural\"", false),
            ("roomSpeeds==8,9", false),
        ],
    )
    .await;

    scope.finish().await.expect("cleanup failed");
}

/// Clause 4.9, `unequal` operator: "If the Query Term value target element
/// corresponds to a Relationship, a `ListRelationship` or a `VocabProperty` and is a
/// list of URIs … The target value does not include any of the list values, if
/// the target value is an array (e.g. matches `["http://example.com/blue",
/// "http://example.com/yellow", "http://example.com/green"]`, but not
/// `["http://example.com/blue", "http://example.com/red",
/// "http://example.com/green"]`)", and the same rule stated over a value list of
/// literals: "The target value does not include any of the list values".
#[tokio::test]
async fn regression_query_targets_a_negated_value_list_query_term_denies_every_member() {
    let (scope, id) = seeded("Building:q-negated-value-list-term").await;

    assert_selection(
        &scope,
        &id,
        &[
            ("roomTags!=\"industrial\",\"agricultural\"", true),
            ("roomTags!=\"industrial\",\"commercial\"", false),
            ("roomLinks!=\"urn:ngsi-ld:Room:8\",\"urn:ngsi-ld:Room:9\"", true),
            ("roomLinks!=\"urn:ngsi-ld:Room:9\",\"urn:ngsi-ld:Room:2\"", false),
            ("roomVocab!=\"http://example.org/Female\",\"http://example.org/Nobody\"", true),
            ("roomVocab!=\"http://example.org/Female\",\"http://example.org/Male\"", false),
            ("roomLabel!=\"rot\",\"rouge\"", true),
            ("roomLabel!=\"rot\",\"Tuer\"", false),
        ],
    )
    .await;

    scope.finish().await.expect("cleanup failed");
}

/// Clause 4.9, `equal` operator: "If there is no equality between the target
/// value data type and the Query Term value data type, then it shall be
/// considered as not matching", and `unequal` operator: "If the data type of the
/// target value and the data type of the Query Term value are different, then
/// they shall be considered unequal."
///
/// `roomSpeeds` holds the numbers 1, 2 and 3, so the string `"2"` is a different
/// data type from every member: `==` must not match it and `!=` must.
#[tokio::test]
async fn regression_query_targets_a_datatype_mismatch_fails_equal_and_satisfies_unequal() {
    let (scope, id) = seeded("Building:q-datatype-mismatch").await;

    assert_selection(
        &scope,
        &id,
        &[
            ("roomSpeeds==2", true),
            ("roomSpeeds==\"2\"", false),
            ("roomSpeeds!=\"2\"", true),
            ("roomSpeeds!=2", false),
            // A boolean against the same numeric list is the same rule.
            ("roomSpeeds==true", false),
            ("roomSpeeds!=true", true),
        ],
    )
    .await;

    scope.finish().await.expect("cleanup failed");
}

/// Clause 4.9: "When a Query Term only defines an attribute path (production
/// rule named Attribute), the matching Entities shall be those which define the
/// target element (Property or a Relationship), regardless of any target value
/// or object" — and with a target language specified the target element is the
/// matching key-value pair of the `languageMap`.
#[tokio::test]
async fn regression_query_targets_existence_of_a_target_language_requires_that_key() {
    let (scope, id) = seeded("Building:q-language-existence").await;

    assert_selection(&scope, &id, &[("roomLabel", true), ("roomLabel[en]", true), ("roomLabel[xx]", false)]).await;

    scope.finish().await.expect("cleanup failed");
}

/// Clause 4.9, `unequal` operator: "If the data type of the target value and the
/// data type of the Query Term value are different, then they shall be
/// considered unequal."
///
/// `roomTemp` is the number 20, so a Query Term of any other data type — the
/// string `"20"`, the boolean `true`, the `DateTime` `2020-01-01T00:00:00Z` —
/// is unequal to it and `!=` matches. The rule is the whole difference between
/// the two operators: `equal` states it the other way, "If there is no equality
/// between the target value data type and the Query Term value data type, then
/// it shall be considered as not matching", so `==` must reject every one of
/// them.
#[tokio::test]
async fn regression_query_targets_a_mistyped_query_term_is_unequal_to_a_scalar_property() {
    let (scope, id) = seeded("Building:q-scalar-datatype-mismatch").await;

    assert_selection(
        &scope,
        &id,
        &[
            ("roomTemp==20", true),
            ("roomTemp!=20", false),
            ("roomTemp!=\"20\"", true),
            ("roomTemp==\"20\"", false),
            ("roomTemp!=true", true),
            ("roomTemp==true", false),
            ("roomTemp!=\"2020-01-01T00:00:00Z\"", true),
            ("roomTemp==\"2020-01-01T00:00:00Z\"", false),
            // The same rule with the types the other way round: a number is not
            // the data type of a string target.
            ("roomName!=5", true),
            ("roomName==5", false),
            ("roomName!=\"atrium\"", false),
            ("roomName!=\"foyer\"", true),
            // And against a structured target of another data type: the
            // `valueList` holds strings, so a numeric Query Term is unequal to
            // it rather than simply unmatched.
            ("roomTags!=5", true),
            ("roomTags==5", false),
        ],
    )
    .await;

    scope.finish().await.expect("cleanup failed");
}

/// Clause 4.9, `unequal` operator, over a range operand: the same final rule
/// applies — "If the data type of the target value and the data type of the
/// Query Term value are different, then they shall be considered unequal" — so a
/// string target is unequal to a numeric range rather than dropped by the
/// comparison it cannot take part in.
#[tokio::test]
async fn regression_query_targets_a_mistyped_range_is_unequal_to_a_scalar_property() {
    let (scope, id) = seeded("Building:q-range-datatype-mismatch").await;

    assert_selection(
        &scope,
        &id,
        &[
            ("roomTemp==10..30", true),
            ("roomTemp!=10..30", false),
            ("roomTemp!=25..30", true),
            ("roomName!=10..30", true),
            ("roomName==10..30", false),
        ],
    )
    .await;

    scope.finish().await.expect("cleanup failed");
}
