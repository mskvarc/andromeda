//! Operations on a snapshot are local whatever the local parameter says.

use andromeda_test_suite::{
    broker,
    context_source::ContextSource,
    identity_matcher::has_id,
    namespace::Namespace,
    outcome::Done,
    snapshot_capture::await_status,
    source_answer,
};
use googletest::prelude::*;
use http::Method;
use ngsi_ld::{
    model::{Duration, Entity},
    query::Locality,
    registration::{CSourceRegistration, DistributedOperation, EntityInfo, RegistrationInfo, RegistrationMode},
    snapshot::{QueryDocument, Snapshot, SnapshotId, SnapshotStatus},
};

/// Pins: Query Entities with NGSILD-Snapshot over a registration whose source was queried while the Snapshot was
/// captured answers the captured local Entity and never contacts the source again, with or without local false
/// Clause: 5.5.15, 6.3.18, 6.3.22
/// Tenant: private (it counts every request its context source receives, and the unscoped requests of other tests
/// reach every registration of the tenant)
#[gtest]
#[tokio::test]
async fn n_snp_qry_6402_operations_on_a_snapshot_are_local_whatever_the_local_parameter_says() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::private_tenant(&broker::front()?, &namespace).await?;
    let source = ContextSource::start().await?;
    let vehicle_id = namespace.urn("Vehicle", 1)?;
    let vehicle_type = namespace.entity_type("Vehicle")?;
    let snapshot_id = format!("urn:ngsi-ld:Snapshot:{namespace}-1").parse::<SnapshotId>()?;
    let vehicle = Entity::builder()
        .id(vehicle_id.clone())
        .entity_type(vehicle_type.clone())
        .property("speed", 10)
        .build()?;
    broker.entities().create(&vehicle).send().await?.done()?;
    let source_entities = [];
    source
        .answer(Method::GET, "/ngsi-ld/v1/entities", source_answer::entities(&source_entities))
        .await?;
    let registration = CSourceRegistration::builder(source.root_endpoint()?)
        .id(namespace.registration_id(1)?)
        .information([RegistrationInfo::builder()
            .entities([EntityInfo::new(vehicle_type.clone()).with_id_pattern(namespace.id_pattern("Vehicle")?)])
            .build()?])
        .mode(RegistrationMode::Inclusive)
        .operations([DistributedOperation::QueryEntity])
        .build()?;
    broker.registrations().register(&registration).send().await?;
    let snapshot = Snapshot::builder()
        .id(snapshot_id.clone())
        .lifetime("PT1H".parse::<Duration>()?)
        .query(QueryDocument::builder().entities([vehicle_type.as_str()]).build()?)
        .build()?;
    broker.snapshots().create(&snapshot).send().await?;
    await_status(&broker, &snapshot_id, "success".parse::<SnapshotStatus>()?).await?;
    let captured_asks = source.received_on(Method::GET, "/ngsi-ld/v1/entities").await?.len();
    expect_that!(captured_asks, ge(1));
    let page = broker.on_snapshot(&snapshot_id).entities().query().entity_type(&vehicle_type).send().await?;
    expect_that!(page.items(), len(eq(1)));
    expect_that!(page.items(), contains(has_id(&vehicle_id)));
    let page_2 = broker
        .on_snapshot(&snapshot_id)
        .entities()
        .query()
        .entity_type(&vehicle_type)
        .locality(Locality::Distributed)
        .send()
        .await?;
    expect_that!(page_2.items(), len(eq(1)));
    let asks_after = source.received_on(Method::GET, "/ngsi-ld/v1/entities").await?.len();
    expect_that!(asks_after, eq(captured_asks));
    broker.snapshots().delete(&snapshot_id).send().await?;
    Ok(())
}
