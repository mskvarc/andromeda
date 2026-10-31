//! An item with no uri to report against fails the request and stores nothing.

use andromeda_test_suite::{
    broker,
    namespace::Namespace,
    problem::{answered, problem},
    wire::{content_type, request_to},
};
use bytes::Bytes;
use googletest::prelude::*;
use http::{Method, StatusCode};
use ngsi_ld::{
    http::headers::Accept,
    model::{APPLICATION_JSON, ErrorType, NgsiLdMediaType},
};
use serde_json::{Value, json};

/// Pins: Batch Entity Upsert of a valid entity beside a null item, an item without an id, or an item whose id is
/// not a URI answers 400 `BadRequestData` naming the item, and stores nothing each time
/// Clause: 5.6.8.4, Table 5.2.4-1, Table 5.2.17-1, 5.5.4
#[gtest]
#[tokio::test]
async fn o_bat_ups_9002_an_item_with_no_uri_to_report_against_fails_the_request_and_stores_nothing() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let valid_id = namespace.urn("BatchEntityError", "valid")?;
    let valid = json!({
        "id": valid_id.to_string(),
        "type": format!("BatchEntityError_{namespace}"),
        "speed": {"type": "Property", "value": 1}
    });
    let cases = [
        (Value::Null, "batch item 1 is null".to_owned()),
        (
            json!({"type": format!("BatchEntityError_{namespace}")}),
            "batch item 1 has no 'id' string".to_owned(),
        ),
        (
            json!({"id": format!("not a uri {namespace}"), "type": format!("BatchEntityError_{namespace}")}),
            "the 'id' of batch item 1 is not a URI: invalid URI: not a valid URN, URL or IRI".to_owned(),
        ),
    ];
    for (second_item, detail) in cases {
        // The second item is not an entity the model can write, so the body is sent as written.
        let request = content_type(request_to(&broker, Method::POST, "entityOperations/upsert")?, &APPLICATION_JSON)?
            .body(Bytes::from(json!([valid.clone(), second_item]).to_string()));
        let answer = broker.send_unchecked(request).await;
        expect_that!(&answer, problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData).with_detail(&detail));
        let answer_2 = broker
            .with_responses()
            .with_accept(Accept::new(NgsiLdMediaType::Json))
            .entities()
            .retrieve(&valid_id)
            .send()
            .await;
        expect_that!(&answer_2, answered(StatusCode::NOT_FOUND));
    }
    Ok(())
}
