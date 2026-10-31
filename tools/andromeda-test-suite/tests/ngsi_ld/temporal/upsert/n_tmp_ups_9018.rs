//! A malformed simplified attribute is refused.

use andromeda_test_suite::{
    broker,
    namespace::Namespace,
    problem::{answered, problem},
};
use googletest::prelude::*;
use http::StatusCode;
use ngsi_ld::{
    http::{UncheckedDocument, headers::Accept, operation::CreateOrUpdateTemporalEvolution},
    model::{ErrorType, Lossless, NgsiLdMediaType, RawJson, evolution::TemporalEntity},
};

/// Pins: Create Temporal Evolution of an Entity whose simplified speed carries a member beside values, an instance
/// object, a three-element pair, a numeric instant or an unwrapped language map answers 400 `BadRequestData`, and
/// no Temporal Evolution of that Entity exists
/// Clause: 4.5.9, 5.2.20, 5.6.11.3
#[gtest]
#[tokio::test]
async fn n_tmp_ups_9018_a_malformed_simplified_attribute_is_refused() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let simplified_probe_member_beside_id = namespace.urn("SimplifiedProbe", "member-beside")?;
    let simplified_probe_type = namespace.entity_type("SimplifiedProbe")?;
    let simplified_probe_instance_element_id = namespace.urn("SimplifiedProbe", "instance-element")?;
    let simplified_probe_three_elements_id = namespace.urn("SimplifiedProbe", "three-elements")?;
    let simplified_probe_instant_not_a_string_id = namespace.urn("SimplifiedProbe", "instant-not-a-string")?;
    let simplified_probe_unwrapped_language_map_id = namespace.urn("SimplifiedProbe", "unwrapped-language-map")?;
    let simplified_probe_member_beside_evolution = TemporalEntity::builder()
        .id(simplified_probe_member_beside_id.clone())
        .entity_type(simplified_probe_type.clone())
        .build()?;
    // The model refuses it (expected a JSON object at speed.values), so `speed` is written as text.
    let speed: RawJson = r#"{"type": "Property", "unitCode": "KMH", "values": [[55, "2022-08-09T18:25:02Z"]]}"#.parse()?;
    let document = UncheckedDocument::try_from(simplified_probe_member_beside_evolution.render(Lossless::Normalized))?.set_member("speed", &speed);
    let request = broker
        .unchecked(
            &CreateOrUpdateTemporalEvolution::builder()
                .entity(&simplified_probe_member_beside_evolution)
                .build(),
        )?
        .document(&document);
    let answer = broker.send_unchecked(request).await;
    expect_that!(&answer, problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData));
    let answer_2 = broker
        .with_responses()
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .temporal()
        .retrieve(&simplified_probe_member_beside_id)
        .send()
        .await;
    expect_that!(&answer_2, answered(StatusCode::NOT_FOUND));
    let simplified_probe_instance_element_evolution = TemporalEntity::builder()
        .id(simplified_probe_instance_element_id.clone())
        .entity_type(simplified_probe_type.clone())
        .build()?;
    // The model refuses it (an attribute in normalized representation needs a `type` member at speed.values), so `speed` is written as text.
    let speed_2: RawJson = r#"{"type": "Property", "values": [{"value": 55, "observedAt": "2022-08-09T18:25:02Z"}]}"#.parse()?;
    let document_2 = UncheckedDocument::try_from(simplified_probe_instance_element_evolution.render(Lossless::Normalized))?.set_member("speed", &speed_2);
    let request_2 = broker
        .unchecked(
            &CreateOrUpdateTemporalEvolution::builder()
                .entity(&simplified_probe_instance_element_evolution)
                .build(),
        )?
        .document(&document_2);
    let answer_3 = broker.send_unchecked(request_2).await;
    expect_that!(&answer_3, problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData));
    let answer_4 = broker
        .with_responses()
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .temporal()
        .retrieve(&simplified_probe_instance_element_id)
        .send()
        .await;
    expect_that!(&answer_4, answered(StatusCode::NOT_FOUND));
    let simplified_probe_three_elements_evolution = TemporalEntity::builder()
        .id(simplified_probe_three_elements_id.clone())
        .entity_type(simplified_probe_type.clone())
        .build()?;
    // The model refuses it (expected a JSON object at speed.values), so `speed` is written as text.
    let speed_3: RawJson = r#"{"type": "Property", "values": [[55, "2022-08-09T18:25:02Z", "2022-08-10T18:25:02Z"]]}"#.parse()?;
    let document_3 = UncheckedDocument::try_from(simplified_probe_three_elements_evolution.render(Lossless::Normalized))?.set_member("speed", &speed_3);
    let request_3 = broker
        .unchecked(
            &CreateOrUpdateTemporalEvolution::builder()
                .entity(&simplified_probe_three_elements_evolution)
                .build(),
        )?
        .document(&document_3);
    let answer_5 = broker.send_unchecked(request_3).await;
    expect_that!(&answer_5, problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData));
    let answer_6 = broker
        .with_responses()
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .temporal()
        .retrieve(&simplified_probe_three_elements_id)
        .send()
        .await;
    expect_that!(&answer_6, answered(StatusCode::NOT_FOUND));
    let simplified_probe_instant_not_a_string_evolution = TemporalEntity::builder()
        .id(simplified_probe_instant_not_a_string_id.clone())
        .entity_type(simplified_probe_type.clone())
        .build()?;
    // The model refuses it (expected a JSON object at speed.values), so `speed` is written as text.
    let speed_4: RawJson = r#"{"type": "Property", "values": [[55, 1660069502]]}"#.parse()?;
    let document_4 = UncheckedDocument::try_from(simplified_probe_instant_not_a_string_evolution.render(Lossless::Normalized))?.set_member("speed", &speed_4);
    let request_4 = broker
        .unchecked(
            &CreateOrUpdateTemporalEvolution::builder()
                .entity(&simplified_probe_instant_not_a_string_evolution)
                .build(),
        )?
        .document(&document_4);
    let answer_7 = broker.send_unchecked(request_4).await;
    expect_that!(&answer_7, problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData));
    let answer_8 = broker
        .with_responses()
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .temporal()
        .retrieve(&simplified_probe_instant_not_a_string_id)
        .send()
        .await;
    expect_that!(&answer_8, answered(StatusCode::NOT_FOUND));
    let simplified_probe_unwrapped_language_ma_evolution = TemporalEntity::builder()
        .id(simplified_probe_unwrapped_language_map_id.clone())
        .entity_type(simplified_probe_type.clone())
        .build()?;
    // The model refuses it (expected a JSON object at speed.languageMaps), so `speed` is written as text.
    let speed_5: RawJson = r#"{"type": "LanguageProperty", "languageMaps": [[{"en": "yes"}, "2022-08-09T18:25:02Z"]]}"#.parse()?;
    let document_5 = UncheckedDocument::try_from(simplified_probe_unwrapped_language_ma_evolution.render(Lossless::Normalized))?.set_member("speed", &speed_5);
    let request_5 = broker
        .unchecked(
            &CreateOrUpdateTemporalEvolution::builder()
                .entity(&simplified_probe_unwrapped_language_ma_evolution)
                .build(),
        )?
        .document(&document_5);
    let answer_9 = broker.send_unchecked(request_5).await;
    expect_that!(&answer_9, problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData));
    let answer_10 = broker
        .with_responses()
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .temporal()
        .retrieve(&simplified_probe_unwrapped_language_map_id)
        .send()
        .await;
    expect_that!(&answer_10, answered(StatusCode::NOT_FOUND));
    Ok(())
}
