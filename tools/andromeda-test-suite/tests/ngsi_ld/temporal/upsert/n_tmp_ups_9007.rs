//! A temporal create refuses the null under a key.

use andromeda_test_suite::{
    broker,
    core_context::core_entry,
    namespace::Namespace,
    problem::{answered, problem},
};
use googletest::prelude::*;
use http::StatusCode;
use ngsi_ld::{
    http::{ContextDelivery, UncheckedDocument, headers::Accept, operation::CreateOrUpdateTemporalEvolution},
    model::{Context, ContextEntry, ErrorType, InlineContext, JsonProperty, Lossless, NgsiLdMediaType, RawJson, evolution::TemporalEntity},
    query::temporal_query,
};

/// Pins: Create or Update Temporal Evolution of a new Entity whose Property value, JSON literal or `JsonProperty`
/// holds urn:ngsi-ld:null under a key answers 400 `BadRequestData` and records nothing
/// Clause: 5.5.4, 5.6.11.4
#[gtest]
#[tokio::test]
async fn n_tmp_ups_9007_a_temporal_create_refuses_the_null_under_a_key() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let body_context = broker
        .with_context(Context::Multiple(vec![
            ContextEntry::Inline(r#"{"settings": {"@id": "https://example.org/settings", "@type": "@json"}}"#.parse::<InlineContext>()?),
            core_entry()?,
        ]))?
        .with_context_delivery(ContextDelivery::LdJsonBody);
    let literal_probe_temporal_structured_value_id = namespace.urn("LiteralProbe", "temporal-structured-value")?;
    let literal_probe_type = namespace.entity_type("LiteralProbe")?;
    let literal_probe_temporal_json_coerced_key_id = namespace.urn("LiteralProbe", "temporal-json-coerced-key")?;
    let literal_probe_temporal_json_value_id = namespace.urn("LiteralProbe", "temporal-json-value")?;
    let literal_probe_temporal_json_property_id = namespace.urn("LiteralProbe", "temporal-json-property")?;
    let literal_probe_temporal_structured_valu_evolution = TemporalEntity::builder()
        .id(literal_probe_temporal_structured_value_id.clone())
        .entity_type(literal_probe_type.clone())
        .build()?;
    // The model refuses it (a request cannot hold the NGSI-LD Null in a member of `value` at config), so `config` is written as text.
    let config_2: RawJson =
        r#"[{"type": "Property", "value": {"street": "x", "country": "urn:ngsi-ld:null"}, "observedAt": "2024-01-01T00:00:00Z"}]"#.parse()?;
    let document = UncheckedDocument::try_from(literal_probe_temporal_structured_valu_evolution.render(Lossless::Normalized))?.set_member("config", &config_2);
    let request = body_context
        .unchecked(
            &CreateOrUpdateTemporalEvolution::builder()
                .entity(&literal_probe_temporal_structured_valu_evolution)
                .build(),
        )?
        .document(&document);
    let answer = body_context.send_unchecked(request).await;
    expect_that!(&answer, problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData));
    let answer_2 = broker
        .with_responses()
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .temporal()
        .retrieve(&literal_probe_temporal_structured_value_id)
        .temporal_q(temporal_query::after("2000-01-01T00:00:00Z".parse()?))
        .send()
        .await;
    expect_that!(&answer_2, answered(StatusCode::NOT_FOUND));
    let literal_probe_temporal_json_coerced_ke_evolution = TemporalEntity::builder()
        .id(literal_probe_temporal_json_coerced_key_id.clone())
        .entity_type(literal_probe_type.clone())
        .build()?;
    // The model refuses it (a request cannot hold the NGSI-LD Null in a member of `value` at config), so `config` is written as text.
    let config_3: RawJson =
        r#"[{"type": "Property", "value": {"settings": {"a": 1, "b": "urn:ngsi-ld:null"}}, "observedAt": "2024-01-01T00:00:00Z"}]"#.parse()?;
    let document_2 =
        UncheckedDocument::try_from(literal_probe_temporal_json_coerced_ke_evolution.render(Lossless::Normalized))?.set_member("config", &config_3);
    let request_2 = body_context
        .unchecked(
            &CreateOrUpdateTemporalEvolution::builder()
                .entity(&literal_probe_temporal_json_coerced_ke_evolution)
                .build(),
        )?
        .document(&document_2);
    let answer_3 = body_context.send_unchecked(request_2).await;
    expect_that!(&answer_3, problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData));
    let answer_4 = broker
        .with_responses()
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .temporal()
        .retrieve(&literal_probe_temporal_json_coerced_key_id)
        .temporal_q(temporal_query::after("2000-01-01T00:00:00Z".parse()?))
        .send()
        .await;
    expect_that!(&answer_4, answered(StatusCode::NOT_FOUND));
    let literal_probe_temporal_json_value_evolution = TemporalEntity::builder()
        .id(literal_probe_temporal_json_value_id.clone())
        .entity_type(literal_probe_type.clone())
        .build()?;
    // The model refuses it (a request cannot hold the NGSI-LD Null in a member of `value` at config), so `config` is written as text.
    let config_4: RawJson =
        r#"[{"type": "Property", "value": {"@type": "@json", "@value": {"a": 1, "b": "urn:ngsi-ld:null"}}, "observedAt": "2024-01-01T00:00:00Z"}]"#.parse()?;
    let document_3 = UncheckedDocument::try_from(literal_probe_temporal_json_value_evolution.render(Lossless::Normalized))?.set_member("config", &config_4);
    let request_3 = body_context
        .unchecked(
            &CreateOrUpdateTemporalEvolution::builder()
                .entity(&literal_probe_temporal_json_value_evolution)
                .build(),
        )?
        .document(&document_3);
    let answer_5 = body_context.send_unchecked(request_3).await;
    expect_that!(&answer_5, problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData));
    let answer_6 = broker
        .with_responses()
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .temporal()
        .retrieve(&literal_probe_temporal_json_value_id)
        .temporal_q(temporal_query::after("2000-01-01T00:00:00Z".parse()?))
        .send()
        .await;
    expect_that!(&answer_6, answered(StatusCode::NOT_FOUND));
    let literal_probe_temporal_json_property_evolution = TemporalEntity::builder()
        .id(literal_probe_temporal_json_property_id.clone())
        .entity_type(literal_probe_type.clone())
        .json_property(
            "config",
            JsonProperty::builder(r#"{"a":1,"b":"urn:ngsi-ld:null"}"#.parse::<RawJson>()?).observed_at("2024-01-01T00:00:00Z".parse()?),
        )
        .build()?;
    let answer_7 = body_context
        .with_responses()
        .temporal()
        .create_or_update(&literal_probe_temporal_json_property_evolution)
        .send()
        .await;
    expect_that!(&answer_7, problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData));
    let answer_8 = broker
        .with_responses()
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .temporal()
        .retrieve(&literal_probe_temporal_json_property_id)
        .temporal_q(temporal_query::after("2000-01-01T00:00:00Z".parse()?))
        .send()
        .await;
    expect_that!(&answer_8, answered(StatusCode::NOT_FOUND));
    Ok(())
}
