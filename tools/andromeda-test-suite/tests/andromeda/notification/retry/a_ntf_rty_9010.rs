//! A durable notification its endpoint answers with a server error gets fewer attempts than one it answers 503.

use andromeda_test_suite::{broker, config::BrokerRole, eventually::retried, namespace::Namespace, notification_sink::NotificationSink, outcome::Done};
use googletest::prelude::*;
use ngsi_ld::{
    http::headers::Accept,
    model::{Entity, NgsiLdMediaType},
    subscription::{Endpoint, NotificationParams, NotificationTrigger, Subscription},
};
use std::time::Duration;
use wiremock::ResponseTemplate;

/// Longer than the 1-second longest retry step of broker-single-tenant, so an attempt still due would have arrived.
const SETTLED: Duration = Duration::from_secs(3);

/// Pins: under durable delivery, a notification whose endpoint answers every attempt with 500 is attempted 2 times
/// (`notifications.delivery.max_server_error_attempts`) and no more, while one whose endpoint answers every attempt
/// with 503 is attempted all 4 times (`notifications.delivery.max_attempts`), each attempt counted in `timesFailed`
/// Clause: 5.8.6, 5.2.14
/// Recorded: Andromeda extension, not v1.9.1: durable notification delivery on broker-single-tenant, with four
/// attempts 100 ms apart, two of them for a server error other than 502, 503 or 504
#[gtest]
#[tokio::test]
async fn a_ntf_rty_9010_a_server_error_gets_the_server_error_attempts_and_unavailability_all_of_them() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker_single_tenant = broker::client(BrokerRole::SingleTenant)?;
    let sink = NotificationSink::start().await?;
    sink.answer("server-error", ResponseTemplate::new(500)).await?;
    sink.answer("unavailable", ResponseTemplate::new(503)).await?;
    let building_type = namespace.entity_type("Building")?;
    let subscribe = async |endpoint: &str| -> Result<_> {
        let subscription_id = namespace.subscription_id(endpoint)?;
        let subscription = Subscription::builder()
            .id(subscription_id.clone())
            .entities([building_type.as_str()])
            .notification_triggers([NotificationTrigger::EntityCreated])
            .notification(NotificationParams::builder(Endpoint::builder(sink.endpoint(endpoint)?).accept(NgsiLdMediaType::Json).build()?).build()?)
            .build()?;
        broker_single_tenant.subscriptions().create(&subscription).send().await?;
        Ok(subscription_id)
    };
    let server_error_id = subscribe("server-error").await?;
    let unavailable_id = subscribe("unavailable").await?;
    let building = Entity::builder()
        .id(namespace.urn("Building", 1)?)
        .entity_type(building_type.clone())
        .property("name", "a")
        .build()?;
    broker_single_tenant.entities().create(&building).send().await?.done()?;
    let times_failed = async |subscription_id| -> Result<Option<u64>> {
        let subscription = broker_single_tenant
            .with_accept(Accept::new(NgsiLdMediaType::Json))
            .subscriptions()
            .retrieve(subscription_id)
            .send()
            .await?;
        Ok(subscription.notification().output().times_failed)
    };
    retried(async || {
        verify_that!(times_failed(&server_error_id).await?, some(eq(2)))?;
        verify_that!(times_failed(&unavailable_id).await?, some(eq(4)))
    })
    .await?;
    expect_that!(sink.count_after("server-error", SETTLED).await?, eq(2));
    expect_that!(sink.received("unavailable").await?, len(eq(4)));
    expect_that!(times_failed(&server_error_id).await?, some(eq(2)));
    expect_that!(times_failed(&unavailable_id).await?, some(eq(4)));
    broker_single_tenant.subscriptions().delete(&server_error_id).send().await?;
    broker_single_tenant.subscriptions().delete(&unavailable_id).send().await?;
    Ok(())
}
