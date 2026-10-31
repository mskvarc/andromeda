//! A cut before page starts at its least recent instance.

use andromeda_test_suite::{broker, namespace::Namespace, outcome::Upserted};
use googletest::prelude::*;
use http::StatusCode;
use ngsi_ld::{
    http::headers::{Accept, ContentRange},
    model::{NgsiLdMediaType, Property, Value, evolution::TemporalEntity},
    query::temporal_query,
};

/// Pins: Retrieve Temporal Evolution of an Entity with timerel before a `timeAt` long after its 25 instances
/// answers 206 with a Content-Range whose range-start is the least recent timestamp in the answer, not the `timeAt`
/// Clause: 6.3.10, 5.7.3
/// Recorded: the base stack caps a temporal answer at 19 instances per attribute (clause 6.3.10 leaves the limit to
/// the implementation), so 25 instances answer a truncated page
#[gtest]
#[tokio::test]
async fn n_tmp_ret_9004_a_cut_before_page_starts_at_its_least_recent_instance() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let vehicle_range_before_id = namespace.urn("Vehicle", "range-before")?;
    let vehicle_type = namespace.entity_type("Vehicle")?;
    let vehicle_range_before_evolution = TemporalEntity::builder()
        .id(vehicle_range_before_id.clone())
        .entity_type(vehicle_type)
        .property(
            "speed",
            Property::builder(Value::Number("0.0".parse()?)).observed_at("2020-01-01T00:00:00Z".parse()?),
        )
        .property(
            "speed",
            Property::builder(Value::Number("1.0".parse()?)).observed_at("2020-01-01T00:01:00Z".parse()?),
        )
        .property(
            "speed",
            Property::builder(Value::Number("2.0".parse()?)).observed_at("2020-01-01T00:02:00Z".parse()?),
        )
        .property(
            "speed",
            Property::builder(Value::Number("3.0".parse()?)).observed_at("2020-01-01T00:03:00Z".parse()?),
        )
        .property(
            "speed",
            Property::builder(Value::Number("4.0".parse()?)).observed_at("2020-01-01T00:04:00Z".parse()?),
        )
        .property(
            "speed",
            Property::builder(Value::Number("5.0".parse()?)).observed_at("2020-01-01T00:05:00Z".parse()?),
        )
        .property(
            "speed",
            Property::builder(Value::Number("6.0".parse()?)).observed_at("2020-01-01T00:06:00Z".parse()?),
        )
        .property(
            "speed",
            Property::builder(Value::Number("7.0".parse()?)).observed_at("2020-01-01T00:07:00Z".parse()?),
        )
        .property(
            "speed",
            Property::builder(Value::Number("8.0".parse()?)).observed_at("2020-01-01T00:08:00Z".parse()?),
        )
        .property(
            "speed",
            Property::builder(Value::Number("9.0".parse()?)).observed_at("2020-01-01T00:09:00Z".parse()?),
        )
        .property(
            "speed",
            Property::builder(Value::Number("10.0".parse()?)).observed_at("2020-01-01T00:10:00Z".parse()?),
        )
        .property(
            "speed",
            Property::builder(Value::Number("11.0".parse()?)).observed_at("2020-01-01T00:11:00Z".parse()?),
        )
        .property(
            "speed",
            Property::builder(Value::Number("12.0".parse()?)).observed_at("2020-01-01T00:12:00Z".parse()?),
        )
        .property(
            "speed",
            Property::builder(Value::Number("13.0".parse()?)).observed_at("2020-01-01T00:13:00Z".parse()?),
        )
        .property(
            "speed",
            Property::builder(Value::Number("14.0".parse()?)).observed_at("2020-01-01T00:14:00Z".parse()?),
        )
        .property(
            "speed",
            Property::builder(Value::Number("15.0".parse()?)).observed_at("2020-01-01T00:15:00Z".parse()?),
        )
        .property(
            "speed",
            Property::builder(Value::Number("16.0".parse()?)).observed_at("2020-01-01T00:16:00Z".parse()?),
        )
        .property(
            "speed",
            Property::builder(Value::Number("17.0".parse()?)).observed_at("2020-01-01T00:17:00Z".parse()?),
        )
        .property(
            "speed",
            Property::builder(Value::Number("18.0".parse()?)).observed_at("2020-01-01T00:18:00Z".parse()?),
        )
        .property(
            "speed",
            Property::builder(Value::Number("19.0".parse()?)).observed_at("2020-01-01T00:19:00Z".parse()?),
        )
        .property(
            "speed",
            Property::builder(Value::Number("20.0".parse()?)).observed_at("2020-01-01T00:20:00Z".parse()?),
        )
        .property(
            "speed",
            Property::builder(Value::Number("21.0".parse()?)).observed_at("2020-01-01T00:21:00Z".parse()?),
        )
        .property(
            "speed",
            Property::builder(Value::Number("22.0".parse()?)).observed_at("2020-01-01T00:22:00Z".parse()?),
        )
        .property(
            "speed",
            Property::builder(Value::Number("23.0".parse()?)).observed_at("2020-01-01T00:23:00Z".parse()?),
        )
        .property(
            "speed",
            Property::builder(Value::Number("24.0".parse()?)).observed_at("2020-01-01T00:24:00Z".parse()?),
        )
        .build()?;
    broker.temporal().create_or_update(&vehicle_range_before_evolution).send().await?.created()?;
    let headed = broker
        .with_heads()
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .temporal()
        .retrieve(&vehicle_range_before_id)
        .temporal_q(temporal_query::before("2021-01-01T00:00:00Z".parse()?))
        .send()
        .await?;
    expect_that!(headed.status, eq(StatusCode::PARTIAL_CONTENT));
    expect_that!(
        headed.head.typed::<ContentRange>().valid().map(|range| range.start().to_string()),
        some(starts_with("2020-01-01T00:"))
    );
    Ok(())
}
