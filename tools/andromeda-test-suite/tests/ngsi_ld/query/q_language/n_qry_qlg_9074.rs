//! A q term on a compound member compares a quoted string only with a String member and a number only with a number
//! member.

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
    query::q::{
        self,
        builders::{PathBuilder, QueryBuilder},
    },
};

/// The `zip` member of the structured `address`.
fn zip() -> PathBuilder {
    q::attr("address").member("zip")
}

/// The `zip` member of the `JsonProperty` `plan`.
fn planned_zip() -> PathBuilder {
    q::attr("plan").member("zip")
}

/// Which of `building_ids` the query `q` selects, as one flag per Building in the order given.
async fn selected(broker: &BrokerClient, building_type: &EntityTypeName, building_ids: &[EntityId], q: impl Fn() -> QueryBuilder) -> Result<Vec<bool>> {
    let mut flags = Vec::with_capacity(building_ids.len());
    for building_id in building_ids {
        let page = broker
            .with_accept(Accept::new(NgsiLdMediaType::Json))
            .entities()
            .query()
            .entity_type(building_type)
            .ids(vec![building_id.clone()])
            .q(q())
            .send()
            .await?;
        flags.push(!page.items().is_empty());
    }
    Ok(flags)
}

/// Pins: over three Buildings whose structured `address` and `JsonProperty` `plan` hold `zip` as the number 3310, the
/// String "3310" and the number 2000, `=="3310"` selects only the String, `==3310` only the number 3310, `!="3310"`
/// both numbers and `!=3310` the String and 2000, a String ordering selects no number, and a conjunction or a
/// disjunction holding such a term is decided by its terms
/// Clause: 4.9, 5.7.2
#[gtest]
#[tokio::test]
async fn n_qry_qlg_9074_a_q_term_on_a_compound_member_compares_only_values_of_the_operand_s_json_type() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let building_type = namespace.entity_type("Building")?;
    let zips = [(Value::from(3310), "3310"), (Value::from("3310"), r#""3310""#), (Value::from(2000), "2000")];
    let mut building_ids = Vec::with_capacity(zips.len());
    for (position, (zip, written)) in zips.into_iter().enumerate() {
        let building_id = namespace.urn("Building", position + 1)?;
        let building = Entity::builder()
            .id(building_id.clone())
            .entity_type(building_type.clone())
            .property("address", object([("zip", zip)]))
            .json_property("plan", JsonProperty::builder(format!(r#"{{"zip": {written}}}"#).parse::<RawJson>()?))
            .build()?;
        broker.entities().create(&building).send().await?.done()?;
        building_ids.push(building_id);
    }
    let select = |q: fn() -> QueryBuilder| selected(&broker, &building_type, &building_ids, q);
    // Clause 4.9: "If the data type of the target value and the data type of the Query Term value are different, then
    // they shall be considered unequal", and an ordering across data types does not match.
    expect_that!(select(|| zip().eq("3310")).await?, elements_are![eq(&false), eq(&true), eq(&false)]);
    expect_that!(select(|| zip().eq(3310)).await?, elements_are![eq(&true), eq(&false), eq(&false)]);
    expect_that!(select(|| zip().ne("3310")).await?, elements_are![eq(&true), eq(&false), eq(&true)]);
    expect_that!(select(|| zip().ne(3310)).await?, elements_are![eq(&false), eq(&true), eq(&true)]);
    expect_that!(
        select(|| zip().any_of(["3310", "2000"])).await?,
        elements_are![eq(&false), eq(&true), eq(&false)]
    );
    expect_that!(select(|| zip().lt("4")).await?, elements_are![eq(&false), eq(&true), eq(&false)]);
    expect_that!(select(|| zip().gt(3000)).await?, elements_are![eq(&true), eq(&false), eq(&false)]);
    expect_that!(select(|| planned_zip().eq("3310")).await?, elements_are![eq(&false), eq(&true), eq(&false)]);
    expect_that!(select(|| planned_zip().eq(3310)).await?, elements_are![eq(&true), eq(&false), eq(&false)]);
    expect_that!(select(|| planned_zip().ne("3310")).await?, elements_are![eq(&true), eq(&false), eq(&true)]);
    expect_that!(
        select(|| zip().eq("3310").or(zip().eq(2000))).await?,
        elements_are![eq(&false), eq(&true), eq(&true)]
    );
    expect_that!(
        select(|| zip().eq("3310").or(planned_zip().eq(3310))).await?,
        elements_are![eq(&true), eq(&true), eq(&false)]
    );
    expect_that!(
        select(|| zip().ne("3310").and(planned_zip().eq(3310))).await?,
        elements_are![eq(&true), eq(&false), eq(&false)]
    );
    expect_that!(
        select(|| zip().eq("3310").and(planned_zip().eq(3310))).await?,
        elements_are![eq(&false), eq(&false), eq(&false)]
    );
    Ok(())
}
