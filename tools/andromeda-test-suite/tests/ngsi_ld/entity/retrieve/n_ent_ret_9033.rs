//! A local read keeps attribute expiry independent.

use andromeda_test_suite::{
    attribute_matcher::any_attribute,
    attribute_member::Member,
    broker,
    entity_matcher::has_attribute,
    namespace::Namespace,
    outcome::Done,
};
use googletest::prelude::*;
use ngsi_ld::{
    http::headers::Accept,
    model::{DateTime, Entity, NgsiLdMediaType, Property},
};

/// Pins: Retrieve Entity of a locally held Entity with `expiresAt` 2099-01-01, speed expiring in 2098 and
/// `fuelLevel` with no `expiresAt` answers the entity-level `expiresAt`, speed expiring in 2098 and `fuelLevel`
/// without one
/// Clause: 4.5.5.2, 4.22, 5.7.1.4
#[gtest]
#[tokio::test]
async fn n_ent_ret_9033_a_local_read_keeps_attribute_expiry_independent() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let transient_probe_local_only_id = namespace.urn("TransientProbe", "local-only")?;
    let transient_probe_type = namespace.entity_type("TransientProbe")?;
    let transient_probe_local_only = Entity::builder()
        .id(transient_probe_local_only_id.clone())
        .entity_type(transient_probe_type)
        .expires_at("2099-01-01T00:00:00Z".parse()?)
        .property("speed", Property::builder(60).expires_at("2098-01-01T00:00:00Z".parse()?))
        .property("fuelLevel", 70)
        .build()?;
    broker.entities().create(&transient_probe_local_only).send().await?.done()?;
    let retrieved = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .retrieve(&transient_probe_local_only_id)
        .send()
        .await?;
    let entity = retrieved.body();
    expect_that!(entity.expires_at, some(eq("2099-01-01T00:00:00Z".parse::<DateTime>()?)));
    expect_that!(entity, has_attribute("speed", any_attribute().expires_at("2098-01-01T00:00:00Z".parse()?)));
    expect_that!(entity, has_attribute("fuelLevel", any_attribute().without(Member::ExpiresAt)));
    broker.entities().delete(&transient_probe_local_only_id).send().await?.done()?;
    Ok(())
}
