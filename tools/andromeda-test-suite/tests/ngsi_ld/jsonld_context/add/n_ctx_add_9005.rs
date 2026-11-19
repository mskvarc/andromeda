//! A context reference that is not a url is bad request data.

use andromeda_test_suite::{broker, namespace::Namespace, problem::problem, wire::request_to};
use bytes::Bytes;
use googletest::prelude::*;
use http::{HeaderName, HeaderValue, Method, StatusCode, header::CONTENT_TYPE};
use ngsi_ld::{
    http::{headers::Accept, operation::ContextKindFilter},
    model::ErrorType,
};

/// Pins: Add @context of a document whose @context is the string "not a url" is refused as `BadRequestData` without
/// a Location, with a detail naming the reference, and stores nothing
/// Clause: 5.13.2.4, 5.5.4, JSON-LD 1.1 section 9.15
/// Tenant: private (the case asserts what the tenant's whole @context store lists, and clause 4.14 keeps each
/// tenant's stored @contexts its own; in the shared tenant every other file's entries are listed too)
#[gtest]
#[tokio::test]
async fn n_ctx_add_9005_a_context_reference_that_is_not_a_url_is_bad_request_data() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let tenant = broker::private_tenant(&broker, &namespace).await?;
    // The @context member names a reference that is no IRI reference, which the model cannot write, so it is sent as
    // written.
    let request = request_to(&tenant, Method::POST, "jsonldContexts")?
        .append_header(CONTENT_TYPE, HeaderValue::from_static("application/json"))
        .body(Bytes::from(r#"{"@context": "not a url"}"#));
    let answer = tenant.send_unchecked(request).await;
    expect_that!(
        &answer,
        problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData)
            .with_detail(r#"the @context names "not a url", which is not a URL"#)
            .without_header(HeaderName::from_static("location"))
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
