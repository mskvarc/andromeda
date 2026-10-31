//! A fragment naming another attribute is bad request data.

use andromeda_test_suite::{
    attribute_matcher::any_attribute,
    attribute_member::Member,
    broker,
    evolution_matcher::{has_evolution, instance_id_at},
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
    http::{headers::Accept, operation::SystemAttributes},
    model::{ErrorType, NgsiLdMediaType, Property, evolution::TemporalEntity},
};

/// Pins: Modify Attribute instance of speed with a Fragment holding fuel answers 400 `BadRequestData`, and the
/// speed instance keeps 60
/// Clause: 5.6.14.3, 5.6.14.4
#[gtest]
#[tokio::test]
async fn n_tmp_mod_9003_a_fragment_naming_another_attribute_is_bad_request_data() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let vehicle_fragment_other_attribute_id = namespace.urn("Vehicle", "fragment-other-attribute")?;
    let vehicle_type = namespace.entity_type("Vehicle")?;
    let vehicle_fragment_other_attribute_evolution = TemporalEntity::builder()
        .id(vehicle_fragment_other_attribute_id.clone())
        .entity_type(vehicle_type)
        .property("speed", Property::builder(60).observed_at("2025-01-01T00:00:00Z".parse()?))
        .property("fuel", Property::builder(40).observed_at("2025-01-01T00:00:00Z".parse()?))
        .build()?;
    broker
        .temporal()
        .create_or_update(&vehicle_fragment_other_attribute_evolution)
        .send()
        .await?
        .created()?;
    let retrieved = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .temporal()
        .retrieve(&vehicle_fragment_other_attribute_id)
        .system_attributes(SystemAttributes::Include)
        .send()
        .await?;
    let evolution = retrieved.body();
    let instance_id = instance_id_at(evolution, &"speed".parse()?, 0)?;
    expect_that!(evolution, has_evolution("speed", nth(0, any_attribute().with_member(Member::CreatedAt))));
    // The instance modification writes only one instance of the path's attribute, so the body is sent as written.
    let request = request_to(&broker, Method::PATCH, &format!("temporal/entities/{vehicle_fragment_other_attribute_id}/attrs/speed/{instance_id}"))?.append_header(CONTENT_TYPE, HeaderValue::from_static("application/json")).body(Bytes::from("{\n    \"fuel\": [\n        {\n            \"type\": \"Property\",\n            \"value\": 129,\n            \"observedAt\": \"2025-01-01T00:00:00Z\"\n        }\n    ]\n}"));
    let answer = broker.send_unchecked(request).await;
    expect_that!(&answer, problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData));
    let retrieved_2 = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .temporal()
        .retrieve(&vehicle_fragment_other_attribute_id)
        .system_attributes(SystemAttributes::Include)
        .send()
        .await?;
    let evolution_2 = retrieved_2.body();
    expect_that!(evolution_2, has_evolution("speed", all!(len(eq(1)), nth(0, any_attribute().value(60)))));
    Ok(())
}
