//! A post query with a context nested in an entity selector is bad request data.

use andromeda_test_suite::{broker, config::SuiteConfig, namespace::Namespace, problem::problem, unchecked_call::exchange};
use googletest::prelude::*;
use http::{Method, StatusCode};
use ngsi_ld::model::ErrorType;

/// Pins: Query Entities by POST refuses a user @context nested in an entities selector, in `geoQ` and in
/// `temporalQ`
/// Clause: 5.5.7, 5.7.2
/// Recorded: 5.5.7 says such parts "should" result in `BadRequestData`; Andromeda refuses them
#[gtest]
#[tokio::test]
async fn n_qry_ent_2001_a_post_query_with_a_context_nested_in_an_entity_selector_is_bad_request_data() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let config = SuiteConfig::get()?;
    let core_context = config.core_context();
    let fields = [("Content-Type", "application/ld+json"), ("Accept", "application/json")];
    // The Query document is nested with members the model has no place for, so each body goes out as written.
    let bodies = [
        format!(
            r#"{{"type": "Query", "entities": [{{"type": "Device_{namespace}", "@context": {{"Device": "https://example.org/Device"}}}}], "@context": "{core_context}"}}"#
        ),
        format!(
            r#"{{"type": "Query", "entities": [{{"type": "Device_{namespace}"}}], "geoQ": {{"geometry": "Point", "coordinates": [13.4, 52.5], "georel": "near;maxDistance==1000", "@context": {{}}}}, "@context": "{core_context}"}}"#
        ),
        format!(
            r#"{{"type": "Query", "entities": [{{"type": "Device_{namespace}"}}], "temporalQ": {{"timerel": "after", "timeAt": "2020-01-01T00:00:00Z", "@context": {{}}}}, "@context": "{core_context}"}}"#
        ),
    ];
    for body in &bodies {
        let request = exchange(&broker, Method::POST, "entityOperations/query", &fields, Some(body.as_str()))?;
        let answer = broker.send_unchecked(request).await;
        expect_that!(&answer, problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData));
    }
    Ok(())
}
