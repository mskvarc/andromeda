//! A compound member declared as `xsd:boolean` sorts among the Booleans.

use andromeda_test_suite::{
    broker,
    identity_matcher::has_id,
    namespace::Namespace,
    nth::nth,
    outcome::Done,
    value::{object, xsd_typed},
};
use googletest::prelude::*;
use ngsi_ld::{
    http::headers::Accept,
    model::{Entity, EntityId, NgsiLdMediaType, Value},
    query::{Locality, ordering::OrderBy},
};

/// Pins: `orderBy=reading[flag],serial` over a compound `reading` whose `flag` is the String "zebra" (`serial` 3),
/// declared the xsd:boolean "true" (`serial` 1) and the JSON boolean true (`serial` 2) orders "zebra", the declared
/// true, the JSON true: a member a value object declares `xsd:boolean` is a Boolean, which ranks after every String,
/// and it equals the JSON true, so `serial` breaks the tie. Read as the String "true" it would sort before "zebra",
/// and read as no value after the JSON true
/// Clause: 4.23.1, 4.23.2, 4.23.3, 4.9, C.6
#[gtest]
#[tokio::test]
async fn n_qry_ord_9390_a_compound_member_declared_as_an_xsd_boolean_sorts_among_the_booleans() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let gauge_type = namespace.entity_type("Gauge")?;
    let word_id = namespace.urn("Gauge", "word")?;
    let declared_id = namespace.urn("Gauge", "declared")?;
    let plain_id = namespace.urn("Gauge", "plain")?;
    let gauge = |id: &EntityId, flag: Value, serial: i64| {
        Entity::builder()
            .id(id.clone())
            .entity_type(gauge_type.clone())
            .property("reading", object([("flag", flag)]))
            .property("serial", serial)
            .build()
    };
    let entities = [
        gauge(&word_id, Value::from("zebra"), 3)?,
        gauge(&declared_id, xsd_typed("boolean", "true")?, 1)?,
        gauge(&plain_id, Value::from(true), 2)?,
    ];
    broker.entities().batch_create(&entities).send().await?.done()?;
    let page = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .query()
        .entity_type(&gauge_type)
        .locality(Locality::Local)
        .ordering("reading[flag],serial".parse::<OrderBy>()?)
        .send()
        .await?;
    expect_that!(page.items(), len(eq(3)));
    for (position, id) in [&word_id, &declared_id, &plain_id].into_iter().enumerate() {
        expect_that!(page.items(), nth(position, has_id(id)));
    }
    Ok(())
}
