//! A keyed and a walked sub attribute array equality select the same entities.

use andromeda_test_suite::{broker, identity_matcher::has_id, namespace::Namespace, nth::nth, outcome::Done};
use googletest::prelude::*;
use ngsi_ld::{
    http::headers::Accept,
    model::{DateTime, Entity, NgsiLdMediaType, Property, Time, Value},
    query::q,
};

/// Pins: Query Entities with q equal to a `DateTime` or Time on a sub-attribute array of typed members selects the
/// same Entities as a single term and as that term repeated in a disjunction, whichever fraction spelling the
/// operand and the members use
/// Clause: 4.9, 4.6.3, C.6
#[gtest]
#[tokio::test]
async fn n_qry_qlg_9059_a_keyed_and_a_walked_sub_attribute_array_equality_select_the_same_entities() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let gate_id = namespace.urn("Gate", 1)?;
    let gate_type = namespace.entity_type("Gate")?;
    let gate_2_id = namespace.urn("Gate", 2)?;
    let gate = Entity::builder()
        .id(gate_id.clone())
        .entity_type(gate_type.clone())
        .property(
            "gate",
            Property::builder("north")
                .property(
                    "opens",
                    Value::Array(vec![Value::from("08:00:00.000Z".parse::<Time>()?), Value::from("20:00:00,5Z".parse::<Time>()?)]),
                )
                .property(
                    "inspected",
                    Value::Array(vec![
                        Value::from("2024-01-02T00:00:00.000Z".parse::<DateTime>()?),
                        Value::from("2024-02-02T00:00:00,25Z".parse::<DateTime>()?),
                    ]),
                ),
        )
        .build()?;
    broker.entities().create(&gate).send().await?.done()?;
    let gate_2 = Entity::builder()
        .id(gate_2_id.clone())
        .entity_type(gate_type.clone())
        .property(
            "gate",
            Property::builder("south")
                .property(
                    "opens",
                    Value::Array(vec![Value::from("09:00:00Z".parse::<Time>()?), Value::from("21:00:00Z".parse::<Time>()?)]),
                )
                .property(
                    "inspected",
                    Value::Array(vec![
                        Value::from("2024-03-02T00:00:00Z".parse::<DateTime>()?),
                        Value::from("2024-04-02T00:00:00Z".parse::<DateTime>()?),
                    ]),
                ),
        )
        .build()?;
    broker.entities().create(&gate_2).send().await?.done()?;
    let page = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .query()
        .entity_type(&gate_type)
        .q(q::attr("gate").sub_attr("opens").eq("08:00:00Z".parse::<Time>()?))
        .send()
        .await?;
    expect_that!(page.items(), len(eq(1)));
    expect_that!(page.items(), nth(0, has_id(&gate_id)));
    let page_2 = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .query()
        .entity_type(&gate_type)
        .q(q::attr("gate")
            .sub_attr("opens")
            .eq("08:00:00Z".parse::<Time>()?)
            .or(q::attr("gate").sub_attr("opens").eq("08:00:00Z".parse::<Time>()?)))
        .send()
        .await?;
    expect_that!(page_2.items(), len(eq(1)));
    expect_that!(page_2.items(), nth(0, has_id(&gate_id)));
    let page_3 = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .query()
        .entity_type(&gate_type)
        .q(q::attr("gate").sub_attr("opens").eq("20:00:00.500Z".parse::<Time>()?))
        .send()
        .await?;
    expect_that!(page_3.items(), len(eq(1)));
    expect_that!(page_3.items(), nth(0, has_id(&gate_id)));
    let page_4 = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .query()
        .entity_type(&gate_type)
        .q(q::attr("gate")
            .sub_attr("opens")
            .eq("20:00:00.500Z".parse::<Time>()?)
            .or(q::attr("gate").sub_attr("opens").eq("20:00:00.500Z".parse::<Time>()?)))
        .send()
        .await?;
    expect_that!(page_4.items(), len(eq(1)));
    expect_that!(page_4.items(), nth(0, has_id(&gate_id)));
    let page_5 = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .query()
        .entity_type(&gate_type)
        .q(q::attr("gate").sub_attr("opens").eq("21:00:00.000Z".parse::<Time>()?))
        .send()
        .await?;
    expect_that!(page_5.items(), len(eq(1)));
    expect_that!(page_5.items(), nth(0, has_id(&gate_2_id)));
    let page_6 = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .query()
        .entity_type(&gate_type)
        .q(q::attr("gate")
            .sub_attr("opens")
            .eq("21:00:00.000Z".parse::<Time>()?)
            .or(q::attr("gate").sub_attr("opens").eq("21:00:00.000Z".parse::<Time>()?)))
        .send()
        .await?;
    expect_that!(page_6.items(), len(eq(1)));
    expect_that!(page_6.items(), nth(0, has_id(&gate_2_id)));
    let page_7 = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .query()
        .entity_type(&gate_type)
        .q(q::attr("gate").sub_attr("inspected").eq("2024-01-02T00:00:00Z".parse::<DateTime>()?))
        .send()
        .await?;
    expect_that!(page_7.items(), len(eq(1)));
    expect_that!(page_7.items(), nth(0, has_id(&gate_id)));
    let page_8 = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .query()
        .entity_type(&gate_type)
        .q(q::attr("gate")
            .sub_attr("inspected")
            .eq("2024-01-02T00:00:00Z".parse::<DateTime>()?)
            .or(q::attr("gate").sub_attr("inspected").eq("2024-01-02T00:00:00Z".parse::<DateTime>()?)))
        .send()
        .await?;
    expect_that!(page_8.items(), len(eq(1)));
    expect_that!(page_8.items(), nth(0, has_id(&gate_id)));
    let page_9 = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .query()
        .entity_type(&gate_type)
        .q(q::attr("gate").sub_attr("inspected").eq("2024-02-02T00:00:00.250000Z".parse::<DateTime>()?))
        .send()
        .await?;
    expect_that!(page_9.items(), len(eq(1)));
    expect_that!(page_9.items(), nth(0, has_id(&gate_id)));
    let page_10 = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .query()
        .entity_type(&gate_type)
        .q(q::attr("gate")
            .sub_attr("inspected")
            .eq("2024-02-02T00:00:00.250000Z".parse::<DateTime>()?)
            .or(q::attr("gate").sub_attr("inspected").eq("2024-02-02T00:00:00.250000Z".parse::<DateTime>()?)))
        .send()
        .await?;
    expect_that!(page_10.items(), len(eq(1)));
    expect_that!(page_10.items(), nth(0, has_id(&gate_id)));
    let page_11 = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .query()
        .entity_type(&gate_type)
        .q(q::attr("gate").sub_attr("inspected").eq("2024-03-02T00:00:00.000Z".parse::<DateTime>()?))
        .send()
        .await?;
    expect_that!(page_11.items(), len(eq(1)));
    expect_that!(page_11.items(), nth(0, has_id(&gate_2_id)));
    let page_12 = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .query()
        .entity_type(&gate_type)
        .q(q::attr("gate")
            .sub_attr("inspected")
            .eq("2024-03-02T00:00:00.000Z".parse::<DateTime>()?)
            .or(q::attr("gate").sub_attr("inspected").eq("2024-03-02T00:00:00.000Z".parse::<DateTime>()?)))
        .send()
        .await?;
    expect_that!(page_12.items(), len(eq(1)));
    expect_that!(page_12.items(), nth(0, has_id(&gate_2_id)));
    Ok(())
}
