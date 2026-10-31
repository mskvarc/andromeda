//! A purge under an unknown map purges by query.

use andromeda_test_suite::{broker, header_matcher::absent, namespace::Namespace, outcome::Done};
use googletest::prelude::*;
use iri_rs::IriRefBuf;
use ngsi_ld::{
    http::headers::{Accept, NgsiLdEntityMap},
    model::{Entity, NgsiLdMediaType, Value},
};

/// Pins: Purge Entities by type and three ids under the `NGSILD-EntityMap` header of an `EntityMap` that was never
/// created answers 204 without that header and deletes all three
/// Clause: 5.5.14, 5.6.21.4, 6.4.3.3
#[gtest]
#[tokio::test]
async fn o_ent_prg_9004_a_purge_under_an_unknown_map_purges_by_query() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let building_id = namespace.urn("Building", 1)?;
    let building_type = namespace.entity_type("Building")?;
    let building_2_id = namespace.urn("Building", 2)?;
    let building_3_id = namespace.urn("Building", 3)?;
    let entities = [
        Entity::builder()
            .id(building_id.clone())
            .entity_type(building_type.clone())
            .property("temperature", Value::Number("30.0".parse()?))
            .build()?,
        Entity::builder()
            .id(building_2_id.clone())
            .entity_type(building_type.clone())
            .property("temperature", Value::Number("30.0".parse()?))
            .build()?,
        Entity::builder()
            .id(building_3_id.clone())
            .entity_type(building_type.clone())
            .property("temperature", Value::Number("30.0".parse()?))
            .build()?,
    ];
    broker.entities().batch_create(&entities).send().await?.done()?;
    // No inference can be made from an EntityMap that cannot be accessed (clause 5.5.14), and Purge Entities offers no way to create one.
    let entity_map = NgsiLdEntityMap::try_new(IriRefBuf::new(format!("urn:ngsi-ld:EntityMap:{namespace}-never-created"))?)?;
    let received = broker
        .with_responses()
        .entities()
        .purge()
        .entity_type(&building_type)
        .ids(vec![building_id.clone(), building_2_id.clone(), building_3_id.clone()])
        .entity_map(&entity_map)
        .send()
        .await?;
    received.outcome.done()?;
    expect_that!(received.head.typed::<NgsiLdEntityMap>(), absent());
    let page = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .query()
        .entity_type(&building_type)
        .send()
        .await?;
    expect_that!(page.items(), is_empty());
    Ok(())
}
