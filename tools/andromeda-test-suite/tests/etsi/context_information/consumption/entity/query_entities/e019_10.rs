//! ETSI `019_10`: entities are selected by a geoquery over their `location` (`georel` equals, near, within, disjoint,
//! contains and intersects), and the `NGSILD-Results-Count` header states how many match (clauses 5.7.2 and 4.10).

use crate::{
    suite_keywords::{create_entity, delete_entities, results_count},
    suite_payloads::{eiffel_tower_located, point},
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
    model::{
        Entity,
        Input,
        geometry::{LinearRing, Polygon, Position, Shape},
    },
    query::geoquery,
};

/// The terms of the `@context` the cases of this file register.
const TERMS: [&str; 3] = ["Building", "name", "subCategory"];

/// The Building `urn:ngsi-ld:Building:<ns>-2` of the cases: named Berlin Brandenburg International Airport, located at
/// the point `13.509_818`, `52.363_457`.
fn berlin_airport(namespace: &Namespace) -> Result<Entity<Input>> {
    Ok(Entity::builder()
        .id(namespace.urn("Building", 2)?)
        .entity_type("Building")
        .property("name", "Berlin Brandenburg International Airport")
        .geo_property("location", point(13.509_818, 52.363_457)?)
        .build()?)
}

/// The Building `urn:ngsi-ld:Building:<ns>-3` of the cases: named Grunewald, located at the point `13.215_983`, `52.470_441`.
fn grunewald(namespace: &Namespace) -> Result<Entity<Input>> {
    Ok(Entity::builder()
        .id(namespace.urn("Building", 3)?)
        .entity_type("Building")
        .property("name", "Grunewald")
        .geo_property("location", point(13.215_983, 52.470_441)?)
        .build()?)
}

/// Creates the three Buildings of the cases, `urn:ngsi-ld:Building:<ns>-1` to `-3`, in `vocabulary`'s context.
async fn create_buildings(broker: &BrokerClient, vocabulary: &Vocabulary, namespace: &Namespace) -> Result<()> {
    create_entity(broker, vocabulary, &eiffel_tower_located(namespace, 1)?).await?;
    create_entity(broker, vocabulary, &berlin_airport(namespace)?).await?;
    create_entity(broker, vocabulary, &grunewald(namespace)?).await
}

/// The polygon of the within and disjoint cases: five positions around the south of Berlin, the first repeated last.
fn berlin_polygon() -> Result<Shape> {
    let corners = [
        (13.161_621_1, 52.543_812_4),
        (13.597_641, 52.484_022_4),
        (13.594_894_4, 52.340_278_2),
        (13.142_395, 52.386_428),
        (13.161_621_1, 52.543_812_4),
    ];
    let ring = LinearRing::try_new(
        corners
            .into_iter()
            .map(|(longitude, latitude)| Position::try_new(longitude, latitude))
            .collect::<Result<_, _>>()?,
    )?;
    Ok(Shape::Polygon(Polygon::try_new(vec![ring])?))
}

/// Pins: Check that one can query with an exact point
/// Clause: 5.7.2, 4.10
#[gtest]
#[tokio::test]
async fn e019_10_01_query_on_equals_point() -> Result<()> {
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
        .geo_q(geoquery::equals(point(13.3986, 52.5547)?))
        .count(ResultsCount::Include)
        .send()
        .await;
    verify_that!(&received, answered(StatusCode::OK))?;
    let received = received?;
    expect_that!(results_count(&received), some(eq("1")));
    delete_entities(
        &broker,
        &[&namespace.urn("Building", 1)?, &namespace.urn("Building", 2)?, &namespace.urn("Building", 3)?],
    )
    .await;
    Ok(())
}

/// Pins: Check that one can query with a max distance
/// Clause: 5.7.2, 4.10
#[gtest]
#[tokio::test]
async fn e019_10_02_query_on_max_distance_point() -> Result<()> {
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
        .geo_q(geoquery::near_max(15_000, point(13.388_930, 52.534_473)?))
        .count(ResultsCount::Include)
        .send()
        .await;
    verify_that!(&received, answered(StatusCode::OK))?;
    let received = received?;
    expect_that!(results_count(&received), some(eq("2")));
    delete_entities(
        &broker,
        &[&namespace.urn("Building", 1)?, &namespace.urn("Building", 2)?, &namespace.urn("Building", 3)?],
    )
    .await;
    Ok(())
}

