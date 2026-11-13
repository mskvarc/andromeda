//! A failed connection to a Subscription's MQTT server leaves the broker ready.

use andromeda_test_suite::{
    broker,
    config::BrokerRole,
    eventually::{Poll, delivery_deadline, eventually},
    health_check::{AggregateStatus, HealthDocument, IndicatorName, MqttConnection, MqttConnectionHealth},
    mqtt_server::MqttServer,
    namespace::Namespace,
    outcome::Done,
    readiness_probe::ReadinessProbe,
};
use googletest::prelude::*;
use http::StatusCode;
use ngsi_ld::{
    model::{Entity, NgsiLdMediaType},
    subscription::{Endpoint, NotificationParams, NotificationTrigger, Subscription},
};

/// The connection to `server` that the `mqtt` component of `document` lists.
fn connection_to(document: &HealthDocument, server: &MqttServer) -> Option<MqttConnection> {
    let details = document.health_check.components.get(&IndicatorName::Mqtt)?.mqtt_details().ok()?;
    details
        .connections
        .into_iter()
        .find(|connection| connection.port == server.port() && connection.url_host().is_ok_and(|host| host == *server.host()))
}

/// Pins: once the MQTT server that received a notification of a Subscription in the default tenant stops, GET /readyz
/// on the management listener answers 200 with the aggregate degraded, and its `mqtt` component reports degraded and
/// lists the connection to that server as unhealthy
/// Clause: -
/// Recorded: Andromeda extension, not v1.9.1: the readiness probe and its `HealthCheck` body
#[gtest]
#[tokio::test]
async fn a_obs_rdy_9002_a_failed_connection_to_a_subscriptions_mqtt_server_leaves_the_broker_ready() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let server = MqttServer::start().await?;
    let probe_type = namespace.entity_type("Probe")?;
    let subscription_id = namespace.subscription_id(1)?;
    let subscription = Subscription::builder()
        .id(subscription_id.clone())
        .entities([probe_type.as_str()])
        .notification_triggers([NotificationTrigger::EntityCreated])
        .notification(
            NotificationParams::builder(
                Endpoint::builder(server.endpoint(&format!("{namespace}/probe"))?)
                    .accept(NgsiLdMediaType::Json)
                    .build()?,
            )
            .build()?,
        )
        .build()?;
    broker.subscriptions().create(&subscription).send().await?;
    let probe = Entity::builder().id(namespace.urn("Probe", 1)?).entity_type(probe_type).build()?;
    broker.entities().create(&probe).send().await?.done()?;
    server.wait_for(1).await?;
    // Deleted before the server stops, so no later delivery dials the server again and replaces the failed connection.
    broker.subscriptions().delete(&subscription_id).send().await?;
    server.stop();
    let probe = ReadinessProbe::of(BrokerRole::FrontManagement)?;
    let (probe, server) = (&probe, &server);
    let (status, document) = eventually(delivery_deadline(), || async move {
        match probe.ask().await {
            Ok(reading) => match connection_to(&reading.document, server).map(|connection| connection.health) {
                Some(MqttConnectionHealth::Unhealthy) => Poll::Ready((reading.status, reading.document)),
                seen => Poll::Pending(format!(
                    "/readyz answered {} listing the connection to the stopped server as {seen:?}",
                    reading.status
                )),
            },
            Err(failure) => Poll::Pending(format!("{failure:?}")),
        }
    })
    .await?;
    expect_that!(status, eq(StatusCode::OK));
    expect_that!(document.health_check.status, eq(AggregateStatus::Degraded));
    let mqtt = document.health_check.components.get(&IndicatorName::Mqtt).or_fail()?;
    expect_that!(mqtt.status, eq("degraded"));
    expect_that!(mqtt.mqtt_details()?.unhealthy_connections, ge(1));
    Ok(())
}
