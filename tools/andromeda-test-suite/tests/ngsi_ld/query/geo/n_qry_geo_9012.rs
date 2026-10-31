//! Near over a named geo property matches on any dataset instance and orders by it.

use andromeda_test_suite::{
    broker,
    identity_matcher::{has_id, ids_in_order},
    namespace::Namespace,
    outcome::Done,
    problem::answered,
    raw_call::target,
    unchecked_call::exchange,
    unchecked_page,
};
use googletest::prelude::*;
use http::{Method, StatusCode};
use ngsi_ld::{
    http::headers::Accept,
    model::{
        AttributeName,
        Entity,
        GeoProperty,
        NgsiLdMediaType,
        geometry::{Position, Shape},
    },
    query::geoquery,
};

/// Pins: With entrance held at 13/52 and, under a `datasetId`, at 20/60 by one Building and at 13.001/52 by
/// another, near;maxDistance==1000 from 20/60 and near;minDistance==100000 from 13/52 on geoproperty=entrance each
/// select the first Building alone, and orderBy=entrance;dist-asc from 13.001/52 answers the second Building first
/// Clause: 4.10, 5.7.2.4, 4.5.5
#[gtest]
#[tokio::test]
async fn n_qry_geo_9012_near_over_a_named_geo_property_matches_on_any_dataset_instance_and_orders_by_it() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let building_type = namespace.entity_type("Building")?;
    let two_id = namespace.urn("Building", "two-instances")?;
    let near_id = namespace.urn("Building", "near")?;
    let entrance = "entrance".parse::<AttributeName>()?;
    let entities = [
        Entity::builder()
            .id(two_id.clone())
            .entity_type(building_type.clone())
            .geo_property("entrance", Shape::Point(Position::try_new(13.0, 52.0)?))
            .geo_property(
                "entrance",
                GeoProperty::builder(Shape::Point(Position::try_new(20.0, 60.0)?)).dataset_id(namespace.dataset_id("Dataset", "remote")?),
            )
            .geo_property("dock", Shape::Point(Position::try_new(1.0, 1.0)?))
            .build()?,
        Entity::builder()
            .id(near_id.clone())
            .entity_type(building_type.clone())
            .geo_property("entrance", Shape::Point(Position::try_new(13.001, 52.0)?))
            .build()?,
    ];
    broker.entities().batch_create(&entities).send().await?.done()?;
    let page = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .query()
        .entity_type(&building_type)
        .geo_q(geoquery::near_max(1000, Shape::Point(Position::try_new(20.0, 60.0)?)).geoproperty(entrance.clone()))
        .send()
        .await?;
    expect_that!(page.items(), len(eq(1)));
    expect_that!(page.items(), contains(has_id(&two_id)));
    let page_2 = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .query()
        .entity_type(&building_type)
        .geo_q(geoquery::near_min(100_000, Shape::Point(Position::try_new(13.0, 52.0)?)).geoproperty(entrance))
        .send()
        .await?;
    expect_that!(page_2.items(), len(eq(1)));
    expect_that!(page_2.items(), contains(has_id(&two_id)));
    // The client writes the ordering origin as GeoJSON, where this read gives it as a bare position.
    let ordered = target(
        "entities",
        &[
            ("type", building_type.as_str()),
            ("orderBy", "entrance;dist-asc"),
            ("orderFrom", "[13.001,52.0]"),
        ],
    );
    let request = exchange(&broker, Method::GET, &ordered, &[("Accept", "application/json")], None)?;
    let answer = broker.send_unchecked(request).await;
    expect_that!(&answer, answered(StatusCode::OK));
    expect_that!(unchecked_page::entities(&answer?)?.as_slice(), ids_in_order(&[near_id, two_id]));
    Ok(())
}
