//! A context source registration operation naming a snapshot is refused and changes nothing.

use andromeda_test_suite::{broker, config::SuiteConfig, namespace::Namespace, problem::answered, snapshot_refusal::refused_on_snapshot};
use googletest::prelude::*;
use http::StatusCode;
use ngsi_ld::{
    http::{
        headers::Accept,
        operation::{DeleteRegistration, RegisterContextSource},
    },
    model::NgsiLdMediaType,
    registration::{CSourceRegistration, RegistrationInfo},
    snapshot::{QueryDocument, Snapshot, SnapshotId},
};

/// Pins: Create and Delete Context Source Registration with NGSILD-Snapshot naming an existing Snapshot answer 422
/// `OperationNotSupported` with the same header and a detail naming the core, temporal and entity-subscription
/// operations; the registration the create names does not exist and the one the delete names remains
/// Clause: 5.5.15, 6.3.22, 5.9.2, 5.9.4
/// Recorded: clause 5.5.15 permits on a Snapshot the operations of the Core and Temporal APIs alone and names no error
/// for another; Andromeda answers `OperationNotSupported`
/// Tenant: private (the unscoped and discovery requests of other tests reach every registration of the tenant)
#[gtest]
#[tokio::test]
async fn o_snp_scp_8100_a_registration_operation_naming_a_snapshot_is_refused_and_changes_nothing() -> Result<()> {
    let namespace = Namespace::fresh();
    let front = broker::front()?;
    let broker = broker::private_tenant(&front, &namespace).await?;
    let source = SuiteConfig::get()?.unreachable_origin().join("source")?;
    let vehicle_type = namespace.entity_type("Vehicle")?;
    let snapshot_id = format!("urn:ngsi-ld:Snapshot:{namespace}-1").parse::<SnapshotId>()?;
    let kept_id = namespace.registration_id("kept")?;
    let refused_id = namespace.registration_id("refused")?;
    let snapshot = Snapshot::builder()
        .id(snapshot_id.clone())
        .query(QueryDocument::builder().entities([vehicle_type.as_str()]).build()?)
        .build()?;
    broker.snapshots().create(&snapshot).send().await?;
    let kept = CSourceRegistration::builder(source.as_str())
        .id(kept_id.clone())
        .information([RegistrationInfo::builder().entities([vehicle_type.as_str()]).build()?])
        .build()?;
    let refused = CSourceRegistration::builder(source.as_str())
        .id(refused_id.clone())
        .information([RegistrationInfo::builder().entities([vehicle_type.as_str()]).build()?])
        .build()?;
    broker.registrations().register(&kept).send().await?;
    let on_snapshot = broker.on_snapshot(&snapshot_id);
    let created = broker
        .send_unchecked(on_snapshot.unchecked(&RegisterContextSource { registration: &refused })?)
        .await;
    expect_that!(&created, refused_on_snapshot(&snapshot_id));
    let deleted = broker.send_unchecked(on_snapshot.unchecked(&DeleteRegistration { id: &kept_id })?).await;
    expect_that!(&deleted, refused_on_snapshot(&snapshot_id));
    let absent = broker
        .with_responses()
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .registrations()
        .retrieve(&refused_id)
        .send()
        .await;
    expect_that!(&absent, answered(StatusCode::NOT_FOUND));
    let remaining = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .registrations()
        .retrieve(&kept_id)
        .send()
        .await?;
    expect_that!(remaining.id(), eq(&kept_id));
    broker.registrations().delete(&kept_id).send().await?;
    broker.snapshots().delete(&snapshot_id).send().await?;
    Ok(())
}
