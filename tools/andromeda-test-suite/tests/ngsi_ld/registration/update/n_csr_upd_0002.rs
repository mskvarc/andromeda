//! An updated endpoint reads back as written.

use andromeda_test_suite::{
    body_json::json_equals,
    broker,
    hosted_context::HostedContext,
    namespace::Namespace,
    problem::answered,
    raw_answer::send_raw,
    registration_vocabulary::{registration_vocabulary, vehicle_and_parking_registration},
    wire::request_to,
};
use googletest::prelude::*;
use http::{Method, StatusCode};
use ngsi_ld::{
    http::ContextDelivery,
    model::{Context, UrlDraft},
    registration::{RegistrationFragment, RegistrationTypeMember},
};

/// Pins: Update Context Source Registration with a fragment holding an endpoint without a path answers 204, and the
/// registration then reads back with that endpoint spelt as written
/// Clause: 5.9.3, 5.10.1, 5.2.9, 5.2.2
#[gtest]
#[tokio::test]
async fn n_csr_upd_0002_an_updated_endpoint_reads_back_as_written() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let ctx = HostedContext::add(&broker, &registration_vocabulary(&namespace)?).await?;
    let body_context = broker
        .with_context(Context::Multiple(vec![ctx.entry()?]))?
        .with_context_delivery(ContextDelivery::LdJsonBody);
    let registration_id = namespace.registration_id(1)?;
    let registration = vehicle_and_parking_registration(&namespace, &registration_id, None)?;
    body_context.registrations().register(&registration).send().await?;
    let fragment = RegistrationFragment::builder()
        .endpoint(UrlDraft::verbatim("http://my.new.csource.org:1026"))
        .type_member(RegistrationTypeMember::Omitted)
        .build()?;
    let answer = broker.with_responses().registrations().update(&registration_id, &fragment).send().await;
    expect_that!(&answer, answered(StatusCode::NO_CONTENT));
    // The endpoint spelling is the subject and the typed endpoint is the normalized URL, so the answer is read as
    // received.
    let read = request_to(&broker, Method::GET, &format!("csourceRegistrations/{registration_id}"))?;
    let response = send_raw(read).await?;
    expect_that!(response.status(), eq(StatusCode::OK));
    expect_that!(response.body(), json_equals("$.endpoint", "http://my.new.csource.org:1026"));
    Ok(())
}
