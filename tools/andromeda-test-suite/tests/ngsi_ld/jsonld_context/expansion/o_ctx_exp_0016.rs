//! An entry held only by another tenant is not resolved from it.

use andromeda_test_suite::{broker, config::SuiteConfig, hosted_context::HostedContext, namespace::Namespace, problem::problem};
use googletest::prelude::*;
use http::StatusCode;
use iri_rs::IriRefBuf;
use ngsi_ld::{
    http::{headers::Accept, operation::ContextKindFilter},
    model::{Context, ContextEntry, Entity, ErrorType, InlineContext},
    registration::{CSourceRegistration, RegistrationInfo},
};

/// Pins: A create in another tenant under a Link naming the first tenant's stored @context by its path on an origin
/// where nothing listens is fetched as the URL it is and answers 504 `LdContextNotAvailable`, and that tenant
/// records nothing as Cached
/// Clause: 4.14, 5.13.1, 6.3.5, 5.5.2
/// Tenant: private (the subject is that one tenant's stored @context is not resolved for another, clause 4.14)
/// Recorded: clause 5.13.1 has Hosted and `ImplicitlyCreated` @contexts served from the broker's storage; Andromeda
/// resolves a reference whose path is `/ngsi-ld/v1/jsonldContexts/<identifier it holds>`, with no query, from the
/// requesting tenant's storage whatever its origin, never fetches it and never records it as Cached
#[gtest]
#[tokio::test]
async fn o_ctx_exp_0016_an_entry_held_only_by_another_tenant_is_not_resolved_from_it() -> Result<()> {
    let namespace = Namespace::fresh();
    let front = broker::front()?;
    let config = SuiteConfig::get()?;
    let unreachable = config.unreachable_origin().origin().ascii_serialization();
    let owner = broker::private_tenant(&front, &namespace).await?;
    let other = front.with_tenant(format!("{namespace}-other").parse()?);
    let provision_id = namespace.registration_id("provision-other")?;
    let provision = CSourceRegistration::builder(config.unreachable_origin().as_str())
        .id(provision_id.clone())
        .information([RegistrationInfo::builder().entities([namespace.entity_type("Sentinel")?.to_string()]).build()?])
        .build()?;
    other.registrations().register(&provision).send().await?;
    other.registrations().delete(&provision_id).send().await?;
    let context = Context::Single(ContextEntry::Inline(
        format!(r#"{{"ownSpeed": "https://test.andromeda/{namespace}/own/speed"}}"#).parse::<InlineContext>()?,
    ));
    let ctx = HostedContext::add(&owner, &context).await?;
    let linked = other.with_context(Context::Single(ContextEntry::Reference(IriRefBuf::new(format!(
        "{unreachable}{}",
        ctx.url.path()
    ))?)))?;
    let vehicle = Entity::builder()
        .id(namespace.urn("Vehicle", 1)?)
        .entity_type(namespace.entity_type("Vehicle")?)
        .property("ownSpeed", 7)
        .build()?;
    let answer = linked.with_responses().entities().create(&vehicle).send().await;
    expect_that!(&answer, problem(StatusCode::GATEWAY_TIMEOUT, ErrorType::LdContextNotAvailable));
    let cached = other
        .with_accept(Accept::any())
        .contexts()
        .list()
        .kind(ContextKindFilter::Cached)
        .send()
        .await?;
    expect_that!(cached.len(), eq(0));
    Ok(())
}
