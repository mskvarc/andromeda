//! A respond async upsert creating an entity is selected by q.

use andromeda_test_suite::{
    broker,
    eventually::retried,
    identity_matcher::has_id,
    namespace::Namespace,
    respond_async::{async_applied, respond_async},
};
use googletest::prelude::*;
use http::StatusCode;
use ngsi_ld::{
    http::{headers::Accept, operation::BatchUpsert},
    model::{Entity, NgsiLdMediaType, Number, Value},
    query::{Locality, q},
};

/// Pins: Batch Entity Upsert of a new Entity with temperature 17.25 and Prefer: respond-async answers 201 with
/// Preference-Applied: respond-async, and q temperature==17.25 then selects the Entity
/// Clause: 5.6.8, 4.9, 5.7.2
/// Recorded: Andromeda extension, not v1.9.1: Prefer: respond-async on batch operations (IETF RFC 7240 section
/// 4.1)
#[gtest]
#[tokio::test]
async fn a_bat_ups_9015_a_respond_async_upsert_creating_an_entity_is_selected_by_q() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let building_id = namespace.urn("Building", "replayed-projection")?;
    let building_type = namespace.entity_type("Building")?;
    let entities = [Entity::builder()
        .id(building_id.clone())
        .entity_type(building_type.clone())
        .property("temperature", Value::Number("17.25".parse::<Number>()?))
        .build()?];
    let request = respond_async(broker.unchecked(&BatchUpsert::builder().entities(&entities).build())?);
    let answer = broker.send_unchecked(request).await;
    expect_that!(&answer, async_applied(StatusCode::CREATED));
    retried(async || {
        let page = broker
            .with_accept(Accept::new(NgsiLdMediaType::Json))
            .entities()
            .query()
            .entity_type(&building_type)
            .locality(Locality::Local)
            .q(q::attr("temperature").eq("17.25".parse::<Number>()?))
            .send()
            .await?;
        verify_that!(page.items(), len(eq(1)))?;
        verify_that!(page.items(), contains(has_id(&building_id)))?;
        Ok(())
    })
    .await?;
    Ok(())
}
