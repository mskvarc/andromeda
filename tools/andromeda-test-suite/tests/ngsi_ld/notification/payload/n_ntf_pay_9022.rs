//! A key values notification keeps a non ascii dataset instance verbatim.

use andromeda_test_suite::{
    body_pattern::BodyPattern,
    broker,
    identity_matcher::has_id,
    namespace::Namespace,
    notification_sink::NotificationSink,
    nth::nth,
    outcome::Done,
    simplified_matcher::has_value_at,
};
use googletest::prelude::*;
use ngsi_ld::{
    model::{Entity, EntityFragment, EntityId, NgsiLdMediaType, Relationship, Representation, SimplifiedSpelling},
    query::{EntitySelector, builders::SelectorTypeBuilder},
    subscription::{Endpoint, NotificationParams, Subscription},
};

/// Pins: A `keyValues` notification of a Relationship with two `datasetId` instances renders it as a dataset map
/// whose entry for the instance naming an IRI with a non-ASCII character is that IRI, exactly as written
/// Clause: 4.6.2, 4.5.4, 4.5.5, 5.8.6
#[gtest]
#[tokio::test]
async fn n_ntf_pay_9022_a_key_values_notification_keeps_a_non_ascii_dataset_instance_verbatim() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let sink = NotificationSink::start().await?;
    let building_type = namespace.entity_type("Building")?;
    let building_id = namespace.urn("Building", "non-ascii-multi")?;
    let subscription_id = namespace.subscription_id("non-ascii-multi")?;
    let ciudad = format!("urn:ngsi-ld:Ciudad:{namespace}-París");
    let spanish_dataset = namespace.dataset_id("Dataset", "spanish-name")?;
    let building = Entity::builder()
        .id(building_id.clone())
        .entity_type(building_type.clone())
        .relationship(
            "locatedAt",
            Relationship::builder(namespace.urn("City", "London")?).dataset_id(namespace.dataset_id("Dataset", "ascii")?),
        )
        .relationship(
            "locatedAt",
            Relationship::builder(ciudad.parse::<EntityId>()?).dataset_id(spanish_dataset.clone()),
        )
        .property("category", "office")
        .build()?;
    broker.entities().create(&building).send().await?.done()?;
    let subscription = Subscription::builder()
        .id(subscription_id.clone())
        .entities([EntitySelector::new(SelectorTypeBuilder::from(&building_type).build()?).with_ids(building_id.clone())])
        .notification(
            NotificationParams::builder(Endpoint::builder(sink.endpoint("non-ascii-multi")?).accept(NgsiLdMediaType::Json).build()?)
                .format(Representation::Simplified)
                .simplified_spelling(SimplifiedSpelling::KeyValues)
                .build()?,
        )
        .build()?;
    broker.subscriptions().create(&subscription).send().await?;
    let fragment = EntityFragment::builder().property("category", "retail").build()?;
    broker.entities().append_attributes(&building_id, &fragment).send().await?.done()?;
    let retail = [BodyPattern::Contains("\"retail\"".to_owned())];
    sink.wait_for_at_least_where("non-ascii-multi", &retail, 1).await?;
    let notifications = sink.received_where("non-ascii-multi", &retail).await?;
    expect_that!(notifications, len(eq(1)));
    let notification = notifications.first().or_fail()?.simplified()?;
    let spanish_key = spanish_dataset.to_string();
    expect_that!(
        notification.data().as_slice(),
        nth(
            0,
            all!(
                has_id(&building_id),
                has_value_at("locatedAt", ["dataset", spanish_key.as_str()], ciudad.as_str())
            )
        )
    );
    broker.subscriptions().delete(&subscription_id).send().await?;
    Ok(())
}
