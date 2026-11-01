//! A temporal q comparison reads a value's data type from its declared datatype.

use andromeda_test_suite::{
    broker::{self, BrokerClient},
    namespace::Namespace,
    outcome::Upserted,
    value::{xsd, xsd_typed},
};
use googletest::prelude::*;
use ngsi_ld::{
    http::headers::Accept,
    model::{AttributeDraft, EntityId, EntityTypeName, Input, NgsiLdMediaType, Property, Value, evolution::TemporalEntity},
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

/// The Temporal Evolution of the Sensor `id` with one `reading` instance `reading`.
async fn record(broker: &BrokerClient, id: &EntityId, sensor_type: &EntityTypeName, reading: impl AttributeDraft<Property<Input>>) -> Result<()> {
    let evolution = TemporalEntity::builder()
        .id(id.clone())
        .entity_type(sensor_type.clone())
        .property("reading", reading)
        .build()?;
    broker.temporal().create_or_update(&evolution).send().await?.created()?;
    Ok(())
}

/// Pins: over six Sensors whose one `reading` instance is the JSON string "4", the JSON number 4, "4" declared
/// xsd:integer by its @type, "4" declared xsd:integer by the valueType, and "INF" and "NaN" declared xsd:double,
/// Query Temporal Evolution of Entities with `q=reading==4` selects the number and both declared integers,
/// `=="4"` only the string, `!=4` the string and both non-finite values, `>3` the number, both declared integers
/// and INF, and `<5` the number and both declared integers
/// Clause: 4.9, 5.7.4, C.6
#[gtest]
#[tokio::test]
async fn n_tmp_qry_9091_a_temporal_q_comparison_reads_the_declared_datatype() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let sensor_type = namespace.entity_type("Sensor")?;
    let string_id = namespace.urn("Sensor", "string")?;
    let number_id = namespace.urn("Sensor", "number")?;
    let typed_integer_id = namespace.urn("Sensor", "typed-integer")?;
    let value_type_integer_id = namespace.urn("Sensor", "value-type-integer")?;
    let infinity_id = namespace.urn("Sensor", "infinity")?;
    let not_a_number_id = namespace.urn("Sensor", "not-a-number")?;
    let observed = || "2024-01-01T00:00:00Z".parse();
    record(&broker, &string_id, &sensor_type, Property::builder(Value::from("4")).observed_at(observed()?)).await?;
    record(&broker, &number_id, &sensor_type, Property::builder(Value::from(4)).observed_at(observed()?)).await?;
    record(
        &broker,
        &typed_integer_id,
        &sensor_type,
        Property::builder(xsd_typed("integer", "4")?).observed_at(observed()?),
    )
    .await?;
    record(
        &broker,
        &value_type_integer_id,
        &sensor_type,
        Property::builder(Value::from("4")).value_type(xsd("integer")?).observed_at(observed()?),
    )
    .await?;
    record(
        &broker,
        &infinity_id,
        &sensor_type,
        Property::builder(xsd_typed("double", "INF")?).observed_at(observed()?),
    )
    .await?;
    record(
        &broker,
        &not_a_number_id,
        &sensor_type,
        Property::builder(xsd_typed("double", "NaN")?).observed_at(observed()?),
    )
    .await?;
    let reading = || q::attr("reading");
    expect_that!(
        selected(&broker, &sensor_type, reading().eq(4)).await?,
        eq(&sorted(&[&number_id, &typed_integer_id, &value_type_integer_id]))
    );
    expect_that!(selected(&broker, &sensor_type, reading().eq("4")).await?, eq(&sorted(&[&string_id])));
    expect_that!(
        selected(&broker, &sensor_type, reading().ne(4)).await?,
        eq(&sorted(&[&string_id, &infinity_id, &not_a_number_id]))
    );
    expect_that!(
        selected(&broker, &sensor_type, reading().gt(3)).await?,
        eq(&sorted(&[&number_id, &typed_integer_id, &value_type_integer_id, &infinity_id]))
    );
    expect_that!(
        selected(&broker, &sensor_type, reading().lt(5)).await?,
        eq(&sorted(&[&number_id, &typed_integer_id, &value_type_integer_id]))
    );
    Ok(())
}
