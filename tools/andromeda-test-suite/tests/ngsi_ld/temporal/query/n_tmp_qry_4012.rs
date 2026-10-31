//! A query body time at with nine fraction digits is bad request data.

use andromeda_test_suite::{broker, namespace::Namespace, outcome::Upserted, problem::problem, wire::request_to};
use bytes::Bytes;
use googletest::prelude::*;
use http::{HeaderValue, Method, StatusCode, header::CONTENT_TYPE};
use ngsi_ld::model::{ErrorType, Property, evolution::TemporalEntity};

/// Pins: Query Temporal Evolution of Entities through a Query body whose `temporalQ` carries a `timeAt` of nine
/// fractional-second digits answers 400 `BadRequestData`
/// Clause: 4.6.3, 5.2.21, 5.7.4.4, 6.24.3.1, 5.5.4
#[gtest]
#[tokio::test]
async fn n_tmp_qry_4012_a_query_body_time_at_with_nine_fraction_digits_is_bad_request_data() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let vehicle_id = namespace.urn("Vehicle", 1)?;
    let vehicle_type = namespace.entity_type("Vehicle")?;
    let vehicle_evolution = TemporalEntity::builder()
        .id(vehicle_id)
        .entity_type(vehicle_type)
        .property("speed", Property::builder(1).observed_at("2026-01-01T00:00:00Z".parse()?))
        .build()?;
    broker.temporal().create_or_update(&vehicle_evolution).send().await?.created()?;
    // The model refuses a member of this document, so it is sent as written.
    let request = request_to(&broker, Method::POST, "temporal/entityOperations/query")?.append_header(CONTENT_TYPE, HeaderValue::from_static("application/json")).body(Bytes::from(format!("{{\n    \"type\": \"Query\",\n    \"entities\": [\n        {{\n            \"type\": \"Vehicle_{namespace}\"\n        }}\n    ],\n    \"temporalQ\": {{\n        \"timerel\": \"after\",\n        \"timeAt\": \"2026-03-04T05:06:07.123456789Z\"\n    }}\n}}")));
    let answer = broker.send_unchecked(request).await;
    expect_that!(&answer, problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData));
    Ok(())
}