/// Pins: Check that one can query with a min distance
/// Clause: 5.7.2, 4.10
#[gtest]
#[tokio::test]
async fn e019_10_03_query_on_min_distance_point() -> Result<()> {
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
        .geo_q(geoquery::near_min(15_000, point(13.388_930, 52.534_473)?))
        .count(ResultsCount::Include)
        .send()
        .await;
    verify_that!(&received, answered(StatusCode::OK))?;
    let received = received?;
    expect_that!(results_count(&received), some(eq("1")));
    delete_entities(
        &broker,
        &[&namespace.urn("Building", 1)?, &namespace.urn("Building", 2)?, &namespace.urn("Building", 3)?],
    )
    .await;
    Ok(())
}

/// Pins: Check that one can query within a polygon
/// Clause: 5.7.2, 4.10
#[gtest]
#[tokio::test]
async fn e019_10_04_query_on_within_polygon() -> Result<()> {
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
        .geo_q(geoquery::within(berlin_polygon()?))
        .count(ResultsCount::Include)
        .send()
        .await;
    verify_that!(&received, answered(StatusCode::OK))?;
    let received = received?;
    expect_that!(results_count(&received), some(eq("2")));
    delete_entities(
        &broker,
        &[&namespace.urn("Building", 1)?, &namespace.urn("Building", 2)?, &namespace.urn("Building", 3)?],
    )
    .await;
    Ok(())
}

/// Pins: Check that one can query with a disjoint polygon
/// Clause: 5.7.2, 4.10
#[gtest]
#[tokio::test]
async fn e019_10_05_query_on_disjoint_polygon() -> Result<()> {
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
        .geo_q(geoquery::disjoint(berlin_polygon()?))
        .count(ResultsCount::Include)
        .send()
        .await;
    verify_that!(&received, answered(StatusCode::OK))?;
    let received = received?;
    expect_that!(results_count(&received), some(eq("1")));
    delete_entities(
        &broker,
        &[&namespace.urn("Building", 1)?, &namespace.urn("Building", 2)?, &namespace.urn("Building", 3)?],
    )
    .await;
    Ok(())
}

/// Pins: Check that one can query with a disjoint point
/// Clause: 5.7.2, 4.10
#[gtest]
#[tokio::test]
async fn e019_10_06_query_on_disjoint_point() -> Result<()> {
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
        .geo_q(geoquery::disjoint(point(13.3986, 52.5547)?))
        .count(ResultsCount::Include)
        .send()
        .await;
    verify_that!(&received, answered(StatusCode::OK))?;
    let received = received?;
    expect_that!(results_count(&received), some(eq("2")));
    delete_entities(
        &broker,
        &[&namespace.urn("Building", 1)?, &namespace.urn("Building", 2)?, &namespace.urn("Building", 3)?],
    )
    .await;
    Ok(())
}

/// Pins: Check that one can query with a contains point
/// Clause: 5.7.2, 4.10
#[gtest]
#[tokio::test]
async fn e019_10_07_query_on_contains_point() -> Result<()> {
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
        .geo_q(geoquery::contains(point(13.3986, 52.5547)?))
        .count(ResultsCount::Include)
        .send()
        .await;
    verify_that!(&received, answered(StatusCode::OK))?;
    let received = received?;
    expect_that!(results_count(&received), some(eq("1")));
    delete_entities(
        &broker,
        &[&namespace.urn("Building", 1)?, &namespace.urn("Building", 2)?, &namespace.urn("Building", 3)?],
    )
    .await;
    Ok(())
}

/// Pins: Check that one can query with an intersects point
/// Clause: 5.7.2, 4.10
#[gtest]
#[tokio::test]
async fn e019_10_08_query_on_intersects_point() -> Result<()> {
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
        .geo_q(geoquery::intersects(point(13.3986, 52.5547)?))
        .count(ResultsCount::Include)
        .send()
        .await;
    verify_that!(&received, answered(StatusCode::OK))?;
    let received = received?;
    expect_that!(results_count(&received), some(eq("1")));
    delete_entities(
        &broker,
        &[&namespace.urn("Building", 1)?, &namespace.urn("Building", 2)?, &namespace.urn("Building", 3)?],
    )
    .await;
    Ok(())
}
