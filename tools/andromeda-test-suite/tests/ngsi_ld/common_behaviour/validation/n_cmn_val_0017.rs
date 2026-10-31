//! Add temporal attributes with an empty fragment changes nothing.

use andromeda_test_suite::{body_json::json_count, broker, evolution_matcher::has_evolution, namespace::Namespace, outcome::Upserted};
use googletest::prelude::*;
use ngsi_ld::{
    http::headers::Accept,
    model::{NgsiLdMediaType, Property, evolution::TemporalEntity},
};

/// Pins: Add Attributes to Temporal Evolution with the empty Fragment {} as application/json answers 204 and adds
/// no instance
/// Clause: 5.4, 5.6.12.4, 6.20.3.1
/// Recorded: clause 5.4 makes a Fragment an IETF RFC 7396 JSON Merge Patch, under which the empty patch leaves its
/// target unchanged, and clause 5.6.12.4 lists no error for an empty Fragment
#[gtest]
#[tokio::test]
async fn n_cmn_val_0017_add_temporal_attributes_with_an_empty_fragment_changes_nothing() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let vehicle_id = namespace.urn("Vehicle", 1)?;
    let vehicle_type = namespace.entity_type("Vehicle")?;
    let vehicle_evolution = TemporalEntity::builder()
        .id(vehicle_id.clone())
        .entity_type(vehicle_type.clone())
        .property("speed", Property::builder(120).observed_at("2020-09-01T12:03:00Z".parse()?))
        .property("speed", Property::builder(80).observed_at("2020-09-01T12:05:00Z".parse()?))
        .property("fuelLevel", Property::builder(67).observed_at("2020-09-01T12:03:00Z".parse()?))
        .property("fuelLevel", Property::builder(53).observed_at("2020-09-01T13:05:00Z".parse()?))
        .property(
            "fuelLevel",
            Property::builder(40)
                .dataset_id(namespace.dataset_id("Dataset", "fuel")?)
                .observed_at("2020-09-01T14:07:00Z".parse()?),
        )
        .build()?;
    broker.temporal().create_or_update(&vehicle_evolution).send().await?.created()?;
    let nothing = TemporalEntity::builder().id(vehicle_id.clone()).entity_type(vehicle_type).build()?.attributes;
    broker.temporal().add_attributes(&vehicle_id, &nothing).send().await?;
    let received = broker
        .with_responses()
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .temporal()
        .retrieve(&vehicle_id)
        .send()
        .await?;
    let evolution = received.outcome.body();
    expect_that!(evolution, has_evolution("speed", len(eq(2))));
    expect_that!(evolution, has_evolution("fuelLevel", len(eq(3))));
    // The member count of the document is read as received, since the model keeps no count of what it read.
    expect_that!(&received.body, json_count("$", 4));
    Ok(())
}
