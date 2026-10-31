//! A respond async batch create is answered by a temporal query at once.

use andromeda_test_suite::{
    attribute_matcher::property,
    broker,
    evolution_matcher::has_evolution,
    identity_matcher::has_id,
    namespace::Namespace,
    nth::nth,
    respond_async::{async_applied, respond_async},
};
use googletest::prelude::*;
use http::StatusCode;
use ngsi_ld::{
    http::{headers::Accept, operation::BatchCreate},
    model::{Entity, NgsiLdMediaType, Property},
    query::temporal_query,
};

/// Pins: Query Temporal Evolution sent straight after a Batch Entity Creation answered 201 with Preference-Applied:
/// respond-async, with no retry, answers the Entity with its one speed instance
/// Clause: 5.6.7, 5.7.4
/// Recorded: Andromeda extension, not v1.9.1: Prefer: respond-async on batch operations (IETF RFC 7240 section
/// 4.1), whose history is queued with the write and answered by every temporal read before the queue is applied;
/// clause 5.7.4 selects the Entities whose Attribute instances fall in the window, and the instance a write
/// recorded is one of them from the moment the write is acknowledged
#[gtest]
#[tokio::test]
async fn a_tmp_qry_9072_a_respond_async_batch_create_is_answered_by_a_temporal_query_at_once() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let car_id = namespace.urn("Car", 1)?;
    let car_type = namespace.entity_type("Car")?;
    let entities = [Entity::builder()
        .id(car_id.clone())
        .entity_type(car_type.clone())
        .property("speed", Property::builder(42).observed_at("2026-03-01T10:00:00Z".parse()?))
        .build()?];
    let request = respond_async(broker.unchecked(&BatchCreate::builder().entities(&entities).build())?);
    let answer = broker.send_unchecked(request).await;
    expect_that!(&answer, async_applied(StatusCode::CREATED));
    // Asked at once, with no retry.
    let page = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .temporal()
        .query(temporal_query::after("2026-01-01T00:00:00Z".parse()?))
        .entity_type(&car_type)
        .send()
        .await?;
    expect_that!(page.items(), len(eq(1)));
    expect_that!(
        page.items(),
        nth(
            0,
            all!(
                has_id(&car_id),
                has_evolution(
                    "speed",
                    all!(len(eq(1)), nth(0, property().value(42).observed_at("2026-03-01T10:00:00Z".parse()?)))
                )
            )
        )
    );
    Ok(())
}
