//! ETSI `019_11`: several entities are queried with a geoquery whose geometry is a Polygon or a Point, and the
//! `NGSILD-Results-Count` header states how many match (clauses 5.7.2 and 4.10).

use crate::{
    suite_keywords::{create_entity, delete_entities, results_count},
    suite_payloads::{point, polygon},
};
use andromeda_test_suite::{
    broker::{self, BrokerClient},
    namespace::Namespace,
    problem::answered,
    vocabulary::Vocabulary,
};
use googletest::prelude::*;
use http::StatusCode;
use ngsi_ld::{
    http::{headers::Accept, operation::ResultsCount},
    model::{Entity, Input},
    query::geoquery,
};

/// The terms of the `@context` the cases of this file register.
const TERMS: [&str; 2] = ["Building", "name"];

/// The area of `urn:ngsi-ld:Building:<ns>-1`, a closed ring of six (longitude, latitude) positions.
const EIFFEL_AREA: [(f64, f64); 6] = [
    (13.287_963_9, 52.564_864_5),
    (13.286_590_6, 52.564_864_5),
    (13.279_724_1, 52.498_867_9),
    (13.477_478, 52.471_270_3),
    (13.504_943_8, 52.537_308_4),
    (13.287_963_9, 52.564_864_5),
];

/// The area of `urn:ngsi-ld:Building:<ns>-2`, a closed ring of four (longitude, latitude) positions.
const SOUTH_AREA: [(f64, f64); 4] = [
    (13.315_773, 52.475_720_9),
    (13.339_119, 52.444_342_1),
    (13.389_244_1, 52.466_937_1),
    (13.315_773, 52.475_720_9),
];

/// The query polygon of the intersects and overlaps cases.
const CROSSING_QUERY: [(f64, f64); 5] = [
    (13.345_642_1, 52.479_484_8),
    (13.364_181_5, 52.442_877_3),
    (13.417_739_9, 52.473_002_3),
    (13.345_985_4, 52.479_903),
    (13.345_642_1, 52.479_484_8),
];

/// The query polygon of the contains case whose geometry lies inside the Eiffel Tower area.
const INNER_QUERY: [(f64, f64); 5] = [
    (13.384_437_6, 52.535_696_6),
    (13.381_691, 52.512_301_2),
    (13.423_576_4, 52.510_002_7),
    (13.433_189_4, 52.527_342_5),
    (13.384_437_6, 52.535_696_6),
];

/// The query polygon that neither Building lies inside, the contains case without a match and the disjoint case.
const OUTER_QUERY: [(f64, f64); 5] = [
    (13.136_901_9, 52.519_822_5),
    (13.136_558_5, 52.520_031_4),
    (13.131_752, 52.498_090_7),
    (13.183_250_4, 52.502_688_7),
    (13.136_901_9, 52.519_822_5),
];

/// The query polygon of the within case.
const ENCLOSING_QUERY: [(f64, f64); 5] = [
    (13.302_383_4, 52.490_306_6),
    (13.274_574_3, 52.465_632),
    (13.334_312_4, 52.418_544_7),
    (13.431_129_5, 52.47107),
    (13.302_383_4, 52.490_306_6),
];

/// The Building `urn:ngsi-ld:Building:<ns>-<n>` named Eiffel Tower, located in `area`.
fn building_in(namespace: &Namespace, n: u32, area: &[(f64, f64)]) -> Result<Entity<Input>> {
    Ok(Entity::builder()
        .id(namespace.urn("Building", n)?)
        .entity_type("Building")
        .property("name", "Eiffel Tower")
        .geo_property("location", polygon(area)?)
        .build()?)
}

/// Creates the two Buildings of the cases: `urn:ngsi-ld:Building:<ns>-1` in the Eiffel Tower area and
/// `urn:ngsi-ld:Building:<ns>-2` in the south area.
async fn create_buildings(broker: &BrokerClient, vocabulary: &Vocabulary, namespace: &Namespace) -> Result<()> {
    create_entity(broker, vocabulary, &building_in(namespace, 1, &EIFFEL_AREA)?).await?;
    create_entity(broker, vocabulary, &building_in(namespace, 2, &SOUTH_AREA)?).await
}

