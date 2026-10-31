//! A sub attribute time equality matches any of several values.

use andromeda_test_suite::{broker, identity_matcher::has_id, namespace::Namespace, nth::nth, outcome::Done};
use googletest::prelude::*;
use ngsi_ld::{
    http::headers::Accept,
    model::{Entity, NgsiLdMediaType, Property, Time, Value},
    query::q,
};

/// Pins: Query Entities with q equality on a sub-attribute holding several Time values matches the entity when any
/// of them is the operand
/// Clause: 4.9, 4.6.3
#[gtest]
#[tokio::test]
async fn n_qry_qlg_0039_a_sub_attribute_time_equality_matches_any_of_several_values() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let shift_id = namespace.urn("Shift", 1)?;
    let shift_type = namespace.entity_type("Shift")?;
    let shift_2_id = namespace.urn("Shift", 2)?;
    let shift = Entity::builder()
        .id(shift_id.clone())
        .entity_type(shift_type.clone())
        .property(
            "gate",
            Property::builder("north").property(
                "opens",
                Value::Array(vec![Value::from("12:00:00Z".parse::<Time>()?), Value::from("13:00:00.500Z".parse::<Time>()?)]),
            ),
        )
        .build()?;
    broker.entities().create(&shift).send().await?.done()?;
    let shift_2 = Entity::builder()
        .id(shift_2_id.clone())
        .entity_type(shift_type.clone())
        .property(
            "gate",
            Property::builder("south").property(
                "opens",
                Value::Array(vec![Value::from("14:00:00Z".parse::<Time>()?), Value::from("15:00:00Z".parse::<Time>()?)]),
            ),
        )
        .build()?;
    broker.entities().create(&shift_2).send().await?.done()?;
    let page = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .query()
        .entity_type(&shift_type)
        .q(q::attr("gate").sub_attr("opens").eq("12:00:00Z".parse::<Time>()?))
        .send()
        .await?;
    expect_that!(page.items(), len(eq(1)));
    expect_that!(page.items(), nth(0, has_id(&shift_id)));
    let page_2 = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .query()
        .entity_type(&shift_type)
        .q(q::attr("gate").sub_attr("opens").eq("13:00:00.5Z".parse::<Time>()?))
        .send()
        .await?;
    expect_that!(page_2.items(), len(eq(1)));
    expect_that!(page_2.items(), nth(0, has_id(&shift_id)));
    let page_3 = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .query()
        .entity_type(&shift_type)
        .q(q::attr("gate").sub_attr("opens").eq("15:00:00Z".parse::<Time>()?))
        .send()
        .await?;
    expect_that!(page_3.items(), len(eq(1)));
    expect_that!(page_3.items(), nth(0, has_id(&shift_2_id)));
    Ok(())
}
