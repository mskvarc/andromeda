//! A post query with a context nested in an entity selector is bad request data.

use andromeda_test_suite::{broker, config::SuiteConfig, namespace::Namespace, problem::problem, written_temporal_query::post};
use googletest::prelude::*;
use http::StatusCode;
use ngsi_ld::model::ErrorType;

/// Pins: Query Temporal Evolution of Entities by POST refuses a user @context nested in an entities selector and in
/// `temporalQ`
/// Clause: 5.5.7, 5.7.4
/// Recorded: 5.5.7 says such parts "should" result in `BadRequestData`; Andromeda refuses them
#[gtest]
#[tokio::test]
async fn n_tmp_qry_2001_a_post_query_with_a_context_nested_in_an_entity_selector_is_bad_request_data() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let core_context = SuiteConfig::get()?.core_context().to_string();
    // A context nested in a selector or in `temporalQ` is a member no Query document the model writes carries, so
    // the bodies are sent as written.
    let answer = post(
        &broker,
        "",
        &format!(
            r#"{{"type": "Query", "entities": [ {{ "type": "Device_{namespace}", "@context": {{ "Device": "https://example.org/Device" }} }} ], "temporalQ": {{ "timerel": "after", "timeAt": "2020-01-01T00:00:00Z" }}, "@context": "{core_context}"}}"#
        ),
        "application/ld+json",
        "application/json",
    )
    .await?;
    expect_that!(&answer, problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData));
    let answer_2 = post(
        &broker,
        "",
        &format!(
            r#"{{"type": "Query", "entities": [ {{ "type": "Device_{namespace}" }} ], "temporalQ": {{ "timerel": "after", "timeAt": "2020-01-01T00:00:00Z", "@context": {{}} }}, "@context": "{core_context}"}}"#
        ),
        "application/ld+json",
        "application/json",
    )
    .await?;
    expect_that!(&answer_2, problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData));
    Ok(())
}
