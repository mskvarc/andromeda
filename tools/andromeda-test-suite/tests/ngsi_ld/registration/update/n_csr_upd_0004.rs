//! A fragment under a scoped user context is bad request data and changes nothing.

use andromeda_test_suite::{
    broker,
    hosted_context::HostedContext,
    namespace::Namespace,
    problem::problem,
    registration_vocabulary::{registration_vocabulary, vehicle_and_parking_registration},
    wire::{content_type, request_to},
};
use bytes::Bytes;
use googletest::prelude::*;
use http::{Method, StatusCode};
use ngsi_ld::{
    http::{ContextDelivery, headers::Accept},
    model::{APPLICATION_LD_JSON, Context, ErrorType, NgsiLdMediaType},
};
use url::Url;

/// Pins: Update Context Source Registration with an application/ld+json fragment whose @context declares a scoped
/// context answers 400 `BadRequestData`, and the registration then reads back with its endpoint unchanged
/// Clause: 5.9.3, 5.5.7
#[gtest]
#[tokio::test]
async fn n_csr_upd_0004_a_fragment_under_a_scoped_user_context_is_bad_request_data_and_changes_nothing() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let ctx = HostedContext::add(&broker, &registration_vocabulary(&namespace)?).await?;
    let body_context = broker
        .with_context(Context::Multiple(vec![ctx.entry()?]))?
        .with_context_delivery(ContextDelivery::LdJsonBody);
    let registration_id = namespace.registration_id(1)?;
    let registration = vehicle_and_parking_registration(&namespace, &registration_id, None)?;
    body_context.registrations().register(&registration).send().await?;
    // The fragment carries a scoped context the model refuses, so the body is written as text.
    let text = format!(
        r#"{{"endpoint": "http://scoped.example.org:1026", "@context": [{{"speed": {{"@id": "https://example.org/speed", "@context": {{}}}}}}, "{}"]}}"#,
        ctx.url
    );
    let request = content_type(
        request_to(&broker, Method::PATCH, &format!("csourceRegistrations/{registration_id}"))?,
        &APPLICATION_LD_JSON,
    )?
    .body(Bytes::from(text));
    let answer = broker.send_unchecked(request).await;
    expect_that!(&answer, problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData));
    let retrieved = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .registrations()
        .retrieve(&registration_id)
        .send()
        .await?;
    expect_that!(retrieved.endpoint(), eq(&"http://my.csource.org:1026/".parse::<Url>()?));
    Ok(())
}
