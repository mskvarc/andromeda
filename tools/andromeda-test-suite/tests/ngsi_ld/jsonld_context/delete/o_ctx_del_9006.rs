//! Reload of the core context is resource not found.

use andromeda_test_suite::{broker, namespace::Namespace, problem::problem};
use googletest::prelude::*;
use http::StatusCode;
use ngsi_ld::{
    http::operation::ContextDeletion,
    model::{ContextId, ErrorType},
};

/// Pins: Delete @context with reload=true of the core @context answers 404 `ResourceNotFound`
/// Clause: 5.13.5.4
/// Tenant: private (a broker that failed to refuse the reload could replace the core @context, which every file relies
/// on, in the shared tenant)
/// Recorded: clause 5.13.5.4 raises `ResourceNotFound` for an identifier no @context storage holds; the core @context is
/// not a stored entry, so its URL is one
#[gtest]
#[tokio::test]
async fn o_ctx_del_9006_reload_of_the_core_context_is_resource_not_found() -> Result<()> {
    let namespace = Namespace::fresh();
    let tenant = broker::private_tenant(&broker::front()?, &namespace).await?;
    let core_id = "https://uri.etsi.org/ngsi-ld/v1/ngsi-ld-core-context-v1.9.jsonld".parse::<ContextId>()?;
    let answer = tenant
        .with_responses()
        .contexts()
        .delete(&core_id)
        .deletion(ContextDeletion::Reload)
        .send()
        .await;
    expect_that!(&answer, problem(StatusCode::NOT_FOUND, ErrorType::ResourceNotFound));
    Ok(())
}
