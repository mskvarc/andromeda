//! A tenant management operation naming a snapshot is refused and changes nothing.

use andromeda_test_suite::{
    broker,
    namespace::Namespace,
    snapshot_refusal::refused_on_snapshot,
    tenant_management::{Activity, TenantView, activity_request, read_tenant},
    tenant_resource::TenantStatus,
    wire::andromeda_request_to,
};
use googletest::prelude::*;
use http::Method;
use ngsi_ld::{
    http::headers::NgsiLdSnapshot,
    snapshot::{QueryDocument, Snapshot, SnapshotId},
};

/// Pins: GET /ngsi-ld/andromeda/v1/tenants, GET /ngsi-ld/andromeda/v1/tenants/{name} and a PATCH of that tenant
/// setting `isActive` false, each with NGSILD-Snapshot naming an existing Snapshot, answer 422 `OperationNotSupported`
/// with the same header and a detail naming the core, temporal and entity-subscription operations, and the tenant is
/// not suspended
/// Clause: 5.5.15, 6.3.22
/// Recorded: Andromeda extension, not v1.9.1: tenant management; a Snapshot lives inside one tenant (clause 5.5.15),
/// so no tenant operation takes one
/// Tenant: private (the PATCH suspends the tenant it names when the header is ignored)
#[gtest]
#[tokio::test]
async fn a_snp_scp_8100_a_tenant_management_operation_naming_a_snapshot_is_refused_and_changes_nothing() -> Result<()> {
    let namespace = Namespace::fresh();
    let front = broker::front()?;
    broker::private_tenant(&front, &namespace).await?;
    let tenant = namespace.tenant()?;
    let vehicle_type = namespace.entity_type("Vehicle")?;
    let snapshot_id = format!("urn:ngsi-ld:Snapshot:{namespace}-1").parse::<SnapshotId>()?;
    let snapshot = Snapshot::builder()
        .id(snapshot_id.clone())
        .query(QueryDocument::builder().entities([vehicle_type.as_str()]).build()?)
        .build()?;
    front.snapshots().create(&snapshot).send().await?;
    // Tenant management is an Andromeda extension no NGSI-LD operation covers, so each request is sent as written.
    for request in [
        andromeda_request_to(&front, Method::GET, "tenants")?,
        andromeda_request_to(&front, Method::GET, &format!("tenants/{tenant}"))?,
        activity_request(&front, &tenant, Activity::Suspended)?,
    ] {
        let answer = front.send_unchecked(request.typed_header(NgsiLdSnapshot::try_new(snapshot_id.clone())?)).await;
        expect_that!(&answer, refused_on_snapshot(&snapshot_id));
    }
    let read = read_tenant(&front, &tenant, TenantView::Plain).await?;
    expect_that!(read.status, some(not(eq(TenantStatus::Suspended))));
    front.snapshots().delete(&snapshot_id).send().await?;
    Ok(())
}
