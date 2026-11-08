//! A body without a context member is bad request data.

use andromeda_test_suite::{broker, namespace::Namespace, problem::problem, wire::request_to};
use bytes::Bytes;
use googletest::prelude::*;
use http::{HeaderName, HeaderValue, Method, StatusCode, header::CONTENT_TYPE};
use ngsi_ld::{
    http::{headers::Accept, operation::ContextKindFilter},
    model::ErrorType,
};

/// Pins: Add @context with a JSON object of terms without a top-level @context is refused as `BadRequestData`
/// without a Location and stores nothing
/// Clause: 5.13.2.3, 5.13.2.4, 5.5.4
/// Tenant: private (the case asserts what the tenant's whole @context store lists, and clause 4.14 keeps each
/// tenant's stored @contexts its own; in the shared tenant every other file's entries are listed too)
#[gtest]
#[tokio::test]
async fn n_ctx_add_0002_a_body_without_a_context_member_is_bad_request_data() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let tenant = broker::private_tenant(&broker, &namespace).await?;
    // The body is no document holding only an @context, which the model cannot write, so it is sent as written.
    let request = request_to(&tenant, Method::POST, "jsonldContexts")?.append_header(CONTENT_TYPE, HeaderValue::from_static("application/json")).body(Bytes::from(format!("{{\n    \"A1\": \"https://test.andromeda/{namespace}/A1\",\n    \"A2\": \"https://test.andromeda/{namespace}/A2\",\n    \"A3\": \"https://test.andromeda/{namespace}/A3\"\n}}")));
    let answer = tenant.send_unchecked(request).await;
    expect_that!(
        &answer,
        problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData).without_header(HeaderName::from_static("location"))
    );
    let contexts = tenant
        .with_accept(Accept::any())
        .contexts()
        .list()
        .kind(ContextKindFilter::Hosted)
        .send()
        .await?;
    expect_that!(contexts, len(eq(0)));
    Ok(())
}
