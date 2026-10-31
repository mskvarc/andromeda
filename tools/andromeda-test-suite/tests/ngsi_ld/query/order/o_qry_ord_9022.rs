//! An undeclared time value ranks among the strings.

use andromeda_test_suite::{broker, identity_matcher::has_id, namespace::Namespace, nth::nth, outcome::Done};
use googletest::prelude::*;
use ngsi_ld::{
    http::headers::Accept,
    model::{Entity, NgsiLdMediaType},
    query::{Locality, ordering::OrderBy},
};

/// Pins: orderBy=name over a Number, a String and plain strings in the Time, Date and `DateTime` forms orders the
/// Number first, then every string by its text: the Time, the Date, the `DateTime` and the String
/// Clause: 4.23.2, 4.6.3, C.6
/// Recorded: a value is a time value only when its `valueType` declares it one (annex C.6); these strings declare
/// none, so they rank in the String group of clause 4.23.2, ordered by their text
#[gtest]
#[tokio::test]
async fn o_qry_ord_9022_an_undeclared_time_value_ranks_among_the_strings() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let building_undeclared_number_id = namespace.urn("Building", "undeclared-number")?;
    let building_type = namespace.entity_type("Building")?;
    let building_undeclared_string_id = namespace.urn("Building", "undeclared-string")?;
    let building_undeclared_time_id = namespace.urn("Building", "undeclared-time")?;
    let building_undeclared_date_id = namespace.urn("Building", "undeclared-date")?;
    let building_undeclared_datetime_id = namespace.urn("Building", "undeclared-datetime")?;
    let entities = [
        Entity::builder()
            .id(building_undeclared_number_id.clone())
            .entity_type(building_type.clone())
            .property("name", 42)
            .build()?,
        Entity::builder()
            .id(building_undeclared_string_id.clone())
            .entity_type(building_type.clone())
            .property("name", "text")
            .build()?,
        Entity::builder()
            .id(building_undeclared_time_id.clone())
            .entity_type(building_type.clone())
            .property("name", "10:00:00Z")
            .build()?,
        Entity::builder()
            .id(building_undeclared_date_id.clone())
            .entity_type(building_type.clone())
            .property("name", "2020-01-01")
            .build()?,
        Entity::builder()
            .id(building_undeclared_datetime_id.clone())
            .entity_type(building_type.clone())
            .property("name", "2020-01-01T00:00:00Z")
            .build()?,
    ];
    broker.entities().batch_create(&entities).send().await?.done()?;
    let page = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .query()
        .entity_type(&building_type)
        .ids(vec![
            building_undeclared_number_id.clone(),
            building_undeclared_string_id.clone(),
            building_undeclared_time_id.clone(),
            building_undeclared_date_id.clone(),
            building_undeclared_datetime_id.clone(),
        ])
        .locality(Locality::Local)
        .ordering("name".parse::<OrderBy>()?)
        .send()
        .await?;
    expect_that!(page.items(), len(eq(5)));
    expect_that!(page.items(), nth(0, has_id(&building_undeclared_number_id)));
    expect_that!(page.items(), nth(1, has_id(&building_undeclared_time_id)));
    expect_that!(page.items(), nth(2, has_id(&building_undeclared_date_id)));
    expect_that!(page.items(), nth(3, has_id(&building_undeclared_datetime_id)));
    expect_that!(page.items(), nth(4, has_id(&building_undeclared_string_id)));
    Ok(())
}
