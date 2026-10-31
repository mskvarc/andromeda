//! A json property array stays a json literal.

use andromeda_test_suite::{
    attribute_matcher::attribute_of,
    broker,
    entity_matcher::has_attribute,
    evolution_matcher::has_evolution,
    namespace::Namespace,
    nth::nth,
    outcome::Upserted,
    simplified_pairs::json_pairs,
};
use googletest::prelude::*;
use ngsi_ld::{
    http::headers::Accept,
    model::{AttributeKind, JsonProperty, NgsiLdMediaType, RawJson, evolution::TemporalEntity},
    query::temporal_query,
};
use serde_json::json;

/// Pins: A `JsonProperty` whose json is `[{"a": 1}, {"b": 2}]` reads back as a `JsonProperty` with that json and no value from
/// Retrieve Temporal Evolution, as `{"json": ...}` in the temporalValues pair, and as that json in the current
/// representation
/// Clause: 5.2.38, 4.5.24, 4.5.9, 5.7.3.4
#[gtest]
#[tokio::test]
async fn n_tmp_trl_9008_a_json_property_array_stays_a_json_literal() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let array_probe_id = namespace.urn("ArrayProbe", 1)?;
    let array_probe_type = namespace.entity_type("ArrayProbe")?;
    let document = r#"[{"a": 1}, {"b": 2}]"#.parse::<RawJson>()?;
    let array_probe_evolution = TemporalEntity::builder()
        .id(array_probe_id.clone())
        .entity_type(array_probe_type)
        .json_property("document", JsonProperty::builder(document.clone()).observed_at("2024-01-01T00:00:00Z".parse()?))
        .build()?;
    broker.temporal().create_or_update(&array_probe_evolution).send().await?.created()?;
    let retrieved = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .temporal()
        .retrieve(&array_probe_id)
        .temporal_q(temporal_query::after("2000-01-01T00:00:00Z".parse()?))
        .send()
        .await?;
    expect_that!(
        retrieved.body(),
        has_evolution("document", nth(0, attribute_of(AttributeKind::JsonProperty).json(json!([{"a": 1}, {"b": 2}]))))
    );
    let retrieved_2 = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .temporal()
        .retrieve_values(&array_probe_id)
        .temporal_q(temporal_query::after("2000-01-01T00:00:00Z".parse()?))
        .send()
        .await?;
    let pairs = json_pairs(retrieved_2.body(), "document").or_fail()?;
    expect_that!(pairs, len(eq(1)));
    expect_that!(pairs.first().map(|pair| &pair.value), some(eq(&document)));
    let retrieved_3 = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .retrieve(&array_probe_id)
        .send()
        .await?;
    expect_that!(
        retrieved_3.body(),
        has_attribute("document", attribute_of(AttributeKind::JsonProperty).json(json!([{"a": 1}, {"b": 2}])))
    );
    Ok(())
}
