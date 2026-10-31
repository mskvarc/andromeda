//! A q filter deletes the entities it selects only.

use andromeda_test_suite::{broker, namespace::Namespace, outcome::Done, problem::problem};
use googletest::prelude::*;
use http::StatusCode;
use ngsi_ld::{
    http::headers::Accept,
    model::{Entity, ErrorType, NgsiLdMediaType},
    query::{Locality, q},
};

/// Pins: Purge Entities with a type and a q filter answers 204, deletes the entity the filter selects and keeps the
/// one it does not
/// Clause: 5.6.21.4, 6.4.3.3, 4.9
#[gtest]
#[tokio::test]
async fn n_ent_prg_0004_a_q_filter_deletes_the_entities_it_selects_only() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let building_high_id = namespace.urn("Building", "high")?;
    let building_type = namespace.entity_type("Building")?;
    let building_low_id = namespace.urn("Building", "low")?;
    let entities = [
        Entity::builder()
            .id(building_high_id.clone())
            .entity_type(building_type.clone())
            .property("airQualityLevel", 50)
            .build()?,
        Entity::builder()
            .id(building_low_id.clone())
            .entity_type(building_type.clone())
            .property("airQualityLevel", 2)
            .build()?,
    ];
    broker.entities().batch_create(&entities).send().await?.done()?;
    broker
        .entities()
        .purge()
        .entity_type(&building_type)
        .q(q::attr("airQualityLevel").gt(30))
        .locality(Locality::Local)
        .send()
        .await?
        .done()?;
    let answer = broker
        .with_responses()
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .retrieve(&building_high_id)
        .send()
        .await;
    expect_that!(&answer, problem(StatusCode::NOT_FOUND, ErrorType::ResourceNotFound));
    let retrieved = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .retrieve(&building_low_id)
        .send()
        .await?;
    let entity = retrieved.body();
    expect_that!(entity.id, eq(&building_low_id));
    Ok(())
}
