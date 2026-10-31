//! A geoquery sheds a mapped entity that moved away.

use andromeda_test_suite::{
    broker,
    identity_matcher::has_id,
    namespace::Namespace,
    outcome::Done,
    problem::answered,
    received_status::ExpectedStatus,
    written_json::json_request_to,
};
use googletest::prelude::*;
use http::{Method, StatusCode};
use ngsi_ld::{
    http::{headers::Accept, operation::EntityMapRequest},
    model::{
        Entity,
        NgsiLdMediaType,
        geometry::{Position, Shape},
    },
    query::geoquery,
};

/// Pins: Query Entities by type and a near geoquery under an `EntityMap` of four Entities inside the circle, after
/// the first page of two, answers a later page without the Entity whose location was replaced far away, and the
/// `EntityMap` no longer lists it
/// Clause: 4.3.6.7, 5.5.9.3, 5.5.14
#[gtest]
#[tokio::test]
async fn n_qry_pag_9003_a_geoquery_sheds_a_mapped_entity_that_moved_away() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let building_type = namespace.entity_type("Building")?;
    let mut entities = Vec::new();
    for n in 1..=4 {
        entities.push(
            Entity::builder()
                .id(namespace.urn("Building", n)?)
                .entity_type(building_type.clone())
                .geo_property("location", Shape::Point(Position::try_new(13.4, 52.52)?))
                .build()?,
        );
    }
    broker.entities().batch_create(&entities).send().await?.done()?;
    let created = broker
        .with_heads()
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entity_maps()
        .create()
        .entity_type(&building_type)
        .geo_q(geoquery::near_max(15000, Shape::Point(Position::try_new(13.388_930, 52.534_473)?)))
        .send()
        .await?
        .expecting(StatusCode::CREATED)?;
    expect_that!(&created.body().entity_map, len(eq(4)));
    let map_id = created.body().id.clone();
    let map_request = EntityMapRequest::reuse(created.entity_map().valid().or_fail()?.clone());
    let json = broker.with_accept(Accept::new(NgsiLdMediaType::Json));
    let first = json
        .entities()
        .query()
        .entity_type(&building_type)
        .geo_q(geoquery::near_max(15000, Shape::Point(Position::try_new(13.388_930, 52.534_473)?)))
        .limit(2)
        .offset(0)
        .entity_map(&map_request)
        .send()
        .await?;
    expect_that!(first.items(), len(eq(2)));
    // The third Entity of the sequence is one the first page did not offer, so it is checked again only by the pages
    // read after it stops fitting.
    let third = json
        .entities()
        .query()
        .entity_type(&building_type)
        .geo_q(geoquery::near_max(15000, Shape::Point(Position::try_new(13.388_930, 52.534_473)?)))
        .limit(1)
        .offset(2)
        .entity_map(&map_request)
        .send()
        .await?;
    expect_that!(third.items(), len(eq(1)));
    let victim = third.items().first().or_fail()?.id.clone();
    // Replace Entity takes an entity with an id, where this body leaves the id to the path.
    let replacement =
        format!(r#"{{"type": "{building_type}", "location": {{"type": "GeoProperty", "value": {{"type": "Point", "coordinates": [20.0, 40.0]}}}}}}"#);
    let request = json_request_to(&broker, Method::PUT, &format!("entities/{victim}"), &replacement)?;
    expect_that!(&broker.send_unchecked(request).await, answered(StatusCode::NO_CONTENT));
    let rest = json
        .entities()
        .query()
        .entity_type(&building_type)
        .geo_q(geoquery::near_max(15000, Shape::Point(Position::try_new(13.388_930, 52.534_473)?)))
        .limit(100)
        .offset(0)
        .entity_map(&map_request)
        .send()
        .await?;
    expect_that!(rest.items(), len(eq(3)));
    expect_that!(rest.items(), not(contains(has_id(&victim))));
    let map = json.entity_maps().retrieve(&map_id).send().await?;
    expect_that!(&map.entity_map, len(eq(3)));
    expect_that!(map.entity_map.contains_key(&victim), eq(false));
    broker.entity_maps().delete(&map_id).send().await?;
    Ok(())
}
