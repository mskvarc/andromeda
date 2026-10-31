//! A subscription notifies under its hosted creation context.

use andromeda_test_suite::{
    broker,
    header_condition::HeaderCondition,
    namespace::Namespace,
    notification_sink::NotificationSink,
    outcome::Done,
    remote_context::RemoteContexts,
};
use googletest::prelude::*;
use http::HeaderName;
use ngsi_ld::{
    model::{Context, Entity, EntityFragment, NgsiLdMediaType},
    query::{EntitySelector, builders::SelectorTypeBuilder},
    subscription::{Endpoint, NotificationParams, Subscription},
};

/// Pins: An application/json notification of a Subscription created under a Link header naming a context that does
/// not end with the core context names a context the broker serves in its Link header
/// Clause: 5.8.2.4, 5.8.6, 5.13.1
/// Recorded: Andromeda hosts the applicable @context as an `ImplicitlyCreated` context and initialises
/// `jsonldContext` with the URL it serves it under
#[gtest]
#[tokio::test]
async fn o_ntf_pay_9024_a_subscription_notifies_under_its_hosted_creation_context() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let sink = NotificationSink::start().await?;
    let contexts = RemoteContexts::start().await?;
    let building_creation_context_hosted_notify_id = namespace.urn("Building", "creation-context-hosted-notify")?;
    let building_type = namespace.entity_type("Building")?;
    let linked = broker.with_context(Context::Single(contexts.reference("contexts/suite")?))?;
    let subscription_creation_context_hosted_notify_id = namespace.subscription_id("creation-context-hosted-notify")?;
    contexts
        .serve(
            "contexts/suite",
            &format!(
                r#"{{
      "@context": {{
        "Probe": "https://test.andromeda/{namespace}/Probe"
      }}
    }}"#
            ),
            Some("application/ld+json"),
        )
        .await?;
    let building_creation_context_hosted_notify = Entity::builder()
        .id(building_creation_context_hosted_notify_id.clone())
        .entity_type(building_type.clone())
        .property("name", "Tour Eiffel")
        .build()?;
    broker.entities().create(&building_creation_context_hosted_notify).send().await?.done()?;
    let subscription_creation_context_hosted_notify = Subscription::builder()
        .id(subscription_creation_context_hosted_notify_id.clone())
        .entities([EntitySelector::new(SelectorTypeBuilder::from(&building_type).build()?).with_ids(building_creation_context_hosted_notify_id.clone())])
        .notification(
            NotificationParams::builder(
                Endpoint::builder(sink.endpoint("creation-context-hosted-notify")?)
                    .accept(NgsiLdMediaType::Json)
                    .build()?,
            )
            .build()?,
        )
        .build()?;
    linked.subscriptions().create(&subscription_creation_context_hosted_notify).send().await?;
    let fragment = EntityFragment::builder().property("name", "Le Tour Eiffel").build()?;
    broker
        .entities()
        .append_attributes(&building_creation_context_hosted_notify_id, &fragment)
        .send()
        .await?
        .done()?;
    let fields = [HeaderCondition::Contains(
        HeaderName::from_static("link"),
        "/ngsi-ld/v1/jsonldContexts/".to_owned(),
    )];
    sink.wait_for_at_least_matching("creation-context-hosted-notify", &[], &fields, 1).await?;
    let notifications = sink.received("creation-context-hosted-notify").await?;
    expect_that!(notifications, len(eq(1)));
    broker.subscriptions().delete(&subscription_creation_context_hosted_notify_id).send().await?;
    Ok(())
}
