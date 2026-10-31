//! A q term on a compound member holding several values compares them as an array.

use andromeda_test_suite::{
    broker::{self, BrokerClient},
    namespace::Namespace,
    outcome::Done,
    value::object,
};
use googletest::prelude::*;
use ngsi_ld::{
    http::headers::Accept,
    model::{Entity, EntityId, EntityTypeName, JsonProperty, NgsiLdMediaType, RawJson, Value},
    query::q::{self, Range, builders::QueryBuilder},
};

/// How many entities of `building_type` with the id `building_id` the query `q` selects.
async fn selected(broker: &BrokerClient, building_type: &EntityTypeName, building_id: &EntityId, q: QueryBuilder) -> Result<usize> {
    let page = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .query()
        .entity_type(building_type)
        .ids(vec![building_id.clone()])
        .q(q)
        .send()
        .await?;
    Ok(page.items().len())
}

/// Pins: over one Building whose structured `address` holds `rooms` as `[4, 5]` and `floors` as two objects, and whose
/// `JsonProperty` `plan` holds `rooms` as `[4, 5]`, `==` and a value list select it when an operand is one of the
/// rooms, `!=` only when no operand is, an ordering and a range select it for neither, `!=` a range does, and a path
/// stepping through `floors` reaches no member at all
/// Clause: 4.9, 5.7.2
#[gtest]
#[tokio::test]
async fn n_qry_qlg_9073_a_q_term_on_a_compound_member_holding_several_values_compares_them_as_an_array() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let building_id = namespace.urn("Building", 1)?;
    let building_type = namespace.entity_type("Building")?;
    let building = Entity::builder()
        .id(building_id.clone())
        .entity_type(building_type.clone())
        .property(
            "address",
            object([
                ("rooms", Value::from([4, 5])),
                (
                    "floors",
                    Value::Array(vec![object([("level", Value::from(1))]), object([("level", Value::from(2))])]),
                ),
            ]),
        )
        .json_property("plan", JsonProperty::builder(r#"{"rooms": [4, 5]}"#.parse::<RawJson>()?))
        .build()?;
    broker.entities().create(&building).send().await?.done()?;
    let rooms = || q::attr("address").member("rooms");
    let planned_rooms = || q::attr("plan").member("rooms");
    let level = || q::attr("address").member("floors").member("level");
    // Clause 4.9 `equal`: the operand "Is included in the target value, if the latter is an array"; `unequal`: it "Is
    // not included in the target value".
    expect_that!(selected(&broker, &building_type, &building_id, rooms().eq(4)).await?, eq(1));
    expect_that!(selected(&broker, &building_type, &building_id, rooms().eq(5)).await?, eq(1));
    expect_that!(selected(&broker, &building_type, &building_id, rooms().eq(6)).await?, eq(0));
    expect_that!(selected(&broker, &building_type, &building_id, rooms().ne(4)).await?, eq(0));
    expect_that!(selected(&broker, &building_type, &building_id, rooms().ne(6)).await?, eq(1));
    expect_that!(selected(&broker, &building_type, &building_id, rooms().any_of([6, 5])).await?, eq(1));
    expect_that!(selected(&broker, &building_type, &building_id, rooms().gt(3)).await?, eq(0));
    expect_that!(selected(&broker, &building_type, &building_id, rooms().lt(9)).await?, eq(0));
    expect_that!(selected(&broker, &building_type, &building_id, rooms().within(1, 9)).await?, eq(0));
    expect_that!(selected(&broker, &building_type, &building_id, rooms().ne(Range::new(1, 9))).await?, eq(1));
    expect_that!(selected(&broker, &building_type, &building_id, planned_rooms().eq(4)).await?, eq(1));
    expect_that!(selected(&broker, &building_type, &building_id, planned_rooms().ne(4)).await?, eq(0));
    expect_that!(selected(&broker, &building_type, &building_id, planned_rooms().ne(6)).await?, eq(1));
    expect_that!(selected(&broker, &building_type, &building_id, planned_rooms().gt(3)).await?, eq(0));
    // A member expression over an array addresses no member (ECMA 262 Property Accessors), so the target element does
    // not exist (clause 4.9).
    expect_that!(selected(&broker, &building_type, &building_id, level().eq(1)).await?, eq(0));
    expect_that!(selected(&broker, &building_type, &building_id, level().ne(3)).await?, eq(0));
    expect_that!(selected(&broker, &building_type, &building_id, level().exists()).await?, eq(0));
    Ok(())
}
