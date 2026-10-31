//! Operations on the temporal evolution alone notify nothing.

use andromeda_test_suite::{
    attribute_matcher::any_attribute,
    body_pattern::BodyPattern,
    broker,
    evolution_matcher::instance_id_where,
    namespace::Namespace,
    notification_sink::NotificationSink,
    outcome::{Done, Upserted},
    problem::answered,
    written_json::json_request_to,
};
use googletest::prelude::*;
use http::{Method, StatusCode};
use ngsi_ld::{
    http::headers::Accept,
    model::{AttributeName, EntityFragment, NgsiLdMediaType, Property, evolution::TemporalEntity},
    subscription::{Endpoint, NotificationParams, NotificationTrigger, Subscription},
};

/// Pins: A Subscription triggered by `entityUpdated`, `entityDeleted`, `attributeCreated`, `attributeUpdated` and
/// `attributeDeleted` is notified of nothing for Modify Attribute instance and Delete Attribute instance of the
/// instance current state holds, Delete Attribute of w from the Temporal Evolution and Delete Temporal Evolution of
/// the Entity
/// Clause: 5.8.6, 5.6.13, 5.6.14, 5.6.15, 5.6.16
/// Recorded: clause 4.3.1 gives the current state to the Core API and the Temporal Evolution to the Temporal API
/// and leaves open whether a Temporal API write reaches the current state; Andromeda keeps the current state in
/// step with the Temporal Evolution, so the Entity a Temporal API write addresses has current state
#[gtest]
#[tokio::test]
async fn o_ntf_trg_9027_operations_on_the_temporal_evolution_alone_notify_nothing() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let sink = NotificationSink::start().await?;
    let subscription_id = namespace.subscription_id("TwnHistoryOnly")?;
    let history_type = namespace.entity_type("TwnHistoryOnly")?;
    let history_id = namespace.urn("TwnHistoryOnly", 1)?;
    let history = TemporalEntity::builder()
        .id(history_id.clone())
        .entity_type(history_type.clone())
        .property("v", Property::builder(1).observed_at("2020-01-01T00:00:00Z".parse()?))
        .property("v", Property::builder(2).observed_at("2020-01-02T00:00:00Z".parse()?))
        .property("w", Property::builder(3).observed_at("2020-01-01T00:00:00Z".parse()?))
        .build()?;
    broker.temporal().create_or_update(&history).send().await?.created()?;
    let subscription = Subscription::builder()
        .id(subscription_id.clone())
        .entities([history_type.as_str()])
        .notification_triggers([
            NotificationTrigger::EntityUpdated,
            NotificationTrigger::EntityDeleted,
            NotificationTrigger::AttributeCreated,
            NotificationTrigger::AttributeUpdated,
            NotificationTrigger::AttributeDeleted,
        ])
        .notification(NotificationParams::builder(Endpoint::builder(sink.endpoint("TwnHistoryOnly")?).accept(NgsiLdMediaType::Json).build()?).build()?)
        .build()?;
    broker.subscriptions().create(&subscription).send().await?;
    let retrieved = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .temporal()
        .retrieve(&history_id)
        .attrs(vec!["v".parse()?])
        .send()
        .await?;
    let instance_id = instance_id_where(retrieved.body(), &"v".parse()?, any_attribute().value(2))?;
    // The fragment is the array-wrapped body Modify Attribute Instance is sent with, which the model has no place for.
    let request = json_request_to(
        &broker,
        Method::PATCH,
        &format!("temporal/entities/{history_id}/attrs/v/{instance_id}"),
        r#"{"v": [{"value": 99}]}"#,
    )?;
    expect_that!(&broker.send_unchecked(request).await, answered(StatusCode::NO_CONTENT));
    broker
        .temporal()
        .delete_instance(&history_id, &"v".parse::<AttributeName>()?, &instance_id)
        .send()
        .await?;
    broker.temporal().delete_attribute(&history_id, &"w".parse::<AttributeName>()?).send().await?;
    broker.temporal().delete(&history_id).send().await?;
    let fence = format!("fence-{namespace}");
    let fragment = EntityFragment::builder().property("fence", fence.as_str()).build()?;
    broker.entities().append_attributes(&history_id, &fragment).send().await?.done()?;
    // A notification owed before the fence write is delivered before the fence's, so once the fence's has arrived the
    // journal holds every notification the earlier writes owed.
    sink.wait_for_at_least_where("TwnHistoryOnly", &[BodyPattern::Contains(fence.clone())], 1)
        .await?;
    expect_that!(sink.received_where("TwnHistoryOnly", &[BodyPattern::DoesNotContain(fence)]).await?, is_empty());
    broker.subscriptions().delete(&subscription_id).send().await?;
    Ok(())
}
