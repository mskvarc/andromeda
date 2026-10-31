//! Geojson query with body json moves the context to a link header.

use andromeda_test_suite::{broker, core_context::core_context_link, namespace::Namespace, outcome::Done, wire::header_values};
use googletest::prelude::*;
use http::header::LINK;
use ngsi_ld::{
    http::headers::{Accept, GeoJsonBody, Link, Prefer},
    model::{
        Entity,
        NgsiLdMediaType,
        geometry::{Position, Shape},
    },
};

/// Pins: Query Entities as geo+json with Prefer body=json leaves the @context out of the `FeatureCollection` and of
/// its Features, and answers it in a Link header
/// Clause: 6.3.6, 5.7.2.4, 4.5.16.3
#[gtest]
#[tokio::test]
async fn n_cmn_prf_0004_geojson_query_with_body_json_moves_the_context_to_a_link_header() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let building_id = namespace.urn("Building", 1)?;
    let building_type = namespace.entity_type("Building")?;
    let building = Entity::builder()
        .id(building_id.clone())
        .entity_type(building_type.clone())
        .geo_property("location", Shape::Point(Position::try_new(2.3522, 48.8566)?))
        .build()?;
    broker.entities().create(&building).send().await?.done()?;
    let headed = broker
        .with_heads()
        .with_accept(Accept::new(NgsiLdMediaType::GeoJson))
        .with_prefer(Prefer::builder().geo_json_body(GeoJsonBody::Json).build()?)
        .entities()
        .query_geo_json()
        .entity_type(&building_type)
        .ids(vec![building_id.clone()])
        .send()
        .await?;
    expect_that!(header_values(headed.head.headers(), &LINK), len(eq(1)));
    expect_that!(headed.head.typed::<Link>().valid(), some(eq(&core_context_link()?)));
    let page = &headed.outcome;
    expect_that!(page.context(), none());
    expect_that!(page.items(), len(eq(1)));
    let feature = page.items().first().or_fail()?;
    expect_that!(feature.entity.id, eq(&building_id));
    expect_that!(feature.context.as_ref(), none());
    Ok(())
}
