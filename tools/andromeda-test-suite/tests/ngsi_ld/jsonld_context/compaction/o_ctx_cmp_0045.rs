//! A json notification links the jsonld context once.

use andromeda_test_suite::{
    broker,
    config::SuiteConfig,
    core_context::core_context,
    header_condition::HeaderCondition,
    identity_matcher::has_id,
    namespace::Namespace,
    notification_sink::NotificationSink,
    nth::nth,
    outcome::Done,
    remote_context::RemoteContexts,
};
use googletest::prelude::*;
use http::HeaderName;
use ngsi_ld::{
    http::ContextDelivery,
    model::{Entity, EntityTypeName, Lossless, NgsiLdMediaType},
    subscription::{Endpoint, NotificationParams, NotificationTrigger, Subscription},
};

/// Pins: An application/json notification carries exactly one @context link, to the subscription's `jsonldContext`
/// as written (a user @context, a URL ending with the core, the v1.8 core) or to the core @context when the
/// subscription was created under nothing else, and no @context in its body
/// Clause: 6.3.8, 5.8.1.4, JSON-LD 1.1 s6.1
/// Recorded: clause 5.8.1.4 has the notification use the @context in `jsonldContext`; Andromeda names a
/// `jsonldContext` that is itself a core release, the versionless URL included, as written, as the subscriber's
/// explicit choice
#[gtest]
#[tokio::test]
async fn o_ctx_cmp_0045_a_json_notification_links_the_jsonld_context_once() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let sink = NotificationSink::start().await?;
    let contexts = RemoteContexts::start().await?;
    let config = SuiteConfig::get()?;
    let core = config.core_context();
    let served = contexts.url("")?;
    let core_context_body = broker.with_context(core_context()?)?.with_context_delivery(ContextDelivery::LdJsonBody);
    let subscription_id = namespace.subscription_id(1)?;
    let subscription_2_id = namespace.subscription_id(2)?;
    let subscription_3_id = namespace.subscription_id(3)?;
    let subscription_4_id = namespace.subscription_id(4)?;
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
    contexts
        .serve(
            "jc/user/wrapper.jsonld",
            &format!(
                r#"{{
      "@context": [
        "{served}jc/user/terms.jsonld",
        "{core}"
      ]
    }}"#
            ),
            Some("application/ld+json"),
        )
        .await?;
    let subscription = Subscription::builder()
        .id(subscription_id.clone())
        .entities([format!("https://test.andromeda/{namespace}/Device")])
        .notification_triggers([NotificationTrigger::EntityCreated])
        .notification(NotificationParams::builder(Endpoint::builder(sink.endpoint(1)?).accept(NgsiLdMediaType::Json).build()?).build()?)
        .build()?;
    core_context_body.subscriptions().create(&subscription).send().await?;
    let subscription_2 = Subscription::builder()
        .id(subscription_2_id)
        .entities([format!("https://test.andromeda/{namespace}/Device")])
        .notification_triggers([NotificationTrigger::EntityCreated])
        .notification(NotificationParams::builder(Endpoint::builder(sink.endpoint(2)?).accept(NgsiLdMediaType::Json).build()?).build()?)
        .jsonld_context(contexts.url("jc/user/terms.jsonld")?)
        .build()?;
    core_context_body.subscriptions().create(&subscription_2).send().await?;
    let subscription_3 = Subscription::builder()
        .id(subscription_3_id)
        .entities([format!("https://test.andromeda/{namespace}/Device")])
        .notification_triggers([NotificationTrigger::EntityCreated])
        .notification(NotificationParams::builder(Endpoint::builder(sink.endpoint(3)?).accept(NgsiLdMediaType::Json).build()?).build()?)
        .jsonld_context(contexts.url("jc/user/wrapper.jsonld")?)
        .build()?;
    core_context_body.subscriptions().create(&subscription_3).send().await?;
    let subscription_4 = Subscription::builder()
        .id(subscription_4_id)
        .entities([format!("https://test.andromeda/{namespace}/Device")])
        .notification_triggers([NotificationTrigger::EntityCreated])
        .notification(NotificationParams::builder(Endpoint::builder(sink.endpoint(4)?).accept(NgsiLdMediaType::Json).build()?).build()?)
        .jsonld_context("https://uri.etsi.org/ngsi-ld/v1/ngsi-ld-core-context-v1.8.jsonld")
        .build()?;
    core_context_body.subscriptions().create(&subscription_4).send().await?;
    let device_notified = Entity::builder()
        .id(device_notified_id.clone())
        .entity_type(format!("https://test.andromeda/{namespace}/Device").parse::<EntityTypeName>()?)
        .property(format!("https://test.andromeda/{namespace}/speed").as_str(), 3)
        .build()?;
    broker.entities().create(&device_notified).send().await?.done()?;
    sink.wait_for_at_least(1, 1).await?;
    let fields = [HeaderCondition::exactly(
        HeaderName::from_static("link"),
        &format!("^<{}>{}", regex::escape(config.core_context().as_str()), ";[^,]*json-ld#context[^,]*$"),
    )?];
    let notifications = sink.received_matching(1, &[], &fields).await?;
    expect_that!(notifications, len(eq(1)));
    let notification = notifications.first().or_fail()?.entities(Lossless::Normalized)?;
    expect_that!(notification.context(), none());
    let item = notification.data().first().or_fail()?;
    expect_that!(item.context.as_ref(), none());
    expect_that!(notification.data().as_slice(), nth(0, has_id(&device_notified_id)));
    sink.wait_for_at_least(2, 1).await?;
    let fields_2 = [HeaderCondition::exactly(
        HeaderName::from_static("link"),
        &format!(
            "^<{}>{}",
            regex::escape(contexts.url("jc/user/terms.jsonld")?.as_str()),
            ";[^,]*json-ld#context[^,]*$"
        ),
    )?];
    let notifications_2 = sink.received_matching(2, &[], &fields_2).await?;
    expect_that!(notifications_2, len(eq(1)));
    let notification_2 = notifications_2.first().or_fail()?.entities(Lossless::Normalized)?;
    expect_that!(notification_2.context(), none());
    let item_2 = notification_2.data().first().or_fail()?;
    expect_that!(item_2.context.as_ref(), none());
    expect_that!(notification_2.data().as_slice(), nth(0, has_id(&device_notified_id)));
    sink.wait_for_at_least(3, 1).await?;
    let fields_3 = [HeaderCondition::exactly(
        HeaderName::from_static("link"),
        &format!(
            "^<{}>{}",
            regex::escape(contexts.url("jc/user/wrapper.jsonld")?.as_str()),
            ";[^,]*json-ld#context[^,]*$"
        ),
    )?];
    let notifications_3 = sink.received_matching(3, &[], &fields_3).await?;
    expect_that!(notifications_3, len(eq(1)));
    let notification_3 = notifications_3.first().or_fail()?.entities(Lossless::Normalized)?;
    expect_that!(notification_3.context(), none());
    let item_3 = notification_3.data().first().or_fail()?;
    expect_that!(item_3.context.as_ref(), none());
    expect_that!(notification_3.data().as_slice(), nth(0, has_id(&device_notified_id)));
    sink.wait_for_at_least(4, 1).await?;
    let fields_4 = [HeaderCondition::exactly(
        HeaderName::from_static("link"),
        "^<https://uri\\.etsi\\.org/ngsi\\-ld/v1/ngsi\\-ld\\-core\\-context\\-v1\\.8\\.jsonld>;[^,]*json-ld#context[^,]*$",
    )?];
    let notifications_4 = sink.received_matching(4, &[], &fields_4).await?;
    expect_that!(notifications_4, len(eq(1)));
    let notification_4 = notifications_4.first().or_fail()?.entities(Lossless::Normalized)?;
    expect_that!(notification_4.context(), none());
    let item_4 = notification_4.data().first().or_fail()?;
    expect_that!(item_4.context.as_ref(), none());
    expect_that!(notification_4.data().as_slice(), nth(0, has_id(&device_notified_id)));
    Ok(())
}
