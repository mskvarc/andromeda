//! Suspending one tenant leaves another serving.

use andromeda_test_suite::{
    attribute_matcher::any_attribute,
    broker,
    entity_matcher::has_attribute,
    identity_matcher::has_id,
    namespace::Namespace,
    outcome::Done,
    problem::problem,
    raw_call::call,
};
use googletest::prelude::*;
use http::{Method, StatusCode};
use ngsi_ld::{
    http::headers::Accept,
    model::{Entity, ErrorType, NgsiLdMediaType, Tenant},
};

/// Pins: suspending one tenant leaves another serving: the suspended one answers 404 `NonexistentTenant` while the
/// other reads its entity and accepts a new one
/// Clause: 4.14, 5.5.10
/// Tenant: private (the subject is suspending one whole tenant beside another)
/// Recorded: Andromeda extension, not v1.9.1: tenant management (the /tenants resources), which clause 5.5.10
/// leaves implementation-specific ("How Tenants are created, is implementation-specific")
#[gtest]
#[tokio::test]
async fn a_mgt_tnt_0006_suspending_one_tenant_leaves_another_serving() -> Result<()> {
    let namespace = Namespace::fresh();
    let front = broker::front()?;
    let serving = broker::private_tenant(&front, &namespace).await?;
    let suspended = front.with_tenant(format!("{namespace}-b").parse::<Tenant>()?);
    let device_id = namespace.urn("Device", 1)?;
    for client in [&serving, &suspended] {
        let device = Entity::builder()
            .id(device_id.clone())
            .entity_type(namespace.entity_type("Device")?)
            .property("reading", 1)
            .build()?;
        client.entities().create(&device).send().await?.done()?;
    }
    // The Tenant resource is an Andromeda extension no NGSI-LD operation covers, so the request is sent as written.
    let suspend = call(
        &front,
        Method::PATCH,
        &format!("tenants/{namespace}-b"),
        &[("Content-Type", "application/json")],
        Some(r#"{"isActive":false}"#),
    )
    .await?;
    expect_that!(suspend.status(), eq(StatusCode::NO_CONTENT));
    let refused = suspended
        .with_responses()
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .retrieve(&device_id)
        .send()
        .await;
    expect_that!(&refused, problem(StatusCode::NOT_FOUND, ErrorType::NonexistentTenant));
    let read = serving
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .retrieve(&device_id)
        .send()
        .await?;
    expect_that!(read.body(), has_id(&device_id));
    let second = Entity::builder()
        .id(namespace.urn("Device", 2)?)
        .entity_type(namespace.entity_type("Device")?)
        .property("reading", 1)
        .build()?;
    serving.entities().create(&second).send().await?.done()?;
    expect_that!(read.body(), has_attribute("reading", any_attribute().value(1)));
    Ok(())
}
