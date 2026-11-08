//! A snapshot belongs to the tenant it was created in.

use andromeda_test_suite::{broker, namespace::Namespace, problem::problem};
use googletest::prelude::*;
use http::StatusCode;
use ngsi_ld::{
    http::headers::Accept,
    model::{ErrorType, NgsiLdMediaType},
    snapshot::{QueryDocument, Snapshot, SnapshotId, SnapshotPriority},
};

/// Pins: One Snapshot id created in the default Tenant and in a named Tenant names two Snapshots, each read under
/// its own Tenant only; a Tenant that does not exist knows neither, and an operation scoped to the default Tenant's
/// Snapshot under the named Tenant answers 404 `ResourceNotFound`
/// Clause: 5.5.15, 4.14, 5.16.3.4
/// Tenant: private (the subject is one Snapshot id in two Tenants)
#[gtest]
#[tokio::test]
async fn n_snp_sta_8104_a_snapshot_belongs_to_the_tenant_it_was_created_in() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let tenant = broker::private_tenant(&broker, &namespace).await?;
    let snapshot_id = format!("urn:ngsi-ld:Snapshot:{namespace}-1").parse::<SnapshotId>()?;
    let vehicle_type = namespace.entity_type("Vehicle")?;
    let snapshot_2_id = format!("urn:ngsi-ld:Snapshot:{namespace}-2").parse::<SnapshotId>()?;
    let unknown_tenant = broker.with_tenant(format!("{namespace}-unknown").parse()?);
    let snapshot = Snapshot::builder()
        .id(format!("urn:ngsi-ld:Snapshot:{namespace}-1").parse::<SnapshotId>()?)
        .query(QueryDocument::builder().entities([vehicle_type.as_str()]).build()?)
        .priority(3)
        .build()?;
    broker.snapshots().create(&snapshot).send().await?;
    let snapshot_2 = Snapshot::builder()
        .id(format!("urn:ngsi-ld:Snapshot:{namespace}-1").parse::<SnapshotId>()?)
        .query(QueryDocument::builder().entities([vehicle_type.as_str()]).build()?)
        .priority(8)
        .build()?;
    tenant.snapshots().create(&snapshot_2).send().await?;
    let snapshot_3 = Snapshot::builder()
        .id(format!("urn:ngsi-ld:Snapshot:{namespace}-2").parse::<SnapshotId>()?)
        .query(QueryDocument::builder().entities([vehicle_type.as_str()]).build()?)
        .build()?;
    broker.snapshots().create(&snapshot_3).send().await?;
    let snapshot_4 = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .snapshots()
        .retrieve(&snapshot_id)
        .send()
        .await?;
    expect_that!(snapshot_4.priority(), some(eq(SnapshotPriority::try_new(3)?)));
    let snapshot_5 = tenant
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .snapshots()
        .retrieve(&snapshot_id)
        .send()
        .await?;
    expect_that!(snapshot_5.priority(), some(eq(SnapshotPriority::try_new(8)?)));
    let answer = unknown_tenant
        .with_responses()
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .snapshots()
        .retrieve(&snapshot_id)
        .send()
        .await;
    expect_that!(&answer, problem(StatusCode::NOT_FOUND, ErrorType::NonexistentTenant));
    let answer_2 = tenant
        .with_responses()
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .snapshots()
        .retrieve(&snapshot_2_id)
        .send()
        .await;
    expect_that!(&answer_2, problem(StatusCode::NOT_FOUND, ErrorType::ResourceNotFound));
    let answer_3 = tenant
        .with_responses()
        .with_accept(Accept::any())
        .on_snapshot(&snapshot_2_id)
        .entities()
        .query()
        .entity_type(&vehicle_type)
        .send()
        .await;
    expect_that!(&answer_3, problem(StatusCode::NOT_FOUND, ErrorType::ResourceNotFound));
    tenant.snapshots().delete(&snapshot_id).send().await?;
    broker.snapshots().delete(&snapshot_2_id).send().await?;
    broker.snapshots().delete(&snapshot_id).send().await?;
    Ok(())
}
