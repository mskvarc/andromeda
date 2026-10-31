//! After a core delete an added instance restores the path entity.

use andromeda_test_suite::{
    attribute_matcher::property,
    broker,
    evolution_matcher::has_evolution,
    identity_matcher::has_id,
    namespace::Namespace,
    nth::nth,
    outcome::Upserted,
    problem::answered,
    typed_fragment::naming_type,
};
use googletest::prelude::*;
use http::StatusCode;
use ngsi_ld::{
    http::headers::Accept,
    model::{NgsiLdMediaType, Property, evolution::TemporalEntity},
};

/// Pins: after the Entity is deleted through the Core API, Add Attributes with a Fragment that names no id answers
/// 204, the added instance joins the Temporal Evolution, and the Entity has current state again under the path's
/// id, in Retrieve and in Query Entities alike
/// Clause: 5.6.12.4, 6.20.3.1, 5.6.6.4, 4.3.1
/// Recorded: clause 4.3.1 gives the current state to the Core API and the Temporal Evolution to the Temporal API
/// and leaves open whether a Temporal API write reaches the current state; Andromeda keeps the current state in
/// step with the Temporal Evolution, so the Entity a Temporal API write addresses has current state
#[gtest]
#[tokio::test]
async fn o_tmp_add_0013_after_a_core_delete_an_added_instance_restores_the_path_entity() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let vehicle_id = namespace.urn("Vehicle", 1)?;
    let vehicle_type = namespace.entity_type("Vehicle")?;
    let fuel_dataset = namespace.dataset_id("Dataset", "fuel")?;
    let vehicle = TemporalEntity::builder()
        .id(vehicle_id.clone())
        .entity_type(vehicle_type.clone())
        .property("speed", Property::builder(120).observed_at("2020-09-01T12:03:00Z".parse()?))
        .property("speed", Property::builder(80).observed_at("2020-09-01T12:05:00Z".parse()?))
        .property("fuelLevel", Property::builder(67).observed_at("2020-09-01T12:03:00Z".parse()?))
        .property("fuelLevel", Property::builder(53).observed_at("2020-09-01T13:05:00Z".parse()?))
        .property(
            "fuelLevel",
            Property::builder(40).observed_at("2020-09-01T14:07:00Z".parse()?).dataset_id(fuel_dataset),
        )
        .build()?;
    broker.temporal().create_or_update(&vehicle).send().await?.created()?;
    broker.entities().delete(&vehicle_id).send().await?;
    let instances = TemporalEntity::builder()
        .id(vehicle_id.clone())
        .entity_type(vehicle_type.clone())
        .property("speed", Property::builder(99).observed_at("2020-09-02T12:00:00Z".parse()?))
        .build()?
        .attributes;
    // The typed operation writes no `type` beside the instances, so the Fragment names it on the written document.
    let appended = broker.send_unchecked(naming_type(&broker, &vehicle_id, &instances, &vehicle_type)?).await;
    expect_that!(&appended, answered(StatusCode::NO_CONTENT));
    let json = broker.with_accept(Accept::new(NgsiLdMediaType::Json));
    let current = json.entities().retrieve(&vehicle_id).send().await?;
    expect_that!(current.body().id, eq(&vehicle_id));
    let listed = json.entities().query().entity_type(&vehicle_type).send().await?;
    expect_that!(listed.items(), len(eq(1)));
    expect_that!(listed.items(), nth(0, has_id(&vehicle_id)));
    let evolution = json.temporal().retrieve(&vehicle_id).send().await?;
    expect_that!(
        evolution.body(),
        has_evolution("speed", contains(property().value(99).observed_at("2020-09-02T12:00:00Z".parse()?)))
    );
    Ok(())
}
