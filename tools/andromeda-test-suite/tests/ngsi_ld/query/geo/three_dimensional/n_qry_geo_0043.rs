//! A three ordinate reference position is accepted.

use andromeda_test_suite::{broker, namespace::Namespace};
use googletest::prelude::*;
use ngsi_ld::{
    http::headers::Accept,
    model::{
        NgsiLdMediaType,
        geometry::{Position, Shape},
    },
    query::geoquery,
};

/// Pins: a geo-query reference position with a third ordinate is valid `GeoJSON` and is answered with 200
/// Clause: 4.10, 4.7.1, 5.7.2
#[gtest]
#[tokio::test]
async fn n_qry_geo_0043_a_three_ordinate_reference_position_is_accepted() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    // No Building of this file exists: the answer is 200 and empty.
    let page = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .query()
        .geo_q(geoquery::near_max(100, Shape::Point(Position::try_with_altitude(13.4, 52.52, 300.0)?)))
        .entity_type(&namespace.entity_type("Building")?)
        .send()
        .await?;
    expect_that!(page.items(), len(eq(0)));
    Ok(())
}
