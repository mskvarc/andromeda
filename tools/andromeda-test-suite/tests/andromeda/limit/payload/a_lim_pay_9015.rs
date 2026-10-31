//! A Property of millions of numbers under the request value budget and over the entity value budget is 413 and
//! creates nothing.

use andromeda_test_suite::{
    broker,
    config::{Budget, SuiteConfig},
    namespace::Namespace,
    problem::{answered, problem, problem_status, request_entity_too_large},
    wire::request_to,
};
use bytes::Bytes;
use googletest::prelude::*;
use http::{HeaderValue, Method, StatusCode, header::CONTENT_TYPE};
use ngsi_ld::{http::headers::Accept, model::NgsiLdMediaType};

/// Pins: Create Entity of a body whose one Property holds as many numbers as the request value budget leaves room
/// for, far more than the entity value budget, answers 413 `RequestEntityTooLarge` naming the entity budget, creates
/// nothing, and leaves the broker answering
/// Clause: 6.3.2
/// Recorded: ETSI GS CIM 009 sets no size for an entity; Andromeda budgets the JSON values of a request (4,000,000) and
/// of an entity outside its geometries and JSON literals (1,000,000), the stack running the defaults, and counts an
/// entity's values while it expands them, so such a body is refused before it is expanded whole
#[gtest]
#[tokio::test]
async fn a_lim_pay_9015_a_property_of_millions_of_numbers_over_the_entity_value_budget_is_413_and_creates_nothing() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let config = SuiteConfig::get()?;
    let request_budget = config.budget(Budget::RequestValue)?;
    let entity_budget = config.budget(Budget::EntityValue)?;
    let numbers = usize::try_from(request_budget - request_budget / 40)?;
    let probe_id = namespace.urn("Probe", "numbers")?;
    // The size of the body is the subject, so it is sent as text rather than built as a value of the model.
    let body = format!(
        r#"{{"id":"{probe_id}","type":"Probe_{namespace}","n":{{"type":"Property","value":[{}0]}}}}"#,
        "7,".repeat(numbers - 1)
    );
    let request = request_to(&broker, Method::POST, "entities")?
        .append_header(CONTENT_TYPE, HeaderValue::from_static("application/json"))
        .body(Bytes::from(body));
    let refused = broker.send_unchecked(request).await;
    expect_that!(
        &refused,
        problem(StatusCode::PAYLOAD_TOO_LARGE, request_entity_too_large()).with_detail_containing(&format!(
            "more than the {entity_budget} JSON values this broker accepts in one entity"
        ))
    );
    expect_that!(problem_status(&refused), some(eq(413)));
    let absent = broker
        .with_responses()
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .retrieve(&probe_id)
        .send()
        .await;
    expect_that!(&absent, answered(StatusCode::NOT_FOUND));
    Ok(())
}
