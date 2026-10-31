//! A temporal evolution keeps a one element json array.

use andromeda_test_suite::{
    attribute_matcher::attribute_of,
    broker,
    evolution_matcher::has_evolution,
    namespace::Namespace,
    outcome::Upserted,
    problem::answered,
    simplified_pairs::json_pairs,
    written_json::json_request_to,
};
use googletest::prelude::*;
use http::{Method, StatusCode};
use ngsi_ld::{
    http::headers::Accept,
    model::{
        AttributeKind,
        DateTime,
        JsonProperty,
        NgsiLdMediaType,
        RawJson,
        evolution::{TemporalEntity, TemporalValue},
    },
    query::temporal_query,
};
use serde_json::json;

/// Pins: A Temporal Evolution of `JsonProperty` instances holding one-element arrays, and an attribute added as
/// jsons pairs, reads back with each one-element array in its normalized instances and in its `temporalValues`
/// pairs
/// Clause: 4.5.9, 4.5.24.2, 5.6.11, 5.6.12
#[gtest]
#[tokio::test]
async fn n_tmp_add_9002_a_temporal_evolution_keeps_a_one_element_json_array() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let probe_id = namespace.urn("JsonArrayProbe", "temporal")?;
    let probe_type = namespace.entity_type("JsonArrayProbe")?;
    let first = "2022-08-09T18:25:02Z".parse::<DateTime>()?;
    let second = "2022-08-10T18:25:02Z".parse::<DateTime>()?;
    let probe = TemporalEntity::builder()
        .id(probe_id.clone())
        .entity_type(probe_type)
        .json_property("tickets", JsonProperty::builder(r#"[{"code": "a"}]"#.parse::<RawJson>()?).observed_at(first))
        .json_property("tickets", JsonProperty::builder(r#"[{"code": "b"}]"#.parse::<RawJson>()?).observed_at(second))
        .build()?;
    broker.temporal().create_or_update(&probe).send().await?.created()?;
    // The attribute is added as a `jsons` pair, which Add Attributes to Temporal Evolution does not write, so the body is sent as written.
    let appended = broker
        .send_unchecked(json_request_to(
            &broker,
            Method::POST,
            &format!("temporal/entities/{probe_id}/attrs"),
            r#"{"pairs": {"type": "JsonProperty", "jsons": [[{"json": [{"code": "p"}]}, "2022-08-09T18:25:02Z"]]}}"#,
        )?)
        .await;
    expect_that!(&appended, answered(StatusCode::NO_CONTENT));
    let json = broker.with_accept(Accept::new(NgsiLdMediaType::Json));
    let normalized = json
        .temporal()
        .retrieve(&probe_id)
        .temporal_q(temporal_query::after("2000-01-01T00:00:00Z".parse()?))
        .send()
        .await?;
    expect_that!(
        normalized.body(),
        has_evolution(
            "tickets",
            unordered_elements_are![
                attribute_of(AttributeKind::JsonProperty).observed_at(first).json(json!([{"code": "a"}])),
                attribute_of(AttributeKind::JsonProperty).observed_at(second).json(json!([{"code": "b"}])),
            ]
        )
    );
    let temporal_values = json
        .temporal()
        .retrieve_values(&probe_id)
        .temporal_q(temporal_query::after("2000-01-01T00:00:00Z".parse()?))
        .send()
        .await?;
    let values = temporal_values.body();
    let tickets = json_pairs(values, "tickets").or_fail()?;
    expect_that!(tickets, len(eq(2)));
    for (code, time) in [("a", first), ("b", second)] {
        let expected = TemporalValue {
            value: format!(r#"[{{"code": "{code}"}}]"#).parse::<RawJson>()?,
            time,
        };
        expect_that!(tickets, contains(eq(&expected)));
    }
    let pairs = json_pairs(values, "pairs").or_fail()?;
    let expected_pair = TemporalValue {
        value: r#"[{"code": "p"}]"#.parse::<RawJson>()?,
        time: first,
    };
    expect_that!(pairs, elements_are![eq(&expected_pair)]);
    Ok(())
}
