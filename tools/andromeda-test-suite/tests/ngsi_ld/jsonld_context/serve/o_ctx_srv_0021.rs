//! The former tenant parameter is invalid request.

use andromeda_test_suite::{broker, hosted_context::HostedContext, namespace::Namespace, problem::problem, wire::request_to};
use googletest::prelude::*;
use http::{Method, StatusCode};
use ngsi_ld::{
    http::headers::Accept,
    model::{Context, ContextEntry, ErrorType, InlineContext},
};

/// Pins: Serve @context with and without NGSILD-Tenant, List @contexts and Delete @context carrying the parameter
/// andromedaTenant answer 400 `InvalidRequest`, and the refused delete removes nothing
/// Clause: 6.3.20, 6.3.14, 5.13.4.4, 5.13.5.4
/// Tenant: private (the subject is serving a non-default tenant's @context, clauses 4.14 and 6.3.14)
/// Recorded: clause 6.3.20 says a parameter the operation does not define "should" be refused as `InvalidRequest`;
/// Andromeda refuses andromedaTenant, a parameter no operation defines
#[gtest]
#[tokio::test]
async fn o_ctx_srv_0021_the_former_tenant_parameter_is_invalid_request() -> Result<()> {
    let namespace = Namespace::fresh();
    let front = broker::front()?;
    let tenant = broker::private_tenant(&front, &namespace).await?;
    let context = Context::Single(ContextEntry::Inline(
        format!(r#"{{"Spot": "https://test.andromeda/{namespace}/Spot"}}"#).parse::<InlineContext>()?,
    ));
    let ctx = HostedContext::add(&tenant, &context).await?;
    let segment = ctx.url.path_segments().and_then(Iterator::last).or_fail()?.to_owned();
    // The parameter is one no operation defines, so each refused request is sent to the path as written.
    let request = request_to(&front, Method::GET, &format!("jsonldContexts/{segment}?andromedaTenant={namespace}"))?;
    let answer = front.send_unchecked(request).await;
    expect_that!(&answer, problem(StatusCode::BAD_REQUEST, ErrorType::InvalidRequest));
    let request_2 = request_to(&tenant, Method::GET, &format!("jsonldContexts/{segment}?andromedaTenant={namespace}"))?;
    let answer_2 = tenant.send_unchecked(request_2).await;
    expect_that!(&answer_2, problem(StatusCode::BAD_REQUEST, ErrorType::InvalidRequest));
    let request_3 = request_to(&tenant, Method::GET, &format!("jsonldContexts?andromedaTenant={namespace}"))?;
    let answer_3 = tenant.send_unchecked(request_3).await;
    expect_that!(&answer_3, problem(StatusCode::BAD_REQUEST, ErrorType::InvalidRequest));
    let request_4 = request_to(&tenant, Method::DELETE, &format!("jsonldContexts/{segment}?andromedaTenant={namespace}"))?;
    let answer_4 = tenant.send_unchecked(request_4).await;
    expect_that!(&answer_4, problem(StatusCode::BAD_REQUEST, ErrorType::InvalidRequest));
    tenant.with_accept(Accept::any()).contexts().serve(&ctx.id).send().await?;
    Ok(())
}
