//! A word class repeated past the compiled size is bad request data.

use andromeda_test_suite::{broker, namespace::Namespace, outcome::Done, problem::problem};
use googletest::prelude::*;
use http::StatusCode;
use ngsi_ld::{
    http::headers::Accept,
    model::{Entity, ErrorType, NgsiLdMediaType},
    query::q,
};

/// Pins: q name~="^\w{255}$" answers 400 `BadRequestData`: 255 repetitions of the Unicode word class exceed the
/// size a pattern may compile to
/// Clause: 4.9, 5.7.2.4
/// Recorded: clause 4.9 hands `RegExp` to IEEE 1003.2, which has no inline options and no shorthand classes;
/// Andromeda admits the case-insensitive option and the shorthand classes \d, \w and \s as extensions, folding case
/// by Unicode simple case folding and classifying by Unicode; a pattern compiling past the engine's size limit is
/// refused as `BadRequestData`
#[gtest]
#[tokio::test]
async fn a_qry_qlg_0049_a_word_class_repeated_past_the_compiled_size_is_bad_request_data() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let oversized_id = namespace.urn("Oversized", 1)?;
    let oversized_type = namespace.entity_type("Oversized")?;
    let oversized = Entity::builder()
        .id(oversized_id)
        .entity_type(oversized_type.clone())
        .property("name", "a")
        .build()?;
    broker.entities().create(&oversized).send().await?.done()?;
    let answer = broker
        .with_responses()
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .query()
        .entity_type(&oversized_type)
        .q(q::attr("name").matches("\"^\\w{255}$\""))
        .send()
        .await;
    expect_that!(&answer, problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData));
    Ok(())
}
