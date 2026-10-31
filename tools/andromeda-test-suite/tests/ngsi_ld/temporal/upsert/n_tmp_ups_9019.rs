//! One instance id on two dataset instances is bad request data.

use andromeda_test_suite::{broker, namespace::Namespace, problem::problem};
use googletest::prelude::*;
use http::StatusCode;
use ngsi_ld::{
    http::{UncheckedDocument, headers::Accept, operation::CreateOrUpdateTemporalEvolution},
    model::{ErrorType, Lossless, NgsiLdMediaType, RawJson, evolution::TemporalEntity},
};

/// Pins: Create Temporal Evolution of speed with an instance of dataset a and an instance of dataset b carrying the
/// same `instanceId` answers 400 `BadRequestData`, and no Temporal Evolution of that Entity exists
/// Clause: 4.5.2, 4.5.7, 5.6.11.4, 5.5.4
#[gtest]
#[tokio::test]
async fn n_tmp_ups_9019_one_instance_id_on_two_dataset_instances_is_bad_request_data() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let vehicle_shared_instance_id_id = namespace.urn("Vehicle", "shared-instance-id")?;
    let vehicle_type = namespace.entity_type("Vehicle")?;
    let vehicle_shared_instance_id_evolution = TemporalEntity::builder()
        .id(vehicle_shared_instance_id_id.clone())
        .entity_type(vehicle_type)
        .build()?;
    // The model refuses it (the attribute cannot stand under its name at speed), so `speed` is written as text.
    let speed: RawJson = format!(r#"[{{"type": "Property", "value": 1, "observedAt": "2021-01-01T00:00:00Z", "instanceId": "urn:ngsi-ld:my-own:instance:{namespace}-shared", "datasetId": "urn:ngsi-ld:Dataset:{namespace}-a"}}, {{"type": "Property", "value": 2, "observedAt": "2021-01-02T00:00:00Z", "instanceId": "urn:ngsi-ld:my-own:instance:{namespace}-shared", "datasetId": "urn:ngsi-ld:Dataset:{namespace}-b"}}]"#).parse()?;
    let document = UncheckedDocument::try_from(vehicle_shared_instance_id_evolution.render(Lossless::Normalized))?.set_member("speed", &speed);
    let request = broker
        .unchecked(&CreateOrUpdateTemporalEvolution::builder().entity(&vehicle_shared_instance_id_evolution).build())?
        .document(&document);
    let answer = broker.send_unchecked(request).await;
    expect_that!(&answer, problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData));
    let answer_2 = broker
        .with_responses()
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .temporal()
        .retrieve(&vehicle_shared_instance_id_id)
        .send()
        .await;
    expect_that!(&answer_2, problem(StatusCode::NOT_FOUND, ErrorType::ResourceNotFound));
    Ok(())
}
