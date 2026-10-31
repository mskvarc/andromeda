//! A subscription notifies under its link header creation context.

use andromeda_test_suite::{
    broker,
    config::SuiteConfig,
    header_condition::HeaderCondition,
    namespace::Namespace,
    notification_sink::NotificationSink,
    outcome::Done,
    remote_context::RemoteContexts,
};
use googletest::prelude::*;
use http::header::LINK;
use ngsi_ld::{
    model::{Context, Entity, EntityFragment, NgsiLdMediaType},
    query::{EntitySelector, builders::SelectorTypeBuilder},
    subscription::{Endpoint, NotificationParams, Subscription},
};

/// Pins: An application/json notification of a Subscription created under a Link header context and with no
/// `jsonldContext` member names that context in its Link header
/// Clause: 5.8.2.4, 5.8.6, 6.3.5
#[gtest]
#[tokio::test]
async fn n_ntf_pay_9023_a_subscription_notifies_under_its_link_header_creation_context() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let sink = NotificationSink::start().await?;
    let config = SuiteConfig::get()?;
    let contexts = RemoteContexts::start().await?;
    let building_type = namespace.entity_type("Building")?;
    let building_id = namespace.urn("Building", "creation-context-notify")?;
    let subscription_id = namespace.subscription_id("creation-context-notify")?;
    let compound = contexts
        .serve(
            &format!("contexts/{namespace}/compound"),
            &format!(
                r#"{{"@context": [{{"Probe": "https://test.andromeda/{namespace}/Probe"}}, "{}"]}}"#,
                config.core_context()
            ),
            Some("application/ld+json"),
        )
        .await?;
    let building = Entity::builder()
        .id(building_id.clone())
        .entity_type(building_type.clone())
        .property("name", "Tour Eiffel")
        .build()?;
    broker.entities().create(&building).send().await?.done()?;
    let linked = broker.with_context(Context::Single(contexts.reference(&format!("contexts/{namespace}/compound"))?))?;
    let subscription = Subscription::builder()
        .id(subscription_id.clone())
        .entities([EntitySelector::new(SelectorTypeBuilder::from(&building_type).build()?).with_ids(building_id.clone())])
        .notification(
            NotificationParams::builder(
                Endpoint::builder(sink.endpoint("creation-context-notify")?)
                    .accept(NgsiLdMediaType::Json)
                    .build()?,
            )
            .build()?,
        )
        .build()?;
    linked.subscriptions().create(&subscription).send().await?;
    let fragment = EntityFragment::builder().property("name", "Le Tour Eiffel").build()?;
    broker.entities().append_attributes(&building_id, &fragment).send().await?.done()?;
    let linking = [HeaderCondition::Contains(LINK, compound.to_string())];
    sink.wait_for_at_least_matching("creation-context-notify", &[], &linking, 1).await?;
    expect_that!(sink.received("creation-context-notify").await?, len(eq(1)));
    broker.subscriptions().delete(&subscription_id).send().await?;
    Ok(())
}
