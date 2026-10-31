//! Query entities as geojson with body json answers one link line with the context link first.

use andromeda_test_suite::{
    broker,
    link_matcher::{link_value, relations_in_order},
    namespace::Namespace,
    outcome::Done,
    wire::header_values,
};
use googletest::prelude::*;
use http::header::LINK;
use ngsi_ld::{
    http::headers::{Accept, GeoJsonBody, Link, Prefer, RelationType},
    model::{
        AttributeName,
        Entity,
        NgsiLdMediaType,
        geometry::{Position, Shape},
    },
};

/// Pins: Query Entities as application/geo+json with Prefer body=json over several pages carries one Link field
/// line holding the @context link first, then prev and next
/// Clause: 6.3.6, 6.3.10, 5.5.9, 5.7.2.4
/// Recorded: Andromeda answers the Link field on one line; RFC 9110 clause 5.3 makes one combined line and several
/// lines equivalent, and the test pins the single line
#[gtest]
#[tokio::test]
async fn o_qry_pag_2701_query_entities_as_geojson_with_body_json_answers_one_link_line_with_the_context_link_first() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let building_id = namespace.urn("Building", 1)?;
    let building_type = namespace.entity_type("Building")?;
    let building_2_id = namespace.urn("Building", 2)?;
    let building_3_id = namespace.urn("Building", 3)?;
    let entities = [
        Entity::builder()
            .id(building_id)
            .entity_type(building_type.clone())
            .geo_property("location", Shape::Point(Position::try_new(13.4, 52.5)?))
            .build()?,
        Entity::builder()
            .id(building_2_id)
            .entity_type(building_type.clone())
            .geo_property("location", Shape::Point(Position::try_new(13.5, 52.5)?))
            .build()?,
        Entity::builder()
            .id(building_3_id)
            .entity_type(building_type.clone())
            .geo_property("location", Shape::Point(Position::try_new(13.6, 52.5)?))
            .build()?,
    ];
    broker.entities().batch_create(&entities).send().await?.done()?;
    let headed = broker
        .with_heads()
        .with_accept(Accept::new(NgsiLdMediaType::GeoJson))
        .with_prefer(Prefer::builder().geo_json_body(GeoJsonBody::Json).build()?)
        .entities()
        .query_geo_json()
        .geometry_property(&"location".parse::<AttributeName>()?)
        .entity_type(&building_type)
        .limit(1)
        .offset(1)
        .send()
        .await?;
    let page = headed.outcome;
    expect_that!(header_values(headed.head.headers(), &LINK), len(eq(1)));
    expect_that!(
        headed.head.typed::<Link>().valid().and_then(|link| link.values().first()),
        some(link_value(RelationType::JsonLdContext).with_media_type("application/ld+json"))
    );
    expect_that!(
        headed.head.typed::<Link>().valid(),
        some(relations_in_order([RelationType::JsonLdContext, RelationType::Prev, RelationType::Next]))
    );
    expect_that!(page.items(), len(eq(1)));
    Ok(())
}
