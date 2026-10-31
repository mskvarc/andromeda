//! A broker without multi tenancy refuses a snapshot in a tenant.

use andromeda_test_suite::{broker, config::BrokerRole, namespace::Namespace, problem::problem};
use googletest::prelude::*;
use http::StatusCode;
use ngsi_ld::{
    client::ClientError,
    model::ErrorType,
    snapshot::{QueryDocument, Snapshot},
};

/// Pins: Create Snapshot naming a Tenant on a broker with multi-tenancy disabled answers 501 `NoMultiTenantSupport`
/// Clause: 5.5.10, 6.3.2, 5.16.1.4
/// Tenant: refused (the subject is a broker without multi-tenancy refusing the Tenant the request names)
#[gtest]
#[tokio::test]
async fn a_snp_cre_8140_a_broker_without_multi_tenancy_refuses_a_snapshot_in_a_tenant() -> Result<()> {
    let namespace = Namespace::fresh();
    let single_tenant = broker::client(BrokerRole::SingleTenant)?.with_tenant(namespace.tenant()?);
    let snapshot = Snapshot::builder()
        .query(QueryDocument::builder().entities([namespace.entity_type("Vehicle")?.to_string()]).build()?)
        .build()?;
    let answer = single_tenant.with_responses().snapshots().create(&snapshot).send().await;
    expect_that!(&answer, problem(StatusCode::NOT_IMPLEMENTED, ErrorType::NoMultiTenantSupport));
    expect_that!(
        answer.as_ref().err().and_then(ClientError::problem).and_then(|details| details.status),
        some(eq(501))
    );
    Ok(())
}
