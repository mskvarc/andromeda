//! Q tells apart integers beyond double precision.

use andromeda_test_suite::{broker, identity_matcher::has_id, namespace::Namespace, outcome::Done};
use googletest::prelude::*;
use ngsi_ld::{
    http::headers::Accept,
    model::{Entity, NgsiLdMediaType, Number},
    query::q,
};

/// Pins: Over a Building whose counter holds 9007199254740993, q counter==9007199254740993 selects it and
/// counter==9007199254740992, the nearest double, does not
/// Clause: 4.9, 5.7.2
/// Recorded: IETF RFC 8259 clause 6 lets an implementation limit the range and precision of numbers, so the spec
/// mandates none; Andromeda's own requirement is arbitrary precision, in storage and in q
#[gtest]
#[tokio::test]
async fn o_qry_qlg_9055_q_tells_apart_integers_beyond_double_precision() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let building_beyond_double_id = namespace.urn("Building", "beyond-double")?;
    let building_type = namespace.entity_type("Building")?;
    let building_beyond_double = Entity::builder()
        .id(building_beyond_double_id.clone())
        .entity_type(building_type.clone())
        .property("counter", 9_007_199_254_740_993_i64)
        .build()?;
    broker.entities().create(&building_beyond_double).send().await?.done()?;
    let page = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .query()
        .entity_type(&building_type)
        .ids(vec![building_beyond_double_id.clone()])
        .q(q::attr("counter").eq("9007199254740993".parse::<Number>()?))
        .send()
        .await?;
    expect_that!(page.items(), len(eq(1)));
    expect_that!(page.items(), contains(has_id(&building_beyond_double_id)));
    let page_2 = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .query()
        .entity_type(&building_type)
        .ids(vec![building_beyond_double_id.clone()])
        .q(q::attr("counter").eq("9007199254740992".parse::<Number>()?))
        .send()
        .await?;
    expect_that!(page_2.items(), len(eq(0)));
    Ok(())
}
