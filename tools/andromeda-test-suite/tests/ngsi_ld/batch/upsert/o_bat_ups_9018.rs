//! A prefixed attribute name breaking the grammar after the colon is a per entity bad request data.

use andromeda_test_suite::{
    attribute_matcher::property,
    broker,
    entity_matcher::has_attribute,
    namespace::Namespace,
    nth::nth,
    partial_outcome::reported,
    problem::answered,
    report_matcher::batch_error,
    wire::{content_type, request_to},
};
use bytes::Bytes;
use googletest::prelude::*;
use http::{Method, StatusCode};
use ngsi_ld::{
    http::{BatchOperationResult, headers::Accept},
    model::{APPLICATION_LD_JSON, ErrorType, NgsiLdMediaType},
};

/// Pins: Batch Entity Upsert refuses the entity whose attribute is named ex:bad-name under an @context defining the
/// prefix ex with `BadRequestData` in the 207 answer and creates nothing of it, while the entity beside it naming
/// `ex:good_name` is created
/// Clause: 4.6.2, 5.6.8
/// Recorded: clause 4.6.2 says an implementation "should" raise `BadRequestData` for a name outside its grammar;
/// Andromeda refuses a name the request's @context reads as a compact IRI whose part after the colon breaks it
#[gtest]
#[tokio::test]
async fn o_bat_ups_9018_a_prefixed_attribute_name_breaking_the_grammar_after_the_colon_is_a_per_entity_bad_request_data() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let vehicle_1_id = namespace.urn("Vehicle", "1")?;
    let vehicle_2_id = namespace.urn("Vehicle", "2")?;
    // The attribute names are the point of the test and the model refuses the first, and each item carries an
    // `@context` of its own, which the client does not write into a batch, so the body is sent as written.
    let body = format!(
        r#"[{{"id": "urn:ngsi-ld:Vehicle:{namespace}-1", "type": "Vehicle_{namespace}", "ex:bad-name": {{"type": "Property", "value": 1}}, "@context": {{"ex": "http://example.org/"}}}}, {{"id": "urn:ngsi-ld:Vehicle:{namespace}-2", "type": "Vehicle_{namespace}", "ex:good_name": {{"type": "Property", "value": 2}}, "@context": {{"ex": "http://example.org/"}}}}]"#
    );
    let request = content_type(request_to(&broker, Method::POST, "entityOperations/upsert")?, &APPLICATION_LD_JSON)?.body(Bytes::from(body));
    let answer = broker.send_unchecked(request).await;
    let report: BatchOperationResult = reported(answer)?;
    expect_that!(&report.errors, len(eq(1)));
    expect_that!(&report.errors, nth(0, batch_error().entity(&vehicle_1_id).problem(ErrorType::BadRequestData)));
    expect_that!(&report.success, nth(0, eq(&vehicle_2_id)));
    let answer_2 = broker
        .with_responses()
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .retrieve(&vehicle_1_id)
        .send()
        .await;
    expect_that!(&answer_2, answered(StatusCode::NOT_FOUND));
    let retrieved = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .retrieve(&vehicle_2_id)
        .send()
        .await?;
    let entity = retrieved.body();
    expect_that!(entity, has_attribute("http://example.org/good_name", property().value(2)));
    Ok(())
}
