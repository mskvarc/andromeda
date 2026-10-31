//! A repetition count past 255 is bad request data.

use andromeda_test_suite::{broker, identity_matcher::has_id, namespace::Namespace, nth::nth, outcome::Done, problem::problem};
use googletest::prelude::*;
use http::StatusCode;
use ngsi_ld::{
    http::headers::Accept,
    model::{Entity, ErrorType, NgsiLdMediaType},
    query::{IdPattern, q},
};

/// Pins: a q pattern or an `idPattern` repeating an atom 256 times answers 400 `BadRequestData`, while a repetition
/// bound of 255 is evaluated and answers 200
/// Clause: 4.9, 5.7.2.4
/// Recorded: IEEE 1003.2 guarantees repetition counts up to `RE_DUP_MAX`, at least 255, and leaves larger ones to
/// the implementation; Andromeda refuses them as `BadRequestData`
#[gtest]
#[tokio::test]
async fn o_qry_qlg_0048_a_repetition_count_past_255_is_bad_request_data() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let counted_id = namespace.urn("Counted", 1)?;
    let counted_type = namespace.entity_type("Counted")?;
    let counted = Entity::builder()
        .id(counted_id.clone())
        .entity_type(counted_type.clone())
        .property("name", "a")
        .build()?;
    broker.entities().create(&counted).send().await?.done()?;
    let answer = broker
        .with_responses()
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .query()
        .entity_type(&counted_type)
        .q(q::attr("name").matches("\"a{256}\""))
        .send()
        .await;
    expect_that!(&answer, problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData));
    let answer_2 = broker
        .with_responses()
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .query()
        .entity_type(&counted_type)
        .id_pattern("a{256}".parse::<IdPattern>()?)
        .send()
        .await;
    expect_that!(&answer_2, problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData));
    let page = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .query()
        .entity_type(&counted_type)
        .q(q::attr("name").matches("\"^a{0,255}$\""))
        .send()
        .await?;
    expect_that!(page.items(), len(eq(1)));
    expect_that!(page.items(), nth(0, has_id(&counted_id)));
    Ok(())
}
