//! Dataset id none retrieves the default instance alone.

use andromeda_test_suite::{
    attribute_matcher::property,
    body_json::{json_absent, json_count, json_equals},
    broker,
    entity_matcher::{has_attribute, has_instances},
    namespace::Namespace,
    outcome::Done,
    wire::request_to,
};
use googletest::prelude::*;
use http::{HeaderValue, Method, header::ACCEPT};
use ngsi_ld::{
    http::headers::Accept,
    model::{DatasetId, Entity, NgsiLdMediaType, Property},
};

/// Pins: Retrieve Entity with attrs=temperature over the default instance 1, an instance with an http `datasetId` 2
/// and one with a URN `datasetId` 3 answers all three, and with `datasetId=@none` answers temperature as exactly
/// the default instance {type: Property, value: 1}
/// Clause: 5.7.1, 6.5.3.1, 4.5.5
#[gtest]
#[tokio::test]
async fn n_ent_ret_9034_dataset_id_none_retrieves_the_default_instance_alone() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let building_distinct_datasets_id = namespace.urn("Building", "distinct-datasets")?;
    let building_type = namespace.entity_type("Building")?;
    let http_dataset = "http://example.test/dataset".parse::<DatasetId>()?;
    let urn_dataset = namespace.dataset_id("Dataset", "urn-dataset")?;
    let building_distinct_datasets = Entity::builder()
        .id(building_distinct_datasets_id.clone())
        .entity_type(building_type)
        .property("temperature", 1)
        .property("temperature", Property::builder(2).dataset_id(http_dataset.clone()))
        .property("temperature", Property::builder(3).dataset_id(urn_dataset.clone()))
        .build()?;
    broker.entities().create(&building_distinct_datasets).send().await?.done()?;
    let retrieved = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .retrieve(&building_distinct_datasets_id)
        .attrs(vec!["temperature".parse()?])
        .send()
        .await?;
    let entity = retrieved.body();
    expect_that!(entity, has_instances("temperature", 3));
    expect_that!(entity, has_attribute("temperature", property().dataset_id(http_dataset).value(2)));
    expect_that!(entity, has_attribute("temperature", property().dataset_id(urn_dataset).value(3)));
    // The client writes no `datasetId` parameter on a retrieval, so the request is sent as written.
    let request_2 = request_to(
        &broker,
        Method::GET,
        &format!("entities/{building_distinct_datasets_id}?attrs=temperature&datasetId=%40none"),
    )?
    .append_header(ACCEPT, HeaderValue::from_static("application/json"));
    let retrieved_2 = broker.send_unchecked(request_2).await?;
    // That the attribute is exactly the two members of the default instance is a count of the document, so the
    // answer is read as received.
    let body_2 = retrieved_2.body();
    expect_that!(body_2, json_count("$.temperature.*", 2));
    expect_that!(body_2, json_equals("$.temperature.type", "Property"));
    expect_that!(body_2, json_equals("$.temperature.value", 1));
    expect_that!(body_2, json_absent("$.temperature.datasetId"));
    Ok(())
}
