//! Suspend refuses the data path and resume serves it again.

use andromeda_test_suite::{
    attribute_matcher::any_attribute,
    broker,
    entity_matcher::has_attribute,
    namespace::Namespace,
    outcome::Done,
    problem::problem,
    raw_call::call,
    tenant_resource::{TenantResource, TenantStatus, TenantType},
    wire::header_value,
};
use googletest::prelude::*;
use http::{HeaderName, Method, StatusCode};
use ngsi_ld::{
    http::headers::Accept,
    model::{Entity, ErrorType, NgsiLdMediaType},
};

/// Pins: a tenant reads as active or dormant and in service; suspending it answers 204, refuses its data path with
/// 404 `NonexistentTenant` echoing the tenant, and reports it suspended; resuming it answers 204 and serves the
/// data written before the suspension
/// Clause: 5.5.10, 6.3.14
/// Tenant: private (the subject is suspending and resuming a whole tenant)
/// Recorded: Andromeda extension, not v1.9.1: tenant management (the /tenants resources), which clause 5.5.10
/// leaves implementation-specific ("How Tenants are created, is implementation-specific")
#[gtest]
#[tokio::test]
async fn a_mgt_tnt_0001_suspend_refuses_the_data_path_and_resume_serves_it_again() -> Result<()> {
    let namespace = Namespace::fresh();
    let front = broker::front()?;
    let private = broker::private_tenant(&front, &namespace).await?;
    let tenant_header = HeaderName::from_static("ngsild-tenant");
    let device_id = namespace.urn("Device", 1)?;
    let device = Entity::builder()
        .id(device_id.clone())
        .entity_type(namespace.entity_type("Device")?)
        .property("reading", 1)
        .build()?;
    private.entities().create(&device).send().await?.done()?;
    // The Tenant resource is an Andromeda extension no NGSI-LD operation covers, so its requests are sent as written.
    let tenant_path = format!("tenants/{namespace}");
    let active = call(&front, Method::GET, &tenant_path, &[("Accept", "application/json")], None).await?;
    expect_that!(active.status(), eq(StatusCode::OK));
    let active_resource = TenantResource::read(&active)?;
    expect_that!(active_resource.id, eq(&format!("urn:ngsi-ld:Tenant:{namespace}")));
    expect_that!(active_resource.resource_type, eq(TenantType::Tenant));
    expect_that!(active_resource.tenant_name.to_string(), eq(&namespace.to_string()));
    expect_that!(active_resource.is_active, some(eq(true)));
    expect_that!(active_resource.status, some(any!(eq(TenantStatus::Active), eq(TenantStatus::Dormant))));
    let suspend = call(
        &front,
        Method::PATCH,
        &tenant_path,
        &[("Content-Type", "application/json")],
        Some(r#"{"isActive":false}"#),
    )
    .await?;
    expect_that!(suspend.status(), eq(StatusCode::NO_CONTENT));
    let refused = private
        .with_responses()
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .retrieve(&device_id)
        .send()
        .await;
    expect_that!(
        &refused,
        problem(StatusCode::NOT_FOUND, ErrorType::NonexistentTenant).with_header(tenant_header.clone(), &namespace.to_string())
    );
    let suspended = call(&front, Method::GET, &tenant_path, &[("Accept", "application/json")], None).await?;
    expect_that!(suspended.status(), eq(StatusCode::OK));
    let suspended_resource = TenantResource::read(&suspended)?;
    expect_that!(suspended_resource.tenant_name.to_string(), eq(&namespace.to_string()));
    expect_that!(suspended_resource.is_active, some(eq(false)));
    expect_that!(suspended_resource.status, some(eq(TenantStatus::Suspended)));
    let resume = call(
        &front,
        Method::PATCH,
        &tenant_path,
        &[("Content-Type", "application/json")],
        Some(r#"{"isActive":true}"#),
    )
    .await?;
    expect_that!(resume.status(), eq(StatusCode::NO_CONTENT));
    let served = private
        .with_responses()
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .retrieve(&device_id)
        .send()
        .await?;
    expect_that!(header_value(served.head.headers(), &tenant_header), some(eq(namespace.to_string().as_str())));
    expect_that!(served.outcome.body(), has_attribute("reading", any_attribute().value(1)));
    Ok(())
}
