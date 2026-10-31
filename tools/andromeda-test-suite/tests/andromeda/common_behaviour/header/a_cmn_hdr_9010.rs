//! With a configured api root prefix every location starts with its path.

use andromeda_test_suite::{
    broker,
    config::{BrokerRole, SuiteConfig},
    eventually::retried,
    hosted_context::HostedContext,
    namespace::Namespace,
    wire::header_value,
};
use googletest::prelude::*;
use http::header::LOCATION;
use ngsi_ld::{
    http::headers::Accept,
    model::{Context, ContextEntry, Entity, InlineContext, NgsiLdMediaType, Property, evolution::TemporalEntity},
    query::temporal_query,
    registration::{CSourceRegistration, RegistrationInfo},
    snapshot::{QueryDocument, Snapshot, SnapshotClone, SnapshotId, SnapshotStatus},
    subscription::{Endpoint, NotificationParams, Subscription},
};

/// Pins: On a broker whose configured apiRoot carries the path prefix /broker, Create Entity, Create or Update Temporal
/// Evolution, Create `EntityMap` for Query Entities and for Query Temporal Evolution, Register Context Source,
/// Create Subscription, Create Context Source Registration Subscription, Create Snapshot, Clone Snapshot and Add
/// @context each answer a Location that is a relative path starting with /broker/ngsi-ld/v1/ and carrying no origin
/// Clause: 6.2, 6.4.3.1, 6.8.3.1, 6.10.3.1, 6.12.3.1, 6.18.3.1, 6.29.3.1, 6.34.3.1, 6.35.3.1, 6.36.3.1, 6.38.3.1
/// Recorded: Andromeda extension, not v1.9.1: `server.public_url` is the apiRoot, and the relative path of a created
/// resource starts with its path (clause 6.2 allows a prefix string), so it resolves through the proxy that strips
/// the prefix
#[gtest]
#[tokio::test]
async fn a_cmn_hdr_9010_with_a_configured_api_root_prefix_every_location_starts_with_its_path() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker_prefixed = broker::client(BrokerRole::Prefixed)?.with_responses();
    let config = SuiteConfig::get()?;
    let unreachable_notify = config.unreachable_origin().join("notify")?;
    let vehicle_type = namespace.entity_type("Vehicle")?;
    let bus_type = namespace.entity_type("Bus")?;
    let vehicle_id = namespace.urn("Vehicle", 1)?;
    let vehicle_2_id = namespace.urn("Vehicle", 2)?;
    let registration_id = namespace.registration_id(1)?;
    let subscription_id = namespace.subscription_id(1)?;
    let subscription_2_id = namespace.subscription_id(2)?;
    let snapshot_source_id = format!("urn:ngsi-ld:Snapshot:{namespace}-source").parse::<SnapshotId>()?;
    let snapshot_clone_id = format!("urn:ngsi-ld:Snapshot:{namespace}-clone").parse::<SnapshotId>()?;
    let root = "/broker/ngsi-ld/v1";

    let vehicle = Entity::builder()
        .id(vehicle_id.clone())
        .entity_type(vehicle_type.clone())
        .property("speed", 7)
        .build()?;
    let created = broker_prefixed.entities().create(&vehicle).send().await?;
    expect_that!(
        header_value(created.head.headers(), &LOCATION),
        some(eq(format!("{root}/entities/{vehicle_id}").as_str()))
    );

    let vehicle_evolution = TemporalEntity::builder()
        .id(vehicle_2_id.clone())
        .entity_type(vehicle_type.clone())
        .property("speed", Property::builder(9).observed_at("2024-01-01T00:00:00Z".parse()?))
        .build()?;
    let created_2 = broker_prefixed.temporal().create_or_update(&vehicle_evolution).send().await?;
    expect_that!(
        header_value(created_2.head.headers(), &LOCATION),
        some(eq(format!("{root}/temporal/entities/{vehicle_2_id}").as_str()))
    );

    let entity_map = broker_prefixed
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entity_maps()
        .create()
        .entity_type(&vehicle_type)
        .send()
        .await?;
    expect_that!(entity_map.status.as_u16(), eq(201));
    let map_id = entity_map.outcome.body().id.clone();
    expect_that!(
        header_value(entity_map.head.headers(), &LOCATION),
        some(eq(format!("{root}/entityMaps/{map_id}").as_str()))
    );

    let temporal_map = broker_prefixed
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entity_maps()
        .create_temporal(temporal_query::before("2099-01-01T00:00:00Z".parse()?))
        .entity_type(&vehicle_type)
        .send()
        .await?;
    expect_that!(temporal_map.status.as_u16(), eq(201));
    let temporal_map_id = temporal_map.outcome.body().id.clone();
    expect_that!(
        header_value(temporal_map.head.headers(), &LOCATION),
        some(eq(format!("{root}/temporal/entityMaps/{temporal_map_id}").as_str()))
    );

    let registration = CSourceRegistration::builder(config.unreachable_origin().as_str())
        .id(registration_id.clone())
        .information([RegistrationInfo::builder().entities([bus_type.as_str()]).build()?])
        .build()?;
    let registered = broker_prefixed.registrations().register(&registration).send().await?;
    expect_that!(
        header_value(registered.head.headers(), &LOCATION),
        some(eq(format!("{root}/csourceRegistrations/{registration_id}").as_str()))
    );

    let subscription = Subscription::builder()
        .id(subscription_id.clone())
        .entities([vehicle_type.as_str()])
        .notification(NotificationParams::builder(Endpoint::builder(unreachable_notify.clone()).build()?).build()?)
        .build()?;
    let subscribed = broker_prefixed.subscriptions().create(&subscription).send().await?;
    expect_that!(
        header_value(subscribed.head.headers(), &LOCATION),
        some(eq(format!("{root}/subscriptions/{subscription_id}").as_str()))
    );

    let registration_subscription = Subscription::registration_builder()
        .id(subscription_2_id.clone())
        .entities([bus_type.as_str()])
        .notification(NotificationParams::builder(Endpoint::builder(unreachable_notify).build()?).build()?)
        .build()?;
    let subscribed_2 = broker_prefixed.registration_subscriptions().create(&registration_subscription).send().await?;
    expect_that!(
        header_value(subscribed_2.head.headers(), &LOCATION),
        some(eq(format!("{root}/csourceSubscriptions/{subscription_2_id}").as_str()))
    );

    let snapshot = Snapshot::builder()
        .id(snapshot_source_id.clone())
        .query(QueryDocument::builder().entities([vehicle_type.as_str()]).build()?)
        .build()?;
    let snapshotted = broker_prefixed.snapshots().create(&snapshot).send().await?;
    expect_that!(
        header_value(snapshotted.head.headers(), &LOCATION),
        some(eq(format!("{root}/snapshots/{snapshot_source_id}").as_str()))
    );
    retried(async || {
        let status = broker_prefixed
            .with_accept(Accept::new(NgsiLdMediaType::Json))
            .snapshots()
            .retrieve(&snapshot_source_id)
            .send()
            .await?;
        verify_that!(status.outcome.status(), eq("success".parse::<SnapshotStatus>()?))?;
        Ok(())
    })
    .await?;

    let clone = SnapshotClone::builder().id(snapshot_clone_id.clone()).build()?;
    let cloned = broker_prefixed.snapshots().clone_snapshot(&snapshot_source_id, &clone).send().await?;
    expect_that!(
        header_value(cloned.head.headers(), &LOCATION),
        some(eq(format!("{root}/snapshots/{snapshot_clone_id}").as_str()))
    );

    let context = Context::Single(ContextEntry::Inline(
        format!(r#"{{"locatedSpeed_{namespace}": "https://example.org/{namespace}/locatedSpeed"}}"#).parse::<InlineContext>()?,
    ));
    let hosted = HostedContext::add(&broker::client(BrokerRole::Prefixed)?, &context).await?;
    let hosted_prefix = format!("{root}/jsonldContexts/urn:");
    expect_that!(hosted.location, starts_with(hosted_prefix.as_str()));

    let broker_cleanup = broker::client(BrokerRole::Prefixed)?;
    broker_cleanup.contexts().delete(&hosted.id).send().await?;
    broker_cleanup.snapshots().delete(&snapshot_clone_id).send().await?;
    broker_cleanup.snapshots().delete(&snapshot_source_id).send().await?;
    broker_cleanup.registration_subscriptions().delete(&subscription_2_id).send().await?;
    broker_cleanup.subscriptions().delete(&subscription_id).send().await?;
    broker_cleanup.registrations().delete(&registration_id).send().await?;
    broker_cleanup.entity_maps().delete(&map_id).send().await?;
    broker_cleanup.entity_maps().delete(&temporal_map_id).send().await?;
    broker_cleanup.temporal().delete(&vehicle_2_id).send().await?;
    broker_cleanup.entities().delete(&vehicle_id).send().await?;
    Ok(())
}
