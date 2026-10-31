//! A minimal entity is created and reads back.

use andromeda_test_suite::{broker, entity_matcher::member_count, namespace::Namespace, outcome::Done};
use googletest::prelude::*;
use ngsi_ld::{
    http::headers::{Accept, Location},
    model::{Entity, NgsiLdMediaType, OneOrMany},
};

/// Pins: a Create Entity carrying only id and type answers 201 with a Location naming the entity, and the entity
/// reads back with exactly that id and type
/// Clause: 5.6.1, 6.4.3.1, 5.7.1
#[gtest]
#[tokio::test]
async fn n_ent_cre_0001_a_minimal_entity_is_created_and_reads_back() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let building_id = namespace.urn("Building", 1)?;
    let building_type = namespace.entity_type("Building")?;
    let building = Entity::builder().id(building_id.clone()).entity_type(building_type.clone()).build()?;
    let received = broker.with_responses().entities().create(&building).send().await?;
    received.outcome.done()?;
    expect_that!(
        received.head.typed::<Location>().valid().map(|location| location.as_iri_ref().as_str()),
        some(contains_substring(format!("urn:ngsi-ld:Building:{namespace}-1")))
    );
    let retrieved = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .retrieve(&building_id)
        .send()
        .await?;
    let entity = retrieved.body();
    expect_that!(member_count(entity), eq(2));
    expect_that!(entity.id, eq(&building_id));
    expect_that!(entity.entity_type, eq(&OneOrMany::one(building_type.clone())));
    Ok(())
}
