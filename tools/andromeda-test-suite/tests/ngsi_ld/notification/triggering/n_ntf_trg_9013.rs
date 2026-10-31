//! A subscription q on a list family notifies.

use andromeda_test_suite::{body_pattern::BodyPattern, broker, namespace::Namespace, notification_sink::NotificationSink, outcome::Done, problem::answered};
use googletest::prelude::*;
use http::StatusCode;
use ngsi_ld::{
    http::{UncheckedDocument, operation::CreateEntity},
    model::{Attribute, AttributeName, Entity, ListProperty, Lossless, NgsiLdMediaType, RawJson, Value, VocabProperty, VocabTerm},
    query::q,
    subscription::{Endpoint, NotificationParams, NotificationTrigger, Subscription},
};

/// Pins: Subscriptions whose q tests a member of a `valueList`, of an `objectList` and of a vocab array, and a
/// scalar control, are notified of the Entity creation, while one testing a value no list holds is notified first
/// of the update that adds it
/// Clause: 4.9, 5.8.6, 5.2.12
#[gtest]
#[tokio::test]
async fn n_ntf_trg_9013_a_subscription_q_on_a_list_family_notifies() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let sink = NotificationSink::start().await?;
    let subscription_eval_value_list_id = namespace.subscription_id("eval-value-list")?;
    let building_type = namespace.entity_type("Building")?;
    let subscription_eval_object_list_id = namespace.subscription_id("eval-object-list")?;
    let subscription_eval_vocab_id = namespace.subscription_id("eval-vocab")?;
    let subscription_eval_control_id = namespace.subscription_id("eval-control")?;
    let subscription_eval_absent_id = namespace.subscription_id("eval-absent")?;
    let building_eval_equal_id = namespace.urn("Building", "eval-equal")?;
    let subscription_eval_value_list = Subscription::builder()
        .id(subscription_eval_value_list_id.clone())
        .entities([building_type.as_str()])
        .notification_triggers([NotificationTrigger::EntityCreated, NotificationTrigger::AttributeUpdated])
        .q(q::attr("roomTags").eq("commercial").build()?)
        .notification(NotificationParams::builder(Endpoint::builder(sink.endpoint("eval-value-list")?).accept(NgsiLdMediaType::Json).build()?).build()?)
        .build()?;
    broker.subscriptions().create(&subscription_eval_value_list).send().await?;
    let subscription_eval_object_list = Subscription::builder()
        .id(subscription_eval_object_list_id.clone())
        .entities([building_type.as_str()])
        .notification_triggers([NotificationTrigger::EntityCreated, NotificationTrigger::AttributeUpdated])
        .q(q::attr("roomLinks").eq(format!("urn:ngsi-ld:Room:{namespace}-2").as_str()).build()?)
        .notification(NotificationParams::builder(Endpoint::builder(sink.endpoint("eval-object-list")?).accept(NgsiLdMediaType::Json).build()?).build()?)
        .build()?;
    broker.subscriptions().create(&subscription_eval_object_list).send().await?;
    let subscription_eval_vocab = Subscription::builder()
        .id(subscription_eval_vocab_id.clone())
        .entities([building_type.as_str()])
        .notification_triggers([NotificationTrigger::EntityCreated, NotificationTrigger::AttributeUpdated])
        .q(q::attr("roomVocab").eq("http://example.org/Other").build()?)
        .notification(NotificationParams::builder(Endpoint::builder(sink.endpoint("eval-vocab")?).accept(NgsiLdMediaType::Json).build()?).build()?)
        .build()?;
    broker.subscriptions().create(&subscription_eval_vocab).send().await?;
    let subscription_eval_control = Subscription::builder()
        .id(subscription_eval_control_id.clone())
        .entities([building_type.as_str()])
        .notification_triggers([NotificationTrigger::EntityCreated, NotificationTrigger::AttributeUpdated])
        .q(q::attr("roomName").eq("atrium").build()?)
        .notification(NotificationParams::builder(Endpoint::builder(sink.endpoint("eval-control")?).accept(NgsiLdMediaType::Json).build()?).build()?)
        .build()?;
    broker.subscriptions().create(&subscription_eval_control).send().await?;
    let subscription_eval_absent = Subscription::builder()
        .id(subscription_eval_absent_id.clone())
        .entities([building_type.as_str()])
        .notification_triggers([NotificationTrigger::EntityCreated, NotificationTrigger::AttributeUpdated])
        .q(q::attr("roomTags").eq("industrial").build()?)
        .notification(NotificationParams::builder(Endpoint::builder(sink.endpoint("eval-absent")?).accept(NgsiLdMediaType::Json).build()?).build()?)
        .build()?;
    broker.subscriptions().create(&subscription_eval_absent).send().await?;
    let building_eval_equal = Entity::builder()
        .id(building_eval_equal_id.clone())
        .entity_type(building_type.clone())
        .list_property("roomTags", ListProperty::builder(vec![Value::from("residential"), Value::from("commercial")]))
        .vocab_property(
            "roomVocab",
            VocabProperty::builder(vec![
                "http://example.org/Male".parse::<VocabTerm>()?,
                "http://example.org/Other".parse::<VocabTerm>()?,
            ]),
        )
        .property("roomName", "atrium")
        .build()?;
    // The model refuses it (the model reads it as written otherwise [at $.roomLinks.objectList[0] (written "urn:ngsi-ld:Room:zqns-1")]: {"id":"urn:ngsi-ld:Building:zqns-eval-equal","type":"Building_zqns","roomTags":{"type":"ListProperty","valueList":["residential","commercial"]},"roomLinks":{"type":"ListRelationship","objectList":[{"object":"urn:ngsi-ld:Room:zqns-1"},{"object":"urn:ngsi-ld:Room:zqns-2"}]},"roomVocab":{"type":"VocabProperty","vocab":["http://example.org/Male","http://example.org/Other"]},"roomName":{"type":"Property","value":"atrium"}}), so `roomLinks` is written as text.
    let room_links: RawJson =
        format!(r#"{{"type": "ListRelationship", "objectList": ["urn:ngsi-ld:Room:{namespace}-1", "urn:ngsi-ld:Room:{namespace}-2"]}}"#).parse()?;
    let document = UncheckedDocument::from_entity(&building_eval_equal, Lossless::Normalized).set_member("roomLinks", &room_links);
    let request = broker
        .unchecked(&CreateEntity::builder().entity(&building_eval_equal).build())?
        .document(&document);
    let answer = broker.send_unchecked(request).await;
    expect_that!(&answer, answered(StatusCode::CREATED));
    sink.wait_for_at_least("eval-value-list", 1).await?;
    sink.wait_for_at_least("eval-object-list", 1).await?;
    sink.wait_for_at_least("eval-vocab", 1).await?;
    sink.wait_for_at_least("eval-control", 1).await?;
    // The barrier update is owed the next notification; notifications of one subscription are delivered in order, so a notification of the creation would be in the journal before it.
    let patch = Attribute::from(ListProperty::builder(vec![Value::from("industrial")]).build()?).into_patch();
    broker
        .entities()
        .update_attribute(&building_eval_equal_id, &"roomTags".parse::<AttributeName>()?, patch)
        .send()
        .await?
        .done()?;
    let selected = [BodyPattern::Contains("industrial".to_owned())];
    sink.wait_for_at_least_where("eval-absent", &selected, 1).await?;
    let notifications = sink.received("eval-absent").await?;
    expect_that!(notifications, len(eq(1)));
    broker.subscriptions().delete(&subscription_eval_value_list_id).send().await?;
    broker.subscriptions().delete(&subscription_eval_object_list_id).send().await?;
    broker.subscriptions().delete(&subscription_eval_vocab_id).send().await?;
    broker.subscriptions().delete(&subscription_eval_control_id).send().await?;
    broker.subscriptions().delete(&subscription_eval_absent_id).send().await?;
    Ok(())
}
