//! A creation context not ending with the core is hosted.

use andromeda_test_suite::{broker, config::SuiteConfig, namespace::Namespace, outcome::Done, remote_context::RemoteContexts};
use googletest::prelude::*;
use ngsi_ld::{
    http::headers::Accept,
    model::{Context, Entity, NgsiLdMediaType},
    query::{EntitySelector, builders::SelectorTypeBuilder},
    subscription::{Endpoint, NotificationParams, Subscription},
};

/// Pins: A Subscription created under a Link header naming a context that does not end with the core context is
/// retrieved with `jsonldContext` naming a context the broker serves, not the context the Link named
/// Clause: 5.8.2.4, 5.13.1, 6.3.5
/// Recorded: the applicable @context is the named context followed by the core context, which no single URI names;
/// Andromeda hosts it as an `ImplicitlyCreated` context and initialises `jsonldContext` with the URL it serves it
/// under
#[gtest]
#[tokio::test]
async fn o_sub_cre_9010_a_creation_context_not_ending_with_the_core_is_hosted() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let contexts = RemoteContexts::start().await?;
    let config = SuiteConfig::get()?;
    let unreachable = config.unreachable_origin().origin().ascii_serialization();
    let building_id = namespace.urn("Building", "creation-context-hosted")?;
    let building_type = namespace.entity_type("Building")?;
    let subscription_id = namespace.subscription_id("creation-context-hosted")?;
    let suite_context = contexts
        .serve(
            &format!("contexts/{namespace}/suite"),
            &format!(r#"{{"@context": {{"Probe": "https://test.andromeda/{namespace}/Probe"}}}}"#),
            Some("application/ld+json"),
        )
        .await?;
    let building = Entity::builder()
        .id(building_id.clone())
        .entity_type(building_type.clone())
        .property("name", "Tour Eiffel")
        .build()?;
    broker.entities().create(&building).send().await?.done()?;
    let linked = broker.with_context(Context::Single(contexts.reference(&format!("contexts/{namespace}/suite"))?))?;
    let subscription = Subscription::builder()
        .id(subscription_id.clone())
        .entities([EntitySelector::new(SelectorTypeBuilder::from(&building_type).build()?).with_ids(building_id)])
        .notification(NotificationParams::builder(Endpoint::builder(format!("{unreachable}/notify")).accept(NgsiLdMediaType::Json).build()?).build()?)
        .build()?;
    linked.subscriptions().create(&subscription).send().await?;
    let stored = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .subscriptions()
        .retrieve(&subscription_id)
        .send()
        .await?;
    expect_that!(
        stored.jsonld_context().map(url::Url::as_str),
        some(contains_substring("/ngsi-ld/v1/jsonldContexts/"))
    );
    expect_that!(stored.jsonld_context(), some(not(eq(&suite_context))));
    broker.subscriptions().delete(&subscription_id).send().await?;
    Ok(())
}
