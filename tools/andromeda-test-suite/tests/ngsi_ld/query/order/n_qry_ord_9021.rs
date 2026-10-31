//! Mixed datatypes follow the comparison order.

use andromeda_test_suite::{broker, identity_matcher::ids_in_order, namespace::Namespace, problem::answered, value::object};
use googletest::prelude::*;
use http::StatusCode;
use ngsi_ld::{
    http::{headers::Accept, operation::QueryEntities},
    model::{Date, DateTime, Entity, Lossless, NgsiLdMediaType, Time, Value},
    query::{
        Locality,
        builders::SelectorTypeBuilder,
        ordering::{OrderBy, builders::EntityOrderingBuilder},
    },
};

/// Pins: `orderBy=name` over values of every datatype orders Number, String, Object, Array, Boolean, Time, Date,
/// `DateTime`, Null, then the Entity without the attribute; orderBy=name;desc reverses the datatypes and still ends
/// with Null, then the Entity without the attribute
/// Clause: 4.23.2
#[gtest]
#[tokio::test]
async fn n_qry_ord_9021_mixed_datatypes_follow_the_comparison_order() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let building_datatype_number_id = namespace.urn("Building", "datatype-number")?;
    let building_type = namespace.entity_type("Building")?;
    let building_datatype_string_id = namespace.urn("Building", "datatype-string")?;
    let building_datatype_object_id = namespace.urn("Building", "datatype-object")?;
    let building_datatype_array_id = namespace.urn("Building", "datatype-array")?;
    let building_datatype_boolean_id = namespace.urn("Building", "datatype-boolean")?;
    let building_datatype_time_id = namespace.urn("Building", "datatype-time")?;
    let building_datatype_date_id = namespace.urn("Building", "datatype-date")?;
    let building_datatype_datetime_id = namespace.urn("Building", "datatype-datetime")?;
    let building_datatype_null_id = namespace.urn("Building", "datatype-null")?;
    let building_datatype_absent_id = namespace.urn("Building", "datatype-absent")?;
    let building_datatype_number = Entity::builder()
        .id(building_datatype_number_id.clone())
        .entity_type(building_type.clone())
        .property("name", 42)
        .build()?;
    let building_datatype_string = Entity::builder()
        .id(building_datatype_string_id.clone())
        .entity_type(building_type.clone())
        .property("name", "text")
        .build()?;
    let building_datatype_object = Entity::builder()
        .id(building_datatype_object_id.clone())
        .entity_type(building_type.clone())
        .property("name", object([("nested", Value::from(1))]))
        .build()?;
    let building_datatype_array = Entity::builder()
        .id(building_datatype_array_id.clone())
        .entity_type(building_type.clone())
        .property("name", Value::Array(vec![Value::from(1), Value::from(2), Value::from(3)]))
        .build()?;
    let building_datatype_boolean = Entity::builder()
        .id(building_datatype_boolean_id.clone())
        .entity_type(building_type.clone())
        .property("name", true)
        .build()?;
    let building_datatype_time = Entity::builder()
        .id(building_datatype_time_id.clone())
        .entity_type(building_type.clone())
        .property("name", Value::from("10:00:00Z".parse::<Time>()?))
        .build()?;
    let building_datatype_date = Entity::builder()
        .id(building_datatype_date_id.clone())
        .entity_type(building_type.clone())
        .property("name", Value::from("2020-01-01".parse::<Date>()?))
        .build()?;
    let building_datatype_datetime = Entity::builder()
        .id(building_datatype_datetime_id.clone())
        .entity_type(building_type.clone())
        .property("name", Value::from("2020-01-01T00:00:00Z".parse::<DateTime>()?))
        .build()?;
    let building_datatype_null = Entity::builder()
        .id(building_datatype_null_id.clone())
        .entity_type(building_type.clone())
        .property("name", Value::JsonNull)
        .build()?;
    let building_datatype_absent = Entity::builder()
        .id(building_datatype_absent_id.clone())
        .entity_type(building_type.clone())
        .property("unrelated", 1)
        .build()?;
    let entities = [
        building_datatype_number,
        building_datatype_string,
        building_datatype_object,
        building_datatype_array,
        building_datatype_boolean,
        building_datatype_time,
        building_datatype_date,
        building_datatype_datetime,
        building_datatype_null,
        building_datatype_absent,
    ];
    let answer = broker.with_responses().entities().batch_create(&entities).send().await;
    expect_that!(&answer, answered(StatusCode::CREATED));
    let ids = [
        building_datatype_number_id,
        building_datatype_string_id,
        building_datatype_object_id,
        building_datatype_array_id,
        building_datatype_boolean_id,
        building_datatype_time_id,
        building_datatype_date_id,
        building_datatype_datetime_id,
        building_datatype_null_id,
        building_datatype_absent_id,
    ];
    let [number, string, object, array, boolean, time, date, datetime, null, absent] = &ids;
    let selector = SelectorTypeBuilder::from(&building_type).build()?;
    for (order_by, expected) in [
        ("name", [number, string, object, array, boolean, time, date, datetime, null, absent]),
        ("name;desc", [datetime, date, time, boolean, array, object, string, number, null, absent]),
    ] {
        let ordering = EntityOrderingBuilder::from(order_by.parse::<OrderBy>()?).build()?;
        let query = QueryEntities::builder()
            .entity_type(&selector)
            .ids(&ids)
            .ordering(&ordering)
            .locality(Locality::Local)
            .format(Lossless::Normalized)
            .build();
        let page = broker.with_accept(Accept::new(NgsiLdMediaType::Json)).send(&query).await?;
        expect_that!(page.items(), ids_in_order(&expected.map(Clone::clone)));
    }
    Ok(())
}
