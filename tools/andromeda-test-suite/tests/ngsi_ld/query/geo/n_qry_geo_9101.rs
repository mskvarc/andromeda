//! A geo-query after a load finds an entity created just before it.

use andromeda_test_suite::{broker, identity_matcher::has_id, namespace::Namespace, nth::nth, outcome::Done};
use googletest::prelude::*;
use ngsi_ld::{
    http::headers::Accept,
    model::{
        Entity,
        Input,
        NgsiLdMediaType,
        geometry::{Position, Shape},
    },
    query::geoquery,
};

/// How many Spots the load creates, more than a geo-query takes into the in-memory spatial index itself before it
/// narrows, so the query may be answered while the index is still taking the load in.
const LOADED: u32 = 10_500;

/// How many Spots one Batch Create carries.
const BATCH: usize = 500;

/// Pins: after Batch Create of 10 500 Spots of one type at (13.88893, 52.534473), about 34 km from the reference
/// point (13.38893, 52.534473), and Create Entity of one more Spot at the reference point, Query Entities of that
/// type with near;maxDistance==1000 around the reference point, sent at once, answers exactly the last Spot
/// Clause: 4.10, 5.7.2.4
#[gtest]
#[tokio::test]
async fn n_qry_geo_9101_a_geo_query_after_a_load_finds_an_entity_created_just_before_it() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let spot_type = namespace.entity_type("Spot")?;
    let last_id = namespace.urn("Spot", "last")?;
    let reference = Shape::Point(Position::try_new(13.38893, 52.534_473)?);
    let load = (0..LOADED)
        .map(|index| -> Result<Entity<Input>> {
            Ok(Entity::builder()
                .id(namespace.urn("Spot", format!("loaded-{index}"))?)
                .entity_type(spot_type.clone())
                .geo_property("location", Shape::Point(Position::try_new(13.88893, 52.534_473)?))
                .build()?)
        })
        .collect::<Result<Vec<_>>>()?;
    for batch in load.chunks(BATCH) {
        broker.entities().batch_create(batch).send().await?.done()?;
    }
    let last = Entity::builder()
        .id(last_id.clone())
        .entity_type(spot_type.clone())
        .geo_property("location", reference.clone())
        .build()?;
    broker.entities().create(&last).send().await?.done()?;
    let answer = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .query()
        .entity_type(&spot_type)
        .geo_q(geoquery::near_max(1000, reference))
        .send()
        .await?;
    expect_that!(answer.items(), len(eq(1)));
    expect_that!(answer.items(), nth(0, has_id(&last_id)));
    Ok(())
}
