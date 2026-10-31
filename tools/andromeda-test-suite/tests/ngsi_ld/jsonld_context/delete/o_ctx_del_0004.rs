//! Reload the core context is resource not found.

use andromeda_test_suite::{broker, namespace::Namespace, problem::problem};
use googletest::prelude::*;
use http::StatusCode;
use ngsi_ld::{
    http::{headers::Accept, operation::ContextDeletion},
    model::{ContextId, ErrorType},
};

/// Pins: Delete @context with reload=true of the core @context is refused as `ResourceNotFound`, before and after a
/// Serve of it
/// Clause: 5.13.5.4, 5.13.4.4
/// Tenant: private (the case addresses the core @context URL, which no operation may store or replace; a broker that
/// failed to refuse it could act on the shared tenant)
/// Recorded: clause 5.13.5.4 raises `ResourceNotFound` for an identifier no @context storage holds; the core @context is
/// not a stored entry, so its URL is one
#[gtest]
#[tokio::test]
async fn o_ctx_del_0004_reload_the_core_context_is_resource_not_found() -> Result<()> {
    let namespace = Namespace::fresh();
    let tenant = broker::private_tenant(&broker::front()?, &namespace).await?;
    let core_id = "https://uri.etsi.org/ngsi-ld/v1/ngsi-ld-core-context-v1.9.jsonld".parse::<ContextId>()?;
    let served = tenant.with_responses().with_accept(Accept::any()).contexts().serve(&core_id).send().await;
    expect_that!(&served, problem(StatusCode::NOT_FOUND, ErrorType::ResourceNotFound));
    let reloaded = tenant
        .with_responses()
        .contexts()
        .delete(&core_id)
        .deletion(ContextDeletion::Reload)
        .send()
        .await;
    expect_that!(&reloaded, problem(StatusCode::NOT_FOUND, ErrorType::ResourceNotFound));
    let served_again = tenant.with_responses().with_accept(Accept::any()).contexts().serve(&core_id).send().await;
    expect_that!(&served_again, problem(StatusCode::NOT_FOUND, ErrorType::ResourceNotFound));
    Ok(())
}
