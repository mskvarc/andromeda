//! An item with no uri fails the request and deletes nothing.

use andromeda_test_suite::{
    broker,
    namespace::Namespace,
    outcome::Done,
    problem::{answered, problem},
    written_json::json_request_to,
};
use googletest::prelude::*;
use http::{Method, StatusCode};
use ngsi_ld::{
    http::headers::Accept,
    model::{Entity, ErrorType, NgsiLdMediaType},
};

/// Pins: Batch Entity Delete of an existing entity beside a null item, the number 42 or a string that is not a URI
/// answers 400 `BadRequestData` naming the item, and deletes nothing each time
/// Clause: 5.6.10.3, 5.6.10.4, Table 5.2.17-1, 5.5.4
#[gtest]
#[tokio::test]
async fn o_bat_del_9001_an_item_with_no_uri_fails_the_request_and_deletes_nothing() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let batch_entity_error_existing_id = namespace.urn("BatchEntityError", "existing")?;
    let batch_entity_error_type = namespace.entity_type("BatchEntityError")?;
    let batch_entity_error_existing = Entity::builder()
        .id(batch_entity_error_existing_id.clone())
        .entity_type(batch_entity_error_type)
        .property("speed", 1)
        .build()?;
    let entities = [batch_entity_error_existing];
    let created = broker.entities().batch_create(&entities).send().await?.done()?;
    expect_that!(created, contains(eq(&batch_entity_error_existing_id)));
    let existing = format!(r#""urn:ngsi-ld:BatchEntityError:{namespace}-existing""#);
    let cases = [
        (format!("[{existing}, null]"), "batch item 1 is null"),
        (format!("[{existing}, 42]"), "batch item 1 is not an entity id string"),
        (
            format!(r#"[{existing}, "not a uri"]"#),
            "the 'id' of batch item 1 is not a URI: invalid URI: not a valid URN, URL or IRI",
        ),
    ];
    for (body, detail) in &cases {
        // A batch delete body other than ids is no document the model can write, so it is sent as written.
        let request = json_request_to(&broker, Method::POST, "entityOperations/delete", body)?;
        let answer = broker.send_unchecked(request).await;
        expect_that!(&answer, problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData).with_detail(detail));
        let answer_2 = broker
            .with_responses()
            .with_accept(Accept::new(NgsiLdMediaType::Json))
            .entities()
            .retrieve(&batch_entity_error_existing_id)
            .send()
            .await;
        expect_that!(&answer_2, answered(StatusCode::OK));
    }
    Ok(())
}
