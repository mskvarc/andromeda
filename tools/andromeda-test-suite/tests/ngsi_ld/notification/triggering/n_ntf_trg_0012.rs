//! A subscription q range with a language matches any one string of it.

use andromeda_test_suite::{broker, identity_matcher::has_id, namespace::Namespace, notification_sink::NotificationSink, nth::nth, problem::answered};
use googletest::prelude::*;
use http::StatusCode;
use ngsi_ld::{
    http::{UncheckedDocument, operation::CreateEntity},
    model::{Entity, Lossless, NgsiLdMediaType, RawJson},
    query::q,
    subscription::{Endpoint, NotificationParams, NotificationTrigger, Subscription},
};

/// Pins: A subscription whose q is name`[en]`=="C".."E" is not notified of an Entity whose en strings all lie
/// outside the range and is notified of one holding one en string inside it
/// Clause: 4.9, 5.8.6
#[gtest]
#[tokio::test]
async fn n_ntf_trg_0012_a_subscription_q_range_with_a_language_matches_any_one_string_of_it() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let sink = NotificationSink::start().await?;
    let subscription_id = namespace.subscription_id(1)?;
    let door_type = namespace.entity_type("Door")?;
    let door_hall_id = namespace.urn("Door", "hall")?;
    let door_door_id = namespace.urn("Door", "door")?;
    let subscription = Subscription::builder()
        .id(subscription_id.clone())
        .entities([door_type.as_str()])
        .notification_triggers([NotificationTrigger::EntityCreated])
        .q(q::attr("name").member("en").within("C", "E").build()?)
        .notification(NotificationParams::builder(Endpoint::builder(sink.endpoint(1)?).accept(NgsiLdMediaType::Json).build()?).build()?)
        .build()?;
    broker.subscriptions().create(&subscription).send().await?;
    let door_hall = Entity::builder().id(door_hall_id).entity_type(door_type.clone()).build()?;
    // The model refuses it (a language map entry with several strings), so `name` is written as text.
    let name: RawJson = r#"{"type": "LanguageProperty", "languageMap": {"en": ["Gate", "Hall"], "de": "Diele"}}"#.parse()?;
    let document = UncheckedDocument::from_entity(&door_hall, Lossless::Normalized).set_member("name", &name);
    let request = broker.unchecked(&CreateEntity::builder().entity(&door_hall).build())?.document(&document);
    let answer = broker.send_unchecked(request).await;
    expect_that!(&answer, answered(StatusCode::CREATED));
    let door_door = Entity::builder().id(door_door_id.clone()).entity_type(door_type.clone()).build()?;
    // The model refuses it (a language map entry with several strings), so `name` is written as text.
    let name_2: RawJson = r#"{"type": "LanguageProperty", "languageMap": {"en": ["Gate", "Door"]}}"#.parse()?;
    let document_2 = UncheckedDocument::from_entity(&door_door, Lossless::Normalized).set_member("name", &name_2);
    let request_2 = broker.unchecked(&CreateEntity::builder().entity(&door_door).build())?.document(&document_2);
    let answer_2 = broker.send_unchecked(request_2).await;
    expect_that!(&answer_2, answered(StatusCode::CREATED));
    // One notification, of the Entity created second: the first sent none.
    sink.wait_for_at_least(1, 1).await?;
    let notifications = sink.received(1).await?;
    expect_that!(notifications, len(eq(1)));
    let notification = notifications.first().or_fail()?.entities(Lossless::Normalized)?;
    expect_that!(notification.subscription_id(), eq(&subscription_id));
    expect_that!(notification.data().as_slice(), nth(0, has_id(&door_door_id)));
    Ok(())
}
