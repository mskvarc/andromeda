//! A content range and a next link travel together.

use andromeda_test_suite::{broker, namespace::Namespace, outcome::Upserted};
use googletest::prelude::*;
use http::StatusCode;
use ngsi_ld::{
    http::headers::{Accept, ContentRange, ContentRangeSize},
    model::{NgsiLdMediaType, Property, Value, evolution::TemporalEntity},
    query::temporal_query,
};

/// Pins: Query Temporal Evolution of Entities with `lastN` 25 and limit 1 over two Entities of 25 instances each
/// answers 206 with a Content-Range in the `DateTime` unit of size 25, one Entity and a next Link
/// Clause: 4.12, 6.3.10, 5.7.4
/// Recorded: the base stack caps a temporal answer at 19 instances per attribute (clause 6.3.10 leaves the limit to
/// the implementation), so 25 instances answer a truncated page
#[gtest]
#[tokio::test]
async fn n_tmp_qry_9027_a_content_range_and_a_next_link_travel_together() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let vehicle_range_1_id = namespace.urn("Vehicle", "range-1")?;
    let vehicle_type = namespace.entity_type("Vehicle")?;
    let vehicle_range_2_id = namespace.urn("Vehicle", "range-2")?;
    let vehicle_range_1_evolution = TemporalEntity::builder()
        .id(vehicle_range_1_id.clone())
        .entity_type(vehicle_type.clone())
        .property(
            "speed",
            Property::builder(Value::Number("0.0".parse()?)).observed_at("2024-01-01T00:00:00Z".parse()?),
        )
        .property(
            "speed",
            Property::builder(Value::Number("1.0".parse()?)).observed_at("2024-01-01T00:01:00Z".parse()?),
        )
        .property(
            "speed",
            Property::builder(Value::Number("2.0".parse()?)).observed_at("2024-01-01T00:02:00Z".parse()?),
        )
        .property(
            "speed",
            Property::builder(Value::Number("3.0".parse()?)).observed_at("2024-01-01T00:03:00Z".parse()?),
        )
        .property(
            "speed",
            Property::builder(Value::Number("4.0".parse()?)).observed_at("2024-01-01T00:04:00Z".parse()?),
        )
        .property(
            "speed",
            Property::builder(Value::Number("5.0".parse()?)).observed_at("2024-01-01T00:05:00Z".parse()?),
        )
        .property(
            "speed",
            Property::builder(Value::Number("6.0".parse()?)).observed_at("2024-01-01T00:06:00Z".parse()?),
        )
        .property(
            "speed",
            Property::builder(Value::Number("7.0".parse()?)).observed_at("2024-01-01T00:07:00Z".parse()?),
        )
        .property(
            "speed",
            Property::builder(Value::Number("8.0".parse()?)).observed_at("2024-01-01T00:08:00Z".parse()?),
        )
        .property(
            "speed",
            Property::builder(Value::Number("9.0".parse()?)).observed_at("2024-01-01T00:09:00Z".parse()?),
        )
        .property(
            "speed",
            Property::builder(Value::Number("10.0".parse()?)).observed_at("2024-01-01T00:10:00Z".parse()?),
        )
        .property(
            "speed",
            Property::builder(Value::Number("11.0".parse()?)).observed_at("2024-01-01T00:11:00Z".parse()?),
        )
        .property(
            "speed",
            Property::builder(Value::Number("12.0".parse()?)).observed_at("2024-01-01T00:12:00Z".parse()?),
        )
        .property(
            "speed",
            Property::builder(Value::Number("13.0".parse()?)).observed_at("2024-01-01T00:13:00Z".parse()?),
        )
        .property(
            "speed",
            Property::builder(Value::Number("14.0".parse()?)).observed_at("2024-01-01T00:14:00Z".parse()?),
        )
        .property(
            "speed",
            Property::builder(Value::Number("15.0".parse()?)).observed_at("2024-01-01T00:15:00Z".parse()?),
        )
        .property(
            "speed",
            Property::builder(Value::Number("16.0".parse()?)).observed_at("2024-01-01T00:16:00Z".parse()?),
        )
        .property(
            "speed",
            Property::builder(Value::Number("17.0".parse()?)).observed_at("2024-01-01T00:17:00Z".parse()?),
        )
        .property(
            "speed",
            Property::builder(Value::Number("18.0".parse()?)).observed_at("2024-01-01T00:18:00Z".parse()?),
        )
        .property(
            "speed",
            Property::builder(Value::Number("19.0".parse()?)).observed_at("2024-01-01T00:19:00Z".parse()?),
        )
        .property(
            "speed",
            Property::builder(Value::Number("20.0".parse()?)).observed_at("2024-01-01T00:20:00Z".parse()?),
        )
        .property(
            "speed",
            Property::builder(Value::Number("21.0".parse()?)).observed_at("2024-01-01T00:21:00Z".parse()?),
        )
        .property(
            "speed",
            Property::builder(Value::Number("22.0".parse()?)).observed_at("2024-01-01T00:22:00Z".parse()?),
        )
        .property(
            "speed",
            Property::builder(Value::Number("23.0".parse()?)).observed_at("2024-01-01T00:23:00Z".parse()?),
        )
        .property(
            "speed",
            Property::builder(Value::Number("24.0".parse()?)).observed_at("2024-01-01T00:24:00Z".parse()?),
        )
        .build()?;
    broker.temporal().create_or_update(&vehicle_range_1_evolution).send().await?.created()?;
    let vehicle_range_2_evolution = TemporalEntity::builder()
        .id(vehicle_range_2_id.clone())
        .entity_type(vehicle_type.clone())
        .property(
            "speed",
            Property::builder(Value::Number("0.0".parse()?)).observed_at("2024-01-01T00:00:00Z".parse()?),
        )
        .property(
            "speed",
            Property::builder(Value::Number("1.0".parse()?)).observed_at("2024-01-01T00:01:00Z".parse()?),
        )
        .property(
            "speed",
            Property::builder(Value::Number("2.0".parse()?)).observed_at("2024-01-01T00:02:00Z".parse()?),
        )
        .property(
            "speed",
            Property::builder(Value::Number("3.0".parse()?)).observed_at("2024-01-01T00:03:00Z".parse()?),
        )
        .property(
            "speed",
            Property::builder(Value::Number("4.0".parse()?)).observed_at("2024-01-01T00:04:00Z".parse()?),
        )
        .property(
            "speed",
            Property::builder(Value::Number("5.0".parse()?)).observed_at("2024-01-01T00:05:00Z".parse()?),
        )
        .property(
            "speed",
            Property::builder(Value::Number("6.0".parse()?)).observed_at("2024-01-01T00:06:00Z".parse()?),
        )
        .property(
            "speed",
            Property::builder(Value::Number("7.0".parse()?)).observed_at("2024-01-01T00:07:00Z".parse()?),
        )
        .property(
            "speed",
            Property::builder(Value::Number("8.0".parse()?)).observed_at("2024-01-01T00:08:00Z".parse()?),
        )
        .property(
            "speed",
            Property::builder(Value::Number("9.0".parse()?)).observed_at("2024-01-01T00:09:00Z".parse()?),
        )
        .property(
            "speed",
            Property::builder(Value::Number("10.0".parse()?)).observed_at("2024-01-01T00:10:00Z".parse()?),
        )
        .property(
            "speed",
            Property::builder(Value::Number("11.0".parse()?)).observed_at("2024-01-01T00:11:00Z".parse()?),
        )
        .property(
            "speed",
            Property::builder(Value::Number("12.0".parse()?)).observed_at("2024-01-01T00:12:00Z".parse()?),
        )
        .property(
            "speed",
            Property::builder(Value::Number("13.0".parse()?)).observed_at("2024-01-01T00:13:00Z".parse()?),
        )
        .property(
            "speed",
            Property::builder(Value::Number("14.0".parse()?)).observed_at("2024-01-01T00:14:00Z".parse()?),
        )
        .property(
            "speed",
            Property::builder(Value::Number("15.0".parse()?)).observed_at("2024-01-01T00:15:00Z".parse()?),
        )
        .property(
            "speed",
            Property::builder(Value::Number("16.0".parse()?)).observed_at("2024-01-01T00:16:00Z".parse()?),
        )
        .property(
            "speed",
            Property::builder(Value::Number("17.0".parse()?)).observed_at("2024-01-01T00:17:00Z".parse()?),
        )
        .property(
            "speed",
            Property::builder(Value::Number("18.0".parse()?)).observed_at("2024-01-01T00:18:00Z".parse()?),
        )
        .property(
            "speed",
            Property::builder(Value::Number("19.0".parse()?)).observed_at("2024-01-01T00:19:00Z".parse()?),
        )
        .property(
            "speed",
            Property::builder(Value::Number("20.0".parse()?)).observed_at("2024-01-01T00:20:00Z".parse()?),
        )
        .property(
            "speed",
            Property::builder(Value::Number("21.0".parse()?)).observed_at("2024-01-01T00:21:00Z".parse()?),
        )
        .property(
            "speed",
            Property::builder(Value::Number("22.0".parse()?)).observed_at("2024-01-01T00:22:00Z".parse()?),
        )
        .property(
            "speed",
            Property::builder(Value::Number("23.0".parse()?)).observed_at("2024-01-01T00:23:00Z".parse()?),
        )
        .property(
            "speed",
            Property::builder(Value::Number("24.0".parse()?)).observed_at("2024-01-01T00:24:00Z".parse()?),
        )
        .build()?;
    broker.temporal().create_or_update(&vehicle_range_2_evolution).send().await?.created()?;
    let headed = broker
        .with_heads()
        .with_accept(Accept::new(NgsiLdMediaType::JsonLd))
        .temporal()
        .query(temporal_query::after("2023-12-31T00:00:00Z".parse()?).last_n(25))
        .entity_type(&vehicle_type)
        .ids(vec![vehicle_range_1_id.clone(), vehicle_range_2_id.clone()])
        .limit(1)
        .offset(0)
        .send()
        .await?;
    expect_that!(headed.status, eq(StatusCode::PARTIAL_CONTENT));
    let page = headed.outcome;
    expect_that!(headed.head.typed::<ContentRange>().valid(), some(anything()));
    expect_that!(
        headed.head.typed::<ContentRange>().valid().map(ContentRange::size),
        some(eq(ContentRangeSize::LastN(25.try_into()?)))
    );
    expect_that!(page.next(), some(anything()));
    expect_that!(page.items(), len(eq(1)));
    Ok(())
}