/// Pins: Check that one can query with an exact polygon
/// Clause: 5.7.2, 4.10
#[gtest]
#[tokio::test]
async fn e019_11_01_query_on_equals_polygon() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let vocabulary = Vocabulary::register(&broker, &namespace, &TERMS).await?;
    create_buildings(&broker, &vocabulary, &namespace).await?;
    let received = broker
        .with_context(vocabulary.context())?
        .with_accept(Accept::any())
        .with_responses()
        .entities()
        .query()
        .entity_type("Building")
        .geo_q(geoquery::equals(polygon(&EIFFEL_AREA)?))
        .count(ResultsCount::Include)
        .send()
        .await;
    verify_that!(&received, answered(StatusCode::OK))?;
    let received = received?;
    expect_that!(results_count(&received), some(eq("1")));
    delete_entities(&broker, &[&namespace.urn("Building", 1)?, &namespace.urn("Building", 2)?]).await;
    Ok(())
}

/// Pins: Check that one can query with an intersects polygon
/// Clause: 5.7.2, 4.10
#[gtest]
#[tokio::test]
async fn e019_11_02_query_on_intersects_polygon() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let vocabulary = Vocabulary::register(&broker, &namespace, &TERMS).await?;
    create_buildings(&broker, &vocabulary, &namespace).await?;
    let received = broker
        .with_context(vocabulary.context())?
        .with_accept(Accept::any())
        .with_responses()
        .entities()
        .query()
        .entity_type("Building")
        .geo_q(geoquery::intersects(polygon(&CROSSING_QUERY)?))
        .count(ResultsCount::Include)
        .send()
        .await;
    verify_that!(&received, answered(StatusCode::OK))?;
    let received = received?;
    expect_that!(results_count(&received), some(eq("1")));
    delete_entities(&broker, &[&namespace.urn("Building", 1)?, &namespace.urn("Building", 2)?]).await;
    Ok(())
}

/// Pins: Check that one can query with a contains polygon
/// Clause: 5.7.2, 4.10
#[gtest]
#[tokio::test]
async fn e019_11_03_query_on_contains_polygon_matching() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let vocabulary = Vocabulary::register(&broker, &namespace, &TERMS).await?;
    create_buildings(&broker, &vocabulary, &namespace).await?;
    let received = broker
        .with_context(vocabulary.context())?
        .with_accept(Accept::any())
        .with_responses()
        .entities()
        .query()
        .entity_type("Building")
        .geo_q(geoquery::contains(polygon(&INNER_QUERY)?))
        .count(ResultsCount::Include)
        .send()
        .await;
    verify_that!(&received, answered(StatusCode::OK))?;
    let received = received?;
    expect_that!(results_count(&received), some(eq("1")));
    delete_entities(&broker, &[&namespace.urn("Building", 1)?, &namespace.urn("Building", 2)?]).await;
    Ok(())
}

/// Pins: Check that one can query with a contains polygon without matching geometries
/// Clause: 5.7.2, 4.10
#[gtest]
#[tokio::test]
async fn e019_11_04_query_on_contains_polygon_not_matching() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let vocabulary = Vocabulary::register(&broker, &namespace, &TERMS).await?;
    create_buildings(&broker, &vocabulary, &namespace).await?;
    let received = broker
        .with_context(vocabulary.context())?
        .with_accept(Accept::any())
        .with_responses()
        .entities()
        .query()
        .entity_type("Building")
        .geo_q(geoquery::contains(polygon(&OUTER_QUERY)?))
        .count(ResultsCount::Include)
        .send()
        .await;
    verify_that!(&received, answered(StatusCode::OK))?;
    let received = received?;
    expect_that!(results_count(&received), some(eq("0")));
    delete_entities(&broker, &[&namespace.urn("Building", 1)?, &namespace.urn("Building", 2)?]).await;
    Ok(())
}

