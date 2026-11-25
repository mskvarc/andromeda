//! A `valueType`, `objectType` or `vocab` the request's @context expands to no IRI is bad request data.

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
use serde_json::{Value, json};

/// Values whose expansion under `{"ex": "http://example.org/"}` is no absolute IRI: an IRI admits no `{` and no `%`
/// that two hexadecimal digits do not follow (IETF RFC 3987), and `urn` and `https` are no terms of the context.
const UNEXPANDABLE_VALUES: [&str; 4] = ["ex:a{b", "ex:%", "urn:x:{", "https://example.org/{"];

/// The attribute `name` of each family that names a type or a vocabulary term, written with `value`.
fn attributes(value: &str) -> [(&'static str, Value); 3] {
    [
        ("speed", json!({"type": "Property", "value": 1, "valueType": value})),
        ("owner", json!({"type": "Relationship", "object": "urn:ngsi-ld:Person:o", "objectType": value})),
        ("category", json!({"type": "VocabProperty", "vocab": value})),
    ]
}

/// Pins: Create Entity whose @context defines the prefix ex and whose Property speed has the valueType, whose
/// Relationship owner has the objectType or whose `VocabProperty` category has the vocab `ex:a{b`, `ex:%`, `urn:x:{` or
/// `https://example.org/{` answers 400 `BadRequestData` for each and creates nothing
/// Clause: 4.5.2.2, 4.5.3.2, 4.6.2, 5.6.1.4
/// Recorded: Andromeda refuses a type or vocabulary term its @context expands to no absolute IRI, which would
/// otherwise be stored as a name that reads back as another
#[gtest]
#[tokio::test]
async fn o_ent_cre_9058_a_value_type_object_type_or_vocab_that_expands_to_no_iri_is_bad_request_data() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let mut position = 0_u32;
    for value in UNEXPANDABLE_VALUES {
        for (name, attribute) in attributes(value) {
            position += 1;
            let vehicle_id = namespace.urn("Vehicle", position)?;
            // The value is the point of the test and the model refuses it, so the body is sent as written.
            let body = json!({
                "id": vehicle_id.to_string(),
                "type": format!("Vehicle_{namespace}"),
                name: attribute,
                "@context": {"ex": "http://example.org/"}
            });
            let request = request_to(&broker, Method::POST, "entities")?
                .append_header(CONTENT_TYPE, HeaderValue::from_static("application/ld+json"))
                .body(Bytes::from(serde_json::to_vec(&body)?));
            let answer = broker.send_unchecked(request).await;
            expect_that!(&answer, problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData), "{name} {value}");
            let answer_2 = broker
                .with_responses()
                .with_accept(Accept::new(NgsiLdMediaType::Json))
                .entities()
                .retrieve(&vehicle_id)
                .send()
                .await;
            expect_that!(&answer_2, answered(StatusCode::NOT_FOUND), "{name} {value}");
        }
    }
    Ok(())
}
