//! A q comparison on a structured value member reads the member's declared datatype.

use andromeda_test_suite::{
    broker::{self, BrokerClient},
    namespace::Namespace,
    outcome::Done,
    value::{object, xsd_typed},
};
use googletest::prelude::*;
use ngsi_ld::{
    http::headers::Accept,
    model::{Entity, EntityId, EntityTypeName, NgsiLdMediaType, Value},
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

/// `ids` as sorted text.
fn sorted(ids: &[&EntityId]) -> Vec<String> {
    let mut ids: Vec<String> = ids.iter().map(ToString::to_string).collect();
    ids.sort();
    ids
}

/// Pins: over seven Buildings whose `address` holds the member `reading` as the JSON string "4", the JSON number 4,
/// "4" declared xsd:integer by its own @type, "true" declared xsd:boolean, and "INF", "-INF" and "NaN" declared
/// xsd:double, `address[reading]==4` selects the number and the declared integer, `=="4"` only the string, `!=4` the
/// string, the Boolean and the three non-finite values, `!="4"` all but the string, `>3` the number, the declared
/// integer and INF, `<3` only -INF, `==true` only the declared Boolean and `!=true` all but it
/// Clause: 4.9, 4.5.2, C.6
#[gtest]
#[tokio::test]
async fn n_qry_qlg_9076_a_structured_value_member_compares_by_its_declared_datatype() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let building_type = namespace.entity_type("Building")?;
    let string_id = namespace.urn("Building", "string")?;
    let number_id = namespace.urn("Building", "number")?;
    let typed_integer_id = namespace.urn("Building", "typed-integer")?;
    let typed_boolean_id = namespace.urn("Building", "typed-boolean")?;
    let infinity_id = namespace.urn("Building", "infinity")?;
    let negative_infinity_id = namespace.urn("Building", "negative-infinity")?;
    let not_a_number_id = namespace.urn("Building", "not-a-number")?;
    let building = |id: &EntityId, reading: Value| {
        Entity::builder()
            .id(id.clone())
            .entity_type(building_type.clone())
            .property("address", object([("reading", reading)]))
            .build()
    };
    let entities = [
        building(&string_id, Value::from("4"))?,
        building(&number_id, Value::from(4))?,
        building(&typed_integer_id, xsd_typed("integer", "4")?)?,
        building(&typed_boolean_id, xsd_typed("boolean", "true")?)?,
        building(&infinity_id, xsd_typed("double", "INF")?)?,
        building(&negative_infinity_id, xsd_typed("double", "-INF")?)?,
        building(&not_a_number_id, xsd_typed("double", "NaN")?)?,
    ];
    broker.entities().batch_create(&entities).send().await?.done()?;
    let reading = || q::attr("address").member("reading");
    let every = [
        &string_id,
        &number_id,
        &typed_integer_id,
        &typed_boolean_id,
        &infinity_id,
        &negative_infinity_id,
        &not_a_number_id,
    ];
    let all_but = |excluded: &EntityId| sorted(&every.iter().copied().filter(|id| *id != excluded).collect::<Vec<_>>());
    expect_that!(
        selected(&broker, &building_type, reading().eq(4)).await?,
        eq(&sorted(&[&number_id, &typed_integer_id]))
    );
    expect_that!(selected(&broker, &building_type, reading().eq("4")).await?, eq(&sorted(&[&string_id])));
    expect_that!(
        selected(&broker, &building_type, reading().ne(4)).await?,
        eq(&sorted(&[&string_id, &typed_boolean_id, &infinity_id, &negative_infinity_id, &not_a_number_id]))
    );
    expect_that!(selected(&broker, &building_type, reading().ne("4")).await?, eq(&all_but(&string_id)));
    expect_that!(
        selected(&broker, &building_type, reading().gt(3)).await?,
        eq(&sorted(&[&number_id, &typed_integer_id, &infinity_id]))
    );
    expect_that!(selected(&broker, &building_type, reading().lt(3)).await?, eq(&sorted(&[&negative_infinity_id])));
    expect_that!(selected(&broker, &building_type, reading().eq(true)).await?, eq(&sorted(&[&typed_boolean_id])));
    expect_that!(selected(&broker, &building_type, reading().ne(true)).await?, eq(&all_but(&typed_boolean_id)));
    Ok(())
}
