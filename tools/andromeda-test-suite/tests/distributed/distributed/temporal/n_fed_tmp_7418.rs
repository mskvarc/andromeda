//! The pairs of one dataset held by two brokers merge into one object.

use andromeda_test_suite::{
    body_json::{json_count, json_equals, json_exists},
    broker,
    config::{BrokerRole, PeerRole},
    namespace::Namespace,
    outcome::Upserted,
    wire::request_to,
};
use bytes::Bytes;
use googletest::prelude::*;
use http::{Method, StatusCode};
use ngsi_ld::{
    model::{Property, evolution::TemporalEntity},
    registration::{CSourceRegistration, DistributedOperation, EntityInfo, RegistrationInfo},
};

/// Requires `body` to hold one `speed` object of `dataset` whose four pairs are the instances of both brokers.
fn expect_the_union_of_both_brokers(body: &Bytes, dataset: &str) {
    expect_that!(body, json_equals("$.speed.datasetId", dataset));
    expect_that!(body, json_count("$.speed.values", 4));
    expect_that!(body, json_exists("$.speed.values[?(@[0] == 10 && @[1] == '2020-08-01T12:00:00Z')]"));
    expect_that!(body, json_exists("$.speed.values[?(@[0] == 20 && @[1] == '2020-08-01T12:05:00Z')]"));
    expect_that!(body, json_exists("$.speed.values[?(@[0] == 30 && @[1] == '2020-08-01T12:10:00Z')]"));
    expect_that!(body, json_exists("$.speed.values[?(@[0] == 40 && @[1] == '2020-08-01T12:15:00Z')]"));
}

/// Pins: Retrieve Temporal Evolution as `temporalValues`, by format and by options, over one speed dataset split
/// across A and B answers one dataset object whose pairs are the union of the instances of both brokers
/// Clause: 5.7.3.4, 4.5.5, 4.5.9
#[gtest]
#[tokio::test]
async fn n_fed_tmp_7418_the_pairs_of_one_dataset_held_by_two_brokers_merge_into_one_object() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let vehicle_id = namespace.urn("Vehicle", 1)?;
    let vehicle_type = namespace.entity_type("Vehicle")?;
    let dataset_2 = namespace.dataset_id("Dataset", 2)?;
    let broker_b = broker::client(BrokerRole::B)?;
    let registration_id = namespace.registration_id(1)?;
    let vehicle_evolution = TemporalEntity::builder()
        .id(vehicle_id.clone())
        .entity_type(vehicle_type.clone())
        .property(
            "speed",
            Property::builder(10).dataset_id(dataset_2.clone()).observed_at("2020-08-01T12:00:00Z".parse()?),
        )
        .property(
            "speed",
            Property::builder(20).dataset_id(dataset_2.clone()).observed_at("2020-08-01T12:05:00Z".parse()?),
        )
        .build()?;
    broker.temporal().create_or_update(&vehicle_evolution).send().await?.created()?;
    let vehicle_evolution_2 = TemporalEntity::builder()
        .id(vehicle_id.clone())
        .entity_type(vehicle_type.clone())
        .property(
            "speed",
            Property::builder(30).dataset_id(dataset_2.clone()).observed_at("2020-08-01T12:10:00Z".parse()?),
        )
        .property(
            "speed",
            Property::builder(40).dataset_id(dataset_2.clone()).observed_at("2020-08-01T12:15:00Z".parse()?),
        )
        .build()?;
    broker_b.temporal().create_or_update(&vehicle_evolution_2).send().await?.created()?;
    let registration = CSourceRegistration::builder(broker::peer(PeerRole::B)?)
        .id(registration_id.clone())
        .information([RegistrationInfo::builder()
            .entities([EntityInfo::new(vehicle_type.clone()).with_id(vehicle_id.clone())])
            .property_names(["speed"])
            .build()?])
        .operations([DistributedOperation::RetrieveTemporal])
        .build()?;
    broker.registrations().register(&registration).send().await?;
    let dataset_text = namespace.urn_text("Dataset", 2);
    // The simplified temporal reader refuses the `datasetId` this answer carries, so the answer is read as received.
    let request = request_to(
        &broker,
        Method::GET,
        &format!("temporal/entities/{vehicle_id}?timerel=before&timeAt=2030-01-01T00%3A00%3A00Z&format=temporalValues"),
    )?;
    let response = broker.send_unchecked(request).await?;
    expect_that!(response.status(), eq(StatusCode::OK));
    expect_the_union_of_both_brokers(response.body(), &dataset_text);
    // The same retrieval, naming the representation in `options`.
    let request_2 = request_to(
        &broker,
        Method::GET,
        &format!("temporal/entities/{vehicle_id}?timerel=before&timeAt=2030-01-01T00%3A00%3A00Z&options=temporalValues"),
    )?;
    let response_2 = broker.send_unchecked(request_2).await?;
    expect_that!(response_2.status(), eq(StatusCode::OK));
    expect_the_union_of_both_brokers(response_2.body(), &dataset_text);
    Ok(())
}
