//! A 207 is json without a context whatever the accept.

use andromeda_test_suite::{
    body_json::{json_absent, json_exists},
    broker,
    namespace::Namespace,
    outcome::Done,
    partial_outcome::PartialSuccess,
    report_matcher::batch_error,
    wire::{header_value, header_values},
};
use googletest::prelude::*;
use http::{
    StatusCode,
    header::{CONTENT_TYPE, LINK},
};
use ngsi_ld::{
    http::headers::Accept,
    model::{Entity, ErrorType, NgsiLdMediaType},
};
use serde_json::Value;

/// Pins: a Batch Entity Creation that answers 207 does so as application/json with neither an @context member nor a
/// Link header, even when application/ld+json was asked for, and reports the existing entity as `AlreadyExists`
/// Clause: 6.3.6, 5.6.7.4, 5.6.7.5, 6.14.3.1
#[gtest]
#[tokio::test]
async fn n_bat_res_0001_a_207_is_json_without_a_context_whatever_the_accept() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let building_existing_id = namespace.urn("Building", "existing")?;
    let building_type = namespace.entity_type("Building")?;
    let building_fresh_id = namespace.urn("Building", "fresh")?;
    let building_existing = Entity::builder().id(building_existing_id.clone()).entity_type(building_type.clone()).build()?;
    broker.entities().create(&building_existing).send().await?.done()?;
    let entities = [
        Entity::builder().id(building_existing_id.clone()).entity_type(building_type.clone()).build()?,
        Entity::builder().id(building_fresh_id.clone()).entity_type(building_type.clone()).build()?,
    ];
    let received = broker
        .with_responses()
        .with_accept(Accept::new(NgsiLdMediaType::JsonLd))
        .entities()
        .batch_create(&entities)
        .send()
        .await?;
    expect_that!(received.status, eq(StatusCode::MULTI_STATUS));
    expect_that!(header_value(received.head.headers(), &CONTENT_TYPE), some(starts_with("application/json")));
    expect_that!(header_values(received.head.headers(), &LINK), is_empty());
    // The absent `@context` member is a fact of the bytes, so the 207 document is read as received.
    let document = serde_json::from_slice::<Value>(&received.body)?;
    expect_that!(document.is_object(), eq(true));
    expect_that!(&received.body, json_exists("$.success"));
    expect_that!(&received.body, json_absent("$['@context']"));
    let report = received.outcome.partial()?;
    expect_that!(&report.success, len(eq(1)));
    expect_that!(&report.success, contains(eq(&building_fresh_id)));
    expect_that!(&report.errors, len(eq(1)));
    expect_that!(
        &report.errors,
        contains(batch_error().entity(&building_existing_id).problem(ErrorType::AlreadyExists))
    );
    Ok(())
}
