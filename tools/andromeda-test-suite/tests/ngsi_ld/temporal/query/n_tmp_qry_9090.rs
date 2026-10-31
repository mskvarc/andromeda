//! A temporal near reaches a long edge of the history where it bows.

use andromeda_test_suite::{broker, identity_matcher::has_id, namespace::Namespace, nth::nth, outcome::Upserted};
use googletest::prelude::*;
use ngsi_ld::{
    http::headers::Accept,
    model::{
        GeoProperty,
        NgsiLdMediaType,
        evolution::TemporalEntity,
        geometry::{LineString, Position, Shape},
    },
    query::{geoquery, temporal_query},
};

/// Pins: a temporal query near;maxDistance==1000 around (0,63.43) answers the Entity whose location history holds the `LineString` (-30,60) to (30,60), and near;maxDistance==500 does not; the great circle through (-30,60) and (30,60) bows north to 63.43, so (0,63.43) is 551.64 m from the edge although it lies 380 km north of both vertices
/// Clause: 4.10, 5.7.4.4
#[gtest]
#[tokio::test]
async fn n_tmp_qry_9090_a_temporal_near_reaches_a_long_edge_of_the_history_where_it_bows() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let route_id = namespace.urn("Route", "temporal")?;
    let route_type = namespace.entity_type("Route")?;
    let route_evolution = TemporalEntity::builder()
        .id(route_id.clone())
        .entity_type(route_type.clone())
        .geo_property(
            "location",
            GeoProperty::builder(Shape::LineString(LineString::try_new(vec![
                Position::try_new(-30.0, 60.0)?,
                Position::try_new(30.0, 60.0)?,
            ])?))
            .observed_at("2020-08-01T12:03:00Z".parse()?),
        )
        .build()?;
    broker.temporal().create_or_update(&route_evolution).send().await?.created()?;
    let page = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .temporal()
        .query(temporal_query::after("2020-01-01T00:00:00Z".parse()?))
        .geo_q(geoquery::near_max(1000, Shape::Point(Position::try_new(0.0, 63.43)?)))
        .entity_type(&route_type)
        .send()
        .await?;
    expect_that!(page.items(), len(eq(1)));
    expect_that!(page.items(), nth(0, has_id(&route_id)));
    let page_2 = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .temporal()
        .query(temporal_query::after("2020-01-01T00:00:00Z".parse()?))
        .geo_q(geoquery::near_max(500, Shape::Point(Position::try_new(0.0, 63.43)?)))
        .entity_type(&route_type)
        .send()
        .await?;
    expect_that!(page_2.items(), len(eq(0)));
    Ok(())
}
