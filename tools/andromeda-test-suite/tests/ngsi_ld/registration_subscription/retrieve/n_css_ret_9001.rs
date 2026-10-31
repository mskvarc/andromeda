//! A csource subscription reads back every member it was written with.

use andromeda_test_suite::{
    broker,
    core_context::{core_context, core_context_list},
    namespace::Namespace,
    notification_sink::NotificationSink,
};
use googletest::prelude::*;
use ngsi_ld::{
    http::{ContextDelivery, headers::Accept},
    model::{DateTime, NgsiLdMediaType, Representation},
    query::q::Query,
    subscription::{Endpoint, NotificationParams, Subscription},
};

/// Pins: Create Context Source Registration Subscription with `subscriptionName`, description, entities, q,
/// `expiresAt` and a `keyValues` notification answers 201, and Retrieve as application/ld+json under the Core
/// @context answers each of them as written
/// Clause: 5.11.2, 5.11.4.5, 5.2.12
#[gtest]
#[tokio::test]
async fn n_css_ret_9001_a_csource_subscription_reads_back_every_member_it_was_written_with() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let sink = NotificationSink::start().await?;
    let core_in_body = broker.with_context(core_context_list()?)?.with_context_delivery(ContextDelivery::LdJsonBody);
    let core_linked = broker.with_context(core_context()?)?;
    let subscription_id = namespace.subscription_id(1)?;
    let vehicle_type = namespace.entity_type("Vehicle")?;
    let expires_at = "2099-01-01T00:00:00Z".parse::<DateTime>()?;
    let subscription = Subscription::registration_builder()
        .id(subscription_id.clone())
        .entities([vehicle_type.as_str()])
        .notification(
            NotificationParams::builder(Endpoint::builder(sink.endpoint(1)?).accept(NgsiLdMediaType::Json).build()?)
                .format(Representation::Simplified)
                .build()?,
        )
        .subscription_name("csource typed round trip")
        .description("every member the client may supply")
        .q("speed>10".parse::<Query>()?)
        .expires_at(expires_at)
        .build()?;
    core_in_body.registration_subscriptions().create(&subscription).send().await?;
    let retrieved = core_linked
        .with_accept(Accept::new(NgsiLdMediaType::JsonLd))
        .registration_subscriptions()
        .retrieve(&subscription_id)
        .send()
        .await?;
    expect_that!(retrieved.name(), some(eq("csource typed round trip")));
    expect_that!(retrieved.description(), some(eq("every member the client may supply")));
    expect_that!(retrieved.q(), some(eq(&"speed>10".parse::<Query>()?)));
    expect_that!(retrieved.expires_at(), some(eq(&expires_at)));
    expect_that!(retrieved.entities(), len(eq(1)));
    expect_that!(
        retrieved.entities().first().map(|selector| selector.render().to_string()),
        some(contains_substring(vehicle_type.as_str()))
    );
    expect_that!(retrieved.notification().format(), some(eq(Representation::Simplified)));
    expect_that!(retrieved.notification().endpoint().uri(), eq(&sink.endpoint(1)?));
    expect_that!(retrieved.notification().endpoint().accept(), some(eq(NgsiLdMediaType::Json)));
    broker.registration_subscriptions().delete(&subscription_id).send().await?;
    Ok(())
}
