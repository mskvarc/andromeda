//! A q comparison across JSON types on a compound member is decided, not failed.

use andromeda_test_suite::{
    broker::{self, BrokerClient},
    namespace::Namespace,
    outcome::Done,
    value::object,
};
use googletest::prelude::*;
use ngsi_ld::{
    http::headers::Accept,
    model::{Entity, EntityId, EntityTypeName, NgsiLdMediaType, Value},
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

/// Pins: over one Building whose `address` holds the String member `city` and the number member `floor`, a number or a
/// boolean compared with `city` and a String compared with `floor` answer 200: `==`, `<`, `>` and a range select
/// nothing, `!=` and an outside range select the Building, and a conjunction holding such a term is decided by its
/// other terms
/// Clause: 4.9, 5.7.2
#[gtest]
#[tokio::test]
async fn n_qry_qlg_9072_a_q_comparison_across_json_types_on_a_compound_member_is_decided_not_failed() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let building_id = namespace.urn("Building", 1)?;
    let building_type = namespace.entity_type("Building")?;
    let building = Entity::builder()
        .id(building_id.clone())
        .entity_type(building_type.clone())
        .property("address", object([("city", Value::from("Celje")), ("floor", Value::from(3))]))
        .build()?;
    broker.entities().create(&building).send().await?.done()?;
    let city = || q::attr("address").member("city");
    let floor = || q::attr("address").member("floor");
    // Clause 4.9: a target of another data type is unequal to the operand, and an ordering across data types does not
    // match.
    expect_that!(selected(&broker, &building_type, &building_id, city().eq(4)).await?, eq(0));
    expect_that!(selected(&broker, &building_type, &building_id, city().ne(4)).await?, eq(1));
    expect_that!(selected(&broker, &building_type, &building_id, city().lt(4)).await?, eq(0));
    expect_that!(selected(&broker, &building_type, &building_id, city().gt(4)).await?, eq(0));
    expect_that!(selected(&broker, &building_type, &building_id, city().within(1, 5)).await?, eq(0));
    expect_that!(selected(&broker, &building_type, &building_id, city().ne(Range::new(1, 5))).await?, eq(1));
    expect_that!(selected(&broker, &building_type, &building_id, city().any_of([1, 2])).await?, eq(0));
    expect_that!(selected(&broker, &building_type, &building_id, city().eq(true)).await?, eq(0));
    expect_that!(selected(&broker, &building_type, &building_id, city().ne(true)).await?, eq(1));
    expect_that!(selected(&broker, &building_type, &building_id, floor().eq("Celje")).await?, eq(0));
    expect_that!(selected(&broker, &building_type, &building_id, floor().ne("Celje")).await?, eq(1));
    expect_that!(selected(&broker, &building_type, &building_id, floor().eq(3)).await?, eq(1));
    expect_that!(selected(&broker, &building_type, &building_id, floor().ne(3)).await?, eq(0));
    expect_that!(selected(&broker, &building_type, &building_id, city().ne(4).and(floor().eq(3))).await?, eq(1));
    expect_that!(selected(&broker, &building_type, &building_id, city().ne(9).and(floor().gt(3))).await?, eq(0));
    Ok(())
}
