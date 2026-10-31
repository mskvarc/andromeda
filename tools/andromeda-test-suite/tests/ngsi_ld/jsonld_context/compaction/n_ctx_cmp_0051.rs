//! An mqtt notification names the context under the same rule.

use andromeda_test_suite::{
    broker,
    config::SuiteConfig,
    core_context::{core_context, core_entry},
    identity_matcher::has_id,
    mqtt_observer::MqttObserver,
    namespace::Namespace,
    nth::nth,
    outcome::Done,
    remote_context::RemoteContexts,
};
use googletest::prelude::*;
use ngsi_ld::{
    http::ContextDelivery,
    model::{Context, Entity, EntityTypeName, Lossless, NgsiLdMediaType},
    subscription::{Endpoint, NotificationParams, NotificationTrigger, Subscription},
};
use url::Url;

/// Pins: An application/ld+json MQTT notification of a subscription whose `jsonldContext` is a user @context
/// without the core carries Content-Type application/ld+json in its metadata and names that @context followed by
/// the core on the Notification and on the Entity
/// Clause: 7.2, 5.8.1.4, 5.2.3
#[gtest]
#[tokio::test]
async fn n_ctx_cmp_0051_an_mqtt_notification_names_the_context_under_the_same_rule() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let contexts = RemoteContexts::start().await?;
    let config = SuiteConfig::get()?;
    let core_context_body = broker.with_context(core_context()?)?.with_context_delivery(ContextDelivery::LdJsonBody);
    let subscription_id = namespace.subscription_id(1)?;
    let mqtt = config.mqtt()?;
    let observer_jsonld_context = MqttObserver::subscribe(
        &mqtt.host().to_string(),
        mqtt.observer_port(),
        1883,
        &format!("{namespace}/jsonld-context"),
        &format!("{namespace}-jsonld-context"),
    )
    .await?;
    let device_notified_id = namespace.urn("Device", "notified")?;
    contexts
        .serve(
            "jc/user/terms.jsonld",
            &format!(
                r#"{{
      "@context": {{
        "Device_{namespace}": "https://test.andromeda/{namespace}/Device",
        "speed": "https://test.andromeda/{namespace}/speed"
      }}
    }}"#
            ),
            Some("application/ld+json"),
        )
        .await?;
    let subscription = Subscription::builder()
        .id(subscription_id.clone())
        .entities([format!("https://test.andromeda/{namespace}/Device")])
        .notification_triggers([NotificationTrigger::EntityCreated])
        .notification(
            NotificationParams::builder(
                Endpoint::builder(format!("mqtt://{}/{namespace}/jsonld-context", mqtt.host()).parse::<Url>()?)
                    .accept(NgsiLdMediaType::JsonLd)
                    .build()?,
            )
            .build()?,
        )
        .jsonld_context(contexts.url("jc/user/terms.jsonld")?)
        .build()?;
    core_context_body.subscriptions().create(&subscription).send().await?;
    let device_notified = Entity::builder()
        .id(device_notified_id.clone())
        .entity_type(format!("https://test.andromeda/{namespace}/Device").parse::<EntityTypeName>()?)
        .property(format!("https://test.andromeda/{namespace}/speed").as_str(), 3)
        .build()?;
    broker.entities().create(&device_notified).send().await?.done()?;
    observer_jsonld_context.wait_for(1).await?;
    let messages = observer_jsonld_context.received().await;
    expect_that!(messages, len(eq(1)));
    let message = messages.last().or_fail()?;
    let notification = message.notification(Lossless::Normalized)?;
    expect_that!(notification.metadata.get("Content-Type").map(String::as_str), some(eq("application/ld+json")));
    let body = &notification.body;
    expect_that!(
        body.context().map(Context::entries).and_then(|entries| entries.first()),
        some(eq(&contexts.reference("jc/user/terms.jsonld")?))
    );
    expect_that!(
        body.context().map(Context::entries).and_then(|entries| entries.get(1)),
        some(eq(&core_entry()?))
    );
    expect_that!(body.context().map(Context::entries).and_then(|entries| entries.get(2)), none());
    let item = body.data().as_slice().first().or_fail()?;
    expect_that!(
        item.context.as_ref().map(Context::entries).and_then(|entries| entries.first()),
        some(eq(&contexts.reference("jc/user/terms.jsonld")?))
    );
    expect_that!(
        item.context.as_ref().map(Context::entries).and_then(|entries| entries.get(1)),
        some(eq(&core_entry()?))
    );
    expect_that!(item.context.as_ref().map(Context::entries).and_then(|entries| entries.get(2)), none());
    expect_that!(body.data().as_slice(), nth(0, has_id(&device_notified_id)));
    Ok(())
}
