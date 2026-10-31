//! A temporal write refuses a relationship seal.

use andromeda_test_suite::{
    broker,
    namespace::Namespace,
    outcome::Upserted,
    problem::{answered, problem},
    wire::request_to,
};
use bytes::Bytes;
use googletest::prelude::*;
use http::{HeaderValue, Method, StatusCode, header::CONTENT_TYPE};
use ngsi_ld::{
    http::{UncheckedDocument, headers::Accept, operation::CreateOrUpdateTemporalEvolution},
    model::{ErrorType, Lossless, NgsiLdMediaType, Property, RawJson, Value, evolution::TemporalEntity},
};

/// Pins: Create Temporal Evolution whose second Relationship instance carries `entityIdSealed` answers 400
/// `BadRequestData` and records nothing, and Add Attributes to an existing Temporal Evolution with a Relationship
/// instance carrying `entityTypeSealed` answers 400 `BadRequestData`
/// Clause: 5.6.11.4, 5.6.12.4, 5.5.4, 4.5.3.2
#[gtest]
#[tokio::test]
async fn n_tmp_ups_9001_a_temporal_write_refuses_a_relationship_seal() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let building_id = namespace.urn("Building", 1)?;
    let building_type = namespace.entity_type("Building")?;
    let building_2_id = namespace.urn("Building", 2)?;
    let building_evolution = TemporalEntity::builder().id(building_id.clone()).entity_type(building_type.clone()).build()?;
    // The model refuses it (member `entityIdSealed` is not allowed here at belongsTo), so `belongsTo` is written as text.
    let belongs_to: RawJson = format!(r#"[{{"type": "Relationship", "object": "urn:ngsi-ld:Site:{namespace}-1", "observedAt": "2024-01-01T00:00:00Z"}}, {{"type": "Relationship", "object": "urn:ngsi-ld:Site:{namespace}-2", "observedAt": "2024-01-02T00:00:00Z", "entityIdSealed": "urn:ngsi-ld:Building:{namespace}-sealed"}}]"#).parse()?;
    let document = UncheckedDocument::try_from(building_evolution.render(Lossless::Normalized))?.set_member("belongsTo", &belongs_to);
    let request = broker
        .unchecked(&CreateOrUpdateTemporalEvolution::builder().entity(&building_evolution).build())?
        .document(&document);
    let answer = broker.send_unchecked(request).await;
    expect_that!(&answer, problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData));
    let answer_2 = broker
        .with_responses()
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .temporal()
        .retrieve(&building_id)
        .send()
        .await;
    expect_that!(&answer_2, answered(StatusCode::NOT_FOUND));
    let building_2_evolution = TemporalEntity::builder()
        .id(building_2_id.clone())
        .entity_type(building_type.clone())
        .property(
            "temperature",
            Property::builder(Value::Number("21.5".parse()?)).observed_at("2024-01-01T00:00:00Z".parse()?),
        )
        .build()?;
    broker.temporal().create_or_update(&building_2_evolution).send().await?.created()?;
    // The model refuses this temporal fragment, so the body is sent as written.
    let request_2 = request_to(&broker, Method::POST, &format!("temporal/entities/{building_2_id}/attrs"))?.append_header(CONTENT_TYPE, HeaderValue::from_static("application/json")).body(Bytes::from(format!("{{\n    \"belongsTo\": [\n        {{\n            \"type\": \"Relationship\",\n            \"object\": \"urn:ngsi-ld:Site:{namespace}-1\",\n            \"entityTypeSealed\": \"Building_{namespace}\"\n        }}\n    ]\n}}")));
    let answer_3 = broker.send_unchecked(request_2).await;
    expect_that!(&answer_3, problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData));
    Ok(())
}
