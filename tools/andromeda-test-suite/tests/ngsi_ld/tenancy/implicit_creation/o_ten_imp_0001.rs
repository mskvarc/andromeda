//! Retrieve on a tenant that does not exist is nonexistent tenant.

use andromeda_test_suite::{broker, namespace::Namespace, problem::problem};
use googletest::prelude::*;
use http::{HeaderName, StatusCode};
use ngsi_ld::{
    http::headers::Accept,
    model::{ErrorType, NgsiLdMediaType},
};

/// Pins: Retrieve Entity on a Tenant that does not exist answers 404 `NonexistentTenant` and echoes the
/// NGSILD-Tenant header
/// Clause: 5.5.10, 6.3.14, 5.7.1.4
/// Tenant: private (the subject is a Tenant that does not exist)
/// Recorded: clause 5.5.10 makes implicit Tenant creation an implementation option and says the other operations on
/// a non-existing Tenant "should" raise `NonexistentTenant`; Andromeda creates Tenants implicitly, exactly on the
/// six operations the clause lists
#[gtest]
#[tokio::test]
async fn o_ten_imp_0001_retrieve_on_a_tenant_that_does_not_exist_is_nonexistent_tenant() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    broker::private_tenant(&broker, &namespace).await?;
    let unknown_tenant = broker.with_tenant(format!("{namespace}-unknown").parse()?);
    let vehicle_id = namespace.urn("Vehicle", 1)?;
    let answer = unknown_tenant
        .with_responses()
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .retrieve(&vehicle_id)
        .send()
        .await;
    expect_that!(
        &answer,
        problem(StatusCode::NOT_FOUND, ErrorType::NonexistentTenant).with_header(HeaderName::from_static("ngsild-tenant"), &format!("{namespace}-unknown"))
    );
    Ok(())
}
