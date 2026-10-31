//! An entity whose batch fragment names no member changes nothing.

use andromeda_test_suite::{
    attribute_matcher::any_attribute,
    body_pattern::BodyPattern,
    broker::{self, BrokerClient},
    core_context::core_context_list,
    entity_matcher::has_attribute,
    evolution_matcher::has_evolution,
    namespace::Namespace,
    notification_sink::NotificationSink,
    outcome::Done,
    problem::answered,
    system_time::{self, SystemMember},
    wire::request_to,
};
use bytes::Bytes;
use googletest::prelude::*;
use http::{HeaderValue, Method, StatusCode, header::CONTENT_TYPE};
use ngsi_ld::{
    http::{
        ContextDelivery,
        headers::Accept,
        operation::{Overwrite, SystemAttributes, UpsertMode},
    },
    model::{DateTime, Entity, EntityFragment, EntityId, NgsiLdMediaType, Representation},
    query::{EntitySelector, builders::SelectorTypeBuilder, temporal_query::TemporalProperty},
    subscription::{Endpoint, NotificationParams, Subscription},
};

/// Pins: Batch Entity Update, Update with `noOverwrite`, Merge and Upsert with update of an entity naming only its
/// id and type each answer 204, leave the entity and every `modifiedAt` as they were, record no temporal instance,
/// and notify no Subscription to the entity
/// Clause: 5.4, 5.6.8.4, 5.6.9.4, 5.6.20.4, 5.8.6
#[gtest]
#[tokio::test]
async fn n_bat_upd_9005_an_entity_whose_batch_fragment_names_no_member_changes_nothing() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let sink = NotificationSink::start().await?;
    let vehicle_id = namespace.urn("Vehicle", 1)?;
    let vehicle_type = namespace.entity_type("Vehicle")?;
    let subscription_id = namespace.subscription_id(1)?;
    let vehicle = Entity::builder()
        .id(vehicle_id.clone())
        .entity_type(vehicle_type.clone())
        .property("speed", 50)
        .property("brandName", "Mercedes")
        .build()?;
    broker.entities().create(&vehicle).send().await?.done()?;
    let subscription = Subscription::builder()
        .id(subscription_id.clone())
        .entities([EntitySelector::new(SelectorTypeBuilder::from(&vehicle_type).build()?).with_ids(vehicle_id.clone())])
        .notification(
            NotificationParams::builder(Endpoint::builder(sink.endpoint(1)?).accept(NgsiLdMediaType::Json).build()?)
                .format(Representation::Normalized)
                .build()?,
        )
        .build()?;
    broker.subscriptions().create(&subscription).send().await?;
    let retrieved = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .retrieve(&vehicle_id)
        .system_attributes(SystemAttributes::Include)
        .send()
        .await?;
    let entity = retrieved.body();
    let times = Times {
        entity: *system_time::of_entity(entity, SystemMember::ModifiedAt)?,
        speed: *system_time::of_attribute(entity, &["speed"], SystemMember::ModifiedAt)?,
        brand: *system_time::of_attribute(entity, &["brandName"], SystemMember::ModifiedAt)?,
    };
    let core_body = broker.with_context(core_context_list()?)?.with_context_delivery(ContextDelivery::LdJsonBody);
    let fragments = [EntityFragment::for_entity(vehicle_id.clone(), vehicle_type.clone()).build()?];
    core_body.entities().batch_update(&fragments).send().await?.done()?;
    expect_unchanged(&broker, &vehicle_id, &times).await?;
    core_body.entities().batch_update(&fragments).overwrite(Overwrite::Keep).send().await?.done()?;
    expect_unchanged(&broker, &vehicle_id, &times).await?;
    core_body.entities().batch_merge(&fragments).send().await?.done()?;
    expect_unchanged(&broker, &vehicle_id, &times).await?;
    let entities = [Entity::builder().id(vehicle_id.clone()).entity_type(vehicle_type.clone()).build()?];
    core_body.entities().batch_upsert(&entities).mode(UpsertMode::Update).send().await?.done()?;
    expect_unchanged(&broker, &vehicle_id, &times).await?;
    let evolution_answer = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .temporal()
        .retrieve(&vehicle_id)
        .time_property(TemporalProperty::ModifiedAt)
        .send()
        .await?;
    let evolution = evolution_answer.body();
    expect_that!(evolution, has_evolution("speed", len(eq(1))));
    expect_that!(evolution, has_evolution("brandName", len(eq(1))));
    // A real change is owed a notification; notifications of one subscription are delivered in order, so one sent for an
    // empty write would be in the journal before it.
    // The body is the bare value, which the concise form of a Partial Attribute Update does not write, so it is sent as written.
    let request = request_to(&broker, Method::PATCH, &format!("entities/{vehicle_id}/attrs/brandName"))?
        .append_header(CONTENT_TYPE, HeaderValue::from_static("application/json"))
        .body(Bytes::from("{\n    \"value\": \"Barrier\"\n}"));
    let answer = broker.send_unchecked(request).await;
    expect_that!(&answer, answered(StatusCode::NO_CONTENT));
    sink.wait_for_at_least_where(1, &[BodyPattern::Contains("\"Barrier\"".to_owned())], 1).await?;
    expect_that!(sink.received(1).await?, len(eq(1)));
    broker.subscriptions().delete(&subscription_id).send().await?;
    Ok(())
}

/// The system times of the Vehicle as the first read gave them.
struct Times {
    /// The `modifiedAt` of the entity.
    entity: DateTime,
    /// The `modifiedAt` of `speed`.
    speed: DateTime,
    /// The `modifiedAt` of `brandName`.
    brand: DateTime,
}

/// Requires the Vehicle `id` to hold its two Properties and the system times of `times`.
async fn expect_unchanged(broker: &BrokerClient, id: &EntityId, times: &Times) -> Result<()> {
    let retrieved = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .retrieve(id)
        .system_attributes(SystemAttributes::Include)
        .send()
        .await?;
    let entity = retrieved.body();
    expect_that!(system_time::of_entity(entity, SystemMember::ModifiedAt)?, eq(&times.entity));
    expect_that!(entity, has_attribute("speed", any_attribute().value(50)));
    expect_that!(system_time::of_attribute(entity, &["speed"], SystemMember::ModifiedAt)?, eq(&times.speed));
    expect_that!(entity, has_attribute("brandName", any_attribute().value("Mercedes")));
    expect_that!(system_time::of_attribute(entity, &["brandName"], SystemMember::ModifiedAt)?, eq(&times.brand));
    Ok(())
}
