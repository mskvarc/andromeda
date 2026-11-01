//! A q comparison reads a value's data type from its declared datatype, not its spelling.

use andromeda_test_suite::{
    broker::{self, BrokerClient},
    namespace::Namespace,
    outcome::Done,
    value::{xsd, xsd_typed},
};
use googletest::prelude::*;
use ngsi_ld::{
    http::headers::Accept,
    model::{AttributeDraft, Entity, EntityId, EntityTypeName, Input, NgsiLdMediaType, Property, Value},
    query::q::{self, builders::QueryBuilder},
};

/// The ids, sorted, of the entities of `building_type` the query `q` selects.
async fn selected(broker: &BrokerClient, building_type: &EntityTypeName, q: QueryBuilder) -> Result<Vec<String>> {
    let page = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .query()
        .entity_type(building_type)
        .q(q)
        .send()
        .await?;
    let mut ids: Vec<String> = page.items().iter().map(|entity| entity.id.to_string()).collect();
    ids.sort();
    Ok(ids)
}

/// The Building `id` whose `reading` is `reading`.
fn building(id: &EntityId, building_type: &EntityTypeName, reading: impl AttributeDraft<Property<Input>>) -> Result<Entity<Input>> {
    Ok(Entity::builder()
        .id(id.clone())
        .entity_type(building_type.clone())
        .property("reading", reading)
        .build()?)
}

/// `ids` as sorted text.
fn sorted(ids: &[&EntityId]) -> Vec<String> {
    let mut ids: Vec<String> = ids.iter().map(ToString::to_string).collect();
    ids.sort();
    ids
}

/// Pins: over eight Buildings whose `reading` is the JSON string "4", the JSON number 4, "4" declared xsd:integer by
/// its @type, "4" declared xsd:integer by the valueType, "true" declared xsd:boolean, and "INF", "-INF" and "NaN"
/// declared xsd:double, `==4` selects the number and both declared integers, `=="4"` only the string, `!=4` the
/// string, the Boolean and the three non-finite values, `!="4"` all but the string, `>3` the number, both declared
/// integers and INF, `<3` only -INF, `==true` only the declared Boolean and `!=true` all but it
/// Clause: 4.9, 4.5.2, C.6
#[gtest]
#[tokio::test]
async fn n_qry_qlg_9075_a_q_comparison_reads_the_declared_datatype_not_the_spelling() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let building_type = namespace.entity_type("Building")?;
    let string_id = namespace.urn("Building", "string")?;
    let number_id = namespace.urn("Building", "number")?;
    let typed_integer_id = namespace.urn("Building", "typed-integer")?;
    let value_type_integer_id = namespace.urn("Building", "value-type-integer")?;
    let typed_boolean_id = namespace.urn("Building", "typed-boolean")?;
    let infinity_id = namespace.urn("Building", "infinity")?;
    let negative_infinity_id = namespace.urn("Building", "negative-infinity")?;
    let not_a_number_id = namespace.urn("Building", "not-a-number")?;
    let entities = [
        building(&string_id, &building_type, Value::from("4"))?,
        building(&number_id, &building_type, Value::from(4))?,
        building(&typed_integer_id, &building_type, xsd_typed("integer", "4")?)?,
        building(
            &value_type_integer_id,
            &building_type,
            Property::builder(Value::from("4")).value_type(xsd("integer")?),
        )?,
        building(&typed_boolean_id, &building_type, xsd_typed("boolean", "true")?)?,
        building(&infinity_id, &building_type, xsd_typed("double", "INF")?)?,
        building(&negative_infinity_id, &building_type, xsd_typed("double", "-INF")?)?,
        building(&not_a_number_id, &building_type, xsd_typed("double", "NaN")?)?,
    ];
    broker.entities().batch_create(&entities).send().await?.done()?;
    let reading = || q::attr("reading");
    let every = [
        &string_id,
        &number_id,
        &typed_integer_id,
        &value_type_integer_id,
        &typed_boolean_id,
        &infinity_id,
        &negative_infinity_id,
        &not_a_number_id,
    ];
    let all_but = |excluded: &EntityId| sorted(&every.iter().copied().filter(|id| *id != excluded).collect::<Vec<_>>());
    expect_that!(
        selected(&broker, &building_type, reading().eq(4)).await?,
        eq(&sorted(&[&number_id, &typed_integer_id, &value_type_integer_id]))
    );
    expect_that!(selected(&broker, &building_type, reading().eq("4")).await?, eq(&sorted(&[&string_id])));
    // Clause 4.9: a target of another data type is unequal to the operand, and NaN equals nothing.
    expect_that!(
        selected(&broker, &building_type, reading().ne(4)).await?,
        eq(&sorted(&[&string_id, &typed_boolean_id, &infinity_id, &negative_infinity_id, &not_a_number_id]))
    );
    expect_that!(selected(&broker, &building_type, reading().ne("4")).await?, eq(&all_but(&string_id)));
    // XSD orders INF above every finite number and -INF below; NaN is unordered.
    expect_that!(
        selected(&broker, &building_type, reading().gt(3)).await?,
        eq(&sorted(&[&number_id, &typed_integer_id, &value_type_integer_id, &infinity_id]))
    );
    expect_that!(selected(&broker, &building_type, reading().lt(3)).await?, eq(&sorted(&[&negative_infinity_id])));
    expect_that!(selected(&broker, &building_type, reading().eq(true)).await?, eq(&sorted(&[&typed_boolean_id])));
    expect_that!(selected(&broker, &building_type, reading().ne(true)).await?, eq(&all_but(&typed_boolean_id)));
    Ok(())
}
