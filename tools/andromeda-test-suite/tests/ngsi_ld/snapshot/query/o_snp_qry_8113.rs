//! A snapshot header on a tenant that does not exist creates no tenant.

use andromeda_test_suite::{broker, namespace::Namespace, problem::problem};
use googletest::prelude::*;
use http::StatusCode;
use ngsi_ld::{http::headers::Accept, model::ErrorType, snapshot::SnapshotId};

/// Pins: Query Entities naming a Tenant that does not exist and a Snapshot answers 404 `NonexistentTenant`, and a
/// later query on that Tenant answers `NonexistentTenant` again
/// Clause: 5.5.10, 5.5.15, 6.3.14
/// Tenant: private (the subject is a Tenant that does not exist)
/// Recorded: clause 5.5.10 makes implicit Tenant creation an implementation option and has other operations on a
/// non-existing Tenant "should" raise `NonexistentTenant`; Andromeda creates a Tenant only on the operations the
/// clause lists, never on a query scoped to a Snapshot
#[gtest]
#[tokio::test]
async fn o_snp_qry_8113_a_snapshot_header_on_a_tenant_that_does_not_exist_creates_no_tenant() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    broker::private_tenant(&broker, &namespace).await?;
    let unknown_tenant = broker.with_tenant(format!("{namespace}-unknown").parse()?);
    let snapshot_absent_id = format!("urn:ngsi-ld:Snapshot:{namespace}-absent").parse::<SnapshotId>()?;
    let answer = unknown_tenant
        .with_responses()
        .with_accept(Accept::any())
        .on_snapshot(&snapshot_absent_id)
        .entities()
        .query()
        .entity_type(&namespace.entity_type("Vehicle")?)
        .send()
        .await;
    expect_that!(&answer, problem(StatusCode::NOT_FOUND, ErrorType::NonexistentTenant));
    let answer_2 = unknown_tenant
        .with_responses()
        .with_accept(Accept::any())
        .entities()
        .query()
        .entity_type(&namespace.entity_type("Vehicle")?)
        .send()
        .await;
    expect_that!(&answer_2, problem(StatusCode::NOT_FOUND, ErrorType::NonexistentTenant));
    Ok(())
}
