//! A lone instance is not selected by another dataset id.

use andromeda_test_suite::{broker, geometry_matcher::geometry, namespace::Namespace, problem::answered};
use googletest::prelude::*;
use http::StatusCode;
use ngsi_ld::{
    http::{UncheckedDocument, headers::Accept, operation::CreateEntity},
    model::{Entity, Lossless, NgsiLdMediaType, RawJson},
};

/// Pins: Retrieve Entity as `GeoJSON` of an Entity whose location is one instance of one dataset answers a Feature
/// whose geometry is null under the `datasetId` of another dataset, and the instance under its own
/// Clause: 4.5.16.1
#[gtest]
#[tokio::test]
async fn n_ent_ret_9013_a_lone_instance_is_not_selected_by_another_dataset_id() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let building_id = namespace.urn("Building", 1)?;
    let building_type = namespace.entity_type("Building")?;
    let two_dataset = namespace.dataset_id("Dataset", "two")?;
    let one_dataset = namespace.dataset_id("Dataset", "one")?;
    let building = Entity::builder().id(building_id.clone()).entity_type(building_type).build()?;
    // The model refuses it (the model reads it as written otherwise [at $.location (written [{"type":"GeoProperty","value":{"type":"Point","coordinates":[1.0,0.0]},"datasetId":"urn:ngsi-ld:Dataset:zqns-one"}])]: {"id":"urn:ngsi-ld:Building:zqns-1","type":"Building_zqns","location":{"type":"GeoProperty","value":{"type":"Point","coordinates":[1.0,0.0]},"datasetId":"urn:ngsi-ld:Dataset:zqns-one"}}), so `location` is written as text.
    let location: RawJson =
        format!(r#"[{{"type": "GeoProperty", "value": {{"type": "Point", "coordinates": [1.0, 0.0]}}, "datasetId": "urn:ngsi-ld:Dataset:{namespace}-one"}}]"#)
            .parse()?;
    let document = UncheckedDocument::from_entity(&building, Lossless::Normalized).set_member("location", &location);
    let request = broker.unchecked(&CreateEntity::builder().entity(&building).build())?.document(&document);
    let answer = broker.send_unchecked(request).await;
    expect_that!(&answer, answered(StatusCode::CREATED));
    let retrieved = broker
        .with_accept(Accept::new(NgsiLdMediaType::GeoJson))
        .entities()
        .retrieve_geo_json(&building_id)
        .dataset_ids(vec![two_dataset])
        .send()
        .await?;
    let feature = retrieved.body();
    expect_that!(feature.geometry, none());
    let retrieved_2 = broker
        .with_accept(Accept::new(NgsiLdMediaType::GeoJson))
        .entities()
        .retrieve_geo_json(&building_id)
        .dataset_ids(vec![one_dataset])
        .send()
        .await?;
    let feature_2 = retrieved_2.body();
    expect_that!(feature_2.geometry.as_ref(), some(geometry().of_type("Point").ordinate([0], 1)));
    Ok(())
}
