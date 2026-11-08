//! Serve any spelling of the core context url is resource not found.

use andromeda_test_suite::{broker, namespace::Namespace, problem::problem};
use googletest::prelude::*;
use http::StatusCode;
use ngsi_ld::{
    http::headers::Accept,
    model::{ContextId, ErrorType},
};

/// Pins: Serve @context of the core @context, under the v1.9 URL, the versionless URL and the v1.8 URL, answers
/// 404 `ResourceNotFound`, content and details alike
/// Clause: 5.13.4.4, 4.4
/// Tenant: private (the case exercises the tenant's own @context storage; clause 4.14 keeps each tenant's stored
/// @contexts its own)
/// Recorded: clause 5.13.4.4 gives back `ResourceNotFound` for an identifier that is not found; the core @context
/// is not a stored entry under any of its URLs
#[gtest]
#[tokio::test]
async fn o_ctx_srv_0029_serve_any_spelling_of_the_core_context_url_is_resource_not_found() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let tenant = broker::private_tenant(&broker, &namespace).await?;
    let answer = tenant
        .with_responses()
        .with_accept(Accept::any())
        .contexts()
        .serve(&"https://uri.etsi.org/ngsi-ld/v1/ngsi-ld-core-context-v1.9.jsonld".parse::<ContextId>()?)
        .send()
        .await;
    expect_that!(&answer, problem(StatusCode::NOT_FOUND, ErrorType::ResourceNotFound));
    let answer_2 = tenant
        .with_responses()
        .with_accept(Accept::any())
        .contexts()
        .serve_details(&"https://uri.etsi.org/ngsi-ld/v1/ngsi-ld-core-context-v1.9.jsonld".parse::<ContextId>()?)
        .send()
        .await;
    expect_that!(&answer_2, problem(StatusCode::NOT_FOUND, ErrorType::ResourceNotFound));
    let answer_3 = tenant
        .with_responses()
        .with_accept(Accept::any())
        .contexts()
        .serve(&"https://uri.etsi.org/ngsi-ld/v1/ngsi-ld-core-context.jsonld".parse::<ContextId>()?)
        .send()
        .await;
    expect_that!(&answer_3, problem(StatusCode::NOT_FOUND, ErrorType::ResourceNotFound));
    let answer_4 = tenant
        .with_responses()
        .with_accept(Accept::any())
        .contexts()
        .serve_details(&"https://uri.etsi.org/ngsi-ld/v1/ngsi-ld-core-context.jsonld".parse::<ContextId>()?)
        .send()
        .await;
    expect_that!(&answer_4, problem(StatusCode::NOT_FOUND, ErrorType::ResourceNotFound));
    let answer_5 = tenant
        .with_responses()
        .with_accept(Accept::any())
        .contexts()
        .serve(&"https://uri.etsi.org/ngsi-ld/v1/ngsi-ld-core-context-v1.8.jsonld".parse::<ContextId>()?)
        .send()
        .await;
    expect_that!(&answer_5, problem(StatusCode::NOT_FOUND, ErrorType::ResourceNotFound));
    let answer_6 = tenant
        .with_responses()
        .with_accept(Accept::any())
        .contexts()
        .serve_details(&"https://uri.etsi.org/ngsi-ld/v1/ngsi-ld-core-context-v1.8.jsonld".parse::<ContextId>()?)
        .send()
        .await;
    expect_that!(&answer_6, problem(StatusCode::NOT_FOUND, ErrorType::ResourceNotFound));
    Ok(())
}
