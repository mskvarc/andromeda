//! Conformance mode pages by offset and refuses a cursor.

use andromeda_test_suite::{
    broker,
    config::BrokerRole,
    namespace::Namespace,
    outcome::Done,
    page_link::{carries_parameter, links},
    problem::answered,
    wire::{header_values, request_to},
};
use googletest::prelude::*;
use http::{
    HeaderValue,
    Method,
    StatusCode,
    header::{ACCEPT, LINK},
};
use ngsi_ld::{
    http::headers::{Accept, Link},
    model::{Entity, NgsiLdMediaType},
    query::Locality,
};

/// Pins: On a broker in conformance mode, Query Entities with limit 2 over three Entities answers a next pointer
/// carrying offset=2, and the same query with an empty cursor parameter answers 400
/// Clause: 5.5.9, 6.3.10
/// Recorded: Andromeda pages by an opaque cursor by default; conformance mode turns every extension off, cursor
/// pagination with them, and pages by offset as clause 5.5.9 describes
#[gtest]
#[tokio::test]
async fn a_lim_pgp_9001_conformance_mode_pages_by_offset_and_refuses_a_cursor() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker_conformance = broker::client(BrokerRole::Conformance)?;
    let item_0_id = namespace.urn("Item", "0")?;
    let item_type = namespace.entity_type("Item")?;
    let item_id = namespace.urn("Item", 1)?;
    let item_2_id = namespace.urn("Item", 2)?;
    let item_0 = Entity::builder().id(item_0_id).entity_type(item_type.clone()).property("name", "i0").build()?;
    broker_conformance.entities().create(&item_0).send().await?.done()?;
    let item = Entity::builder().id(item_id).entity_type(item_type.clone()).property("name", "i1").build()?;
    broker_conformance.entities().create(&item).send().await?.done()?;
    let item_2 = Entity::builder().id(item_2_id).entity_type(item_type.clone()).property("name", "i2").build()?;
    broker_conformance.entities().create(&item_2).send().await?.done()?;
    let headed = broker_conformance
        .with_heads()
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .query()
        .entity_type(&item_type)
        .locality(Locality::Local)
        .limit(2)
        .send()
        .await?;
    let page = headed.outcome;
    expect_that!(header_values(headed.head.headers(), &LINK), len(eq(1)));
    expect_that!(
        headed
            .head
            .typed::<Link>()
            .valid()
            .map(|link| link.values().iter().map(|value| value.target().as_str().to_owned()).collect::<Vec<_>>()),
        some(contains(contains_substring("offset=2")))
    );
    expect_that!(page.next(), some(anything()));
    expect_that!(links(&page), each(not(carries_parameter("cursor"))));
    // The client cannot write a parameter of this read, so it is sent as written.
    let request = request_to(
        &broker_conformance.with_accept(Accept::new(NgsiLdMediaType::Json)),
        Method::GET,
        &format!("entities?type=Item_{namespace}&local=true&limit=2&cursor="),
    )?
    .append_header(ACCEPT, HeaderValue::from_static("application/json"));
    let answer = broker_conformance.with_accept(Accept::new(NgsiLdMediaType::Json)).send_unchecked(request).await;
    expect_that!(&answer, answered(StatusCode::BAD_REQUEST));
    Ok(())
}
