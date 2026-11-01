//! A temporal q equality includes a declared value held among several values by the data type its declaration gives
//! it.

use andromeda_test_suite::{
    broker::{self, BrokerClient},
    namespace::Namespace,
    outcome::Upserted,
    value::{xsd, xsd_typed},
};
use googletest::prelude::*;
use ngsi_ld::{
    http::headers::Accept,
    model::{AttributeDraft, DateTime, EntityId, EntityTypeName, Input, NgsiLdMediaType, Property, Value, evolution::TemporalEntity},
    query::{
        q::{self, builders::QueryBuilder},
        temporal_query,
    },
};

/// The ids, sorted, of the Temporal Evolutions of `sensor_type` the query `q` selects.
async fn selected(broker: &BrokerClient, sensor_type: &EntityTypeName, q: QueryBuilder) -> Result<Vec<String>> {
    let page = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .temporal()
        .query(temporal_query::after("2023-12-31T00:00:00Z".parse()?))
        .entity_type(sensor_type)
        .q(q)
        .send()
        .await?;
    let mut ids: Vec<String> = page.items().iter().map(|evolution| evolution.id.to_string()).collect();
    ids.sort();
    Ok(ids)
}

/// `ids` as sorted text.
fn sorted(ids: &[&EntityId]) -> Vec<String> {
    let mut ids: Vec<String> = ids.iter().map(ToString::to_string).collect();
    ids.sort();
    ids
}

/// The array of `values`.
fn array<const N: usize>(values: [Value; N]) -> Value {
    Value::Array(values.into())
}

/// The Temporal Evolution of the Sensor `id` with one instance `instance` of `name`.
async fn record(broker: &BrokerClient, id: &EntityId, sensor_type: &EntityTypeName, name: &str, instance: impl AttributeDraft<Property<Input>>) -> Result<()> {
    let evolution = TemporalEntity::builder()
        .id(id.clone())
        .entity_type(sensor_type.clone())
        .property(name, instance)
        .build()?;
    broker.temporal().create_or_update(&evolution).send().await?.created()?;
    Ok(())
}

/// Pins: over Sensors whose one `reading` instance holds `["4","7"]` declared xsd:integer by each @type, `["4","7"]`
/// declared xsd:integer by the valueType, `[4,7]`, the plain `["4","7"]`, `["true","false"]` declared xsd:boolean and
/// `["INF","NaN"]` declared xsd:double, Query Temporal Evolution of Entities with `q=reading==4` selects both declared
/// integers and the numbers, `!=4` the rest, `=="4"` only the plain strings, `==true` only the declared Booleans and
/// `>3` none; over Sensors whose one `level` instance holds the sub-attribute `raw`, `level.raw==4` selects
/// `["4","7"]` declared xsd:integer by the valueType, `level.raw=="4"` none, and `level.raw==2024-01-01T00:00:00Z`
/// selects the times declared `DateTime` by the valueType
/// Clause: 4.9, 4.5.2, 5.7.4, C.6
#[gtest]
#[tokio::test]
async fn n_tmp_qry_9092_a_temporal_q_equality_includes_a_declared_value_held_among_several_values() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let sensor_type = namespace.entity_type("Sensor")?;
    let typed_integers_id = namespace.urn("Sensor", "typed-integers")?;
    let value_type_integers_id = namespace.urn("Sensor", "value-type-integers")?;
    let numbers_id = namespace.urn("Sensor", "numbers")?;
    let strings_id = namespace.urn("Sensor", "strings")?;
    let typed_booleans_id = namespace.urn("Sensor", "typed-booleans")?;
    let non_finite_id = namespace.urn("Sensor", "non-finite")?;
    let raw_value_type_id = namespace.urn("Sensor", "raw-value-type")?;
    let raw_times_id = namespace.urn("Sensor", "raw-times")?;
    let observed = || "2024-01-01T00:00:00Z".parse();
    for (id, value) in [
        (&typed_integers_id, array([xsd_typed("integer", "4")?, xsd_typed("integer", "7")?])),
        (&numbers_id, array([Value::from(4), Value::from(7)])),
        (&strings_id, array([Value::from("4"), Value::from("7")])),
        (&typed_booleans_id, array([xsd_typed("boolean", "true")?, xsd_typed("boolean", "false")?])),
        (&non_finite_id, array([xsd_typed("double", "INF")?, xsd_typed("double", "NaN")?])),
    ] {
        record(&broker, id, &sensor_type, "reading", Property::builder(value).observed_at(observed()?)).await?;
    }
    record(
        &broker,
        &value_type_integers_id,
        &sensor_type,
        "reading",
        Property::builder(array([Value::from("4"), Value::from("7")]))
            .value_type(xsd("integer")?)
            .observed_at(observed()?),
    )
    .await?;
    record(
        &broker,
        &raw_value_type_id,
        &sensor_type,
        "level",
        Property::builder(1).observed_at(observed()?).property(
            "raw",
            Property::builder(array([Value::from("4"), Value::from("7")])).value_type(xsd("integer")?),
        ),
    )
    .await?;
    record(
        &broker,
        &raw_times_id,
        &sensor_type,
        "level",
        Property::builder(1).observed_at(observed()?).property(
            "raw",
            Property::builder(array([Value::from("2024-01-01T00:00:00Z"), Value::from("2024-06-01T00:00:00Z")]))
                .value_type("https://uri.etsi.org/ngsi-ld/DateTime".parse()?),
        ),
    )
    .await?;
    let reading = || q::attr("reading");
    let fours = [&typed_integers_id, &value_type_integers_id, &numbers_id];
    expect_that!(selected(&broker, &sensor_type, reading().eq(4)).await?, eq(&sorted(&fours)));
    expect_that!(
        selected(&broker, &sensor_type, reading().ne(4)).await?,
        eq(&sorted(&[&strings_id, &typed_booleans_id, &non_finite_id]))
    );
    expect_that!(selected(&broker, &sensor_type, reading().eq("4")).await?, eq(&sorted(&[&strings_id])));
    expect_that!(selected(&broker, &sensor_type, reading().eq(true)).await?, eq(&sorted(&[&typed_booleans_id])));
    expect_that!(selected(&broker, &sensor_type, reading().gt(3)).await?, is_empty());
    let raw = || q::attr("level").sub_attr("raw");
    expect_that!(selected(&broker, &sensor_type, raw().eq(4)).await?, eq(&sorted(&[&raw_value_type_id])));
    expect_that!(selected(&broker, &sensor_type, raw().eq("4")).await?, is_empty());
    expect_that!(
        selected(&broker, &sensor_type, raw().eq("2024-01-01T00:00:00Z".parse::<DateTime>()?)).await?,
        eq(&sorted(&[&raw_times_id]))
    );
    Ok(())
}
