//! A tenant reads back as a tenant resource.

use andromeda_test_suite::{
    broker,
    namespace::Namespace,
    outcome::Done,
    problem::answered,
    raw_call::target,
    tenant_resource::{TenantResource, TenantType},
    wire::andromeda_request_to,
};
use googletest::prelude::*;
use http::{HeaderValue, Method, StatusCode, header::ACCEPT};
use ngsi_ld::model::{Entity, Tenant};

/// Pins: every tenant implicitly created reads back as a Tenant resource whose id is urn:ngsi-ld:Tenant: followed
/// by its name, and the collection answers a list counting at least those tenants
/// Clause: 5.5.10
/// Tenant: private (the subject is tenants as resources, which only tenants the file creates can show)
/// Recorded: Andromeda extension, not v1.9.1: tenant management (the /tenants resources), which clause 5.5.10
/// leaves implementation-specific ("How Tenants are created, is implementation-specific")
#[gtest]
#[tokio::test]
async fn a_mgt_tnt_0007_a_tenant_reads_back_as_a_tenant_resource() -> Result<()> {
    let namespace = Namespace::fresh();
    let front = broker::front()?;
    broker::private_tenant(&front, &namespace).await?;
    let implicit = front.with_tenant(format!("{namespace}-b").parse::<Tenant>()?);
    let device = Entity::builder()
        .id(namespace.urn("Device", 1)?)
        .entity_type(namespace.entity_type("Device")?)
        .property("reading", 1)
        .build()?;
    implicit.entities().create(&device).send().await?.done()?;
    // The Tenant resource is an Andromeda extension no NGSI-LD operation covers, so its requests are sent as written.
    for name in [namespace.to_string(), format!("{namespace}-b")] {
        let request = andromeda_request_to(&front, Method::GET, &target(&format!("tenants/{name}"), &[]))?
            .append_header(ACCEPT, HeaderValue::from_static("application/json"));
        let answer = front.send_unchecked(request).await;
        expect_that!(&answer, answered(StatusCode::OK));
        let resource = TenantResource::read(&answer?)?;
        expect_that!(resource.resource_type, eq(TenantType::Tenant));
        expect_that!(resource.id, eq(&format!("urn:ngsi-ld:Tenant:{name}")));
        expect_that!(resource.tenant_name.to_string(), eq(name.as_str()));
    }
    let request = andromeda_request_to(&front, Method::GET, "tenants")?.append_header(ACCEPT, HeaderValue::from_static("application/json"));
    let answer = front.send_unchecked(request).await;
    expect_that!(&answer, answered(StatusCode::OK));
    let listing = answer?;
    expect_that!(TenantResource::read_all(&listing), ok(anything()));
    let total = listing
        .headers()
        .get("NGSILD-Results-Count")
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.parse::<u64>().ok());
    expect_that!(total, some(ge(2)));
    Ok(())
}
