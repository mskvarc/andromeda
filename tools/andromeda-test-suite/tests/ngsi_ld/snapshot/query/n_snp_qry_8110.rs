//! Operations under a snapshot ignore local false and contact no source.

use andromeda_test_suite::{
    broker,
    context_source::ContextSource,
    eventually::retried,
    identity_matcher::has_id,
    namespace::Namespace,
    nth::nth,
    outcome::Done,
};
use googletest::prelude::*;
use ngsi_ld::{
    http::headers::{Accept, NgsiLdSnapshot},
    model::{Entity, NgsiLdMediaType},
    query::Locality,
    registration::{CSourceRegistration, DistributedOperation, EntityInfo, RegistrationInfo},
    snapshot::{QueryDocument, Snapshot, SnapshotId, SnapshotStatus},
};

/// Pins: Create Entity and Query Entities with NGSILD-Snapshot and local false answer 201 and 200 with the same
/// header, the query answers the Entity created under the Snapshot alone, and the registered source of the type is
/// never contacted
/// Clause: 5.5.15, 5.5.13, 6.3.22
/// Tenant: private (it counts every request its context source receives, and the discovery and unscoped requests of
/// other tests reach every registration of the tenant)
#[gtest]
#[tokio::test]
async fn n_snp_qry_8110_operations_under_a_snapshot_ignore_local_false_and_contact_no_source() -> Result<()> {
    let namespace = Namespace::fresh();
    let front = broker::front()?;
    let broker = broker::private_tenant(&front, &namespace).await?;
    let source = ContextSource::start().await?;
    let snapshot_id = format!("urn:ngsi-ld:Snapshot:{namespace}-1").parse::<SnapshotId>()?;
    let vehicle_type = namespace.entity_type("Vehicle")?;
    let vehicle_id = namespace.urn("Vehicle", 1)?;
    let speed = namespace.attribute("speed")?;
    let registration_id = namespace.registration_id(1)?;
    let snapshot = Snapshot::builder()
        .id(snapshot_id.clone())
        .query(QueryDocument::builder().entities([vehicle_type.as_str()]).build()?)
        .build()?;
    broker.snapshots().create(&snapshot).send().await?;
    retried(async || {
        let captured = broker
            .with_accept(Accept::new(NgsiLdMediaType::Json))
            .snapshots()
            .retrieve(&snapshot_id)
            .send()
            .await?;
        verify_that!(captured.status(), eq("empty".parse::<SnapshotStatus>()?))?;
        Ok(())
    })
    .await?;
    let registration = CSourceRegistration::builder(source.endpoint("remote")?)
        .id(registration_id.clone())
        .information([RegistrationInfo::builder()
            .entities([EntityInfo::new(vehicle_type.clone()).with_id_pattern(namespace.id_pattern("Vehicle")?)])
            .property_names([speed.to_string()])
            .build()?])
        .operations([DistributedOperation::QueryEntity, DistributedOperation::CreateEntity])
        .build()?;
    broker.registrations().register(&registration).send().await?;
    let vehicle = Entity::builder()
        .id(vehicle_id.clone())
        .entity_type(vehicle_type.clone())
        .property(speed.clone(), 5)
        .build()?;
    let created = broker
        .with_responses()
        .on_snapshot(&snapshot_id)
        .entities()
        .create(&vehicle)
        .locality(Locality::Distributed)
        .send()
        .await?;
    created.outcome.done()?;
    expect_that!(
        created.head.typed::<NgsiLdSnapshot>().valid().map(NgsiLdSnapshot::snapshot),
        some(eq(&snapshot_id))
    );
    let queried = broker
        .with_heads()
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .on_snapshot(&snapshot_id)
        .entities()
        .query()
        .entity_type(&vehicle_type)
        .locality(Locality::Distributed)
        .send()
        .await?;
    expect_that!(
        queried.head.typed::<NgsiLdSnapshot>().valid().map(NgsiLdSnapshot::snapshot),
        some(eq(&snapshot_id))
    );
    expect_that!(queried.outcome.items(), len(eq(1)));
    expect_that!(queried.outcome.items(), nth(0, has_id(&vehicle_id)));
    expect_that!(source.received().await?, is_empty());
    broker.registrations().delete(&registration_id).send().await?;
    broker.snapshots().delete(&snapshot_id).send().await?;
    Ok(())
}
