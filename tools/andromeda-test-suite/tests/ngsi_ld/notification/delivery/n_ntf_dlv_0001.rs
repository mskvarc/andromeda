//! An mqtt notification reaches the topic of every endpoint form.

use andromeda_test_suite::{broker, config::SuiteConfig, identity_matcher::has_id, mqtt_observer::MqttObserver, namespace::Namespace, nth::nth, outcome::Done};
use googletest::prelude::*;
use ngsi_ld::{
    model::{Entity, Lossless, NgsiLdMediaType},
    subscription::{Endpoint, NotificationParams, NotificationTrigger, Subscription},
};
use url::Url;

/// Pins: A notification of a subscription whose endpoint is an mqtt URI, with no user and the default port, a
/// non-default port, a user alone, a user and password, or a user, password and non-default port, is published once
/// on that broker, port and topic, as a message whose metadata is an object and whose body is the Notification of
/// that subscription
/// Clause: 7.2, 5.8.6, 5.2.15
#[gtest]
#[tokio::test]
async fn n_ntf_dlv_0001_an_mqtt_notification_reaches_the_topic_of_every_endpoint_form() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let config = SuiteConfig::get()?;
    let mqtt = config.mqtt()?;
    let building_type = namespace.entity_type("Building")?;
    let building_id = namespace.urn("Building", 1)?;
    let non_default = mqtt.non_default_port();
    let host = mqtt.host().to_string();
    let auth_host = mqtt.auth_host().to_string();
    let forms = [
        (1, format!("mqtt://{host}/{namespace}/forms/1"), host.clone(), 1883),
        (2, format!("mqtt://{host}:{non_default}/{namespace}/forms/2"), host.clone(), non_default),
        (3, format!("mqtt://user@{host}/{namespace}/forms/3"), host.clone(), 1883),
        (
            4,
            format!("mqtt://user_with_password:password@{auth_host}/{namespace}/forms/4"),
            auth_host.clone(),
            1883,
        ),
        (
            5,
            format!("mqtt://user_with_password:password@{auth_host}:{non_default}/{namespace}/forms/5"),
            auth_host.clone(),
            non_default,
        ),
    ];
    let mut observers = Vec::new();
    for (n, uri, observed_host, port) in &forms {
        observers.push(
            MqttObserver::subscribe(
                observed_host,
                mqtt.observer_port(),
                *port,
                &format!("{namespace}/forms/{n}"),
                &format!("{namespace}-forms-{n}"),
            )
            .await?,
        );
        let subscription = Subscription::builder()
            .id(namespace.subscription_id(n)?)
            .entities([building_type.as_str()])
            .notification_triggers([NotificationTrigger::EntityCreated])
            .notification(NotificationParams::builder(Endpoint::builder(uri.parse::<Url>()?).accept(NgsiLdMediaType::Json).build()?).build()?)
            .build()?;
        broker.subscriptions().create(&subscription).send().await?;
    }
    let building = Entity::builder().id(building_id.clone()).entity_type(building_type.clone()).build()?;
    broker.entities().create(&building).send().await?.done()?;
    for ((n, _, _, _), observer) in forms.iter().zip(&observers) {
        observer.wait_for(1).await?;
        let messages = observer.received().await;
        expect_that!(messages, len(eq(1)));
        let message = messages.first().or_fail()?;
        let notification = message.notification(Lossless::Normalized)?;
        expect_that!(notification.body.subscription_id(), eq(&namespace.subscription_id(n)?));
        expect_that!(notification.body.data().as_slice(), nth(0, has_id(&building_id)));
    }
    Ok(())
}
