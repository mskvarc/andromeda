//! A simplified values member with a dataset id is bad request data.

use andromeda_test_suite::{broker, namespace::Namespace, problem::problem};
use googletest::prelude::*;
use http::StatusCode;
use ngsi_ld::{
    http::{UncheckedDocument, headers::Accept, operation::CreateOrUpdateTemporalEvolution},
    model::{ErrorType, Lossless, NgsiLdMediaType, RawJson, evolution::TemporalEntity},
};

/// Pins: a simplified Property written with a `datasetId` beside its values member is refused as `BadRequestData`
/// and creates nothing, since the simplified object holds the values member only
/// Clause: 4.5.9, 5.6.11.4, 5.5.4
#[gtest]
#[tokio::test]
async fn n_tmp_ups_0020_a_simplified_values_member_with_a_dataset_id_is_bad_request_data() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let vehicle_id = namespace.urn("Vehicle", 1)?;
    let vehicle_type = namespace.entity_type("Vehicle")?;
    let vehicle_evolution = TemporalEntity::builder().id(vehicle_id.clone()).entity_type(vehicle_type).build()?;
    // The model refuses it (expected a JSON object at speed.values), so `speed` is written as text.
    let speed: RawJson = format!(r#"{{"type": "Property", "datasetId": "urn:ngsi-ld:Dataset:{namespace}-sensor", "values": [[55, "2020-08-01T12:00:00Z"], [61, "2020-08-01T13:00:00Z"]]}}"#).parse()?;
    let document = UncheckedDocument::try_from(vehicle_evolution.render(Lossless::Normalized))?.set_member("speed", &speed);
    let request = broker
        .unchecked(&CreateOrUpdateTemporalEvolution::builder().entity(&vehicle_evolution).build())?
        .document(&document);
    let answer = broker.send_unchecked(request).await;
    expect_that!(&answer, problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData));
    let answer_2 = broker
        .with_responses()
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .temporal()
        .retrieve(&vehicle_id)
        .send()
        .await;
    expect_that!(&answer_2, problem(StatusCode::NOT_FOUND, ErrorType::ResourceNotFound));
    Ok(())
}
