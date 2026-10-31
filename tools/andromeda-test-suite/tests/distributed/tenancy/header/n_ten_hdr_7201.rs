//! A request without a tenant is answered without a tenant header.

use andromeda_test_suite::{
    attribute_matcher::any_attribute,
    broker,
    entity_matcher::has_attribute,
    header_matcher::absent,
    namespace::Namespace,
    outcome::Done,
};
use googletest::prelude::*;
use ngsi_ld::{
    http::headers::{Accept, NgsiLdTenant},
    model::Entity,
};

/// Pins: Create Entity and Retrieve Entity without the NGSILD-Tenant header are answered 201 and 200 without an
/// NGSILD-Tenant header
/// Clause: 6.3.14, 4.14
#[gtest]
#[tokio::test]
async fn n_ten_hdr_7201_a_request_without_a_tenant_is_answered_without_a_tenant_header() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let vehicle_id = namespace.urn("Vehicle", 1)?;
    let vehicle_type = namespace.entity_type("Vehicle")?;
    let vehicle = Entity::builder()
        .id(vehicle_id.clone())
        .entity_type(vehicle_type)
        .property("speed", 5)
        .build()?;
    let received = broker.with_responses().entities().create(&vehicle).send().await?;
    received.outcome.done()?;
    expect_that!(received.head.typed::<NgsiLdTenant>(), absent());
    let headed = broker.with_heads().with_accept(Accept::any()).entities().retrieve(&vehicle_id).send().await?;
    expect_that!(headed.head.typed::<NgsiLdTenant>(), absent());
    let retrieved = &headed.outcome;
    let entity = retrieved.body();
    expect_that!(entity, has_attribute("speed", any_attribute().value(5)));
    Ok(())
}
