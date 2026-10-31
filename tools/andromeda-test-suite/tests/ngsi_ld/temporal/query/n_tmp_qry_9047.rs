//! A geoquery reads every dataset.

use andromeda_test_suite::{
    attribute_matcher::any_attribute,
    broker,
    evolution_matcher::{has_evolution, lacks_evolution},
    identity_matcher::has_id,
    namespace::Namespace,
    nth::nth,
    outcome::Upserted,
};
use googletest::prelude::*;
use ngsi_ld::{
    http::headers::Accept,
    model::{
        DatasetId,
        GeoProperty,
        NgsiLdMediaType,
        Property,
        evolution::TemporalEntity,
        geometry::{Position, Shape},
    },
    query::{geoquery, temporal_query},
};

/// Pins: Query Temporal Evolution of Entities with `datasetId=@none` and a geoquery near the location of a named
/// dataset answers the Entity with speed alone holding 10, and a geoquery near no location answers nothing
/// Clause: 5.7.4.4
#[gtest]
#[tokio::test]
async fn n_tmp_qry_9047_a_geoquery_reads_every_dataset() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let tqds_geo_id = namespace.urn("TqdsGeo", 1)?;
    let tqds_geo_type = namespace.entity_type("TqdsGeo")?;
    let tqds_geo_evolution = TemporalEntity::builder()
        .id(tqds_geo_id.clone())
        .entity_type(tqds_geo_type.clone())
        .property("speed", Property::builder(10).observed_at("2020-08-01T12:00:00Z".parse()?))
        .geo_property(
            "location",
            GeoProperty::builder(Shape::Point(Position::try_new(-8.5, 41.2)?))
                .observed_at("2020-08-01T12:00:00Z".parse()?)
                .dataset_id(namespace.dataset_id("Dataset", "gps")?),
        )
        .build()?;
    broker.temporal().create_or_update(&tqds_geo_evolution).send().await?.created()?;
    let json = broker.with_accept(Accept::new(NgsiLdMediaType::Json));
    let near = json
        .temporal()
        .query(temporal_query::before("2030-01-01T00:00:00Z".parse()?))
        .entity_type(&tqds_geo_type)
        .geo_q(geoquery::near_max(2000, Shape::Point(Position::try_new(-8.503, 41.202)?)))
        .dataset_ids(vec![DatasetId::DefaultInstance])
        .send()
        .await?;
    expect_that!(near.items(), len(eq(1)));
    expect_that!(near.items(), nth(0, has_id(&tqds_geo_id)));
    expect_that!(near.items().first(), some(has_evolution("speed", nth(0, any_attribute().value(10)))));
    expect_that!(near.items().first(), some(lacks_evolution("location")));
    expect_that!(near.items().first().map(|entity| entity.attributes.len()), some(eq(1)));
    let far = json
        .temporal()
        .query(temporal_query::before("2030-01-01T00:00:00Z".parse()?))
        .entity_type(&tqds_geo_type)
        .geo_q(geoquery::near_max(2000, Shape::Point(Position::try_new(10.0, 50.0)?)))
        .dataset_ids(vec![DatasetId::DefaultInstance])
        .send()
        .await?;
    expect_that!(far.items(), is_empty());
    Ok(())
}
