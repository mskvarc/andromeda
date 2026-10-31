//! An untyped string in a time value form ranks as a string.

use andromeda_test_suite::{broker, identity_matcher::has_id, namespace::Namespace, nth::nth, outcome::Done};
use googletest::prelude::*;
use ngsi_ld::{
    http::headers::Accept,
    model::{Entity, NgsiLdMediaType, Time, Value},
    query::ordering::OrderBy,
};

/// Pins: Query Entities with `orderBy=reading` ranks the untyped strings 12:00:00Z and 2024-01-01 as Strings,
/// before a typed Time, and orders the two by their text
/// Clause: 4.23.1, 4.23.2, C.6
/// Recorded: clause 4.23.2 ranks Strings before Time, Date and `DateTime`; Andromeda gives a value one of the three
/// time data types only when it is declared as one, so an untyped string ranks as a String whatever its form
#[gtest]
#[tokio::test]
async fn o_qry_ord_9029_an_untyped_string_in_a_time_value_form_ranks_as_a_string() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let observation_typed_time_id = namespace.urn("Observation", "typed-time")?;
    let observation_type = namespace.entity_type("Observation")?;
    let observation_plain_time_id = namespace.urn("Observation", "plain-time")?;
    let observation_plain_date_id = namespace.urn("Observation", "plain-date")?;
    let entities = [
        Entity::builder()
            .id(observation_typed_time_id.clone())
            .entity_type(observation_type.clone())
            .property("reading", Value::from("01:00:00Z".parse::<Time>()?))
            .build()?,
        Entity::builder()
            .id(observation_plain_time_id.clone())
            .entity_type(observation_type.clone())
            .property("reading", "12:00:00Z")
            .build()?,
        Entity::builder()
            .id(observation_plain_date_id.clone())
            .entity_type(observation_type.clone())
            .property("reading", "2024-01-01")
            .build()?,
    ];
    broker.entities().batch_create(&entities).send().await?.done()?;
    let page = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .query()
        .entity_type(&observation_type)
        .ordering("reading".parse::<OrderBy>()?)
        .send()
        .await?;
    expect_that!(page.items(), len(eq(3)));
    expect_that!(page.items(), nth(0, has_id(&observation_plain_time_id)));
    expect_that!(page.items(), nth(1, has_id(&observation_plain_date_id)));
    expect_that!(page.items(), nth(2, has_id(&observation_typed_time_id)));
    Ok(())
}
