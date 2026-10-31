//! A subscription created without a context is initialised with the core context.

use andromeda_test_suite::{broker, config::SuiteConfig, namespace::Namespace, notification_sink::NotificationSink};
use googletest::prelude::*;
use ngsi_ld::{
    http::headers::Accept,
    model::NgsiLdMediaType,
    subscription::{Endpoint, NotificationParams, Subscription},
};

/// Pins: Create Subscription with an application/json body and no Link header initialises `jsonldContext` with the
/// Core @context, the @context applicable when the request supplies none
/// Clause: 5.8.1.4, 5.5.5, 6.3.5
#[gtest]
#[tokio::test]
async fn n_sub_cre_0004_a_subscription_created_without_a_context_is_initialised_with_the_core_context() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let sink = NotificationSink::start().await?;
    let config = SuiteConfig::get()?;
    let subscription_id = namespace.subscription_id(1)?;
    let building_type = namespace.entity_type("Building")?;
    let subscription = Subscription::builder()
        .id(subscription_id.clone())
        .entities([building_type.as_str()])
        .notification(NotificationParams::builder(Endpoint::builder(sink.endpoint(1)?).accept(NgsiLdMediaType::Json).build()?).build()?)
        .build()?;
    broker.subscriptions().create(&subscription).send().await?;
    let subscription_2 = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .subscriptions()
        .retrieve(&subscription_id)
        .send()
        .await?;
    expect_that!(subscription_2.id(), eq(&subscription_id));
    expect_that!(subscription_2.jsonld_context(), some(eq(config.core_context())));
    Ok(())
}
