//! A link header context initialises the jsonld context.

use andromeda_test_suite::{
    broker,
    config::SuiteConfig,
    namespace::Namespace,
    notification_sink::NotificationSink,
    outcome::Done,
    remote_context::RemoteContexts,
};
use googletest::prelude::*;
use ngsi_ld::{
    http::headers::Accept,
    model::{Context, Entity, NgsiLdMediaType},
    query::{EntitySelector, builders::SelectorTypeBuilder},
    subscription::{Endpoint, NotificationParams, Subscription},
};

/// Pins: A Subscription created with no `jsonldContext` member and no @context member, under a Link header naming a
/// context that ends with the core context, is retrieved with `jsonldContext` holding that context
/// Clause: 5.8.2.4, 6.3.5
#[gtest]
#[tokio::test]
async fn n_sub_cre_9009_a_link_header_context_initialises_the_jsonld_context() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let sink = NotificationSink::start().await?;
    let contexts = RemoteContexts::start().await?;
    let config = SuiteConfig::get()?;
    let core = config.core_context();
    let building_creation_context_init_id = namespace.urn("Building", "creation-context-init")?;
    let building_type = namespace.entity_type("Building")?;
    let linked = broker.with_context(Context::Single(contexts.reference("contexts/compound")?))?;
    let subscription_creation_context_init_id = namespace.subscription_id("creation-context-init")?;
    contexts
        .serve(
            "contexts/compound",
            &format!(
                r#"{{
      "@context": [
        {{
          "Probe": "https://test.andromeda/{namespace}/Probe"
        }},
        "{core}"
      ]
    }}"#
            ),
            Some("application/ld+json"),
        )
        .await?;
    let building_creation_context_init = Entity::builder()
        .id(building_creation_context_init_id.clone())
        .entity_type(building_type.clone())
        .property("name", "Tour Eiffel")
        .build()?;
    broker.entities().create(&building_creation_context_init).send().await?.done()?;
    let subscription_creation_context_init = Subscription::builder()
        .id(subscription_creation_context_init_id.clone())
        .entities([EntitySelector::new(SelectorTypeBuilder::from(&building_type).build()?).with_ids(building_creation_context_init_id.clone())])
        .notification(
            NotificationParams::builder(
                Endpoint::builder(sink.endpoint("creation-context-init")?)
                    .accept(NgsiLdMediaType::Json)
                    .build()?,
            )
            .build()?,
        )
        .build()?;
    linked.subscriptions().create(&subscription_creation_context_init).send().await?;
    let subscription = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .subscriptions()
        .retrieve(&subscription_creation_context_init_id)
        .send()
        .await?;
    expect_that!(subscription.jsonld_context(), some(eq(&contexts.url("contexts/compound")?)));
    broker.subscriptions().delete(&subscription_creation_context_init_id).send().await?;
    Ok(())
}
