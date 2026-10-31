//! A body over the request value budget is 413 and creates nothing.

use andromeda_test_suite::{
    broker,
    config::{Budget, SuiteConfig},
    namespace::Namespace,
    problem::{answered, problem},
    wire::request_to,
};
use bytes::Bytes;
use googletest::prelude::*;
use http::{HeaderValue, Method, StatusCode, header::CONTENT_TYPE};
use iri_rs::IriRefBuf;
use ngsi_ld::{
    client::ClientError,
    http::headers::Accept,
    model::{ErrorType, NgsiLdMediaType},
};

/// Pins: Create Entity of a body holding one JSON value more than the request budget answers 413
/// `RequestEntityTooLarge` naming the budget and the request path, and creates nothing
/// Clause: 6.3.2, 5.5.2
/// Recorded: ETSI GS CIM 009 sets no size for an entity or a geometry; Andromeda budgets the JSON values of a
/// request (4,000,000), of an entity outside its geometries and JSON literals (1,000,000) and the positions of one
/// geometry (100,000), the stack running the defaults
#[gtest]
#[tokio::test]
async fn a_lim_pay_9001_a_body_over_the_request_value_budget_is_413_and_creates_nothing() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let budget = SuiteConfig::get()?.budget(Budget::RequestValue)?;
    let probe_id = namespace.urn("Probe", "values")?;
    // The size of the body is the subject, so it is sent as text rather than built as a value of the model.
    let body = format!(
        r#"{{"id":"{probe_id}","type":"Probe_{namespace}","n":{{"type":"Property","value":[{}0]}}}}"#,
        "0,".repeat(usize::try_from(budget)?)
    );
    let request = request_to(&broker, Method::POST, "entities")?
        .append_header(CONTENT_TYPE, HeaderValue::from_static("application/json"))
        .body(Bytes::from(body));
    let answer = broker.send_unchecked(request).await;
    expect_that!(
        &answer,
        problem(
            StatusCode::PAYLOAD_TOO_LARGE,
            ErrorType::Other(IriRefBuf::new("https://uri.etsi.org/ngsi-ld/errors/RequestEntityTooLarge".to_owned())?)
        )
        .with_detail_containing(&budget.to_string())
    );
    let reported = answer.as_ref().err().and_then(ClientError::problem).or_fail()?;
    expect_that!(reported.status, some(eq(413)));
    expect_that!(reported.instance.as_ref().map(IriRefBuf::as_str), some(eq("/ngsi-ld/v1/entities")));
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
