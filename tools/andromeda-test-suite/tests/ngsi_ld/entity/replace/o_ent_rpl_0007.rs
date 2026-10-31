//! A replacement without an expires at outlives the old expiry.

use andromeda_test_suite::{
    attribute_matcher::any_attribute,
    broker,
    entity_matcher::has_attribute,
    eventually::retried,
    namespace::Namespace,
    outcome::Done,
    problem::problem,
};
use chrono::{TimeDelta, Utc};
use googletest::prelude::*;
use http::StatusCode;
use ngsi_ld::{
    http::{headers::Accept, operation::SystemAttributes},
    model::{DateTime, Entity, ErrorType, NgsiLdMediaType},
};

/// Pins: Replace Entity with a replacement that carries no `expiresAt` leaves the Entity without one, so it is
/// still served after its old `expiresAt` has passed, while an Entity that kept the same `expiresAt` is removed
/// Clause: 4.22, 4.3.6.8, 5.6.18.4, 6.3.11
/// Recorded: 4.22 lets an expired Entity "be automatically removed" by a periodic clean-up whose lag the spec
/// leaves open; Andromeda removes it, and the file waits up to 60 seconds for that
#[gtest]
#[tokio::test]
async fn o_ent_rpl_0007_a_replacement_without_an_expires_at_outlives_the_old_expiry() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let vehicle_replaced_id = namespace.urn("Vehicle", "replaced")?;
    let vehicle_type = namespace.entity_type("Vehicle")?;
    let vehicle_kept_id = namespace.urn("Vehicle", "kept")?;
    let in_10_seconds: DateTime = (Utc::now() + TimeDelta::seconds(10)).format("%Y-%m-%dT%H:%M:%SZ").to_string().parse()?;
    let entities = [
        Entity::builder()
            .id(vehicle_replaced_id.clone())
            .entity_type(vehicle_type.clone())
            .expires_at(in_10_seconds)
            .property("speed", 60)
            .build()?,
        Entity::builder()
            .id(vehicle_kept_id.clone())
            .entity_type(vehicle_type.clone())
            .expires_at(in_10_seconds)
            .property("speed", 60)
            .build()?,
    ];
    broker.entities().batch_create(&entities).send().await?.done()?;
    let vehicle_replaced_replacement = Entity::builder()
        .id(vehicle_replaced_id.clone())
        .entity_type(vehicle_type.clone())
        .property("speed", 70)
        .build()?;
    broker.entities().replace(&vehicle_replaced_replacement).send().await?.done()?;
    retried(async || {
        let answer = broker
            .with_responses()
            .with_accept(Accept::new(NgsiLdMediaType::Json))
            .entities()
            .retrieve(&vehicle_kept_id)
            .send()
            .await;
        verify_that!(&answer, problem(StatusCode::NOT_FOUND, ErrorType::ResourceNotFound))?;
        Ok(())
    })
    .await?;
    let retrieved = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .retrieve(&vehicle_replaced_id)
        .system_attributes(SystemAttributes::Include)
        .send()
        .await?;
    let entity = retrieved.body();
    expect_that!(entity.expires_at, none());
    expect_that!(entity, has_attribute("speed", any_attribute().value(70)));
    Ok(())
}
