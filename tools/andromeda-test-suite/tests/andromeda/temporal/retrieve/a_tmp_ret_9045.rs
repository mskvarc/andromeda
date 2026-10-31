//! A deleted evolution records no later deletion.

use andromeda_test_suite::{
    broker,
    config::BrokerRole,
    namespace::Namespace,
    outcome::{Done, Upserted},
    problem::problem,
};
use googletest::prelude::*;
use http::StatusCode;
use ngsi_ld::{
    http::headers::Accept,
    model::{ErrorType, NgsiLdMediaType, Property, evolution::TemporalEntity},
    query::temporal_query,
};

/// Pins: on a broker that defers its history, Retrieve Temporal Evolution of an Entity whose Temporal Evolution was
/// deleted before the Entity itself answers 404 `ResourceNotFound`, and so does Query Temporal Evolution of its type,
/// whether or not the later writes have been applied
/// Clause: 5.6.16, 5.6.6
/// Recorded: the deferred temporal append mode is Andromeda's; clause 5.6.16 deletes the Temporal Evolution, and the
/// later clause 5.6.6 deletion of the Entity ends nothing of an evolution that no longer exists, so it records
/// nothing and creates no evolution
#[gtest]
#[tokio::test]
async fn a_tmp_ret_9045_a_deleted_evolution_records_no_later_deletion() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker_deferred = broker::client(BrokerRole::Deferred)?;
    let car_id = namespace.urn("Car", 1)?;
    let car_type = namespace.entity_type("Car")?;
    let car = TemporalEntity::builder()
        .id(car_id.clone())
        .entity_type(car_type.clone())
        .property("speed", Property::builder(42).observed_at("2026-03-01T10:00:00Z".parse()?))
        .build()?;
    broker_deferred.temporal().create_or_update(&car).send().await?.created()?;
    broker_deferred.temporal().delete(&car_id).send().await?;
    broker_deferred.entities().delete(&car_id).send().await?.done()?;
    let absent = broker_deferred
        .with_responses()
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .temporal()
        .retrieve(&car_id)
        .send()
        .await;
    expect_that!(&absent, problem(StatusCode::NOT_FOUND, ErrorType::ResourceNotFound));
    let page = broker_deferred
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .temporal()
        .query(temporal_query::after("2026-01-01T00:00:00Z".parse()?))
        .entity_type(&car_type)
        .send()
        .await?;
    expect_that!(page.items(), len(eq(0)));
    Ok(())
}
