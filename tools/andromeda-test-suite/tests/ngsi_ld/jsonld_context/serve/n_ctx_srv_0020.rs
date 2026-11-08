//! A tenants context is not served to the default tenant.

use andromeda_test_suite::{broker, hosted_context::HostedContext, namespace::Namespace, problem::problem};
use googletest::prelude::*;
use http::StatusCode;
use ngsi_ld::{
    http::headers::Accept,
    model::{Context, ContextEntry, ErrorType, InlineContext},
};

/// Pins: Serve @context without NGSILD-Tenant reads the default tenant, which does not hold another tenant's Hosted
/// @context, and answers 404 `ResourceNotFound`
/// Clause: 4.14, 6.3.14, 5.13.4.4
/// Tenant: private (the subject is serving a non-default tenant's @context, clauses 4.14 and 6.3.14)
#[gtest]
#[tokio::test]
async fn n_ctx_srv_0020_a_tenants_context_is_not_served_to_the_default_tenant() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let tenant = broker::private_tenant(&broker, &namespace).await?;
    let context = Context::Single(ContextEntry::Inline(
        format!(r#"{{"Spot": "https://test.andromeda/{namespace}/Spot"}}"#).parse::<InlineContext>()?,
    ));
    let ctx = HostedContext::add(&tenant, &context).await?;
    let answer = broker.with_responses().with_accept(Accept::any()).contexts().serve(&ctx.id).send().await;
    expect_that!(&answer, problem(StatusCode::NOT_FOUND, ErrorType::ResourceNotFound));
    tenant.with_accept(Accept::any()).contexts().serve(&ctx.id).send().await?;
    Ok(())
}
