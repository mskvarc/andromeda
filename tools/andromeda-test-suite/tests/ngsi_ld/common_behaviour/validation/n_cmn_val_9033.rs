//! Every reader names a body that is not json in the same words.

use andromeda_test_suite::{broker, namespace::Namespace, problem::problem, wire::request_to};
use bytes::Bytes;
use googletest::prelude::*;
use http::{HeaderValue, Method, StatusCode, header::CONTENT_TYPE};
use ngsi_ld::{client::ClientError, model::ErrorType};

/// The detail of the problem `answer` carries, if it carries one.
fn detail_of<T, E>(answer: &Result<T, ClientError<E>>) -> Option<&str> {
    answer
        .as_ref()
        .err()
        .and_then(ClientError::problem)
        .and_then(|problem| problem.detail.as_deref())
}

/// Pins: A request body that is not a JSON document is refused as `InvalidRequest` with the detail "request body is
/// not a JSON document: <parser error>" by the JSON-LD entity reader, the batch reader, the temporal reader and the
/// hosted context reader alike
/// Clause: 5.5.4, 6.3.4
#[gtest]
#[tokio::test]
async fn n_cmn_val_9033_every_reader_names_a_body_that_is_not_json_in_the_same_words() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    // A body that is not JSON is the defect under test, so each body is sent as written.
    let bodies = [
        (Method::POST, "entities", format!("{{\"id\": \"urn:ngsi-ld:Vehicle:{namespace}-1\",\n")),
        (
            Method::POST,
            "entityOperations/create",
            format!("[{{\"id\": \"urn:ngsi-ld:Vehicle:{namespace}-2\",\n"),
        ),
        (Method::POST, "temporal/entities", format!("{{\"id\": \"urn:ngsi-ld:Vehicle:{namespace}-3\",\n")),
        (Method::POST, "jsonldContexts", "{\"@context\": {\n".to_owned()),
    ];
    for (method, path, body) in bodies {
        let request = request_to(&broker, method, path)?
            .append_header(CONTENT_TYPE, HeaderValue::from_static("application/json"))
            .body(Bytes::from(body));
        let answer = broker.send_unchecked(request).await;
        expect_that!(&answer, problem(StatusCode::BAD_REQUEST, ErrorType::InvalidRequest));
        expect_that!(detail_of(&answer), some(starts_with("request body is not a JSON document: ")));
    }
    Ok(())
}
