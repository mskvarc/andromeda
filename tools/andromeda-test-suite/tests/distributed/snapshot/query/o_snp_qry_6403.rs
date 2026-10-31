//! A tenant snapshot is reached only with its tenant.

use andromeda_test_suite::{broker, cleanup::Cleanup, config::SuiteConfig, eventually::retried, namespace::Namespace, outcome::Done, problem::problem};
use googletest::prelude::*;
use http::StatusCode;
use ngsi_ld::{
    http::headers::Accept,
    model::{Entity, ErrorType, NgsiLdMediaType},
    registration::{CSourceRegistration, RegistrationInfo},
    snapshot::{QueryDocument, Snapshot, SnapshotId, SnapshotStatus},
};

/// Pins: Query Entities with NGSILD-Snapshot naming a Snapshot created in a tenant answers the captured Entity
/// under that tenant and 404 under the default tenant
/// Clause: 5.5.15, 4.14, 6.3.22
/// Tenant: private (the Snapshot is bound to a tenant and the subject is the tenant it is created in)
/// Recorded: 5.5.15 requires both the Snapshot and the Tenant to be specified and names no error when the Tenant is
/// missing; Andromeda answers 404 `ResourceNotFound` as for an unknown Snapshot
#[gtest]
#[tokio::test]
async fn o_snp_qry_6403_a_tenant_snapshot_is_reached_only_with_its_tenant() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let config = SuiteConfig::get()?;
    let tenant = broker.with_tenant(namespace.tenant()?);
    let registration_provision_id = namespace.registration_id("provision")?;
    let sentinel_type = namespace.entity_type("Sentinel")?;
    let vehicle_id = namespace.urn("Vehicle", 1)?;
    let vehicle_type = namespace.entity_type("Vehicle")?;
    let snapshot_id = format!("urn:ngsi-ld:Snapshot:{namespace}-1").parse::<SnapshotId>()?;
    let registration_provision = CSourceRegistration::builder(config.unreachable_origin().as_str())
        .id(registration_provision_id.clone())
        .information([RegistrationInfo::builder().entities([sentinel_type.as_str()]).build()?])
        .build()?;
    tenant.registrations().register(&registration_provision).send().await?;
    // Registrations are deleted when the test ends: a forward to one left behind fails at its endpoint, and the
    // cooldown that opens there holds back a later test's registration naming the same endpoint.
    let _registration_provision_cleanup = Cleanup::of(&tenant).registration(&registration_provision_id);
    tenant.registrations().delete(&registration_provision_id).send().await?;
    let vehicle = Entity::builder()
        .id(vehicle_id)
        .entity_type(vehicle_type.clone())
        .property("speed", 10)
        .build()?;
    tenant.entities().create(&vehicle).send().await?.done()?;
    let snapshot = Snapshot::builder()
        .id(format!("urn:ngsi-ld:Snapshot:{namespace}-1").parse::<SnapshotId>()?)
        .query(QueryDocument::builder().entities([vehicle_type.as_str()]).build()?)
        .lifetime("PT1H".parse()?)
        .build()?;
    tenant.snapshots().create(&snapshot).send().await?;
    retried(async || {
        let snapshot_2 = tenant
            .with_accept(Accept::new(NgsiLdMediaType::Json))
            .snapshots()
            .retrieve(&snapshot_id)
            .send()
            .await?;
        verify_that!(snapshot_2.status(), eq("success".parse::<SnapshotStatus>()?))?;
        Ok(())
    })
    .await?;
    let page = tenant
        .with_accept(Accept::any())
        .on_snapshot(&snapshot_id)
        .entities()
        .query()
        .entity_type(&vehicle_type)
        .send()
        .await?;
    expect_that!(page.items(), len(eq(1)));
    let answer = broker
        .with_responses()
        .with_accept(Accept::any())
        .on_snapshot(&snapshot_id)
        .entities()
        .query()
        .entity_type(&vehicle_type)
        .send()
        .await;
    expect_that!(&answer, problem(StatusCode::NOT_FOUND, ErrorType::ResourceNotFound));
    tenant.snapshots().delete(&snapshot_id).send().await?;
    Ok(())
}
