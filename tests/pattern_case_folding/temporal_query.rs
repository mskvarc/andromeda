//! `q` and `idPattern` on the Temporal API (clause 5.7.4.4), whose selection
//! the store answers from the Entity catalogue and the instances in the window.

use crate::fixture::{CASE_FORMS, Spellings, WINDOW_OPENS, encoded_filter, identifiers};
use andromeda_test_fixtures::context::TEST_SUITE_CONTEXT;
use andromeda_test_harness::environment::TestEnvironment;
use pretty_assertions::assert_eq;
use std::collections::BTreeSet;

/// The Entities of `fixture` a temporal query over a window holding every
/// observation selects with `criteria`.
async fn selected(scope: &TestEnvironment, fixture: &Spellings, criteria: &str) -> BTreeSet<String> {
    let query = format!("type={}&timerel=after&timeAt={WINDOW_OPENS}&{criteria}", fixture.entity_type);
    let response = scope
        .client()
        .query_temporal_entities(&query, Some(TEST_SUITE_CONTEXT))
        .await
        .expect("temporal query request");
    identifiers(&response)
}

/// Clause 5.7.4.4 S2 evaluates `q` against the instances in the window, and a
/// case-insensitive pattern folds there as it does on the Core API.
#[tokio::test]
async fn case_folding_temporal_query_a_case_insensitive_q_pattern_folds_over_the_window() {
    let scope = TestEnvironment::from_suite("pattern_case_folding").await.expect("isolated test environment");
    let fixture = Spellings::create(&scope, "TemporalBee").await;

    let criteria = format!("q={}", encoded_filter(r#"name~="(?i)^čebela$""#));
    assert_eq!(selected(&scope, &fixture, &criteria).await, fixture.ids_of(&CASE_FORMS));

    scope.finish().await.expect("cleanup failed");
}

/// Clause 5.7.4.4 S1 selects by `idPattern` from the Entity catalogue; the
/// scoped group folds `BEE` onto the `Bee` the identifiers are written with.
#[tokio::test]
async fn case_folding_temporal_query_an_identifier_pattern_folds_the_part_its_group_encloses() {
    let scope = TestEnvironment::from_suite("pattern_case_folding").await.expect("isolated test environment");
    let fixture = Spellings::create(&scope, "TemporalHive").await;

    let pattern = format!("^{}:(?i:BEE):[0-3]$", regex::escape(scope.id_prefix()));
    let criteria = format!("idPattern={}", urlencoding::encode(&pattern));
    assert_eq!(selected(&scope, &fixture, &criteria).await, fixture.ids_of(&CASE_FORMS));

    scope.finish().await.expect("cleanup failed");
}
