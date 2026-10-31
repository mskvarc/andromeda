//! A tenants context is served under its header.

use andromeda_test_suite::{broker, context_details, hosted_context::HostedContext, namespace::Namespace};
use googletest::prelude::*;
use ngsi_ld::{
    http::headers::Accept,
    model::{Context, ContextEntry, InlineContext, StoredContextKind},
};

/// Pins: Serve @context with NGSILD-Tenant naming the tenant that added a Hosted @context answers the document as
/// stored, and its details as kind Hosted
/// Clause: 4.14, 6.3.14, 5.13.4.4
/// Tenant: private (the subject is serving a non-default tenant's @context, clauses 4.14 and 6.3.14)
#[gtest]
#[tokio::test]
async fn n_ctx_srv_0019_a_tenants_context_is_served_under_its_header() -> Result<()> {
    let namespace = Namespace::fresh();
    let tenant = broker::private_tenant(&broker::front()?, &namespace).await?;
    let context = Context::Single(ContextEntry::Inline(
        format!(r#"{{"Spot": "https://test.andromeda/{namespace}/Spot"}}"#).parse::<InlineContext>()?,
    ));
    let ctx = HostedContext::add(&tenant, &context).await?;
    let served = tenant.with_accept(Accept::any()).contexts().serve(&ctx.id).send().await?;
    expect_that!(served, eq(&context));
    let answer = context_details::serve_details(&tenant, &ctx.id).await?;
    expect_that!(context_details::metadata(&answer)?.kind, eq(&StoredContextKind::Hosted));
    Ok(())
}
