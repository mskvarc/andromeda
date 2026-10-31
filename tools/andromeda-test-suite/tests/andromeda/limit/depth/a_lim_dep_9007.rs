//! An attribute of 61 levels is refused and not stored.

use andromeda_test_suite::{
    broker,
    namespace::Namespace,
    problem::{answered, problem},
    value::object,
};
use googletest::prelude::*;
use http::StatusCode;
use ngsi_ld::{
    http::headers::Accept,
    model::{Entity, ErrorType, NgsiLdMediaType, Value},
};

/// Pins: Create Entity with p whose value nests 61 objects answers 400 `BadRequestData` naming p and the 32-level
/// limit, and the entity does not exist
/// Clause: 5.6.1.4, 5.5.4
#[gtest]
#[tokio::test]
async fn a_lim_dep_9007_an_attribute_of_61_levels_is_refused_and_not_stored() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let nesting_probe_id = namespace.urn("NestingProbe", 1)?;
    let nesting_probe_type = namespace.entity_type("NestingProbe")?;
    let nesting_probe = Entity::builder()
        .id(nesting_probe_id.clone())
        .entity_type(nesting_probe_type)
        .property("p", (0..61).fold(Value::from("leaf"), |inner, _| object([("k", inner)])))
        .build()?;
    let answer = broker.with_responses().entities().create(&nesting_probe).send().await;
    expect_that!(
        &answer,
        problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData)
            .with_detail_containing("'https://uri.etsi.org/ngsi-ld/default-context/p'")
            .with_detail_containing("32 levels")
    );
    let answer_2 = broker
        .with_responses()
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .retrieve(&nesting_probe_id)
        .send()
        .await;
    expect_that!(&answer_2, answered(StatusCode::NOT_FOUND));
    Ok(())
}
