//! A durable update notified after its entity was deleted still carries the entity and its system attributes.

use andromeda_test_suite::{
    attribute_matcher::property,
    body_pattern::BodyPattern,
    broker,
    config::BrokerRole,
    entity_matcher::has_attribute,
    eventually::retried,
    identity_matcher::has_id,
    namespace::Namespace,
    notification_sink::NotificationSink,
    nth::nth,
    outcome::Done,
    problem::answered,
    system_time::{self, SystemMember},
    type_matcher::has_type,
};
use googletest::prelude::*;
use http::StatusCode;
use ngsi_ld::{
    http::headers::Accept,
    model::{Entity, EntityFragment, EntityId, Lossless, NgsiLdMediaType},
    subscription::{Endpoint, NotificationParams, NotificationTrigger, Subscription, SystemAttributeReporting},
};
use wiremock::{
    Mock,
    ResponseTemplate,
    matchers::{method, path},
};

/// Pins: under durable delivery an Entity update whose first delivery failed is retried after the Entity was deleted,
/// and the notification still carries the Entity as the update left it with its createdAt and modifiedAt
/// Clause: 5.8.6, 4.8
/// Recorded: Andromeda extension, not v1.9.1: durable notification delivery (notifications.delivery.mode durable on
/// broker-durable), where a queued event outlives the Entity it describes
#[gtest]
#[tokio::test]
async fn a_ntf_pay_0023_a_durable_update_delivered_after_its_entity_was_deleted_still_carries_the_entity_and_its_system_attributes() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker_durable = broker::client(BrokerRole::Durable)?;
    let sink = NotificationSink::start().await?;
    let building_type = namespace.entity_type("Building")?;
    let building_id = format!("urn:ngsi-ld:Building:{namespace}").parse::<EntityId>()?;
    let renamed = [BodyPattern::Contains("Tour Eiffel".to_owned())];
    sink.mount(
        Mock::given(method("POST"))
            .and(path("/notify/1"))
            .respond_with(ResponseTemplate::new(500))
            .with_priority(5),
    )
    .await?;
    let subscription = Subscription::builder()
        .id(namespace.subscription_id(1)?)
        .entities([building_type.as_str()])
        .notification_triggers([NotificationTrigger::EntityUpdated])
        .notification(
            NotificationParams::builder(Endpoint::builder(sink.endpoint(1)?).accept(NgsiLdMediaType::Json).build()?)
                .system_attributes(SystemAttributeReporting::Include)
                .build()?,
        )
        .build()?;
    broker_durable.subscriptions().create(&subscription).send().await?;
    let building = Entity::builder()
        .id(building_id.clone())
        .entity_type(building_type.clone())
        .property("name", "Eiffel Tower")
        .build()?;
    broker_durable.entities().create(&building).send().await?.done()?;
    let fragment = EntityFragment::builder().property("name", "Tour Eiffel").build()?;
    broker_durable.entities().update_attributes(&building_id, &fragment).send().await?.done()?;
    retried(async || {
        verify_that!(sink.received_where(1, &renamed).await?, len(eq(1)))?;
        Ok(())
    })
    .await?;
    broker_durable.entities().delete(&building_id).send().await?.done()?;
    sink.mount(
        Mock::given(method("POST"))
            .and(path("/notify/1"))
            .respond_with(ResponseTemplate::new(204))
            .with_priority(1),
    )
    .await?;
    let answer = broker_durable
        .with_responses()
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .retrieve(&building_id)
        .send()
        .await;
    expect_that!(&answer, answered(StatusCode::NOT_FOUND));
    let notifications = sink.wait_for_at_least_where(1, &renamed, 2).await?;
    let notification = notifications.first().or_fail()?.entities(Lossless::Normalized)?;
    let item = notification.data().as_slice().first().or_fail()?;
    expect_that!(system_time::of_entity(item, SystemMember::CreatedAt).ok(), some(anything()));
    expect_that!(system_time::of_entity(item, SystemMember::ModifiedAt).ok(), some(anything()));
    expect_that!(
        notification.data().as_slice(),
        nth(
            0,
            all!(
                has_id(&building_id),
                has_type(&building_type),
                has_attribute("name", property().value("Tour Eiffel"))
            )
        )
    );
    Ok(())
}
