//! Sys attrs adds the timestamps and leaves the other members.

use andromeda_test_suite::{broker, config::SuiteConfig, core_context::core_context_list, namespace::Namespace, notification_sink::NotificationSink};
use googletest::prelude::*;
use ngsi_ld::{
    http::{ContextDelivery, headers::Accept, operation::SystemAttributes},
    model::{NgsiLdMediaType, Representation},
    subscription::{Endpoint, NotificationParams, Subscription},
};

/// Pins: Retrieve Context Source Registration Subscription without options answers no `createdAt` and with
/// `options=sysAttrs` answers one, and both answer the same `jsonldContext` the subscription was initialised with
/// Clause: 5.11.4.4, 5.2.12, 5.8.1.4, 6.3.11
#[gtest]
#[tokio::test]
async fn n_css_ret_0001_sys_attrs_adds_the_timestamps_and_leaves_the_other_members() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let sink = NotificationSink::start().await?;
    let config = SuiteConfig::get()?;
    let core_in_body = broker.with_context(core_context_list()?)?.with_context_delivery(ContextDelivery::LdJsonBody);
    let subscription_id = namespace.subscription_id(1)?;
    let building_type = namespace.entity_type("Building")?;
    let subscription = Subscription::registration_builder()
        .id(subscription_id.clone())
        .entities([building_type.as_str()])
        .notification(
            NotificationParams::builder(Endpoint::builder(sink.endpoint(1)?).accept(NgsiLdMediaType::Json).build()?)
                .format(Representation::Simplified)
                .build()?,
        )
        .build()?;
    core_in_body.registration_subscriptions().create(&subscription).send().await?;
    let plain = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .registration_subscriptions()
        .retrieve(&subscription_id)
        .send()
        .await?;
    expect_that!(plain.id(), eq(&subscription_id));
    expect_that!(plain.output().created_at, none());
    expect_that!(plain.jsonld_context(), some(eq(config.core_context())));
    let with_system_attributes = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .registration_subscriptions()
        .retrieve(&subscription_id)
        .system_attributes(SystemAttributes::Include)
        .send()
        .await?;
    expect_that!(with_system_attributes.output().created_at, some(anything()));
    expect_that!(with_system_attributes.jsonld_context(), eq(plain.jsonld_context()));
    broker.registration_subscriptions().delete(&subscription_id).send().await?;
    Ok(())
}
