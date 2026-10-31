//! Without a default instance 1 0 keeps the first instance held in every representation.

use andromeda_test_suite::{
    attribute_matcher::any_attribute,
    broker,
    entity_matcher::has_attribute,
    namespace::Namespace,
    outcome::Done,
    received_status::ExpectedStatus,
    simplified_matcher::has_value,
};
use googletest::prelude::*;
use http::StatusCode;
use ngsi_ld::{
    http::{
        headers::{Accept, Prefer, SpecVersion},
        operation::RepresentationSpelling,
    },
    model::{Entity, Lossless, NgsiLdMediaType, Property},
};

/// Pins: Retrieve Entity of an attribute holding two instances with distinct `datasetIds` and no default one, with
/// Prefer ngsi-ld=1.0 in the normalized and the `keyValues` representation, answers 203 with Preference-Applied
/// ngsi-ld=1.0 and the first instance written in both
/// Clause: 4.3.6.8, 4.5.5, 6.3.6
/// Recorded: Table 4.3.6.8-1 notes 2 and 3 return one instance for 1.0 "with preference to the default instance"
/// and name none when there is no default instance; Andromeda keeps the first instance written, not the first
/// `datasetId` in order
#[gtest]
#[tokio::test]
async fn o_cmn_prf_9009_without_a_default_instance_1_0_keeps_the_first_instance_held_in_every_representation() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let vehicle_same_instance_id = namespace.urn("Vehicle", "same-instance")?;
    let vehicle_type = namespace.entity_type("Vehicle")?;
    let vehicle_same_instance = Entity::builder()
        .id(vehicle_same_instance_id.clone())
        .entity_type(vehicle_type)
        .property("speed", Property::builder(10).dataset_id(namespace.dataset_id("Dataset", "z")?))
        .property("speed", Property::builder(20).dataset_id(namespace.dataset_id("Dataset", "a")?))
        .build()?;
    broker.entities().create(&vehicle_same_instance).send().await?.done()?;
    let retrieved = broker
        .with_heads()
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .with_prefer(Prefer::builder().version("1.0".parse::<SpecVersion>()?).build()?)
        .entities()
        .retrieve(&vehicle_same_instance_id)
        .form(Lossless::Normalized)
        .spelling(RepresentationSpelling::default().in_options().explicit_normalized())
        .send()
        .await?
        .expecting(StatusCode::NON_AUTHORITATIVE_INFORMATION)?;
    expect_that!(retrieved.preference_applied(), some(eq(&"1.0".parse::<SpecVersion>()?)));
    let entity = retrieved.body();
    expect_that!(entity, has_attribute("speed", any_attribute().value(10)));
    let retrieved_2 = broker
        .with_heads()
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .with_prefer(Prefer::builder().version("1.0".parse::<SpecVersion>()?).build()?)
        .entities()
        .retrieve_simplified(&vehicle_same_instance_id)
        .spelling(RepresentationSpelling::default().in_options().key_values())
        .send()
        .await?
        .expecting(StatusCode::NON_AUTHORITATIVE_INFORMATION)?;
    expect_that!(retrieved_2.preference_applied(), some(eq(&"1.0".parse::<SpecVersion>()?)));
    let entity_2 = retrieved_2.body();
    expect_that!(entity_2, has_value("speed", 10));
    Ok(())
}
