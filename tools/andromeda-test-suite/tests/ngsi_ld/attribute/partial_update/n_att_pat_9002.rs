//! A partial update holding only a context changes nothing.

use andromeda_test_suite::{
    attribute_matcher::any_attribute,
    broker,
    config::SuiteConfig,
    entity_matcher::has_attribute,
    namespace::Namespace,
    outcome::Done,
    problem::answered,
    system_time::{self, SystemMember},
    wire::{content_type, request_to},
};
use bytes::Bytes;
use googletest::prelude::*;
use http::{Method, StatusCode};
use ngsi_ld::{
    http::{headers::Accept, operation::SystemAttributes},
    model::{APPLICATION_LD_JSON, Entity, NgsiLdMediaType},
};

/// Pins: Partial Attribute Update as application/ld+json with a fragment holding only the Core @context answers 204,
/// and the entity and every modifiedAt stay as they were
/// Clause: 5.4, 5.6.4.4, 6.3.5
#[gtest]
#[tokio::test]
async fn n_att_pat_9002_a_partial_update_holding_only_a_context_changes_nothing() -> Result<()> {
    let namespace = Namespace::fresh();
    let config = SuiteConfig::get()?;
    let broker = broker::front()?;
    let vehicle_id = namespace.urn("Vehicle", 1)?;
    let vehicle_type = namespace.entity_type("Vehicle")?;
    let vehicle = Entity::builder()
        .id(vehicle_id.clone())
        .entity_type(vehicle_type)
        .property("speed", 50)
        .property("brandName", "Mercedes")
        .build()?;
    broker.entities().create(&vehicle).send().await?.done()?;
    let retrieved = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .retrieve(&vehicle_id)
        .system_attributes(SystemAttributes::Include)
        .send()
        .await?;
    let entity = retrieved.body();
    let entity_modified = system_time::of_entity(entity, SystemMember::ModifiedAt)?;
    let speed_modified = system_time::of_attribute(entity, &["speed"], SystemMember::ModifiedAt)?;
    let brand_modified = system_time::of_attribute(entity, &["brandName"], SystemMember::ModifiedAt)?;
    // A fragment holding only an `@context` is no attribute the model can write, so it is sent as written.
    let request = content_type(
        request_to(&broker, Method::PATCH, &format!("entities/{vehicle_id}/attrs/speed"))?,
        &APPLICATION_LD_JSON,
    )?
    .body(Bytes::from(format!(r#"{{"@context": ["{}"]}}"#, config.core_context().as_str())));
    let answer = broker.send_unchecked(request).await;
    expect_that!(&answer, answered(StatusCode::NO_CONTENT));
    let retrieved_2 = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .retrieve(&vehicle_id)
        .system_attributes(SystemAttributes::Include)
        .send()
        .await?;
    let entity_2 = retrieved_2.body();
    expect_that!(system_time::of_entity(entity_2, SystemMember::ModifiedAt)?, eq(entity_modified));
    expect_that!(entity_2, has_attribute("speed", any_attribute().value(50)));
    expect_that!(system_time::of_attribute(entity_2, &["speed"], SystemMember::ModifiedAt)?, eq(speed_modified));
    expect_that!(entity_2, has_attribute("brandName", any_attribute().value("Mercedes")));
    expect_that!(
        system_time::of_attribute(entity_2, &["brandName"], SystemMember::ModifiedAt)?,
        eq(brand_modified)
    );
    Ok(())
}
