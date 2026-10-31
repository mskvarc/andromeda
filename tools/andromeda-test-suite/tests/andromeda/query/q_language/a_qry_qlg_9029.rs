//! A repeated duplication symbol is bad request data.

use andromeda_test_suite::{broker, namespace::Namespace, outcome::Done, problem::problem};
use googletest::prelude::*;
use http::StatusCode;
use ngsi_ld::{
    http::headers::Accept,
    model::{Entity, ErrorType, NgsiLdMediaType},
    query::{Locality, q},
};

/// Pins: Query Entities with the q pattern Merc** answers 400 `BadRequestData`
/// Clause: 4.9
/// Recorded: IEEE 1003.2 leaves a backslash before an ordinary character, an inline option, a repeated duplication
/// symbol, an undefined class name and a range outside the POSIX locale undefined or unspecified; Andromeda refuses
/// each with `BadRequestData` so that the stored-query and the subscription evaluators never read one pattern two
/// ways
#[gtest]
#[tokio::test]
async fn a_qry_qlg_9029_a_repeated_duplication_symbol_is_bad_request_data() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let building_repeated_quantifier_id = namespace.urn("Building", "repeated-quantifier")?;
    let building_type = namespace.entity_type("Building")?;
    let building_repeated_quantifier = Entity::builder()
        .id(building_repeated_quantifier_id.clone())
        .entity_type(building_type.clone())
        .property("name", "Mercedes")
        .build()?;
    broker.entities().create(&building_repeated_quantifier).send().await?.done()?;
    let answer = broker
        .with_responses()
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .query()
        .entity_type(&building_type)
        .locality(Locality::Local)
        .ids(vec![building_repeated_quantifier_id.clone()])
        .q(q::attr("name").matches("\"Merc**\""))
        .send()
        .await;
    expect_that!(&answer, problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData));
    Ok(())
}
