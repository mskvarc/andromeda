//! `q` and `idPattern` on the Core API (clauses 4.9, 5.7.2), answered by the
//! store from the restatement it is handed.

use crate::fixture::{CASE_FORMS, ONE_WORD, Spellings, encoded_filter, identifiers};
use andromeda_test_fixtures::context::TEST_SUITE_CONTEXT;
use andromeda_test_harness::environment::TestEnvironment;
use pretty_assertions::assert_eq;
use reqwest::StatusCode;
use serde_json::json;
use std::collections::BTreeSet;

/// The Entities of `entity_type` a `q` filter selects.
async fn selected_by_filter(scope: &TestEnvironment, entity_type: &str, filter: &str) -> BTreeSet<String> {
    let query = format!("type={entity_type}&q={}", encoded_filter(filter));
    let response = scope
        .client()
        .query_entities(&query, "application/json", Some(TEST_SUITE_CONTEXT))
        .await
        .expect("query request");
    identifiers(&response)
}

/// `(?i)` folds the caron-bearing capital as well as the ASCII letters, so
/// every case form of the word is selected and the uncaroned word is not.
#[tokio::test]
async fn unicode_pattern_entity_query_a_case_insensitive_pattern_selects_every_case_form_of_a_non_ascii_word() {
    let scope = TestEnvironment::from_suite("unicode_pattern").await.expect("isolated test environment");
    let fixture = Spellings::create(&scope, "CoreBee").await;

    let selected = selected_by_filter(&scope, &fixture.entity_type, r#"name~="(?i)^čebela$""#).await;
    assert_eq!(selected, fixture.ids_of(&CASE_FORMS));

    scope.finish().await.expect("cleanup failed");
}

/// `(?i:…)` folds only what it encloses: the initial letter compares either
/// way, the rest exactly.
#[tokio::test]
async fn unicode_pattern_entity_query_a_scoped_group_folds_only_what_it_encloses() {
    let scope = TestEnvironment::from_suite("unicode_pattern").await.expect("isolated test environment");
    let fixture = Spellings::create(&scope, "ScopedBee").await;

    let selected = selected_by_filter(&scope, &fixture.entity_type, r#"name~="^(?i:č)ebela$""#).await;
    assert_eq!(selected, fixture.ids_of(&["čebela", "Čebela"]));

    scope.finish().await.expect("cleanup failed");
}

/// `(?-i)` ends the case-insensitive span: `če` compares either way, `bela`
/// exactly, so `ČEBELA` is not selected and `ČEbela` is.
#[tokio::test]
async fn unicode_pattern_entity_query_a_cleared_option_compares_the_rest_of_the_pattern_exactly() {
    let scope = TestEnvironment::from_suite("unicode_pattern").await.expect("isolated test environment");
    let fixture = Spellings::create(&scope, "ClearedBee").await;

    let selected = selected_by_filter(&scope, &fixture.entity_type, r#"name~="(?i)^če(?-i)bela$""#).await;
    assert_eq!(selected, fixture.ids_of(&["čebela", "Čebela", "ČEbela"]));

    scope.finish().await.expect("cleanup failed");
}

/// `!~=` selects exactly what the fold set leaves out (clause 4.9).
#[tokio::test]
async fn unicode_pattern_entity_query_a_negated_match_selects_what_the_fold_set_excludes() {
    let scope = TestEnvironment::from_suite("unicode_pattern").await.expect("isolated test environment");
    let fixture = Spellings::create(&scope, "NegatedBee").await;

    let selected = selected_by_filter(&scope, &fixture.entity_type, r#"name!~="(?i)^čebela$""#).await;
    assert_eq!(selected, fixture.ids_of(&["cebela", "Škofja Loka"]));

    scope.finish().await.expect("cleanup failed");
}

/// `(?m)` makes `^` and `$` line anchors without changing `.`, which still
/// matches a newline as clause 4.9's IEEE 1003.2 reading has it. Handed the
/// client's text, PostgreSQL read its own `m` option, which also stops `.` at a
/// newline, and folded only ASCII under `i`, so it selected nothing here.
#[tokio::test]
async fn unicode_pattern_entity_query_the_multi_line_option_keeps_a_dot_matching_a_newline() {
    let scope = TestEnvironment::from_suite("unicode_pattern").await.expect("isolated test environment");
    let entity_type = scope.entity_type("LineBee");
    let id = scope.id("Line:1");
    let entity = json!({
        "id": id,
        "type": entity_type,
        "name": {"type": "Property", "value": "prvi\nČebela"},
        "@context": [TEST_SUITE_CONTEXT]
    });
    let created = scope.client().post_entity(&entity).send().await.expect("create entity request");
    assert_eq!(created.status, StatusCode::CREATED, "{:?}", created.body);

    let selected = selected_by_filter(&scope, &entity_type, r#"name~="(?im)^prvi.čebela$""#).await;
    assert_eq!(selected, BTreeSet::from([id]));

    scope.finish().await.expect("cleanup failed");
}

/// `\w` holds `č`, `Č` and `Š` as it holds the ASCII letters, and `\s` the
/// space between two words. Handed the client's text, PostgreSQL classified by
/// the `COLLATE "C"` the value carries and selected only `cebela`.
#[tokio::test]
async fn unicode_pattern_entity_query_a_word_class_selects_non_ascii_words() {
    let scope = TestEnvironment::from_suite("unicode_pattern").await.expect("isolated test environment");
    let fixture = Spellings::create(&scope, "WordBee").await;

    let one_word = selected_by_filter(&scope, &fixture.entity_type, r#"name~="^\w+$""#).await;
    assert_eq!(one_word, fixture.ids_of(&ONE_WORD));
    let two_words = selected_by_filter(&scope, &fixture.entity_type, r#"name~="^\w+\s\w+$""#).await;
    assert_eq!(two_words, fixture.ids_of(&["Škofja Loka"]));
    let not_one_word = selected_by_filter(&scope, &fixture.entity_type, r#"name!~="^\w+$""#).await;
    assert_eq!(not_one_word, fixture.ids_of(&["Škofja Loka"]));

    scope.finish().await.expect("cleanup failed");
}

/// A digit class in an `idPattern`, beside a word class, selects the
/// identifiers the fixture numbers.
#[tokio::test]
async fn unicode_pattern_entity_query_an_identifier_pattern_with_shorthand_classes_selects_by_them() {
    let scope = TestEnvironment::from_suite("unicode_pattern").await.expect("isolated test environment");
    let fixture = Spellings::create(&scope, "NumberedBee").await;

    let pattern = format!(r"^{}:\w+:[0-3]\D*$", regex::escape(scope.id_prefix()));
    let query = format!("type={}&idPattern={}", fixture.entity_type, urlencoding::encode(&pattern));
    let response = scope
        .client()
        .query_entities(&query, "application/json", Some(TEST_SUITE_CONTEXT))
        .await
        .expect("query request");
    assert_eq!(identifiers(&response), fixture.ids_of(&CASE_FORMS));

    scope.finish().await.expect("cleanup failed");
}

/// An `idPattern` folds the part a scoped group encloses and compares the
/// identifier prefix exactly. PostgreSQL itself refuses a flag group anywhere
/// but at the start of a pattern, so the client's text would not even compile
/// in the store.
#[tokio::test]
async fn unicode_pattern_entity_query_an_identifier_pattern_folds_the_part_its_group_encloses() {
    let scope = TestEnvironment::from_suite("unicode_pattern").await.expect("isolated test environment");
    let entity_type = scope.entity_type("Hive");
    let mut ids = Vec::new();
    for local in ["Hive:1", "HIVE:2", "hive:3", "Hivex:4"] {
        let id = scope.id(local);
        let entity = json!({"id": id, "type": entity_type, "@context": [TEST_SUITE_CONTEXT]});
        let created = scope.client().post_entity(&entity).send().await.expect("create entity request");
        assert_eq!(created.status, StatusCode::CREATED, "{local}: {:?}", created.body);
        ids.push(id);
    }

    let pattern = format!("^{}:(?i:hive):", regex::escape(scope.id_prefix()));
    let query = format!("type={entity_type}&idPattern={}", urlencoding::encode(&pattern));
    let response = scope
        .client()
        .query_entities(&query, "application/json", Some(TEST_SUITE_CONTEXT))
        .await
        .expect("query request");
    assert_eq!(identifiers(&response), ids[..3].iter().cloned().collect()); // clone: the expected set owns its identifiers.

    scope.finish().await.expect("cleanup failed");
}
