//! A self intersecting polygon reads back as written and is judged repaired.

use andromeda_test_suite::{
    attribute_matcher::geo_property,
    broker,
    entity_matcher::has_attribute,
    geometry_round_trip::{polygon, rectangle},
    identity_matcher::has_id,
    namespace::Namespace,
    outcome::Done,
};
use googletest::prelude::*;
use ngsi_ld::{
    http::headers::Accept,
    model::{
        AttributeName,
        Entity,
        GeoProperty,
        NgsiLdMediaType,
        SimplifiedValue,
        geometry::{Geometry, Position, Shape},
    },
    query::{geoquery, type_selection},
};

/// Pins: A Building whose location, footprint and footprint.outline each hold the self-intersecting Polygon
/// `[[[0,0]`,`[2,2]`,`[2,0]`,`[0,2]`,`[0,0]`]] reads each back as that Polygon in the normalized and the simplified
/// representation, and is selected by intersects at `[0.3,1.0]` on location and on footprint, by within a box
/// around it and by near;maxDistance==1000 at `[0.3,1.0]` on footprint, and not by intersects at `[1.0,0.3]`, which
/// lies between the two lobes
/// Clause: 4.7.1, 4.7.2, 4.10
/// Recorded: clause 4.7.1 makes the `GeoProperty` value the value read back, and clause 4.7.2 accepts every
/// geometry meeting IETF RFC 7946, whose section 3.1.6 sets no simplicity rule on a ring; Andromeda keeps a
/// self-intersecting Polygon as written and judges a geoquery against its `ST_MakeValid` repair, whose lobes are
/// the triangles (0,0)-(1,1)-(0,2) and (1,1)-(2,2)-(2,0)
#[gtest]
#[tokio::test]
async fn n_qry_geo_9013_a_self_intersecting_polygon_reads_back_as_written_and_is_judged_repaired() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let building_id = namespace.urn("Building", "bow-tie")?;
    let building_type = namespace.entity_type("Building")?;
    let footprint = "footprint".parse::<AttributeName>()?;
    let bow_tie = || -> Result<Shape> { Ok(Shape::Polygon(polygon(&[&[(0.0, 0.0), (2.0, 2.0), (2.0, 0.0), (0.0, 2.0), (0.0, 0.0)]])?)) };
    let building = Entity::builder()
        .id(building_id.clone())
        .entity_type(building_type.clone())
        .geo_property("location", bow_tie()?)
        .geo_property("footprint", GeoProperty::builder(bow_tie()?).geo_property("outline", bow_tie()?))
        .build()?;
    broker.entities().create(&building).send().await?.done()?;
    let json = broker.with_accept(Accept::new(NgsiLdMediaType::Json));
    let retrieved = json.entities().retrieve(&building_id).send().await?;
    let entity = retrieved.body();
    expect_that!(entity, has_attribute("location", geo_property().geometry(bow_tie()?)));
    expect_that!(
        entity,
        has_attribute(
            "footprint",
            geo_property()
                .geometry(bow_tie()?)
                .sub_attribute("outline", geo_property().geometry(bow_tie()?))
        )
    );
    let simplified = json.entities().retrieve_simplified(&building_id).send().await?;
    expect_that!(
        simplified.body().attributes.get(&"location".parse::<AttributeName>()?),
        some(eq(&SimplifiedValue::Geometry(Geometry::from(bow_tie()?))))
    );
    let intersecting = |point: Position, geoproperty: Option<&AttributeName>| {
        let query = geoquery::intersects(Shape::Point(point));
        match geoproperty {
            Some(name) => query.geoproperty(name.clone()),
            None => query,
        }
    };
    let selected_type = type_selection::entity_type(&building_type).build()?;
    let page = json
        .entities()
        .query()
        .entity_type(selected_type.clone())
        .geo_q(intersecting(Position::try_new(0.3, 1.0)?, None))
        .send()
        .await?;
    expect_that!(page.items(), len(eq(1)));
    expect_that!(page.items(), contains(has_id(&building_id)));
    let page_2 = json
        .entities()
        .query()
        .entity_type(selected_type.clone())
        .geo_q(intersecting(Position::try_new(0.3, 1.0)?, Some(&footprint)))
        .send()
        .await?;
    expect_that!(page_2.items(), len(eq(1)));
    expect_that!(page_2.items(), contains(has_id(&building_id)));
    let page_3 = json
        .entities()
        .query()
        .entity_type(selected_type.clone())
        .geo_q(intersecting(Position::try_new(1.0, 0.3)?, Some(&footprint)))
        .send()
        .await?;
    expect_that!(page_3.items(), is_empty());
    let page_4 = json
        .entities()
        .query()
        .entity_type(selected_type.clone())
        .geo_q(geoquery::within(rectangle(-1.0, -1.0, 3.0, 3.0)?).geoproperty(footprint.clone()))
        .send()
        .await?;
    expect_that!(page_4.items(), len(eq(1)));
    expect_that!(page_4.items(), contains(has_id(&building_id)));
    let page_5 = json
        .entities()
        .query()
        .entity_type(selected_type)
        .geo_q(geoquery::near_max(1000, Shape::Point(Position::try_new(0.3, 1.0)?)).geoproperty(footprint))
        .send()
        .await?;
    expect_that!(page_5.items(), len(eq(1)));
    expect_that!(page_5.items(), contains(has_id(&building_id)));
    Ok(())
}
