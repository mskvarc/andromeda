//! Q tells apart decimals beyond binary64 precision.

use andromeda_test_suite::{broker, identity_matcher::has_id, namespace::Namespace, outcome::Done};
use googletest::prelude::*;
use ngsi_ld::{
    http::headers::Accept,
    model::{Entity, NgsiLdMediaType, Number, Value},
    query::{Locality, q},
};

/// Pins: Entities holding reading 0.100000000000000005 (and large 18446744073709551616) and reading
/// 0.100000000000000004 are created, the first reads back with both numbers digit for digit, and q selects by ==,
/// !=, <, >, a range and a list of the two decimals exactly as decimal arithmetic orders them
/// Clause: 4.9, 5.6.1, 5.7.1, 5.7.2
/// Recorded: IETF RFC 8259 clause 6 lets an implementation limit the range and precision of numbers, so the spec
/// mandates none; Andromeda's own requirement is arbitrary precision, in storage and in q
#[gtest]
#[tokio::test]
async fn o_qry_qlg_9051_q_tells_apart_decimals_beyond_binary64_precision() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let building_exact_number_id = namespace.urn("Building", "exact-number")?;
    let building_type = namespace.entity_type("Building")?;
    let building_adjacent_number_id = namespace.urn("Building", "adjacent-number")?;
    let building_exact_number = Entity::builder()
        .id(building_exact_number_id.clone())
        .entity_type(building_type.clone())
        .property("reading", Value::Number("0.100000000000000005".parse()?))
        .property("large", Value::Number("18446744073709551616".parse()?))
        .build()?;
    broker.entities().create(&building_exact_number).send().await?.done()?;
    let building_adjacent_number = Entity::builder()
        .id(building_adjacent_number_id.clone())
        .entity_type(building_type.clone())
        .property("reading", Value::Number("0.100000000000000004".parse()?))
        .build()?;
    broker.entities().create(&building_adjacent_number).send().await?.done()?;
    let received = broker
        .with_responses()
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .retrieve(&building_exact_number_id)
        .send()
        .await?;
    expect_that!(String::from_utf8_lossy(&received.body), contains_substring("0.100000000000000005"));
    expect_that!(String::from_utf8_lossy(&received.body), contains_substring("18446744073709551616"));
    let page = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .query()
        .entity_type(&building_type)
        .locality(Locality::Local)
        .q(q::attr("reading").eq("0.100000000000000005".parse::<Number>()?))
        .send()
        .await?;
    expect_that!(page.items(), len(eq(1)));
    expect_that!(page.items(), contains(has_id(&building_exact_number_id)));
    let page_2 = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .query()
        .entity_type(&building_type)
        .locality(Locality::Local)
        .q(q::attr("reading").ne("0.100000000000000005".parse::<Number>()?))
        .send()
        .await?;
    expect_that!(page_2.items(), len(eq(1)));
    expect_that!(page_2.items(), contains(has_id(&building_adjacent_number_id)));
    let page_3 = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .query()
        .entity_type(&building_type)
        .locality(Locality::Local)
        .q(q::attr("reading").lt("0.100000000000000005".parse::<Number>()?))
        .send()
        .await?;
    expect_that!(page_3.items(), len(eq(1)));
    expect_that!(page_3.items(), contains(has_id(&building_adjacent_number_id)));
    let page_4 = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .query()
        .entity_type(&building_type)
        .locality(Locality::Local)
        .q(q::attr("reading").gt("0.100000000000000004".parse::<Number>()?))
        .send()
        .await?;
    expect_that!(page_4.items(), len(eq(1)));
    expect_that!(page_4.items(), contains(has_id(&building_exact_number_id)));
    let page_5 = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .query()
        .entity_type(&building_type)
        .locality(Locality::Local)
        .q(q::attr("reading").within("0.100000000000000004".parse::<Number>()?, "0.100000000000000005".parse::<Number>()?))
        .send()
        .await?;
    expect_that!(page_5.items(), len(eq(2)));
    expect_that!(page_5.items(), contains(has_id(&building_adjacent_number_id)));
    expect_that!(page_5.items(), contains(has_id(&building_exact_number_id)));
    let page_6 = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .query()
        .entity_type(&building_type)
        .locality(Locality::Local)
        .q(q::attr("reading").any_of(["0.100000000000000004".parse::<Number>()?, "0.100000000000000005".parse::<Number>()?]))
        .send()
        .await?;
    expect_that!(page_6.items(), len(eq(2)));
    expect_that!(page_6.items(), contains(has_id(&building_adjacent_number_id)));
    expect_that!(page_6.items(), contains(has_id(&building_exact_number_id)));
    Ok(())
}
