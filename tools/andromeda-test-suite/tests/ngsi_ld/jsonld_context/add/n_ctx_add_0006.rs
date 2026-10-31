//! A context array holding a number or an array is bad request data.

use andromeda_test_suite::{broker, namespace::Namespace, problem::problem, remote_context::RemoteContexts, wire::request_to};
use bytes::Bytes;
use googletest::prelude::*;
use http::{HeaderName, HeaderValue, Method, StatusCode, header::CONTENT_TYPE};
use ngsi_ld::{
    http::{headers::Accept, operation::ContextKindFilter},
    model::ErrorType,
};

/// Pins: Add @context with an @context array holding a number, or holding another array, is refused as
/// `BadRequestData` without a Location and stores nothing
/// Clause: 5.13.2.3, 5.13.2.4, 5.5.4, JSON-LD 1.1 9.15
/// Tenant: private (the case asserts what the tenant's whole @context store lists, and clause 4.14 keeps each
/// tenant's stored @contexts its own; in the shared tenant every other file's entries are listed too)
#[gtest]
#[tokio::test]
async fn n_ctx_add_0006_a_context_array_holding_a_number_or_an_array_is_bad_request_data() -> Result<()> {
    let namespace = Namespace::fresh();
    let tenant = broker::private_tenant(&broker::front()?, &namespace).await?;
    let contexts = RemoteContexts::start().await?;
    let served = contexts.url("jc/a.jsonld")?;
    // The array holds a member no context entry can be, which the model cannot write, so each body is sent as
    // written.
    let request = request_to(&tenant, Method::POST, "jsonldContexts")?
        .append_header(CONTENT_TYPE, HeaderValue::from_static("application/json"))
        .body(Bytes::from(format!("{{\n    \"@context\": [\n        \"{served}\",\n        5\n    ]\n}}")));
    let answer = tenant.send_unchecked(request).await;
    expect_that!(
        &answer,
        problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData).without_header(HeaderName::from_static("location"))
    );
    let hosted = tenant
        .with_accept(Accept::any())
        .contexts()
        .list()
        .kind(ContextKindFilter::Hosted)
        .send()
        .await?;
    expect_that!(hosted, len(eq(0)));
    let request_2 = request_to(&tenant, Method::POST, "jsonldContexts")?
        .append_header(CONTENT_TYPE, HeaderValue::from_static("application/json"))
        .body(Bytes::from(format!(
            "{{\n    \"@context\": [\n        [\n            \"{served}\"\n        ]\n    ]\n}}"
        )));
    let answer_2 = tenant.send_unchecked(request_2).await;
    expect_that!(
        &answer_2,
        problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData).without_header(HeaderName::from_static("location"))
    );
    let hosted_2 = tenant
        .with_accept(Accept::any())
        .contexts()
        .list()
        .kind(ContextKindFilter::Hosted)
        .send()
        .await?;
    expect_that!(hosted_2, len(eq(0)));
    Ok(())
}
