//! A negated subscription q on a list family is the complement of membership.

use andromeda_test_suite::{body_pattern::BodyPattern, broker, namespace::Namespace, notification_sink::NotificationSink, outcome::Done};
use googletest::prelude::*;
use ngsi_ld::{
    model::{Attribute, AttributeName, Entity, ListProperty, ListRelationship, NgsiLdMediaType, Property, Value, VocabProperty, VocabTerm},
    query::q::Query,
    subscription::{Endpoint, NotificationParams, NotificationTrigger, Subscription},
};

/// Pins: Subscriptions whose q is unequal to a value no `valueList` or `objectList` holds are notified of the
/// Entity creation, while those unequal to a value the list holds are notified first of the update that removes it
/// Clause: 4.9, 5.8.6, 5.2.12
#[gtest]
#[tokio::test]
async fn n_ntf_trg_9014_a_negated_subscription_q_on_a_list_family_is_the_complement_of_membership() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let sink = NotificationSink::start().await?;
    let building_type = namespace.entity_type("Building")?;
    let building_id = namespace.urn("Building", "eval-negated")?;
    let room_9 = namespace.urn_text("Room", 9);
    let room_1 = namespace.urn_text("Room", 1);
    let cases = [
        ("eval-not-in-list", r#"roomTags!="industrial""#.to_owned()),
        ("eval-not-in-object-list", format!(r#"roomLinks!="{room_9}""#)),
        ("eval-in-list", r#"roomTags!="commercial""#.to_owned()),
        ("eval-in-object-list", format!(r#"roomLinks!="{room_1}""#)),
    ];
    for (name, q) in &cases {
        let subscription = Subscription::builder()
            .id(namespace.subscription_id(name)?)
            .entities([building_type.as_str()])
            .notification_triggers([NotificationTrigger::EntityCreated, NotificationTrigger::AttributeUpdated])
            .q(q.parse::<Query>()?)
            .notification(NotificationParams::builder(Endpoint::builder(sink.endpoint(name)?).accept(NgsiLdMediaType::Json).build()?).build()?)
            .build()?;
        broker.subscriptions().create(&subscription).send().await?;
    }
    let building = Entity::builder()
        .id(building_id.clone())
        .entity_type(building_type.clone())
        .list_property("roomTags", ListProperty::builder(vec![Value::from("residential"), Value::from("commercial")]))
        .list_relationship(
            "roomLinks",
            ListRelationship::builder(vec![namespace.urn("Room", 1)?, namespace.urn("Room", 2)?]),
        )
        .vocab_property(
            "roomVocab",
            VocabProperty::builder(vec![
                "http://example.org/Male".parse::<VocabTerm>()?,
                "http://example.org/Other".parse::<VocabTerm>()?,
            ]),
        )
        .property("roomName", Property::builder("atrium"))
        .build()?;
    broker.entities().create(&building).send().await?.done()?;
    sink.wait_for_at_least("eval-not-in-list", 1).await?;
    sink.wait_for_at_least("eval-not-in-object-list", 1).await?;
    // The barrier update is owed the next notification; notifications of one subscription are delivered in order, so a
    // notification of the creation would be in the journal before it.
    let tags = Attribute::from(ListProperty::builder(vec![Value::from("industrial")]).build()?).into_patch();
    broker
        .entities()
        .update_attribute(&building_id, &"roomTags".parse::<AttributeName>()?, tags)
        .send()
        .await?
        .done()?;
    sink.wait_for_at_least_where("eval-in-list", &[BodyPattern::Contains("industrial".to_owned())], 1)
        .await?;
    expect_that!(sink.received("eval-in-list").await?, len(eq(1)));
    let links = Attribute::from(ListRelationship::builder(vec![namespace.urn("Room", 3)?]).build()?).into_patch();
    broker
        .entities()
        .update_attribute(&building_id, &"roomLinks".parse::<AttributeName>()?, links)
        .send()
        .await?
        .done()?;
    sink.wait_for_at_least_where("eval-in-object-list", &[BodyPattern::Contains(namespace.urn_text("Room", 3))], 1)
        .await?;
    expect_that!(sink.received("eval-in-object-list").await?, len(eq(1)));
    for (name, _) in &cases {
        broker.subscriptions().delete(&namespace.subscription_id(name)?).send().await?;
    }
    Ok(())
}
