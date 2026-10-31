//! A deletion in a relayed part beats a newer instance another holder answers.

use andromeda_test_suite::{
    body_json::json_equals,
    body_pattern::BodyPattern,
    broker,
    context_source::ContextSource,
    namespace::Namespace,
    notification_sink::NotificationSink,
    problem::answered,
    relayed_copy::{notification_text, placed_copy_id, register_copy_source, register_numbered_copy_source, relay},
    source_answer,
};
use googletest::prelude::*;
use http::{Method, StatusCode};
use ngsi_ld::{
    model::{APPLICATION_LD_JSON, Entity, NgsiLdMediaType, Property},
    query::EntitySplitting,
    subscription::{Endpoint, NotificationParams, Subscription},
};

/// Pins: Under a subscription with `splitEntities`, a relayed part with owner urn:ngsi-ld:null and speed 40 of an
/// entity another holder answers with an owner observed later and a temp of 21 is notified with owner still null,
/// speed 40 and temp 21
/// Clause: 5.8.6, 4.5.5.3
#[gtest]
#[tokio::test]
async fn n_ntf_pay_6818_a_notified_deletion_beats_a_newer_instance_a_holder_answers() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let sink = NotificationSink::start().await?;
    let source = ContextSource::start().await?;
    let vehicle_type = namespace.entity_type("Vehicle")?;
    let split_id = namespace.urn("Vehicle", "split")?;
    let other_holder = Entity::output_builder()
        .id(split_id.clone())
        .entity_type(vehicle_type.clone())
        .property("owner", Property::builder("Ada").observed_at("2026-10-06T13:00:00Z".parse()?))
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
    // The entity model writes no NGSI-LD Null as the bare value of an attribute, so the data is written as text.
    let data = format!(
        r#"[{{"id":"{split_id}","type":"{vehicle_type}","owner":"urn:ngsi-ld:null","speed":{{"type":"Property","value":40,"observedAt":"2026-10-06T12:00:00Z"}}}}]"#
    );
    let body = notification_text(&namespace, &copy_id, &data)?;
    let answer = relay(&broker, &APPLICATION_LD_JSON, &body).await?;
    expect_that!(&answer, answered(StatusCode::OK));
    let completed = sink
        .wait_for_at_least_where(1, &[BodyPattern::Selects("$.data[?(@.owner == 'urn:ngsi-ld:null')]".parse()?)], 1)
        .await?;
    expect_that!(completed, len(eq(1)));
    let received = completed.first().or_fail()?;
    // A bare NGSI-LD Null as an attribute is no value the entity model reads, so the notification is read as received.
    expect_that!(&received.body, json_equals("$.data[0].owner", "urn:ngsi-ld:null"));
    expect_that!(&received.body, json_equals("$.data[0].speed.value", 40));
    expect_that!(&received.body, json_equals("$.data[0].temp.value", 21));
    Ok(())
}
