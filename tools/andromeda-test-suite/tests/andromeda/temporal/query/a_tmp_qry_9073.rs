//! A respond async upsert changing the type is answered under both types at once.

use andromeda_test_suite::{
    broker,
    evolution_matcher::has_evolution,
    identity_matcher::has_id,
    namespace::Namespace,
    nth::nth,
    outcome::Done,
    respond_async::{async_applied, respond_async},
    type_matcher::lists_type,
};
use googletest::prelude::*;
use http::StatusCode;
use ngsi_ld::{
    http::{headers::Accept, operation::BatchUpsert},
    model::{Entity, NgsiLdMediaType, Property},
    query::temporal_query,
};

/// Pins: Query Temporal Evolution of type B and of type A sent straight after a Batch Entity Upsert answered 204
/// with Preference-Applied: respond-async replaced an Entity of type A with one of type B, with no retry, each
/// answers the Entity under both types with its two level instances
/// Clause: 5.6.8, 5.7.4.4, 4.5.6
/// Recorded: Andromeda extension, not v1.9.1: Prefer: respond-async on batch operations (IETF RFC 7240 section
/// 4.1), whose history is queued with the write and answered by every temporal read before the queue is applied;
/// clause 4.5.6 makes the Temporal Evolution the Entity of clause 4.5.1, whose list of Entity Type names Andromeda
/// only grows, and clause 5.7.4.4 selects by that list whether or not the queued history has been applied
#[gtest]
#[tokio::test]
async fn a_tmp_qry_9073_a_respond_async_upsert_changing_the_type_is_answered_under_both_types_at_once() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let shape_id = namespace.urn("Shape", 1)?;
    let type_a = namespace.entity_type("A")?;
    let type_b = namespace.entity_type("B")?;
    let shape = Entity::builder()
        .id(shape_id.clone())
        .entity_type(type_a.clone())
        .property("level", Property::builder(1).observed_at("2026-03-01T10:00:00Z".parse()?))
        .build()?;
    broker.entities().create(&shape).send().await?.done()?;
    let replacement = [Entity::builder()
        .id(shape_id.clone())
        .entity_type(type_b.clone())
        .property("level", Property::builder(2).observed_at("2026-03-01T11:00:00Z".parse()?))
        .build()?];
    let request = respond_async(broker.unchecked(&BatchUpsert::builder().entities(&replacement).build())?);
    let answer = broker.send_unchecked(request).await;
    expect_that!(&answer, async_applied(StatusCode::NO_CONTENT));
    // Asked at once, with no retry.
    for queried in [&type_b, &type_a] {
        let page = broker
            .with_accept(Accept::new(NgsiLdMediaType::Json))
            .temporal()
            .query(temporal_query::after("2026-01-01T00:00:00Z".parse()?))
            .entity_type(queried)
            .send()
            .await?;
        expect_that!(page.items(), len(eq(1)));
        expect_that!(
            page.items(),
            nth(
                0,
                all!(has_id(&shape_id), lists_type(&type_a), lists_type(&type_b), has_evolution("level", len(eq(2))))
            )
        );
    }
    Ok(())
}
