//! An entity past its expires at is not found before the sweep deletes it.

use andromeda_test_suite::{broker, instant_ahead::minutes_ahead, namespace::Namespace, outcome::Done, problem::problem};
use googletest::prelude::*;
use http::StatusCode;
use ngsi_ld::{
    http::headers::Accept,
    model::{DateTime, Entity, ErrorType, NgsiLdMediaType},
};

/// Pins: Retrieve Entity, right after Create Entity of an Entity whose `expiresAt` passed a minute ago, answers 404
/// `ResourceNotFound` whether or not the expiry sweep has deleted it yet
/// Clause: 4.22, 5.6.3.4
#[gtest]
#[tokio::test]
async fn n_ent_ret_9042_an_entity_past_its_expires_at_is_not_found_before_the_sweep_deletes_it() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let alert_expired_id = namespace.urn("Alert", "expired")?;
    let alert_type = namespace.entity_type("Alert")?;
    let a_minute_ago: DateTime = minutes_ahead(-1).parse()?;
    let alert_expired = Entity::builder()
        .id(alert_expired_id.clone())
        .entity_type(alert_type)
        .property("severity", "high")
        .expires_at(a_minute_ago)
        .build()?;
    broker.entities().create(&alert_expired).send().await?.done()?;
    let retrieved = broker
        .with_responses()
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .retrieve(&alert_expired_id)
        .send()
        .await;
    expect_that!(&retrieved, problem(StatusCode::NOT_FOUND, ErrorType::ResourceNotFound));
    Ok(())
}
