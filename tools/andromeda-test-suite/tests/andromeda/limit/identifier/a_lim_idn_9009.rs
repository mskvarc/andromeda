//! An entity at the bound has its temporal evolution under the same id.

use andromeda_test_suite::{broker, evolution_matcher::has_evolution, exact_length::padded, namespace::Namespace};
use googletest::prelude::*;
use ngsi_ld::{
    http::headers::{Accept, Location},
    model::{Entity, EntityId, NgsiLdMediaType, Property},
};

/// Pins: Create Entity whose id is exactly 1,024 octets answers 201, and both Retrieve Entity and Retrieve Temporal
/// Evolution of an Entity answer it under that id
/// Clause: 5.6.1, 5.7.1, 5.7.3, 4.3.1
/// Recorded: ETSI GS CIM 009 sets no length on an identifier; Andromeda bounds every identifier it stores at 1,024
/// octets, the value as stored, which for a term is its expansion
#[gtest]
#[tokio::test]
async fn a_lim_idn_9009_an_entity_at_the_bound_has_its_temporal_evolution_under_the_same_id() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let id = padded(&namespace.urn_text("Building", "entropy:ieqh524yng5by1a2rogubbb8ayn1b7o259owoo3s"), 1024)?.parse::<EntityId>()?;
    let building = Entity::builder()
        .id(id.clone())
        .entity_type(namespace.entity_type("Building")?)
        .property("attr0", Property::builder(0).observed_at("2024-01-01T00:00:00Z".parse()?))
        .build()?;
    let created = broker.with_responses().entities().create(&building).send().await?;
    expect_that!(
        created.head.typed::<Location>().valid().map(|location| location.as_iri_ref().as_str()),
        some(eq(format!("/ngsi-ld/v1/entities/{id}").as_str()))
    );
    let retrieved = broker.with_accept(Accept::new(NgsiLdMediaType::Json)).entities().retrieve(&id).send().await?;
    expect_that!(retrieved.body().id, eq(&id));
    expect_that!(retrieved.body().id.to_string().len(), eq(1024));
    // A temporal read defaults to the observedAt axis (Table 6.19.3.1-1), which holds only the instances that carry it.
    let evolution = broker.with_accept(Accept::new(NgsiLdMediaType::Json)).temporal().retrieve(&id).send().await?;
    expect_that!(evolution.body().id, eq(&id));
    expect_that!(evolution.body(), has_evolution("attr0", len(eq(1))));
    Ok(())
}
