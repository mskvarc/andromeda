//! A deletion in a relayed part is notified beside the local attributes and the other holder's.

use andromeda_test_suite::{
    body_json::json_equals,
    body_pattern::BodyPattern,
    broker,
    context_source::ContextSource,
    namespace::Namespace,
    notification_sink::NotificationSink,
    outcome::Done,
    problem::answered,
    relayed_copy::{notification_text, placed_copy_id, register_copy_source, register_numbered_copy_source, relay},
    source_answer,
};
use googletest::prelude::*;
use http::{Method, StatusCode};
use ngsi_ld::{
    model::{APPLICATION_LD_JSON, Entity, NgsiLdMediaType},
    query::EntitySplitting,
    subscription::{Endpoint, NotificationParams, Subscription},
};

/// Pins: Under a subscription with `splitEntities`, a relayed part with colour urn:ngsi-ld:null and speed 40 of an
/// entity this broker holds with a name and another holder answers with a temp of 21 is notified with colour null,
/// speed 40, the local name and temp 21
/// Clause: 5.8.6, 4.5.5.3
#[gtest]
#[tokio::test]
async fn n_ntf_pay_6819_a_notified_deletion_of_an_attribute_no_holder_has_is_relayed_beside_the_merged_holders() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let sink = NotificationSink::start().await?;
    let source = ContextSource::start().await?;
    let vehicle_type = namespace.entity_type("Vehicle")?;
    let split_id = namespace.urn("Vehicle", "split")?;
    let other_holder = Entity::output_builder()
        .id(split_id.clone())
        .entity_type(vehicle_type.clone())
        .property("temp", 21)
        .build()?;
    source
        .answer(
            Method::GET,
            &format!("/s2/ngsi-ld/v1/entities/{split_id}"),
            source_answer::entity(&other_holder),
        )
        .await?;
    register_copy_source(&broker, &source, &namespace, "s1").await?;
    register_numbered_copy_source(&broker, &source, &namespace, "s2", 2).await?;
    let subscription = Subscription::builder()
        .id(namespace.subscription_id(1)?)
        .entities([vehicle_type.as_str()])
        .split_entities(EntitySplitting::Split)
        .notification(NotificationParams::builder(Endpoint::builder(sink.endpoint(1)?).accept(NgsiLdMediaType::Json).build()?).build()?)
        .build()?;
    broker.subscriptions().create(&subscription).send().await?;
    let copy_id = placed_copy_id(&source, "s1", &vehicle_type).await?;
    let local = Entity::builder()
        .id(split_id.clone())
        .entity_type(vehicle_type.clone())
        .property("name", "Roadster")
        .build()?;
    broker.entities().create(&local).send().await?.done()?;
    let with_name = sink
        .wait_for_at_least_where(1, &[BodyPattern::Selects("$.data[?(@.name)]".parse()?)], 1)
        .await?;
    expect_that!(with_name, len(eq(1)));
    // The entity model writes no NGSI-LD Null as the bare value of an attribute, so the data is written as text.
    let data = format!(r#"[{{"id":"{split_id}","type":"{vehicle_type}","colour":"urn:ngsi-ld:null","speed":{{"type":"Property","value":40}}}}]"#);
    let body = notification_text(&namespace, &copy_id, &data)?;
    let answer = relay(&broker, &APPLICATION_LD_JSON, &body).await?;
    expect_that!(&answer, answered(StatusCode::OK));
    let completed = sink
        .wait_for_at_least_where(1, &[BodyPattern::Selects("$.data[?(@.colour == 'urn:ngsi-ld:null')]".parse()?)], 1)
        .await?;
    expect_that!(completed, len(eq(1)));
    let received = completed.first().or_fail()?;
    // A bare NGSI-LD Null as an attribute is no value the entity model reads, so the notification is read as received.
    expect_that!(&received.body, json_equals("$.data[0].colour", "urn:ngsi-ld:null"));
    expect_that!(&received.body, json_equals("$.data[0].speed.value", 40));
    expect_that!(&received.body, json_equals("$.data[0].name.value", "Roadster"));
    expect_that!(&received.body, json_equals("$.data[0].temp.value", 21));
    Ok(())
}
