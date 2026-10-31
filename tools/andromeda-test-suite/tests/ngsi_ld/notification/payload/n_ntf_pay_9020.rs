//! A concise notification keeps a non ascii relationship object verbatim.

use andromeda_test_suite::{
    attribute_matcher::any_attribute,
    body_pattern::BodyPattern,
    broker,
    entity_matcher::has_attribute,
    identity_matcher::has_id,
    namespace::Namespace,
    notification_sink::NotificationSink,
    nth::nth,
    outcome::Done,
};
use googletest::prelude::*;
use ngsi_ld::{
    model::{Entity, EntityFragment, EntityId, Lossless, NgsiLdMediaType, Representation},
    query::{EntitySelector, builders::SelectorTypeBuilder},
    subscription::{Endpoint, NotificationParams, Subscription},
};

/// Pins: A concise notification carries the object of a Relationship naming an IRI with a non-ASCII character
/// exactly as written
/// Clause: 4.6.2, 4.5.1, 5.8.6
#[gtest]
#[tokio::test]
async fn n_ntf_pay_9020_a_concise_notification_keeps_a_non_ascii_relationship_object_verbatim() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let sink = NotificationSink::start().await?;
    let building_non_ascii_concise_id = namespace.urn("Building", "non-ascii-concise")?;
    let building_type = namespace.entity_type("Building")?;
    let subscription_non_ascii_concise_id = namespace.subscription_id("non-ascii-concise")?;
    let ngsi_ld_ciudad_id = format!("urn:ngsi-ld:Ciudad:{namespace}-París").parse::<EntityId>()?;
    let building_non_ascii_concise = Entity::builder()
        .id(building_non_ascii_concise_id.clone())
        .entity_type(building_type.clone())
        .relationship("locatedAt", format!("urn:ngsi-ld:Ciudad:{namespace}-París").parse::<EntityId>()?)
        .property("category", "office")
        .build()?;
    broker.entities().create(&building_non_ascii_concise).send().await?.done()?;
    let subscription_non_ascii_concise = Subscription::builder()
        .id(subscription_non_ascii_concise_id.clone())
        .entities([EntitySelector::new(SelectorTypeBuilder::from(&building_type).build()?).with_ids(building_non_ascii_concise_id.clone())])
        .notification(
            NotificationParams::builder(Endpoint::builder(sink.endpoint("non-ascii-concise")?).accept(NgsiLdMediaType::Json).build()?)
                .format(Representation::Concise)
                .build()?,
        )
        .build()?;
    broker.subscriptions().create(&subscription_non_ascii_concise).send().await?;
    let fragment = EntityFragment::builder().property("category", "retail").build()?;
    broker
        .entities()
        .append_attributes(&building_non_ascii_concise_id, &fragment)
        .send()
        .await?
        .done()?;
    let selected = [BodyPattern::Contains("\"retail\"".to_owned())];
    sink.wait_for_at_least_where("non-ascii-concise", &selected, 1).await?;
    let selected_2 = [BodyPattern::Contains("\"retail\"".to_owned())];
    let notifications = sink.received_where("non-ascii-concise", &selected_2).await?;
    expect_that!(notifications, len(eq(1)));
    let notification = notifications.first().or_fail()?.entities(Lossless::Concise)?;
    expect_that!(
        notification.data().as_slice(),
        nth(
            0,
            all!(
                has_id(&building_non_ascii_concise_id),
                has_attribute("locatedAt", any_attribute().object(ngsi_ld_ciudad_id))
            )
        )
    );
    broker.subscriptions().delete(&subscription_non_ascii_concise_id).send().await?;
    Ok(())
}
