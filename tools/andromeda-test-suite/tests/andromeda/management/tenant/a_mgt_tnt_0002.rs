//! Options add system attributes and usage.

use andromeda_test_suite::{
    broker,
    namespace::Namespace,
    outcome::Done,
    problem::problem,
    raw_call::{call, target},
    tenant_resource::TenantResource,
    wire::request_to,
};
use googletest::prelude::*;
use http::{HeaderValue, Method, StatusCode, header::ACCEPT};
use ngsi_ld::model::{Entity, ErrorType};

/// Pins: a tenant read carries no system attributes and no usage by default, options `sysAttrs` adds `createdAt`
/// and `modifiedAt`, options usage adds a nonzero `databaseSize`, the entity count and the snapshot and pool
/// figures, and an unknown options keyword is refused with `InvalidRequest`
/// Clause: 6.3.20
/// Tenant: private (the subject is a whole tenant's usage, its entity count included)
/// Recorded: Andromeda extension, not v1.9.1: tenant management (the /tenants resources), which clause 5.5.10
/// leaves implementation-specific ("How Tenants are created, is implementation-specific")
#[gtest]
#[tokio::test]
async fn a_mgt_tnt_0002_options_add_system_attributes_and_usage() -> Result<()> {
    let namespace = Namespace::fresh();
    let front = broker::front()?;
    let private = broker::private_tenant(&front, &namespace).await?;
    for n in [1, 2] {
        let device = Entity::builder()
            .id(namespace.urn("Device", n)?)
            .entity_type(namespace.entity_type("Device")?)
            .property("reading", 1)
            .build()?;
        private.entities().create(&device).send().await?.done()?;
    }
    let accept = [("Accept", "application/json")];
    // The Tenant resource and its `options` are an Andromeda extension no NGSI-LD operation covers, so the requests are
    // sent as written.
    let tenant_path = format!("tenants/{namespace}");
    let plain = call(&front, Method::GET, &tenant_path, &accept, None).await?;
    expect_that!(plain.status(), eq(StatusCode::OK));
    let plain_resource = TenantResource::read(&plain)?;
    expect_that!(plain_resource.tenant_name.to_string(), eq(&namespace.to_string()));
    expect_that!(plain_resource.created_at, none());
    expect_that!(plain_resource.modified_at, none());
    expect_that!(plain_resource.database_size, none());
    expect_that!(plain_resource.entity_count, none());
    let with_system_attributes = call(&front, Method::GET, &target(&tenant_path, &[("options", "sysAttrs")]), &accept, None).await?;
    expect_that!(with_system_attributes.status(), eq(StatusCode::OK));
    let system_resource = TenantResource::read(&with_system_attributes)?;
    expect_that!(system_resource.tenant_name.to_string(), eq(&namespace.to_string()));
    expect_that!(system_resource.created_at, some(anything()));
    expect_that!(system_resource.modified_at, some(anything()));
    let with_usage = call(&front, Method::GET, &target(&tenant_path, &[("options", "usage")]), &accept, None).await?;
    expect_that!(with_usage.status(), eq(StatusCode::OK));
    let usage = TenantResource::read(&with_usage)?;
    expect_that!(usage.tenant_name.to_string(), eq(&namespace.to_string()));
    expect_that!(usage.entity_count, some(eq(2)));
    expect_that!(usage.pool_connections, some(anything()));
    expect_that!(usage.database_size, some(gt(0)));
    expect_that!(usage.snapshot_count, some(anything()));
    let unknown_request =
        request_to(&front, Method::GET, &target(&tenant_path, &[("options", "bogus")]))?.append_header(ACCEPT, HeaderValue::from_static("application/json"));
    let unknown = front.send_unchecked(unknown_request).await;
    expect_that!(&unknown, problem(StatusCode::BAD_REQUEST, ErrorType::InvalidRequest));
    Ok(())
}
