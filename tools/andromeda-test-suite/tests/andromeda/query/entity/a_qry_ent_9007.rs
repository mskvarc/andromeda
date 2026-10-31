//! An id pattern holding a unicode property class is bad request data.

use andromeda_test_suite::{broker, namespace::Namespace, outcome::Done, problem::problem};
use googletest::prelude::*;
use http::StatusCode;
use ngsi_ld::{
    http::headers::Accept,
    model::{Entity, ErrorType, NgsiLdMediaType},
    query::{IdPattern, Locality},
};

/// Pins: Query Entities with `idPattern` \p{L} answers 400 `BadRequestData`, not a broker fault
/// Clause: 4.9, 5.7.2.4
/// Recorded: IEEE 1003.2 leaves a backslash before an ordinary character, an inline option, a repeated duplication
/// symbol, an undefined class name and a range outside the POSIX locale undefined or unspecified; Andromeda refuses
/// each with `BadRequestData` so that the stored-query and the subscription evaluators never read one pattern two
/// ways
#[gtest]
#[tokio::test]
async fn a_qry_ent_9007_an_id_pattern_holding_a_unicode_property_class_is_bad_request_data() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let building_dialect_a_id = namespace.urn("Building", "dialect-a")?;
    let building_type = namespace.entity_type("Building")?;
    let building_dialect_a = Entity::builder()
        .id(building_dialect_a_id.clone())
        .entity_type(building_type.clone())
        .property("name", "Mercedes")
        .build()?;
    broker.entities().create(&building_dialect_a).send().await?.done()?;
    let answer = broker
        .with_responses()
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .query()
        .entity_type(&building_type)
        .locality(Locality::Local)
        .ids(vec![building_dialect_a_id.clone()])
        .id_pattern("\\p{L}".parse::<IdPattern>()?)
        .send()
        .await;
    expect_that!(&answer, problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData));
    Ok(())
}
