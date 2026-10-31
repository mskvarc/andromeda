//! Serve under a tenant that does not exist is nonexistent tenant.

use andromeda_test_suite::{broker, namespace::Namespace, problem::problem};
use googletest::prelude::*;
use http::StatusCode;
use ngsi_ld::{
    http::headers::Accept,
    model::{ContextId, ErrorType, Tenant},
};

/// Pins: Serve @context with NGSILD-Tenant naming a tenant that does not exist answers 404 `NonexistentTenant`
/// Clause: 5.5.10, 6.3.14, 5.13.4.4
/// Tenant: private (the subject is a tenant that does not exist)
/// Recorded: clause 5.5.10 says an operation on a tenant that does not exist "should" raise `NonexistentTenant`;
/// Andromeda raises it on Serve @context, which creates no tenant
#[gtest]
#[tokio::test]
async fn o_ctx_srv_0022_serve_under_a_tenant_that_does_not_exist_is_nonexistent_tenant() -> Result<()> {
    let namespace = Namespace::fresh();
    let front = broker::front()?;
    broker::private_tenant(&front, &namespace).await?;
    let missing = front.with_tenant(format!("{namespace}-none").parse::<Tenant>()?);
    let answer = missing
        .with_responses()
        .with_accept(Accept::any())
        .contexts()
        .serve(&format!("urn:ngsi-ld:JsonLdContext:{namespace}-any").parse::<ContextId>()?)
        .send()
        .await;
    expect_that!(&answer, problem(StatusCode::NOT_FOUND, ErrorType::NonexistentTenant));
    Ok(())
}
