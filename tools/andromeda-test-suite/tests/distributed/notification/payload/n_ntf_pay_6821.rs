//! A relayed part deleting its scope cannot be completed and relays nothing.

use andromeda_test_suite::{
    body_pattern::BodyPattern,
    broker,
    context_source::ContextSource,
    identity_matcher::has_id,
    namespace::Namespace,
    notification_sink::NotificationSink,
    nth::nth,
    problem::answered,
    relayed_copy::{notification_text, notification_text_numbered, placed_copy_id, register_copy_source, register_numbered_copy_source, relay},
    source_answer,
};
use googletest::prelude::*;
use http::{Method, StatusCode};
use ngsi_ld::{
    model::{APPLICATION_LD_JSON, Entity, Lossless, NgsiLdMediaType},
    query::EntitySplitting,
    subscription::{Endpoint, NotificationParams, Subscription},
};

/// Pins: Under a subscription with `splitEntities`, a relayed part with its scope urn:ngsi-ld:null is relayed to the
/// subscriber as nothing, while a later notification of another entity is relayed
/// Clause: 5.8.6, 4.5.5.3
#[gtest]
#[tokio::test]
async fn n_ntf_pay_6821_a_part_deleting_its_scope_cannot_be_completed_and_relays_nothing() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let sink = NotificationSink::start().await?;
    let source = ContextSource::start().await?;
    let vehicle_type = namespace.entity_type("Vehicle")?;
    let split_id = namespace.urn("Vehicle", "split")?;
    let sentinel_id = namespace.urn("Vehicle", "sentinel")?;
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
    // The entity model writes no NGSI-LD Null as the scope, so the data is written as text.
    let scope_deleted = format!(r#"[{{"id":"{split_id}","type":"{vehicle_type}","speed":{{"type":"Property","value":40}},"scope":"urn:ngsi-ld:null"}}]"#);
    let body = notification_text(&namespace, &copy_id, &scope_deleted)?;
    let answer = relay(&broker, &APPLICATION_LD_JSON, &body).await?;
    expect_that!(&answer, answered(StatusCode::OK));
    let sentinel = format!(r#"[{{"id":"{sentinel_id}","type":"{vehicle_type}","speed":{{"type":"Property","value":41}}}}]"#);
    let body_2 = notification_text_numbered(&namespace, 2, &copy_id, &sentinel)?;
    let answer_2 = relay(&broker, &APPLICATION_LD_JSON, &body_2).await?;
    expect_that!(&answer_2, answered(StatusCode::OK));
    let relayed = sink
        .wait_for_at_least_where(1, &[BodyPattern::Selects(format!("$.data[?(@.id == '{sentinel_id}')]").parse()?)], 1)
        .await?;
    expect_that!(relayed, len(eq(1)));
    let all = sink.received(1).await?;
    expect_that!(all, len(eq(1)));
    let received = all.first().or_fail()?;
    expect_that!(received.entities(Lossless::Normalized)?.data(), nth(0, has_id(&sentinel_id)));
    Ok(())
}
