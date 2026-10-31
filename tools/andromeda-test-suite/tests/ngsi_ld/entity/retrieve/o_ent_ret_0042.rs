//! An attribute expires at reads back without sys attrs.

use andromeda_test_suite::{
    attribute_matcher::any_attribute,
    broker,
    entity_matcher::has_attribute,
    namespace::Namespace,
    outcome::Done,
    system_time::{self, SystemMember},
};
use googletest::prelude::*;
use ngsi_ld::{
    http::headers::Accept,
    model::{Entity, NgsiLdMediaType, Property},
};

/// Pins: Retrieve Entity without options answers an Attribute's `expiresAt` as written
/// Clause: 4.22, 6.3.11, 5.7.1.4
/// Recorded: 6.3.11 requires `expiresAt` with `sysAttrs` and says nothing of a request without it; Andromeda
/// answers `expiresAt` either way, as a member the client wrote
#[gtest]
#[tokio::test]
async fn o_ent_ret_0042_an_attribute_expires_at_reads_back_without_sys_attrs() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let vehicle_id = namespace.urn("Vehicle", 1)?;
    let vehicle_type = namespace.entity_type("Vehicle")?;
    let vehicle = Entity::builder()
        .id(vehicle_id.clone())
        .entity_type(vehicle_type)
        .property("speed", Property::builder(90).expires_at("2099-12-31T23:59:59Z".parse()?))
        .build()?;
    broker.entities().create(&vehicle).send().await?.done()?;
    let retrieved = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .retrieve(&vehicle_id)
        .send()
        .await?;
    let entity = retrieved.body();
    expect_that!(system_time::of_attribute(entity, &["speed"], SystemMember::CreatedAt).ok(), none());
    expect_that!(
        entity,
        has_attribute("speed", any_attribute().value(90).expires_at("2099-12-31T23:59:59Z".parse()?))
    );
    Ok(())
}
