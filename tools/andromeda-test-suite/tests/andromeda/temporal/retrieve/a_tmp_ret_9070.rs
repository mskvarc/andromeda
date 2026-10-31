//! A respond async delete after the evolution was deleted records nothing.

use andromeda_test_suite::{
    broker,
    namespace::Namespace,
    outcome::Upserted,
    problem::problem,
    respond_async::{async_applied, respond_async},
};
use googletest::prelude::*;
use http::StatusCode;
use ngsi_ld::{
    http::{headers::Accept, operation::BatchDelete},
    model::{ErrorType, NgsiLdMediaType, Property, evolution::TemporalEntity},
    query::temporal_query,
};

/// Pins: Retrieve Temporal Evolution of an Entity whose Temporal Evolution was deleted before a Batch Entity Delete
/// answered 204 with Preference-Applied: respond-async deleted the Entity itself, sent straight after it with no
/// retry, answers 404 `ResourceNotFound`, and Query Temporal Evolution of its type answers nothing
/// Clause: 5.6.16, 5.6.10, 5.7.3, 5.7.4
/// Recorded: Andromeda extension, not v1.9.1: Prefer: respond-async on batch operations (IETF RFC 7240 section
/// 4.1), whose history is queued with the write and answered by every temporal read before the queue is applied;
/// clause 5.6.16 deletes the Temporal Evolution, and the later clause 5.6.10 deletion of the Entity ends nothing of
/// an evolution that no longer exists, so the end it queues is shown in none and creates none
#[gtest]
#[tokio::test]
async fn a_tmp_ret_9070_a_respond_async_delete_after_the_evolution_was_deleted_records_nothing() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let car_id = namespace.urn("Car", 1)?;
    let car_type = namespace.entity_type("Car")?;
    let car = TemporalEntity::builder()
        .id(car_id.clone())
        .entity_type(car_type.clone())
        .property("speed", Property::builder(42).observed_at("2026-03-01T10:00:00Z".parse()?))
        .build()?;
    broker.temporal().create_or_update(&car).send().await?.created()?;
    broker.temporal().delete(&car_id).send().await?;
    let ids = [car_id.clone()];
    let request = respond_async(broker.unchecked(&BatchDelete { ids: &ids })?);
    let answer = broker.send_unchecked(request).await;
    expect_that!(&answer, async_applied(StatusCode::NO_CONTENT));
    // Asked at once, with no retry.
    let absent = broker
        .with_responses()
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .temporal()
        .retrieve(&car_id)
        .send()
        .await;
    expect_that!(&absent, problem(StatusCode::NOT_FOUND, ErrorType::ResourceNotFound));
    let page = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .temporal()
        .query(temporal_query::after("2026-01-01T00:00:00Z".parse()?))
        .entity_type(&car_type)
        .send()
        .await?;
    expect_that!(page.items(), len(eq(0)));
    Ok(())
}