/// Pins: Check that one can query with a disjoint polygon
/// Clause: 5.7.2, 4.10
#[gtest]
#[tokio::test]
async fn e019_11_05_query_on_disjoint_polygon() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let vocabulary = Vocabulary::register(&broker, &namespace, &TERMS).await?;
    create_buildings(&broker, &vocabulary, &namespace).await?;
    let received = broker
        .with_context(vocabulary.context())?
        .with_accept(Accept::any())
        .with_responses()
        .entities()
        .query()
        .entity_type("Building")
        .geo_q(geoquery::disjoint(polygon(&OUTER_QUERY)?))
        .count(ResultsCount::Include)
        .send()
        .await;
    verify_that!(&received, answered(StatusCode::OK))?;
    let received = received?;
    expect_that!(results_count(&received), some(eq("2")));
    delete_entities(&broker, &[&namespace.urn("Building", 1)?, &namespace.urn("Building", 2)?]).await;
    Ok(())
}

/// Pins: Check that one can query with a within polygon
/// Clause: 5.7.2, 4.10
#[gtest]
#[tokio::test]
async fn e019_11_06_query_on_within_polygon() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let vocabulary = Vocabulary::register(&broker, &namespace, &TERMS).await?;
    create_buildings(&broker, &vocabulary, &namespace).await?;
    let received = broker
        .with_context(vocabulary.context())?
        .with_accept(Accept::any())
        .with_responses()
        .entities()
        .query()
        .entity_type("Building")
        .geo_q(geoquery::within(polygon(&ENCLOSING_QUERY)?))
        .count(ResultsCount::Include)
        .send()
        .await;
    verify_that!(&received, answered(StatusCode::OK))?;
    let received = received?;
    expect_that!(results_count(&received), some(eq("1")));
    delete_entities(&broker, &[&namespace.urn("Building", 1)?, &namespace.urn("Building", 2)?]).await;
    Ok(())
}

/// Pins: Check that one can query with an overlaps polygon
/// Clause: 5.7.2, 4.10
#[gtest]
#[tokio::test]
async fn e019_11_07_query_on_overlaps_polygon() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let vocabulary = Vocabulary::register(&broker, &namespace, &TERMS).await?;
    create_buildings(&broker, &vocabulary, &namespace).await?;
    let received = broker
        .with_context(vocabulary.context())?
        .with_accept(Accept::any())
        .with_responses()
        .entities()
        .query()
        .entity_type("Building")
        .geo_q(geoquery::overlaps(polygon(&CROSSING_QUERY)?))
        .count(ResultsCount::Include)
        .send()
        .await;
    verify_that!(&received, answered(StatusCode::OK))?;
    let received = received?;
    expect_that!(results_count(&received), some(eq("1")));
    delete_entities(&broker, &[&namespace.urn("Building", 1)?, &namespace.urn("Building", 2)?]).await;
    Ok(())
}

/// Pins: Check that one can query with a contains point
/// Clause: 5.7.2, 4.10
#[gtest]
#[tokio::test]
async fn e019_11_08_query_on_contains_point() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let vocabulary = Vocabulary::register(&broker, &namespace, &TERMS).await?;
    create_buildings(&broker, &vocabulary, &namespace).await?;
    let received = broker
        .with_context(vocabulary.context())?
        .with_accept(Accept::any())
        .with_responses()
        .entities()
        .query()
        .entity_type("Building")
        .geo_q(geoquery::contains(point(13.344_268_8, 52.465_004_5)?))
        .count(ResultsCount::Include)
        .send()
        .await;
    verify_that!(&received, answered(StatusCode::OK))?;
    let received = received?;
    expect_that!(results_count(&received), some(eq("1")));
    delete_entities(&broker, &[&namespace.urn("Building", 1)?, &namespace.urn("Building", 2)?]).await;
    Ok(())
}
