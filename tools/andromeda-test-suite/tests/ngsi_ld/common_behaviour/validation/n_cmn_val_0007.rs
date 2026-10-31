//! Add temporal attributes with a fragment id naming another entity is bad request data.

use andromeda_test_suite::{
    attribute_matcher::any_attribute,
    broker,
    evolution_matcher::has_evolution,
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

/// Pins: an Add Attributes to Temporal Evolution of an Entity whose Fragment id names another Entity than the
/// target is refused with 400 `BadRequestData`, and the target is unchanged
/// Clause: 5.4, 5.5.4, 5.6.12.4, 6.20.3.1
#[gtest]
#[tokio::test]
async fn n_cmn_val_0007_add_temporal_attributes_with_a_fragment_id_naming_another_entity_is_bad_request_data() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let vehicle_id = namespace.urn("Vehicle", 1)?;
    let vehicle_type = namespace.entity_type("Vehicle")?;
    let vehicle_evolution = TemporalEntity::builder()
        .id(vehicle_id.clone())
        .entity_type(vehicle_type)
        .property("speed", Property::builder(120).observed_at("2020-09-01T12:03:00Z".parse()?))
        .property("speed", Property::builder(80).observed_at("2020-09-01T12:05:00Z".parse()?))
        .build()?;
    broker.temporal().create_or_update(&vehicle_evolution).send().await?.created()?;
    // The temporal attribute addition names its own target in the fragment, so a fragment naming another entity is sent as written.
    let request = request_to(&broker, Method::POST, &format!("temporal/entities/{vehicle_id}/attrs"))?.append_header(CONTENT_TYPE, HeaderValue::from_static("application/json")).body(Bytes::from(format!("{{\n    \"id\": \"urn:ngsi-ld:Vehicle:{namespace}-2\",\n    \"speed\": [\n        {{ \"type\": \"Property\", \"value\": 99, \"observedAt\": \"2020-09-02T12:00:00Z\" }}\n    ]\n}}")));
    let answer = broker.send_unchecked(request).await;
    expect_that!(&answer, problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData));
    let retrieved = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .temporal()
        .retrieve(&vehicle_id)
        .send()
        .await?;
    let evolution = retrieved.body();
    expect_that!(
        evolution,
        has_evolution(
            "speed",
            all!(
                len(eq(2)),
                contains(any_attribute().value(120).value(120)),
                contains(any_attribute().value(120)).times(eq(1)),
                contains(any_attribute().value(80).value(80)),
                contains(any_attribute().value(80)).times(eq(1))
            )
        )
    );
    Ok(())
}
