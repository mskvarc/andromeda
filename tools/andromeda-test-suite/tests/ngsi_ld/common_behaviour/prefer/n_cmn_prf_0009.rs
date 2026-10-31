//! Version 1 0 removes the dataset id.

use andromeda_test_suite::{
    attribute_matcher::any_attribute,
    attribute_member::Member,
    body_json::json_absent,
    broker,
    entity_matcher::has_attribute,
    namespace::Namespace,
    outcome::Done,
};
use googletest::prelude::*;
use http::StatusCode;
use ngsi_ld::{
    http::headers::{Accept, Prefer, SpecVersion},
    model::{Entity, NgsiLdMediaType, Property, Value},
};

/// Pins: Retrieve Entity under Prefer ngsi-ld=1.0 removes the `datasetId` of an attribute, which arrived at version
/// 1.3, and answers 203 with the version echoed
/// Clause: 4.3.6.8, 6.3.6, 5.7.1.4, 6.5.3.1
#[gtest]
#[tokio::test]
async fn n_cmn_prf_0009_version_1_0_removes_the_dataset_id() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let building_id = namespace.urn("Building", 1)?;
    let building_type = namespace.entity_type("Building")?;
    let building = Entity::builder()
        .id(building_id.clone())
        .entity_type(building_type)
        .property(
            "temperature",
            Property::builder(Value::Number("25.0".parse()?)).dataset_id(namespace.dataset_id("Dataset", "temp1")?),
        )
        .build()?;
    broker.entities().create(&building).send().await?.done()?;
    let received = broker
        .with_responses()
        .with_accept(Accept::new(NgsiLdMediaType::JsonLd))
        .with_prefer(Prefer::builder().version("1.0".parse::<SpecVersion>()?).build()?)
        .entities()
        .retrieve(&building_id)
        .send()
        .await?;
    expect_that!(received.status, eq(StatusCode::NON_AUTHORITATIVE_INFORMATION));
    expect_that!(received.outcome.preference_applied(), some(eq(&"1.0".parse::<SpecVersion>()?)));
    let entity = received.outcome.body();
    expect_that!(entity.id, eq(&building_id));
    expect_that!(entity, has_attribute("temperature", any_attribute().value(25).without(Member::DatasetId)));
    // No `datasetId` anywhere in the document is a fact of the bytes, so it is read as received.
    expect_that!(&received.body, json_absent("$..datasetId"));
    Ok(())
}
