//! A reified scope outside the grammar is refused.

use andromeda_test_suite::{
    broker,
    namespace::Namespace,
    problem::{answered, problem},
};
use googletest::prelude::*;
use http::StatusCode;
use ngsi_ld::{
    http::{UncheckedDocument, headers::Accept, operation::CreateOrUpdateTemporalEvolution},
    model::{ErrorType, Lossless, NgsiLdMediaType, Property, RawJson, evolution::TemporalEntity},
};

/// Pins: Create Temporal Evolution of an Entity whose reified scope holds /a-b, /1st or the number 7 answers 400
/// `BadRequestData`, and no Temporal Evolution of that Entity exists
/// Clause: 4.18, 4.5.6, 5.5.4, 5.6.11.4
#[gtest]
#[tokio::test]
async fn n_tmp_ups_9014_a_reified_scope_outside_the_grammar_is_refused() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let vehicle_reified_hyphen_id = namespace.urn("Vehicle", "reified-hyphen")?;
    let vehicle_type = namespace.entity_type("Vehicle")?;
    let vehicle_reified_digit_level_id = namespace.urn("Vehicle", "reified-digit-level")?;
    let vehicle_reified_number_id = namespace.urn("Vehicle", "reified-number")?;
    let vehicle_reified_hyphen_evolution = TemporalEntity::builder()
        .id(vehicle_reified_hyphen_id.clone())
        .entity_type(vehicle_type.clone())
        .property("speed", Property::builder(80).observed_at("2020-06-01T00:00:00Z".parse()?))
        .build()?;
    // The model refuses it (invalid scope at scope), so `scope` is written as text.
    let scope: RawJson = r#"[{"type": "Property", "value": "/a-b", "observedAt": "2020-06-01T00:00:00Z"}]"#.parse()?;
    let document = UncheckedDocument::try_from(vehicle_reified_hyphen_evolution.render(Lossless::Normalized))?.set_member("scope", &scope);
    let request = broker
        .unchecked(&CreateOrUpdateTemporalEvolution::builder().entity(&vehicle_reified_hyphen_evolution).build())?
        .document(&document);
    let answer = broker.send_unchecked(request).await;
    expect_that!(&answer, problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData));
    let answer_2 = broker
        .with_responses()
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .temporal()
        .retrieve(&vehicle_reified_hyphen_id)
        .send()
        .await;
    expect_that!(&answer_2, answered(StatusCode::NOT_FOUND));
    let vehicle_reified_digit_level_evolution = TemporalEntity::builder()
        .id(vehicle_reified_digit_level_id.clone())
        .entity_type(vehicle_type.clone())
        .property("speed", Property::builder(80).observed_at("2020-06-01T00:00:00Z".parse()?))
        .build()?;
    // The model refuses it (invalid scope at scope), so `scope` is written as text.
    let scope_2: RawJson = r#"[{"type": "Property", "value": "/1st", "observedAt": "2020-06-01T00:00:00Z"}]"#.parse()?;
    let document_2 = UncheckedDocument::try_from(vehicle_reified_digit_level_evolution.render(Lossless::Normalized))?.set_member("scope", &scope_2);
    let request_2 = broker
        .unchecked(
            &CreateOrUpdateTemporalEvolution::builder()
                .entity(&vehicle_reified_digit_level_evolution)
                .build(),
        )?
        .document(&document_2);
    let answer_3 = broker.send_unchecked(request_2).await;
    expect_that!(&answer_3, problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData));
    let answer_4 = broker
        .with_responses()
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .temporal()
        .retrieve(&vehicle_reified_digit_level_id)
        .send()
        .await;
    expect_that!(&answer_4, answered(StatusCode::NOT_FOUND));
    let vehicle_reified_number_evolution = TemporalEntity::builder()
        .id(vehicle_reified_number_id.clone())
        .entity_type(vehicle_type.clone())
        .property("speed", Property::builder(80).observed_at("2020-06-01T00:00:00Z".parse()?))
        .build()?;
    // The model refuses it (`scope` must be a string at scope), so `scope` is written as text.
    let scope_3: RawJson = r#"[{"type": "Property", "value": 7, "observedAt": "2020-06-01T00:00:00Z"}]"#.parse()?;
    let document_3 = UncheckedDocument::try_from(vehicle_reified_number_evolution.render(Lossless::Normalized))?.set_member("scope", &scope_3);
    let request_3 = broker
        .unchecked(&CreateOrUpdateTemporalEvolution::builder().entity(&vehicle_reified_number_evolution).build())?
        .document(&document_3);
    let answer_5 = broker.send_unchecked(request_3).await;
    expect_that!(&answer_5, problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData));
    let answer_6 = broker
        .with_responses()
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .temporal()
        .retrieve(&vehicle_reified_number_id)
        .send()
        .await;
    expect_that!(&answer_6, answered(StatusCode::NOT_FOUND));
    Ok(())
}
