//! Aggregates of one dataset are not combined and what only the source holds joins.

use andromeda_test_suite::{
    body_json::{json_count, json_exists},
    broker,
    config::{BrokerRole, PeerRole},
    namespace::Namespace,
    outcome::Upserted,
    wire::request_to,
};
use googletest::prelude::*;
use http::{Method, StatusCode};
use ngsi_ld::{
    model::{Property, evolution::TemporalEntity},
    registration::{CSourceRegistration, DistributedOperation, EntityInfo, RegistrationInfo},
};

/// Pins: Retrieve Temporal Evolution as `aggregatedValues` avg per day answers one default speed aggregate, the
/// speed aggregate of the dataset only broker B holds, and the `fuelLevel` only broker B holds
/// Clause: 5.7.3.4, 4.5.19
/// Recorded: clause 5.7.3.4 merges remote data into the local evolution but does not say how two aggregates of one
/// attribute and dataset combine; Andromeda keeps the local aggregate and adds what only the source holds
#[gtest]
#[tokio::test]
async fn o_fed_tmp_7420_aggregates_of_one_dataset_are_not_combined_and_what_only_the_source_holds_joins() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let vehicle_id = namespace.urn("Vehicle", 1)?;
    let vehicle_type = namespace.entity_type("Vehicle")?;
    let broker_b = broker::client(BrokerRole::B)?;
    let dataset_2 = namespace.dataset_id("Dataset", 2)?;
    let registration_id = namespace.registration_id(1)?;
    let vehicle_evolution = TemporalEntity::builder()
        .id(vehicle_id.clone())
        .entity_type(vehicle_type.clone())
        .property("speed", Property::builder(10).observed_at("2020-08-01T12:00:00Z".parse()?))
        .property("speed", Property::builder(20).observed_at("2020-08-01T13:00:00Z".parse()?))
        .build()?;
    broker.temporal().create_or_update(&vehicle_evolution).send().await?.created()?;
    let vehicle_evolution_2 = TemporalEntity::builder()
        .id(vehicle_id.clone())
        .entity_type(vehicle_type.clone())
        .property("speed", Property::builder(90).observed_at("2020-08-01T14:00:00Z".parse()?))
        .property(
            "speed",
            Property::builder(50).dataset_id(dataset_2.clone()).observed_at("2020-08-01T14:30:00Z".parse()?),
        )
        .property("fuelLevel", Property::builder(70).observed_at("2020-08-01T14:00:00Z".parse()?))
        .build()?;
    broker_b.temporal().create_or_update(&vehicle_evolution_2).send().await?.created()?;
    let registration = CSourceRegistration::builder(broker::peer(PeerRole::B)?)
        .id(registration_id.clone())
        .information([RegistrationInfo::builder()
            .entities([EntityInfo::new(vehicle_type.clone()).with_id(vehicle_id.clone())])
            .property_names(["speed", "fuelLevel"])
            .build()?])
        .operations([DistributedOperation::RetrieveTemporal])
        .build()?;
    broker.registrations().register(&registration).send().await?;
    // The aggregated reader holds one aggregate per attribute name, and this answer lists two for speed, so the
    // answer is read as received.
    let request = request_to(
        &broker,
        Method::GET,
        &format!(
            "temporal/entities/{vehicle_id}?timerel=before&timeAt=2030-01-01T00%3A00%3A00Z&format=aggregatedValues&aggrMethods=avg&aggrPeriodDuration=P1D"
        ),
    )?;
    let response = broker.send_unchecked(request).await?;
    expect_that!(response.status(), eq(StatusCode::OK));
    expect_that!(response.body(), json_count("$.speed", 2));
    expect_that!(
        response.body(),
        json_exists(&format!("$.speed[?(@.datasetId == '{}')]", namespace.urn_text("Dataset", 2)))
    );
    expect_that!(response.body(), json_exists("$.fuelLevel"));
    Ok(())
}
