//! A self intersecting polygon history reads back as written and is judged repaired.

use andromeda_test_suite::{
    attribute_matcher::any_attribute,
    broker,
    evolution_matcher::{has_evolution, instance_id_at},
    identity_matcher::has_id,
    namespace::Namespace,
    nth::nth,
    outcome::Upserted,
};
use googletest::prelude::*;
use ngsi_ld::{
    http::headers::Accept,
    model::{
        Attribute,
        AttributeName,
        GeoProperty,
        NgsiLdMediaType,
        evolution::TemporalEntity,
        geometry::{LinearRing, Polygon, Position, Shape},
    },
    query::{geoquery, temporal_query},
};

/// Pins: A temporal Building whose footprint instance holds the self-intersecting Polygon
/// `[[[0,0]`,`[2,2]`,`[2,0]`,`[0,2]`,`[0,0]`]] reads that instance back as that Polygon, is selected by a temporal
/// intersects at `[0.3,1.0]` on footprint and not at `[1.0,0.3]`; once the instance is modified to the same Polygon
/// shifted by 10 in longitude it reads back as the shifted Polygon, selected at `[10.3,1.0]` and not at
/// `[11.0,0.3]`
/// Clause: 4.7.1, 4.7.2, 4.10, 5.6.14
/// Recorded: clause 4.7.1 makes the `GeoProperty` value the value read back, and clause 4.7.2 accepts every
/// geometry meeting IETF RFC 7946, whose section 3.1.6 sets no simplicity rule on a ring; Andromeda keeps a
/// self-intersecting Polygon as written and judges a geoquery against its `ST_MakeValid` repair, whose lobes are
/// the triangles (0,0)-(1,1)-(0,2) and (1,1)-(2,2)-(2,0)
#[gtest]
#[tokio::test]
async fn n_tmp_qry_9077_a_self_intersecting_polygon_history_reads_back_as_written_and_is_judged_repaired() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let building_bow_tie_history_id = namespace.urn("Building", "bow-tie-history")?;
    let building_type = namespace.entity_type("Building")?;
    let building_bow_tie_history_evolution = TemporalEntity::builder()
        .id(building_bow_tie_history_id.clone())
        .entity_type(building_type.clone())
        .geo_property(
            "footprint",
            GeoProperty::builder(Shape::Polygon(Polygon::try_new(vec![LinearRing::try_new(vec![
                Position::try_new(0.0, 0.0)?,
                Position::try_new(2.0, 2.0)?,
                Position::try_new(2.0, 0.0)?,
                Position::try_new(0.0, 2.0)?,
                Position::try_new(0.0, 0.0)?,
            ])?])?))
            .observed_at("2020-10-01T12:00:00Z".parse()?),
        )
        .build()?;
    broker
        .temporal()
        .create_or_update(&building_bow_tie_history_evolution)
        .send()
        .await?
        .created()?;
    let retrieved = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .temporal()
        .retrieve(&building_bow_tie_history_id)
        .send()
        .await?;
    let evolution = retrieved.body();
    expect_that!(
        evolution,
        has_evolution(
            "footprint",
            all!(
                len(eq(1)),
                nth(
                    0,
                    any_attribute()
                        .value_member(["type"], "Polygon")
                        .length_at(["coordinates"], 1)
                        .length_at(["coordinates", "0"], 5)
                        .length_at(["coordinates", "0", "0"], 2)
                        .value_member(["coordinates", "0", "0", "0"], 0)
                        .value_member(["coordinates", "0", "0", "1"], 0)
                        .length_at(["coordinates", "0", "1"], 2)
                        .value_member(["coordinates", "0", "1", "0"], 2)
                        .value_member(["coordinates", "0", "1", "1"], 2)
                        .length_at(["coordinates", "0", "2"], 2)
                        .value_member(["coordinates", "0", "2", "0"], 2)
                        .value_member(["coordinates", "0", "2", "1"], 0)
                        .length_at(["coordinates", "0", "3"], 2)
                        .value_member(["coordinates", "0", "3", "0"], 0)
                        .value_member(["coordinates", "0", "3", "1"], 2)
                        .length_at(["coordinates", "0", "4"], 2)
                        .value_member(["coordinates", "0", "4", "0"], 0)
                        .value_member(["coordinates", "0", "4", "1"], 0)
                )
            )
        )
    );
    let instance_id = instance_id_at(evolution, &"footprint".parse()?, 0)?;
    let page = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .temporal()
        .query(temporal_query::after("2020-10-01T00:00:00Z".parse()?))
        .geo_q(geoquery::intersects(Shape::Point(Position::try_new(0.3, 1.0)?)).geoproperty("footprint"))
        .entity_type(&building_type)
        .send()
        .await?;
    expect_that!(page.items(), len(eq(1)));
    expect_that!(page.items(), nth(0, has_id(&building_bow_tie_history_id)));
    let page_2 = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .temporal()
        .query(temporal_query::after("2020-10-01T00:00:00Z".parse()?))
        .geo_q(geoquery::intersects(Shape::Point(Position::try_new(1.0, 0.3)?)).geoproperty("footprint"))
        .entity_type(&building_type)
        .send()
        .await?;
    expect_that!(page_2.items(), len(eq(0)));
    let instance = Attribute::from(
        GeoProperty::builder(Shape::Polygon(Polygon::try_new(vec![LinearRing::try_new(vec![
            Position::try_new(10.0, 0.0)?,
            Position::try_new(12.0, 2.0)?,
            Position::try_new(12.0, 0.0)?,
            Position::try_new(10.0, 2.0)?,
            Position::try_new(10.0, 0.0)?,
        ])?])?))
        .observed_at("2020-10-01T12:00:00Z".parse()?)
        .build()?,
    );
    broker
        .temporal()
        .modify_instance(&building_bow_tie_history_id, &"footprint".parse::<AttributeName>()?, &instance_id, &instance)
        .send()
        .await?;
    let retrieved_2 = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .temporal()
        .retrieve(&building_bow_tie_history_id)
        .send()
        .await?;
    let evolution_2 = retrieved_2.body();
    expect_that!(
        evolution_2,
        has_evolution(
            "footprint",
            all!(
                len(eq(1)),
                nth(
                    0,
                    any_attribute()
                        .value_member(["type"], "Polygon")
                        .length_at(["coordinates"], 1)
                        .length_at(["coordinates", "0"], 5)
                        .length_at(["coordinates", "0", "0"], 2)
                        .value_member(["coordinates", "0", "0", "0"], 10)
                        .value_member(["coordinates", "0", "0", "1"], 0)
                        .length_at(["coordinates", "0", "1"], 2)
                        .value_member(["coordinates", "0", "1", "0"], 12)
                        .value_member(["coordinates", "0", "1", "1"], 2)
                        .length_at(["coordinates", "0", "2"], 2)
                        .value_member(["coordinates", "0", "2", "0"], 12)
                        .value_member(["coordinates", "0", "2", "1"], 0)
                        .length_at(["coordinates", "0", "3"], 2)
                        .value_member(["coordinates", "0", "3", "0"], 10)
                        .value_member(["coordinates", "0", "3", "1"], 2)
                        .length_at(["coordinates", "0", "4"], 2)
                        .value_member(["coordinates", "0", "4", "0"], 10)
                        .value_member(["coordinates", "0", "4", "1"], 0)
                )
            )
        )
    );
    let page_3 = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .temporal()
        .query(temporal_query::after("2020-10-01T00:00:00Z".parse()?))
        .geo_q(geoquery::intersects(Shape::Point(Position::try_new(10.3, 1.0)?)).geoproperty("footprint"))
        .entity_type(&building_type)
        .send()
        .await?;
    expect_that!(page_3.items(), len(eq(1)));
    expect_that!(page_3.items(), nth(0, has_id(&building_bow_tie_history_id)));
    let page_4 = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .temporal()
        .query(temporal_query::after("2020-10-01T00:00:00Z".parse()?))
        .geo_q(geoquery::intersects(Shape::Point(Position::try_new(11.0, 0.3)?)).geoproperty("footprint"))
        .entity_type(&building_type)
        .send()
        .await?;
    expect_that!(page_4.items(), len(eq(0)));
    Ok(())
}
