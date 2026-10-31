//! A subscription created without a context is initialised with the core context.

use andromeda_test_suite::{broker, config::SuiteConfig, core_context::core_context, namespace::Namespace, notification_sink::NotificationSink};
use googletest::prelude::*;
use ngsi_ld::{
    http::headers::Accept,
    model::{NgsiLdMediaType, Representation},
    subscription::{Endpoint, NotificationParams, Subscription},
};

/// Pins: Create Context Source Registration Subscription with an application/json body and no Link header
/// initialises `jsonldContext` with the Core @context, the @context applicable when the request supplies none, as
/// one created under a Link naming the Core @context does
/// Clause: 5.8.1.4, 5.11.2.4, 5.5.5, 6.3.5
#[gtest]
#[tokio::test]
async fn n_css_cre_0006_a_subscription_created_without_a_context_is_initialised_with_the_core_context() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let sink = NotificationSink::start().await?;
    let building_type = namespace.entity_type("Building")?;
    let config = SuiteConfig::get()?;
    let core_context_link = broker.with_context(core_context()?)?;
    let subscription_1_id = namespace.subscription_id(1)?;
    let subscription_2_id = namespace.subscription_id(2)?;
    let subscription_1 = Subscription::registration_builder()
        .id(subscription_1_id.clone())
        .entities([building_type.as_str()])
        .notification(
            NotificationParams::builder(Endpoint::builder(sink.endpoint(1)?).accept(NgsiLdMediaType::Json).build()?)
                .format(Representation::Simplified)
                .build()?,
        )
        .build()?;
    broker.registration_subscriptions().create(&subscription_1).send().await?;
    let subscription_2 = Subscription::registration_builder()
        .id(subscription_2_id.clone())
        .entities([building_type.as_str()])
        .notification(
            NotificationParams::builder(Endpoint::builder(sink.endpoint(1)?).accept(NgsiLdMediaType::Json).build()?)
                .format(Representation::Simplified)
                .build()?,
        )
        .build()?;
    core_context_link.registration_subscriptions().create(&subscription_2).send().await?;
    let retrieved_2 = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .registration_subscriptions()
        .retrieve(&subscription_2_id)
        .send()
        .await?;
    expect_that!(retrieved_2.jsonld_context(), some(eq(config.core_context())));
    let retrieved_1 = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .registration_subscriptions()
        .retrieve(&subscription_1_id)
        .send()
        .await?;
    expect_that!(retrieved_1.jsonld_context(), some(eq(config.core_context())));
    broker.registration_subscriptions().delete(&subscription_1_id).send().await?;
    broker.registration_subscriptions().delete(&subscription_2_id).send().await?;
    Ok(())
}
