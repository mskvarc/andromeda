//! A declared time value ranks by its datatype.

use andromeda_test_suite::{broker, identity_matcher::has_id, namespace::Namespace, nth::nth, outcome::Done};
use googletest::prelude::*;
use ngsi_ld::{
    http::headers::Accept,
    model::{Date, DateTime, Entity, NgsiLdMediaType, Time, Value},
    query::{Locality, ordering::OrderBy},
};

/// Pins: `orderBy=name` over a Number, a String and values typed Time, Date and `DateTime` orders Number, String,
/// Time, Date, `DateTime`
/// Clause: 4.23.2, 4.6.3, C.6
#[gtest]
#[tokio::test]
async fn n_qry_ord_9022_a_declared_time_value_ranks_by_its_datatype() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let building_number_id = namespace.urn("Building", "number")?;
    let building_type = namespace.entity_type("Building")?;
    let building_string_id = namespace.urn("Building", "string")?;
    let building_declared_time_id = namespace.urn("Building", "declared-time")?;
    let building_declared_date_id = namespace.urn("Building", "declared-date")?;
    let building_declared_datetime_id = namespace.urn("Building", "declared-datetime")?;
    let entities = [
        Entity::builder()
            .id(building_number_id.clone())
            .entity_type(building_type.clone())
            .property("name", 42)
            .build()?,
        Entity::builder()
            .id(building_string_id.clone())
            .entity_type(building_type.clone())
            .property("name", "text")
            .build()?,
        Entity::builder()
            .id(building_declared_time_id.clone())
            .entity_type(building_type.clone())
            .property("name", Value::from("10:00:00Z".parse::<Time>()?))
            .build()?,
        Entity::builder()
            .id(building_declared_date_id.clone())
            .entity_type(building_type.clone())
            .property("name", Value::from("2020-01-01".parse::<Date>()?))
            .build()?,
        Entity::builder()
            .id(building_declared_datetime_id.clone())
            .entity_type(building_type.clone())
            .property("name", Value::from("2020-01-01T00:00:00Z".parse::<DateTime>()?))
            .build()?,
    ];
    broker.entities().batch_create(&entities).send().await?.done()?;
    let page = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .query()
        .entity_type(&building_type)
        .ids(vec![
            building_number_id.clone(),
            building_string_id.clone(),
            building_declared_time_id.clone(),
            building_declared_date_id.clone(),
            building_declared_datetime_id.clone(),
        ])
        .locality(Locality::Local)
        .ordering("name".parse::<OrderBy>()?)
        .send()
        .await?;
    expect_that!(page.items(), len(eq(5)));
    expect_that!(page.items(), nth(0, has_id(&building_number_id)));
    expect_that!(page.items(), nth(1, has_id(&building_string_id)));
    expect_that!(page.items(), nth(2, has_id(&building_declared_time_id)));
    expect_that!(page.items(), nth(3, has_id(&building_declared_date_id)));
    expect_that!(page.items(), nth(4, has_id(&building_declared_datetime_id)));
    Ok(())
}
