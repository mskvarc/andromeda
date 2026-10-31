//! A distance ordering measures the nearest dataset instance.

use andromeda_test_suite::{broker, identity_matcher::has_id, namespace::Namespace, nth::nth, outcome::Done};
use googletest::prelude::*;
use ngsi_ld::{
    http::headers::Accept,
    model::{
        Entity,
        GeoProperty,
        NgsiLdMediaType,
        geometry::{Position, Shape},
    },
    query::{
        Locality,
        ordering::{OrderBy, builders::EntityOrderingBuilder},
    },
};

/// Pins: sorting by distance on a `GeoProperty` with several `datasetId` instances ranks the entity by its nearest
/// instance, so the entity whose second instance is 700 m away precedes the one whose only instance is 6 km away
/// Clause: 4.23.3, 4.10, 4.5.5, 5.7.2.4
/// Recorded: clause 4.23.1 says sort by distance is limited to `GeoProperties` and uses the Haversine formula, and
/// clause 5.7.2.4 lets one `datasetId` instance meeting a geospatial restriction suffice, but nothing says which
/// instance of a multi-instance `GeoProperty` a distance ordering measures; Andromeda takes the nearest, in line
/// with the filter
#[gtest]
#[tokio::test]
async fn o_qry_ord_0014_a_distance_ordering_measures_the_nearest_dataset_instance() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let building_id = namespace.urn("Building", 1)?;
    let building_type = namespace.entity_type("Building")?;
    let building_2_id = namespace.urn("Building", 2)?;
    let entities = [
        Entity::builder()
            .id(building_id.clone())
            .entity_type(building_type.clone())
            .geo_property("location", Shape::Point(Position::try_new(13.4, 52.52)?))
            .geo_property("observationPoint", Shape::Point(Position::try_new(13.5, 52.52)?))
            .build()?,
        Entity::builder()
            .id(building_2_id.clone())
            .entity_type(building_type.clone())
            .geo_property("location", Shape::Point(Position::try_new(13.4, 52.52)?))
            .geo_property("observationPoint", Shape::Point(Position::try_new(13.9, 52.52)?))
            .geo_property(
                "observationPoint",
                GeoProperty::builder(Shape::Point(Position::try_new(13.41, 52.52)?)).dataset_id(namespace.dataset_id("Dataset", "sensor-a")?),
            )
            .build()?,
    ];
    broker.entities().batch_create(&entities).send().await?.done()?;
    let page = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .query()
        .entity_type(&building_type)
        .locality(Locality::Local)
        .ordering(EntityOrderingBuilder::from("observationPoint;dist-asc".parse::<OrderBy>()?).origin(Shape::Point(Position::try_new(13.4, 52.52)?)))
        .send()
        .await?;
    expect_that!(page.items(), len(eq(2)));
    expect_that!(page.items(), nth(0, has_id(&building_2_id)));
    expect_that!(page.items(), nth(1, has_id(&building_id)));
    Ok(())
}
