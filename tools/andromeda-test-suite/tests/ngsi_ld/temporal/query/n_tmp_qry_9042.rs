//! Q and a geoquery combine over the window.

use andromeda_test_suite::{broker, identity_matcher::has_id, namespace::Namespace, outcome::Upserted};
use googletest::prelude::*;
use ngsi_ld::{
    http::headers::Accept,
    model::{
        GeoProperty,
        NgsiLdMediaType,
        Property,
        evolution::TemporalEntity,
        geometry::{LinearRing, Polygon, Position, Shape},
    },
    query::{geoquery, q, temporal_query},
};

/// Pins: Query Temporal Evolution of Entities with `q=maxSustainedWind>100` and a geoquery within a Gulf polygon
/// selects a storm whose 150 and in-polygon instances lie in 2005, and nothing over 2006
/// Clause: 5.7.4.4, 4.10
#[gtest]
#[tokio::test]
async fn n_tmp_qry_9042_q_and_a_geoquery_combine_over_the_window() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let tq_storm_id = namespace.urn("TqStorm", 1)?;
    let tq_storm_type = namespace.entity_type("TqStorm")?;
    let tq_storm_evolution = TemporalEntity::builder()
        .id(tq_storm_id.clone())
        .entity_type(tq_storm_type.clone())
        .property("maxSustainedWind", Property::builder(150).observed_at("2005-08-28T00:00:00Z".parse()?))
        .property("maxSustainedWind", Property::builder(30).observed_at("2005-08-31T00:00:00Z".parse()?))
        .geo_property(
            "location",
            GeoProperty::builder(Shape::Point(Position::try_new(-89.0, 26.0)?)).observed_at("2005-08-28T00:00:00Z".parse()?),
        )
        .geo_property(
            "location",
            GeoProperty::builder(Shape::Point(Position::try_new(-80.0, 40.0)?)).observed_at("2005-08-31T00:00:00Z".parse()?),
        )
        .build()?;
    broker.temporal().create_or_update(&tq_storm_evolution).send().await?.created()?;
    let page = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .temporal()
        .query(temporal_query::between("2005-01-01T00:00:00Z".parse()?, "2006-01-01T00:00:00Z".parse()?))
        .geo_q(geoquery::within(Shape::Polygon(Polygon::try_new(vec![LinearRing::try_new(vec![
            Position::try_new(-98.0, 18.0)?,
            Position::try_new(-80.0, 18.0)?,
            Position::try_new(-80.0, 31.0)?,
            Position::try_new(-98.0, 31.0)?,
            Position::try_new(-98.0, 18.0)?,
        ])?])?)))
        .entity_type(&tq_storm_type)
        .q(q::attr("maxSustainedWind").gt(100))
        .send()
        .await?;
    expect_that!(page.items(), len(eq(1)));
    expect_that!(page.items(), contains(has_id(&tq_storm_id)));
    let page_2 = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .temporal()
        .query(temporal_query::between("2006-01-01T00:00:00Z".parse()?, "2007-01-01T00:00:00Z".parse()?))
        .geo_q(geoquery::within(Shape::Polygon(Polygon::try_new(vec![LinearRing::try_new(vec![
            Position::try_new(-98.0, 18.0)?,
            Position::try_new(-80.0, 18.0)?,
            Position::try_new(-80.0, 31.0)?,
            Position::try_new(-98.0, 31.0)?,
            Position::try_new(-98.0, 18.0)?,
        ])?])?)))
        .entity_type(&tq_storm_type)
        .q(q::attr("maxSustainedWind").gt(100))
        .send()
        .await?;
    expect_that!(page_2.items(), len(eq(0)));
    Ok(())
}
