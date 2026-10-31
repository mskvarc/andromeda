//! A batch upsert of a simplified values member with a dataset id creates nothing.

use andromeda_test_suite::{broker, namespace::Namespace, problem::problem, raw_answer::send_raw, written_json::json_request_to};
use googletest::prelude::*;
use http::{Method, StatusCode};
use ngsi_ld::{
    http::headers::Accept,
    model::{ErrorType, NgsiLdMediaType},
};

/// Pins: Batch Upsert of Temporal Entities does not accept a simplified Property written with a datasetId beside its
/// values member: the answer is not a success and the entity is not created
/// Clause: 4.5.9, 5.6.11, 5.5.4
#[gtest]
#[tokio::test]
async fn n_tmp_ups_2310_a_batch_upsert_of_a_simplified_values_member_with_a_dataset_id_creates_nothing() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let vehicle_id = namespace.urn("Vehicle", 1)?;
    let vehicle_type = namespace.entity_type("Vehicle")?;
    let sensor = namespace.dataset_id("Dataset", "sensor")?;
    // The client has no Batch Upsert of Temporal Entities, and the model has no place for a `datasetId` beside a
    // `values` member, so the batch is sent as written.
    let body = format!(
        r#"[{{"id": "{vehicle_id}", "type": "{vehicle_type}", "speed": {{"type": "Property", "datasetId": "{sensor}", "values": [[55, "2020-08-01T12:00:00Z"]]}}}}]"#
    );
    let request = json_request_to(&broker, Method::POST, "temporal/entityOperations/upsert", &body)?;
    let answer = send_raw(request).await?;
    expect_that!(answer.status(), not(eq(StatusCode::CREATED)));
    expect_that!(answer.status(), not(eq(StatusCode::NO_CONTENT)));
    let missing = broker
        .with_responses()
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .temporal()
        .retrieve(&vehicle_id)
        .send()
        .await;
    expect_that!(&missing, problem(StatusCode::NOT_FOUND, ErrorType::ResourceNotFound));
    Ok(())
}
