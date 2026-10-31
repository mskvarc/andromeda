//! A time operand matches no untyped string and a quoted one matches it exactly.

use andromeda_test_suite::{broker, identity_matcher::has_id, namespace::Namespace, nth::nth, outcome::Done};
use googletest::prelude::*;
use ngsi_ld::{
    http::headers::Accept,
    model::{DateTime, Entity, NgsiLdMediaType},
    query::q,
};

/// Pins: Query Entities with the bare q=reading==2024-01-01T00:00:00Z selects no Entity whose reading is the
/// untyped string 2024-01-01T00:00:00Z, the quoted operand selects it, and the quoted 2024-01-01T00:00:00.000Z does
/// not
/// Clause: 4.6.3, 4.9, C.6
#[gtest]
#[tokio::test]
async fn n_qry_qlg_9063_a_time_operand_matches_no_untyped_string_and_a_quoted_one_matches_it_exactly() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let observation_untyped_id = namespace.urn("Observation", "untyped")?;
    let observation_type = namespace.entity_type("Observation")?;
    let observation_untyped = Entity::builder()
        .id(observation_untyped_id.clone())
        .entity_type(observation_type.clone())
        .property("reading", "2024-01-01T00:00:00Z")
        .build()?;
    broker.entities().create(&observation_untyped).send().await?.done()?;
    let page = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .query()
        .entity_type(&observation_type)
        .q(q::attr("reading").eq("2024-01-01T00:00:00Z".parse::<DateTime>()?))
        .send()
        .await?;
    expect_that!(page.items(), len(eq(0)));
    let page_2 = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .query()
        .entity_type(&observation_type)
        .q(q::attr("reading").eq("2024-01-01T00:00:00Z"))
        .send()
        .await?;
    expect_that!(page_2.items(), len(eq(1)));
    expect_that!(page_2.items(), nth(0, has_id(&observation_untyped_id)));
    let page_3 = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .query()
        .entity_type(&observation_type)
        .q(q::attr("reading").eq("2024-01-01T00:00:00.000Z"))
        .send()
        .await?;
    expect_that!(page_3.items(), len(eq(0)));
    Ok(())
}
