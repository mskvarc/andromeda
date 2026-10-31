//! A relayed part deleting an instance is completed from the other holder and relays the deletion.

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
    model::{APPLICATION_LD_JSON, Entity, NgsiLdMediaType},
    query::EntitySplitting,
    subscription::{Endpoint, NotificationParams, Subscription},
};

/// Pins: Under a subscription with `splitEntities`, a relayed part deleting the instance of speed with datasetId
/// urn:ngsi-ld:Dataset:{ns}-a by its Property deletion encoding is completed from the other holder and relayed with
/// that deletion beside the other holder's temp
/// Clause: 5.8.6, 4.5.5.3
#[gtest]
#[tokio::test]
async fn n_ntf_pay_6822_a_part_deleting_an_instance_is_completed_and_relays_the_deletion() -> Result<()> {
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
    let dataset_id = namespace.urn_text("Dataset", "a");
    // The entity model writes no NGSI-LD Null as a value, so the data is written as text.
    let data =
        format!(r#"[{{"id":"{split_id}","type":"{vehicle_type}","speed":{{"type":"Property","value":"urn:ngsi-ld:null","datasetId":"{dataset_id}"}}}}]"#);
    let body = notification_text(&namespace, &copy_id, &data)?;
    let answer = relay(&broker, &APPLICATION_LD_JSON, &body).await?;
    expect_that!(&answer, answered(StatusCode::OK));
    let relayed = sink
        .wait_for_at_least_where(1, &[BodyPattern::Selects(format!("$.data[?(@.id == '{split_id}')]").parse()?)], 1)
        .await?;
    expect_that!(relayed, len(eq(1)));
    let received = relayed.first().or_fail()?;
    // A Property holding the NGSI-LD Null is no value the entity model reads, so the notification is read as received.
    expect_that!(&received.body, json_equals("$.data[0].speed.type", "Property"));
    expect_that!(&received.body, json_equals("$.data[0].speed.value", "urn:ngsi-ld:null"));
    expect_that!(&received.body, json_equals("$.data[0].speed.datasetId", dataset_id));
    expect_that!(&received.body, json_equals("$.data[0].temp.value", 21));
    Ok(())
}
