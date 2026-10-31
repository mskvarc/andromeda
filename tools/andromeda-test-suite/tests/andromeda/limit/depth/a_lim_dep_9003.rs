//! Every shape past the limit is refused by a temporal create.

use andromeda_test_suite::{
    broker,
    namespace::Namespace,
    problem::{answered, problem},
    value::object,
};
use googletest::prelude::*;
use http::StatusCode;
use ngsi_ld::{
    http::headers::Accept,
    model::{ErrorType, JsonProperty, ListProperty, NgsiLdMediaType, Property, RawJson, Value, evolution::TemporalEntity},
};

/// Pins: Create Temporal Evolution of an observation of p nesting 33 levels, in each of the five shapes, answers
/// 400 `BadRequestData` naming p and the 32-level limit, and records nothing
/// Clause: 5.6.11.4, 5.5.4
#[gtest]
#[tokio::test]
async fn a_lim_dep_9003_every_shape_past_the_limit_is_refused_by_a_temporal_create() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let nesting_probe_object_chain_id = namespace.urn("NestingProbe", "object-chain")?;
    let nesting_probe_type = namespace.entity_type("NestingProbe")?;
    let nesting_probe_list_chain_id = namespace.urn("NestingProbe", "list-chain")?;
    let nesting_probe_json_literal_id = namespace.urn("NestingProbe", "json-literal")?;
    let nesting_probe_sub_attribute_value_id = namespace.urn("NestingProbe", "sub-attribute-value")?;
    let nesting_probe_sub_attribute_chain_id = namespace.urn("NestingProbe", "sub-attribute-chain")?;
    let nesting_probe_object_chain_evolution = TemporalEntity::builder()
        .id(nesting_probe_object_chain_id.clone())
        .entity_type(nesting_probe_type.clone())
        .property(
            "p",
            Property::builder((0..33).fold(Value::from("leaf"), |inner, _| object([("k", inner)]))).observed_at("2024-01-01T00:00:00Z".parse()?),
        )
        .build()?;
    let answer = broker
        .with_responses()
        .temporal()
        .create_or_update(&nesting_probe_object_chain_evolution)
        .send()
        .await;
    expect_that!(
        &answer,
        problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData)
            .with_detail_containing("'https://uri.etsi.org/ngsi-ld/default-context/p'")
            .with_detail_containing("32 levels")
    );
    let answer_2 = broker
        .with_responses()
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .temporal()
        .retrieve(&nesting_probe_object_chain_id)
        .send()
        .await;
    expect_that!(&answer_2, answered(StatusCode::NOT_FOUND));
    let nesting_probe_list_chain_evolution = TemporalEntity::builder()
        .id(nesting_probe_list_chain_id.clone())
        .entity_type(nesting_probe_type.clone())
        .list_property(
            "p",
            ListProperty::builder(vec![(0..32).fold(Value::from(1), |inner, _| Value::Array(vec![inner]))]).observed_at("2024-01-01T00:00:00Z".parse()?),
        )
        .build()?;
    let answer_3 = broker
        .with_responses()
        .temporal()
        .create_or_update(&nesting_probe_list_chain_evolution)
        .send()
        .await;
    expect_that!(
        &answer_3,
        problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData)
            .with_detail_containing("'https://uri.etsi.org/ngsi-ld/default-context/p'")
            .with_detail_containing("32 levels")
    );
    let answer_4 = broker
        .with_responses()
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .temporal()
        .retrieve(&nesting_probe_list_chain_id)
        .send()
        .await;
    expect_that!(&answer_4, answered(StatusCode::NOT_FOUND));
    let nesting_probe_json_literal_evolution = TemporalEntity::builder()
        .id(nesting_probe_json_literal_id.clone())
        .entity_type(nesting_probe_type.clone())
        .json_property("p", JsonProperty::builder(r#"{"k":{"k":{"k":{"k":{"k":{"k":{"k":{"k":{"k":{"k":{"k":{"k":{"k":{"k":{"k":{"k":{"k":{"k":{"k":{"k":{"k":{"k":{"k":{"k":{"k":{"k":{"k":{"k":{"k":{"k":{"k":{"k":{"k":"leaf"}}}}}}}}}}}}}}}}}}}}}}}}}}}}}}}}}"#.parse::<RawJson>()?).observed_at("2024-01-01T00:00:00Z".parse()?))
        .build()?;
    let answer_5 = broker
        .with_responses()
        .temporal()
        .create_or_update(&nesting_probe_json_literal_evolution)
        .send()
        .await;
    expect_that!(
        &answer_5,
        problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData)
            .with_detail_containing("'https://uri.etsi.org/ngsi-ld/default-context/p'")
            .with_detail_containing("32 levels")
    );
    let answer_6 = broker
        .with_responses()
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .temporal()
        .retrieve(&nesting_probe_json_literal_id)
        .send()
        .await;
    expect_that!(&answer_6, answered(StatusCode::NOT_FOUND));
    let nesting_probe_sub_attribute_value_evolution = TemporalEntity::builder()
        .id(nesting_probe_sub_attribute_value_id.clone())
        .entity_type(nesting_probe_type.clone())
        .property(
            "p",
            Property::builder(1)
                .observed_at("2024-01-01T00:00:00Z".parse()?)
                .property("s", (0..32).fold(Value::from("leaf"), |inner, _| object([("k", inner)]))),
        )
        .build()?;
    let answer_7 = broker
        .with_responses()
        .temporal()
        .create_or_update(&nesting_probe_sub_attribute_value_evolution)
        .send()
        .await;
    expect_that!(
        &answer_7,
        problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData)
            .with_detail_containing("'https://uri.etsi.org/ngsi-ld/default-context/p'")
            .with_detail_containing("32 levels")
    );
    let answer_8 = broker
        .with_responses()
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .temporal()
        .retrieve(&nesting_probe_sub_attribute_value_id)
        .send()
        .await;
    expect_that!(&answer_8, answered(StatusCode::NOT_FOUND));
    let s_attribute = Property::builder(1).property("s", 1);
    let s_attribute_2 = Property::builder(1).property("s", s_attribute);
    let s_attribute_3 = Property::builder(1).property("s", s_attribute_2);
    let s_attribute_4 = Property::builder(1).property("s", s_attribute_3);
    let s_attribute_5 = Property::builder(1).property("s", s_attribute_4);
    let s_attribute_6 = Property::builder(1).property("s", s_attribute_5);
    let s_attribute_7 = Property::builder(1).property("s", s_attribute_6);
    let s_attribute_8 = Property::builder(1).property("s", s_attribute_7);
    let s_attribute_9 = Property::builder(1).property("s", s_attribute_8);
    let s_attribute_10 = Property::builder(1).property("s", s_attribute_9);
    let s_attribute_11 = Property::builder(1).property("s", s_attribute_10);
    let s_attribute_12 = Property::builder(1).property("s", s_attribute_11);
    let s_attribute_13 = Property::builder(1).property("s", s_attribute_12);
    let s_attribute_14 = Property::builder(1).property("s", s_attribute_13);
    let s_attribute_15 = Property::builder(1).property("s", s_attribute_14);
    let s_attribute_16 = Property::builder(1).property("s", s_attribute_15);
    let s_attribute_17 = Property::builder(1).property("s", s_attribute_16);
    let s_attribute_18 = Property::builder(1).property("s", s_attribute_17);
    let s_attribute_19 = Property::builder(1).property("s", s_attribute_18);
    let s_attribute_20 = Property::builder(1).property("s", s_attribute_19);
    let s_attribute_21 = Property::builder(1).property("s", s_attribute_20);
    let s_attribute_22 = Property::builder(1).property("s", s_attribute_21);
    let s_attribute_23 = Property::builder(1).property("s", s_attribute_22);
    let s_attribute_24 = Property::builder(1).property("s", s_attribute_23);
    let s_attribute_25 = Property::builder(1).property("s", s_attribute_24);
    let s_attribute_26 = Property::builder(1).property("s", s_attribute_25);
    let s_attribute_27 = Property::builder(1).property("s", s_attribute_26);
    let s_attribute_28 = Property::builder(1).property("s", s_attribute_27);
    let s_attribute_29 = Property::builder(1).property("s", s_attribute_28);
    let s_attribute_30 = Property::builder(1).property("s", s_attribute_29);
    let s_attribute_31 = Property::builder(1).property("s", s_attribute_30);
    let s_attribute_32 = Property::builder(1).property("s", s_attribute_31);
    let nesting_probe_sub_attribute_chain_evolution = TemporalEntity::builder()
        .id(nesting_probe_sub_attribute_chain_id.clone())
        .entity_type(nesting_probe_type.clone())
        .property(
            "p",
            Property::builder(1).observed_at("2024-01-01T00:00:00Z".parse()?).property("s", s_attribute_32),
        )
        .build()?;
    let answer_9 = broker
        .with_responses()
        .temporal()
        .create_or_update(&nesting_probe_sub_attribute_chain_evolution)
        .send()
        .await;
    expect_that!(
        &answer_9,
        problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData)
            .with_detail_containing("'https://uri.etsi.org/ngsi-ld/default-context/p'")
            .with_detail_containing("32 levels")
    );
    let answer_10 = broker
        .with_responses()
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .temporal()
        .retrieve(&nesting_probe_sub_attribute_chain_id)
        .send()
        .await;
    expect_that!(&answer_10, answered(StatusCode::NOT_FOUND));
    Ok(())
}
