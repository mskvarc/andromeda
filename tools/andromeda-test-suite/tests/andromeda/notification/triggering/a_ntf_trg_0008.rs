//! A word class q notifies the entities the same query selects.

use andromeda_test_suite::{
    body_pattern::BodyPattern,
    broker,
    identity_matcher::has_id,
    namespace::Namespace,
    notification_sink::NotificationSink,
    outcome::Done,
};
use googletest::prelude::*;
use ngsi_ld::{
    http::headers::Accept,
    model::{Entity, NgsiLdMediaType},
    query::q,
    subscription::{Endpoint, NotificationParams, NotificationTrigger, Subscription},
};

/// Pins: a Subscription with q name~="^\w+$" and the `entityCreated` trigger notifies every one-word spelling and
/// not the two-word place name, the Entities a query carrying the same q selects
/// Clause: 4.9, 5.8.6, 5.7.2.4
/// Recorded: clause 4.9 hands `RegExp` to IEEE 1003.2, which has no inline options and no shorthand classes;
/// Andromeda admits the case-insensitive option and the shorthand classes \d, \w and \s as extensions, folding case
/// by Unicode simple case folding and classifying by Unicode
#[gtest]
#[tokio::test]
async fn a_ntf_trg_0008_a_word_class_q_notifies_the_entities_the_same_query_selects() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let sink = NotificationSink::start().await?;
    let subscription_id = namespace.subscription_id(1)?;
    let bee_type = namespace.entity_type("Bee")?;
    let bee_0_id = namespace.urn("Bee", "0")?;
    let bee_id = namespace.urn("Bee", 1)?;
    let bee_2_id = namespace.urn("Bee", 2)?;
    let bee_3_id = namespace.urn("Bee", 3)?;
    let bee_4_id = namespace.urn("Bee", 4)?;
    let bee_5_id = namespace.urn("Bee", 5)?;
    let subscription = Subscription::builder()
        .id(subscription_id.clone())
        .entities([bee_type.as_str()])
        .notification_triggers([NotificationTrigger::EntityCreated])
        .q(q::attr("name").matches("\"^\\w+$\"").build()?)
        .notification(NotificationParams::builder(Endpoint::builder(sink.endpoint(1)?).accept(NgsiLdMediaType::Json).build()?).build()?)
        .build()?;
    broker.subscriptions().create(&subscription).send().await?;
    let entities = [
        Entity::builder()
            .id(bee_0_id.clone())
            .entity_type(bee_type.clone())
            .property("name", "čebela")
            .build()?,
        Entity::builder()
            .id(bee_id.clone())
            .entity_type(bee_type.clone())
            .property("name", "Čebela")
            .build()?,
        Entity::builder()
            .id(bee_2_id.clone())
            .entity_type(bee_type.clone())
            .property("name", "ČEBELA")
            .build()?,
        Entity::builder()
            .id(bee_3_id.clone())
            .entity_type(bee_type.clone())
            .property("name", "ČEbela")
            .build()?,
        Entity::builder()
            .id(bee_4_id.clone())
            .entity_type(bee_type.clone())
            .property("name", "cebela")
            .build()?,
        Entity::builder()
            .id(bee_5_id.clone())
            .entity_type(bee_type.clone())
            .property("name", "Škofja Loka")
            .build()?,
    ];
    broker.entities().batch_create(&entities).send().await?.done()?;
    let selected = [BodyPattern::SelectsBelow(
        "$.data".parse()?,
        format!("$[?(@.id == 'urn:ngsi-ld:Bee:{namespace}-0')]").parse()?,
    )];
    sink.wait_for_at_least_where(1, &selected, 1).await?;
    let selected_2 = [BodyPattern::SelectsBelow(
        "$.data".parse()?,
        format!("$[?(@.id == 'urn:ngsi-ld:Bee:{namespace}-1')]").parse()?,
    )];
    sink.wait_for_at_least_where(1, &selected_2, 1).await?;
    let selected_3 = [BodyPattern::SelectsBelow(
        "$.data".parse()?,
        format!("$[?(@.id == 'urn:ngsi-ld:Bee:{namespace}-2')]").parse()?,
    )];
    sink.wait_for_at_least_where(1, &selected_3, 1).await?;
    let selected_4 = [BodyPattern::SelectsBelow(
        "$.data".parse()?,
        format!("$[?(@.id == 'urn:ngsi-ld:Bee:{namespace}-3')]").parse()?,
    )];
    sink.wait_for_at_least_where(1, &selected_4, 1).await?;
    let selected_5 = [BodyPattern::SelectsBelow(
        "$.data".parse()?,
        format!("$[?(@.id == 'urn:ngsi-ld:Bee:{namespace}-4')]").parse()?,
    )];
    sink.wait_for_at_least_where(1, &selected_5, 1).await?;
    // Every Entity is created in one batch, so a notification for an unselected one would be sent with the selected ones.
    let notifications = sink.received(1).await?;
    expect_that!(notifications, len(ge(5)));
    let selected_6 = [BodyPattern::SelectsBelow(
        "$.data".parse()?,
        format!("$[?(@.id == 'urn:ngsi-ld:Bee:{namespace}-5')]").parse()?,
    )];
    let notifications_2 = sink.received_where(1, &selected_6).await?;
    expect_that!(notifications_2, len(eq(0)));
    let page = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .query()
        .entity_type(&bee_type)
        .q(q::attr("name").matches("\"^\\w+$\""))
        .send()
        .await?;
    expect_that!(page.items(), len(eq(5)));
    expect_that!(page.items(), contains(has_id(&bee_0_id)));
    expect_that!(page.items(), contains(has_id(&bee_id)));
    expect_that!(page.items(), contains(has_id(&bee_2_id)));
    expect_that!(page.items(), contains(has_id(&bee_3_id)));
    expect_that!(page.items(), contains(has_id(&bee_4_id)));
    expect_that!(page.items(), not(contains(has_id(&bee_5_id))));
    Ok(())
}
