//! A csource subscription created under the core context initialises json ld context.

use andromeda_test_suite::{broker, config::SuiteConfig, core_context::core_context_list, namespace::Namespace, notification_sink::NotificationSink};
use googletest::prelude::*;
use ngsi_ld::{
    http::{ContextDelivery, headers::Accept},
    model::NgsiLdMediaType,
    subscription::{Endpoint, NotificationParams, Subscription},
};

/// Pins: Create Context Source Registration Subscription as application/ld+json with the Core @context in the body
/// and no `jsonldContext` answers 201, and Retrieve answers `jsonldContext` as the Core @context
/// Clause: 5.8.1.4, 5.11.2.4, 5.11.4.5
#[gtest]
#[tokio::test]
async fn n_css_cre_9003_a_csource_subscription_created_under_the_core_context_initialises_json_ld_context() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let sink = NotificationSink::start().await?;
    let config = SuiteConfig::get()?;
    let core_context_list_body = broker.with_context(core_context_list()?)?.with_context_delivery(ContextDelivery::LdJsonBody);
    let subscription_id = namespace.subscription_id(1)?;
    let vehicle_type = namespace.entity_type("Vehicle")?;
    let subscription = Subscription::registration_builder()
        .id(subscription_id.clone())
        .entities([vehicle_type.as_str()])
        .notification(NotificationParams::builder(Endpoint::builder(sink.endpoint(1)?).accept(NgsiLdMediaType::Json).build()?).build()?)
        .build()?;
    core_context_list_body.registration_subscriptions().create(&subscription).send().await?;
    let subscription_2 = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .registration_subscriptions()
        .retrieve(&subscription_id)
        .send()
        .await?;
    expect_that!(subscription_2.jsonld_context(), some(eq(config.core_context())));
    broker.registration_subscriptions().delete(&subscription_id).send().await?;
    Ok(())
}
