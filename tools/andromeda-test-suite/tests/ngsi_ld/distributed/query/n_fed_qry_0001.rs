//! A forwarded query carries the context source info as headers.

use andromeda_test_suite::{
    broker,
    cleanup::Cleanup,
    context_source::ContextSource,
    core_context::core_entry,
    header_condition::HeaderCondition,
    hosted_context::HostedContext,
    namespace::Namespace,
    source_answer,
    source_request::SourceRequest,
};
use googletest::prelude::*;
use http::{
    HeaderName,
    HeaderValue,
    Method,
    StatusCode,
    header::{ACCEPT, CONTENT_TYPE, LINK},
};
use ngsi_ld::{
    http::headers::Accept,
    model::{Context, ContextEntry, InlineContext, NgsiLdMediaType},
    registration::{CSourceRegistration, DistributedOperation, EntityInfo, RegistrationInfo},
};

/// Pins: a Query Entities forwarded to a registration with `contextSourceInfo` carries the accept value as Accept, a
/// custom key as a header of its own, the `jsonldContext` URL in a context Link, and no Content-Type, the forwarded GET
/// having no body
/// Clause: 4.3.6.5, 4.3.6.6, 6.3.19, 5.12
#[gtest]
#[tokio::test]
async fn n_fed_qry_0001_a_forwarded_query_carries_the_context_source_info_as_headers() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let source = ContextSource::start().await?;
    let vehicle_type = namespace.entity_type("Vehicle")?;
    let registration_id = namespace.registration_id(1)?;
    let context = Context::Multiple(vec![
        ContextEntry::Inline(format!(r#"{{"speed": "https://test.andromeda/{namespace}/speed"}}"#).parse::<InlineContext>()?),
        core_entry()?,
    ]);
    let ctx = HostedContext::add(&broker, &context).await?;
    source
        .answer_when(
            SourceRequest::any_matching("/.*").priority(1),
            source_answer::text(StatusCode::OK, HeaderValue::from_static("application/ld+json"), "[]"),
        )
        .await?;
    let registration = CSourceRegistration::builder(source.root_endpoint()?)
        .id(registration_id.clone())
        .information([RegistrationInfo::builder()
            .entities([EntityInfo::new(vehicle_type.clone()).with_id_pattern(namespace.id_pattern("Vehicle")?)])
            .build()?])
        .operations([DistributedOperation::QueryEntity])
        .context_source_info(("accept", "application/ld+json"))
        .context_source_info(("contentType", "application/json"))
        .context_source_info(("jsonldContext", ctx.url.as_str()))
        .context_source_info(("X-Custom-Auth", "token-abc"))
        .build()?;
    broker.registrations().register(&registration).send().await?;
    // Registrations are deleted when the test ends: a forward to one left behind fails at its endpoint, and the
    // cooldown that opens there holds back a later test's registration naming the same endpoint.
    let _registration_cleanup = Cleanup::of(&broker).registration(&registration_id);
    broker
        .with_accept(Accept::new(NgsiLdMediaType::JsonLd))
        .entities()
        .query()
        .entity_type(&vehicle_type)
        .id_pattern(namespace.id_pattern("Vehicle")?)
        .send()
        .await?;
    let forwarded_request = SourceRequest::new(Method::GET, "/ngsi-ld/v1/entities");
    expect_that!(source.received_matching(&forwarded_request).await?, len(eq(1)));
    // Clause 6.3.19 sets Content-Type only "whenever a payload body is present"; a forwarded query has none.
    let forwarded_request_2 = SourceRequest::new(Method::GET, "/ngsi-ld/v1/entities")
        .header(HeaderCondition::Is(ACCEPT, "application/ld+json".to_owned()))
        .header(HeaderCondition::Is(HeaderName::from_static("x-custom-auth"), "token-abc".to_owned()))
        .header(HeaderCondition::Contains(LINK, format!("<{}>", ctx.url)))
        .header(HeaderCondition::Absent(CONTENT_TYPE));
    expect_that!(source.received_matching(&forwarded_request_2).await?, len(eq(1)));
    Ok(())
}
