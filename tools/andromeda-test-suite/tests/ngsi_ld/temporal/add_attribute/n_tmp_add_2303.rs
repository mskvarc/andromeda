//! A simplified values member with a dataset id is bad request data.

use andromeda_test_suite::{
    broker,
    evolution_matcher::{has_evolution, lacks_evolution},
    namespace::Namespace,
    outcome::Upserted,
    problem::problem,
    wire::request_to,
};
use bytes::Bytes;
use googletest::prelude::*;
use http::{HeaderValue, Method, StatusCode, header::CONTENT_TYPE};
use ngsi_ld::{
    http::headers::Accept,
    model::{ErrorType, NgsiLdMediaType, Property, evolution::TemporalEntity},
};

/// Pins: Add Attributes to Temporal Evolution refuses a simplified Property written with a `datasetId` beside its
/// values member as `BadRequestData` and adds no instance
/// Clause: 4.5.9, 5.6.12, 5.5.4
#[gtest]
#[tokio::test]
async fn n_tmp_add_2303_a_simplified_values_member_with_a_dataset_id_is_bad_request_data() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let vehicle_id = namespace.urn("Vehicle", 1)?;
    let vehicle_type = namespace.entity_type("Vehicle")?;
    let vehicle_evolution = TemporalEntity::builder()
        .id(vehicle_id.clone())
        .entity_type(vehicle_type)
        .property("brandName", Property::builder("Mercedes").observed_at("2020-08-01T12:00:00Z".parse()?))
        .build()?;
    broker.temporal().create_or_update(&vehicle_evolution).send().await?.created()?;
    // The model refuses this temporal fragment, so the body is sent as written.
    let request = request_to(&broker, Method::POST, &format!("temporal/entities/{vehicle_id}/attrs"))?.append_header(CONTENT_TYPE, HeaderValue::from_static("application/json")).body(Bytes::from(format!("{{\n    \"speed\": {{\n        \"type\": \"Property\",\n        \"datasetId\": \"urn:ngsi-ld:Dataset:{namespace}-sensor\",\n        \"values\": [\n            [55, \"2020-08-01T13:00:00Z\"]\n        ]\n    }}\n}}")));
    let answer = broker.send_unchecked(request).await;
    expect_that!(&answer, problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData));
    let retrieved = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .temporal()
        .retrieve(&vehicle_id)
        .send()
        .await?;
    let evolution = retrieved.body();
    expect_that!(evolution, has_evolution("brandName", len(eq(1))));
    expect_that!(evolution, lacks_evolution("speed"));
    Ok(())
}
