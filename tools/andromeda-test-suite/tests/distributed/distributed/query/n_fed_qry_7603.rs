//! Context source info headers accept and context govern the forwarded query.

use andromeda_test_suite::{
    broker,
    context_source::ContextSource,
    core_context::core_entry,
    header_condition::HeaderCondition,
    hosted_context::HostedContext,
    namespace::Namespace,
    source_answer,
    source_request::SourceRequest,
};
use googletest::prelude::*;
use http::{HeaderName, Method};
use ngsi_ld::{
    http::headers::Accept,
    model::{Context, ContextEntry, InlineContext},
    registration::{CSourceRegistration, DistributedOperation, EntityInfo, RegistrationInfo, RegistrationMode},
};

/// Pins: Query Entities on broker A under an inclusive registration with `contextSourceInfo` X-Auth abcd, accept
/// application/ld+json and a `jsonldContext` URL reaches the source once with the X-Auth header, an Accept of
/// application/ld+json and a Link naming that context
/// Clause: 4.3.6.6, 5.2.9, 6.3.5
#[gtest]
#[tokio::test]
async fn n_fed_qry_7603_context_source_info_headers_accept_and_context_govern_the_forwarded_query() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let source = ContextSource::start().await?;
    let registration_id = namespace.registration_id(1)?;
    let vehicle_type = namespace.entity_type("Vehicle")?;
    let context = Context::Multiple(vec![
        ContextEntry::Inline(format!(r#"{{"speed": "https://test.andromeda/{namespace}/speed"}}"#).parse::<InlineContext>()?),
        core_entry()?,
    ]);
    let ctx = HostedContext::add(&broker, &context).await?;
    let source_entities = [];
    source
        .answer(Method::GET, "/ngsi-ld/v1/entities", source_answer::entities(&source_entities))
        .await?;
    let registration = CSourceRegistration::builder(source.root_endpoint()?)
        .id(registration_id)
        .information([RegistrationInfo::builder()
            .entities([EntityInfo::new(vehicle_type.clone()).with_id_pattern(namespace.id_pattern("Vehicle")?)])
            .build()?])
        .mode(RegistrationMode::Inclusive)
        .operations([DistributedOperation::QueryEntity])
        .context_source_info(("X-Auth", "abcd"))
        .context_source_info(("accept", "application/ld+json"))
        .context_source_info(("jsonldContext", ctx.url.as_str()))
        .build()?;
    broker.registrations().register(&registration).send().await?;
    broker.with_accept(Accept::any()).entities().query().entity_type(&vehicle_type).send().await?;
    let forwarded_request = SourceRequest::new(Method::GET, "/ngsi-ld/v1/entities")
        .header(HeaderCondition::Is(HeaderName::from_static("x-auth"), "abcd".to_owned()))
        .header(HeaderCondition::Contains(HeaderName::from_static("accept"), "application/ld+json".to_owned()))
        .header(HeaderCondition::Contains(HeaderName::from_static("link"), format!("<{}>", ctx.url)));
    let forwarded = source.received_matching(&forwarded_request).await?;
    expect_that!(forwarded, len(eq(1)));
    Ok(())
}
