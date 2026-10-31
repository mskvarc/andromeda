//! With a configured api root every minted url is built on it.

use andromeda_test_suite::{
    attribute_matcher::any_attribute,
    broker,
    config::{BrokerRole, SuiteConfig},
    context_details::metadata,
    entity_matcher::has_attribute,
    hosted_context::HostedContext,
    minted_wrapper::{linked_context, minted_wrapper, served_document},
    namespace::Namespace,
    outcome::Done,
    problem::answered,
    remote_context::RemoteContexts,
    served_context::ServedContext,
};
use googletest::prelude::*;
use http::StatusCode;
use iri_rs::IriRefBuf;
use ngsi_ld::{
    http::{
        ContextDelivery,
        headers::{Accept, Location},
    },
    model::{Context, ContextEntry, Entity, InlineContext, NgsiLdMediaType},
    subscription::{Endpoint, NotificationParams, Subscription},
};

/// Pins: On a broker whose configured `apiRoot` carries a path prefix, a response @context wrapper, a
/// Subscription's `jsonldContext` and a Hosted @context's serving URL are all minted under that `apiRoot`, prefix
/// included, with no query; the Location of Add @context stays the relative path; and a reference to the hosted
/// @context under the prefix, or under the bare API path, on an origin nothing serves, is resolved from storage
/// Clause: 6.2, 5.13.2, 6.29.3.1, 6.3.5
/// Recorded: Andromeda extension, not v1.9.1: `server.public_url` is the `apiRoot` every minted URL is built
/// on (clause 6.2 allows a prefix string), and a reference whose path names the broker's own @context under
/// it, on any origin, names that stored document
#[gtest]
#[tokio::test]
async fn a_ctx_srv_0028_with_a_configured_api_root_every_minted_url_is_built_on_it() -> Result<()> {
    let namespace = Namespace::fresh();
    let config = SuiteConfig::get()?;
    let broker_prefixed = broker::client(BrokerRole::Prefixed)?;
    let api_root = config.api_root_prefixed()?.as_str().trim_end_matches('/').to_owned();
    let contexts = RemoteContexts::start().await?;
    let terms_url = contexts
        .serve(
            "jc/terms.jsonld",
            &format!(
                r#"{{"@context": {{"Device_{namespace}": "https://test.andromeda/{namespace}/Device", "speed": "https://test.andromeda/{namespace}/speed"}}}}"#
            ),
            Some("application/ld+json"),
        )
        .await?;
    let linked = broker_prefixed.with_context(Context::Single(contexts.reference("jc/terms.jsonld")?))?;
    let device = Entity::builder()
        .id(namespace.urn("Device", 1)?)
        .entity_type(namespace.entity_type("Device")?)
        .property("speed", 7)
        .build()?;
    linked.entities().create(&device).send().await?.done()?;
    let received = linked
        .with_responses()
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .retrieve(&namespace.urn("Device", 1)?)
        .send()
        .await?;
    let link = linked_context(&received.head)?;
    expect_that!(link.as_str(), starts_with(format!("{api_root}/ngsi-ld/v1/jsonldContexts/urn:")));
    expect_that!(link.as_str(), not(contains_substring("?")));
    let wrapper = minted_wrapper(&received.head)?;
    let document = served_document(&broker_prefixed, &wrapper).await?;
    expect_that!(document.context.entries(), len(eq(2)));
    expect_that!(
        document.context.references().map(IriRefBuf::as_str).collect::<Vec<_>>(),
        elements_are![eq(&terms_url.as_str()), eq(&config.core_context().as_str())]
    );
    let subscription_id = namespace.subscription_id(1)?;
    let subscription = Subscription::builder()
        .id(subscription_id.clone())
        .entities([namespace.suffixed("Device")])
        .notification(NotificationParams::builder(Endpoint::builder(config.unreachable_origin().join("notify")?.as_str()).build()?).build()?)
        .build()?;
    let created = broker_prefixed
        .with_context(Context::Multiple(vec![
            contexts.reference("jc/terms.jsonld")?,
            ContextEntry::Inline(format!(r#"{{"rate": "https://example.org/{namespace}/rate"}}"#).parse::<InlineContext>()?),
        ]))?
        .with_context_delivery(ContextDelivery::LdJsonBody)
        .with_responses()
        .subscriptions()
        .create(&subscription)
        .send()
        .await?;
    expect_that!(
        created.head.typed::<Location>().valid().map(|location| location.as_iri_ref().as_str()),
        some(starts_with("/broker/ngsi-ld/v1/subscriptions/"))
    );
    let retrieved = broker_prefixed
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .subscriptions()
        .retrieve(&subscription_id)
        .send()
        .await?;
    let subscription_context = retrieved.jsonld_context().or_fail()?;
    expect_that!(subscription_context.as_str(), starts_with(format!("{api_root}/ngsi-ld/v1/jsonldContexts/urn:")));
    expect_that!(subscription_context.as_str(), not(contains_substring("?")));
    let hosted = HostedContext::add(
        &broker_prefixed,
        &Context::Single(ContextEntry::Inline(
            format!(r#"{{"advertisedSpeed_{namespace}": "https://example.org/{namespace}/advertisedSpeed"}}"#).parse::<InlineContext>()?,
        )),
    )
    .await?;
    expect_that!(hosted.location, starts_with("/broker/ngsi-ld/v1/jsonldContexts/urn:"));
    let details = ServedContext::at(&broker_prefixed, &hosted.id.to_string()).details().await?;
    expect_that!(&details, answered(StatusCode::OK));
    expect_that!(
        metadata(details.as_ref().or_fail()?)?.url.as_str(),
        eq(&format!("{api_root}/ngsi-ld/v1/jsonldContexts/{}", hosted.id))
    );
    let advertised = format!("https://example.org/{namespace}/advertisedSpeed");
    for (suffix, reference) in [
        (
            "ref-1",
            format!("https://advertised-reference.invalid/broker/ngsi-ld/v1/jsonldContexts/{}", hosted.id),
        ),
        ("ref-2", format!("https://advertised-reference.invalid/ngsi-ld/v1/jsonldContexts/{}", hosted.id)),
    ] {
        let referring = broker_prefixed.with_context(Context::Single(ContextEntry::Reference(IriRefBuf::new(reference)?)))?;
        let referenced_id = namespace.urn("Device", suffix)?;
        let referenced = Entity::builder()
            .id(referenced_id.clone())
            .entity_type(namespace.entity_type("Device")?)
            .property(namespace.suffixed("advertisedSpeed").as_str(), 9)
            .build()?;
        referring.entities().create(&referenced).send().await?.done()?;
        let read_back = broker_prefixed
            .with_accept(Accept::new(NgsiLdMediaType::Json))
            .entities()
            .retrieve(&referenced_id)
            .send()
            .await?;
        expect_that!(read_back.body(), has_attribute(&advertised, any_attribute().value(9)));
    }
    Ok(())
}
