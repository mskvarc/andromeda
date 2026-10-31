//! An attribute the expiry sweep removed is not found.

use andromeda_test_suite::{broker, eventually::retried, namespace::Namespace, outcome::Done, problem::problem, type_information::attribute_names};
use chrono::{TimeDelta, Utc};
use googletest::prelude::*;
use http::StatusCode;
use ngsi_ld::{
    http::headers::Accept,
    model::{DateTime, Entity, ErrorType, Property},
};

/// Pins: an attribute instance with an `expiresAt` is available until the broker removes it after that instant, and
/// once removed its attribute information is `ResourceNotFound` and the type no longer lists it
/// Clause: 4.22, 5.7.10.4, 5.7.7.4
/// Recorded: 4.22 lets an expired attribute "be automatically removed" by a periodic clean-up whose lag the spec
/// leaves open; Andromeda removes it, and the file waits up to 60 seconds for that
#[gtest]
#[tokio::test]
async fn o_dsc_ati_0004_an_attribute_the_expiry_sweep_removed_is_not_found() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let building_id = namespace.urn("Building", 1)?;
    let building_type = namespace.entity_type("Building")?;
    let height = namespace.attribute("height")?;
    let alarm = namespace.attribute("alarm")?;
    let in_10_seconds: DateTime = (Utc::now() + TimeDelta::seconds(10)).format("%Y-%m-%dT%H:%M:%SZ").to_string().parse()?;
    let building = Entity::builder()
        .id(building_id)
        .entity_type(building_type.clone())
        .property(namespace.attribute("height")?, 1)
        .property(namespace.attribute("alarm")?, Property::builder(2).expires_at(in_10_seconds))
        .build()?;
    broker.entities().create(&building).send().await?.done()?;
    let information = broker.with_accept(Accept::any()).attributes().retrieve(&alarm).send().await?;
    expect_that!(information.attribute_count, some(eq(1)));
    retried(async || {
        let answer = broker.with_responses().with_accept(Accept::any()).attributes().retrieve(&alarm).send().await;
        verify_that!(&answer, problem(StatusCode::NOT_FOUND, ErrorType::ResourceNotFound))?;
        Ok(())
    })
    .await?;
    let information_2 = broker.with_accept(Accept::any()).entity_types().retrieve(&building_type).send().await?;
    expect_that!(information_2.entity_count, eq(1));
    expect_that!(information_2.attribute_details, len(eq(1)));
    expect_that!(attribute_names(&information_2), elements_are![eq(&height)]);
    Ok(())
}
