//! A simplified attribute naming a dataset is refused.

use andromeda_test_suite::{
    attribute_matcher::any_attribute,
    broker,
    evolution_matcher::has_evolution,
    namespace::Namespace,
    nth::nth,
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
    query::temporal_query,
};

/// Pins: Add Attributes to Temporal Evolution of an Entity with speed in the simplified temporal representation
/// carrying a `datasetId` beside its values answers 400 `BadRequestData`, and speed keeps its one instance
/// Clause: 4.5.9, 5.6.12.3, 5.5.4
#[gtest]
#[tokio::test]
async fn n_tmp_add_9005_a_simplified_attribute_naming_a_dataset_is_refused() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let simplified_probe_add_id = namespace.urn("SimplifiedProbe", "add")?;
    let simplified_probe_type = namespace.entity_type("SimplifiedProbe")?;
    let simplified_probe_add_evolution = TemporalEntity::builder()
        .id(simplified_probe_add_id.clone())
        .entity_type(simplified_probe_type)
        .property("speed", Property::builder(1).observed_at("2024-01-01T00:00:00Z".parse()?))
        .build()?;
    broker.temporal().create_or_update(&simplified_probe_add_evolution).send().await?.created()?;
    // The model refuses this temporal fragment, so the body is sent as written.
    let request = request_to(&broker, Method::POST, &format!("temporal/entities/{simplified_probe_add_id}/attrs"))?.append_header(CONTENT_TYPE, HeaderValue::from_static("application/json")).body(Bytes::from(format!("{{\n    \"speed\": {{\n        \"type\": \"Property\",\n        \"datasetId\": \"urn:ngsi-ld:Dataset:{namespace}-radar\",\n        \"values\": [\n            [\n                2,\n                \"2024-01-02T00:00:00Z\"\n            ],\n            [\n                3,\n                \"2024-01-03T00:00:00Z\"\n            ]\n        ]\n    }}\n}}")));
    let answer = broker.send_unchecked(request).await;
    expect_that!(&answer, problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData));
    let retrieved = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .temporal()
        .retrieve(&simplified_probe_add_id)
        .temporal_q(temporal_query::after("2000-01-01T00:00:00Z".parse()?))
        .send()
        .await?;
    let evolution = retrieved.body();
    expect_that!(evolution, has_evolution("speed", all!(len(eq(1)), nth(0, any_attribute().value(1)))));
    Ok(())
}
