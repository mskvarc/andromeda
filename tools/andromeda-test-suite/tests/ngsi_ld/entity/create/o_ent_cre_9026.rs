//! An attribute named twice is bad request data naming it.

use andromeda_test_suite::{
    broker,
    namespace::Namespace,
    problem::{answered, problem},
    wire::request_to,
};
use bytes::Bytes;
use googletest::prelude::*;
use http::{HeaderValue, Method, StatusCode, header::CONTENT_TYPE};
use ngsi_ld::{
    http::headers::Accept,
    model::{ErrorType, NgsiLdMediaType},
};

/// Pins: Create Entity whose body names the attribute speed twice answers 400 `BadRequestData` whose detail names
/// /speed and the repetition, and creates nothing
/// Clause: 5.5.4, 5.6.1.4
/// Recorded: IETF RFC 8259 section 4 says the names within an object "SHOULD be unique" and leaves a repeated name
/// unpredictable; Andromeda refuses one as `BadRequestData` naming it, where keeping the last would drop the first
/// without telling the client
#[gtest]
#[tokio::test]
async fn o_ent_cre_9026_an_attribute_named_twice_is_bad_request_data_naming_it() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let vehicle_repeated_attribute_id = namespace.urn("Vehicle", "repeated-attribute")?;
    // The body names a member twice, which the model cannot write, so it is sent as written.
    let request = request_to(&broker, Method::POST, "entities")?.append_header(CONTENT_TYPE, HeaderValue::from_static("application/json")).body(Bytes::from(format!("{{\"id\": \"urn:ngsi-ld:Vehicle:{namespace}-repeated-attribute\", \"type\": \"RepetitionProbe_{namespace}\", \"speed\": {{\"type\": \"Property\", \"value\": 10}}, \"speed\": {{\"type\": \"Property\", \"value\": 20}}}}")));
    let answer = broker.send_unchecked(request).await;
    expect_that!(
        &answer,
        problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData)
            .with_detail_containing("/speed")
            .with_detail_containing("named twice")
    );
    let answer_2 = broker
        .with_responses()
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .retrieve(&vehicle_repeated_attribute_id)
        .send()
        .await;
    expect_that!(&answer_2, answered(StatusCode::NOT_FOUND));
    Ok(())
}
