//! A geometry at the budget is created read queried and deleted.

use andromeda_test_suite::{
    attribute_matcher::any_attribute,
    broker,
    config::{Budget, SuiteConfig},
    entity_matcher::has_attribute,
    generated_payload::ring_polygon,
    identity_matcher::has_id,
    namespace::Namespace,
    nth::nth,
    outcome::Done,
    problem::problem,
};
use googletest::prelude::*;
use http::StatusCode;
use ngsi_ld::{
    http::headers::Accept,
    model::{
        Entity,
        ErrorType,
        NgsiLdMediaType,
        geometry::{Position, Shape},
    },
    query::geoquery,
};

/// Pins: Create Entity whose location Polygon holds exactly the budget of positions answers 201, reads back with
/// every position count and the sampled positions as sent, is found by an intersects query at the centre of the
/// ring, and is deleted, after which the type still answers queries
/// Clause: 6.3.2, 5.6.1, 5.7.1, 5.7.2, 5.6.6
/// Recorded: ETSI GS CIM 009 sets no size for an entity or a geometry; Andromeda budgets the JSON values of a
/// request (4,000,000), of an entity outside its geometries and JSON literals (1,000,000) and the positions of one
/// geometry (100,000), the stack running the defaults
#[gtest]
#[tokio::test]
async fn a_lim_pay_9006_a_geometry_at_the_budget_is_created_read_queried_and_deleted() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let budget = SuiteConfig::get()?.budget(Budget::Position)?;
    let probe_type = namespace.entity_type("Probe")?;
    let entity_id = namespace.urn("Probe", "at-budget")?;
    let polygon = ring_polygon(budget)?;
    let entity = Entity::builder()
        .id(entity_id.clone())
        .entity_type(probe_type.clone())
        .geo_property("location", polygon.clone())
        .build()?;
    broker.entities().create(&entity).send().await?.done()?;
    // Every position reads back as it was written, the first and the last included.
    let retrieved = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .retrieve(&entity_id)
        .send()
        .await?;
    expect_that!(retrieved.body(), has_attribute("location", any_attribute().geometry(polygon)));
    let found = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .query()
        .entity_type(&probe_type)
        .geo_q(geoquery::intersects(Shape::Point(Position::try_new(14.55, 46.05)?)))
        .send()
        .await?;
    expect_that!(found.items(), len(eq(1)));
    expect_that!(found.items(), nth(0, has_id(&entity_id)));
    broker.entities().delete(&entity_id).send().await?;
    let absent = broker
        .with_responses()
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .retrieve(&entity_id)
        .send()
        .await;
    expect_that!(&absent, problem(StatusCode::NOT_FOUND, ErrorType::ResourceNotFound));
    let remaining = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .query()
        .entity_type(&probe_type)
        .send()
        .await?;
    expect_that!(remaining.items(), len(eq(0)));
    Ok(())
}
