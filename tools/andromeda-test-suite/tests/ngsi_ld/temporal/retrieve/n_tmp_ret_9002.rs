//! A deleted geo property reads back in its temporal evolution.

use andromeda_test_suite::{
    attribute_matcher::{attribute_of, geo_property},
    broker,
    deleted_instance::deleted_instance,
    evolution_matcher::has_evolution,
    namespace::Namespace,
    nth::nth,
    outcome::Done,
    simplified_pairs::geo_pairs,
};
use googletest::prelude::*;
use ngsi_ld::{
    http::{headers::Accept, operation::RepresentationSpelling},
    model::{
        AttributeKind,
        AttributeName,
        Entity,
        GeoValue,
        NgsiLdMediaType,
        geometry::{Position, Shape},
    },
    query::temporal_query::TemporalProperty,
};

/// Pins: After Delete Attribute of a `GeoProperty` written without observedAt, Retrieve Temporal Evolution of the Entity
/// on deletedAt answers 200 with a `GeoProperty` instance whose value is the NGSI-LD Null, and with format temporalValues
/// a pair whose value is the NGSI-LD Null; on modifiedAt it holds the instance holding the Point
/// Clause: 4.5.0, 4.5.7, 4.5.8, 4.8, 5.7.3.4, Table 5.2.21-1
#[gtest]
#[tokio::test]
async fn n_tmp_ret_9002_a_deleted_geo_property_reads_back_in_its_temporal_evolution() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let building_id = namespace.urn("Building", 1)?;
    let building_type = namespace.entity_type("Building")?;
    let building = Entity::builder()
        .id(building_id.clone())
        .entity_type(building_type)
        .geo_property("location", Shape::Point(Position::try_new(13.4, 52.5)?))
        .build()?;
    broker.entities().create(&building).send().await?.done()?;
    broker
        .entities()
        .delete_attribute(&building_id, &"location".parse::<AttributeName>()?)
        .send()
        .await?
        .done()?;
    let client = broker.with_accept(Accept::new(NgsiLdMediaType::Json));
    let deleted = client
        .temporal()
        .retrieve(&building_id)
        .time_property(TemporalProperty::DeletedAt)
        .send()
        .await?;
    expect_that!(
        deleted.body(),
        has_evolution(
            "location",
            all!(len(eq(1)), nth(0, all!(attribute_of(AttributeKind::GeoProperty), deleted_instance())))
        )
    );
    let modified = client
        .temporal()
        .retrieve(&building_id)
        .time_property(TemporalProperty::ModifiedAt)
        .send()
        .await?;
    expect_that!(
        modified.body(),
        has_evolution("location", contains(geo_property().geometry(Shape::Point(Position::try_new(13.4, 52.5)?))))
    );
    let values = client
        .temporal()
        .retrieve_values(&building_id)
        .time_property(TemporalProperty::DeletedAt)
        .spelling(RepresentationSpelling::default().in_options())
        .send()
        .await?;
    let pairs = geo_pairs(values.body(), "location").or_fail()?;
    expect_that!(pairs.iter().any(|pair| pair.value == GeoValue::Null), eq(true));
    Ok(())
}
