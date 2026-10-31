//! Purge entities with a limit is invalid request and purges nothing.

use andromeda_test_suite::{broker, namespace::Namespace, outcome::Done, problem::problem, raw_call::target, unchecked_call::exchange};
use googletest::prelude::*;
use http::{Method, StatusCode};
use ngsi_ld::{
    http::{headers::Accept, operation::ResultsCount},
    model::{Entity, ErrorType, NgsiLdMediaType},
};

/// Pins: Purge Entities with a type and limit 5 answers 400 `InvalidRequest`, and both entities of the type still
/// count
/// Clause: 6.4.3.3, 6.3.20
/// Recorded: Table 6.4.3.3-1 defines no limit for Purge Entities and 6.3.20 says a parameter the operation does not
/// define "should" give `InvalidRequest`
#[gtest]
#[tokio::test]
async fn o_qry_pag_0035_purge_entities_with_a_limit_is_invalid_request_and_purges_nothing() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let building_type = namespace.entity_type("Building")?;
    let entities = [
        Entity::builder().id(namespace.urn("Building", 1)?).entity_type(building_type.clone()).build()?,
        Entity::builder().id(namespace.urn("Building", 2)?).entity_type(building_type.clone()).build()?,
    ];
    broker.entities().batch_create(&entities).send().await?.done()?;
    // The typed Purge takes no limit, so the call goes out as written.
    let type_text = building_type.to_string();
    let purge = target("entities", &[("type", type_text.as_str()), ("limit", "5")]);
    let answer = broker.send_unchecked(exchange(&broker, Method::DELETE, &purge, &[], None)?).await;
    expect_that!(&answer, problem(StatusCode::BAD_REQUEST, ErrorType::InvalidRequest));
    let page = broker
        .with_accept(Accept::new(NgsiLdMediaType::JsonLd))
        .entities()
        .query()
        .entity_type(&building_type)
        .limit(0)
        .count(ResultsCount::Include)
        .send()
        .await?;
    expect_that!(page.total(), some(eq(2)));
    Ok(())
}
