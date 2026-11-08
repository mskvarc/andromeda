//! A `PUT` or `PATCH` on the Entities collection gets a 405 problem document.

use andromeda_test_suite::{
    answered_status::AnsweredStatus,
    broker,
    problem::{instance_of, problem},
    wire::{header_values, request_to},
};
use bytes::Bytes;
use googletest::prelude::*;
use http::{
    HeaderValue,
    Method,
    StatusCode,
    header::{ALLOW, CONTENT_TYPE},
};
use ngsi_ld::model::ErrorType;

/// Pins: a `PUT` or a `PATCH` on `/entities`, with or without a trailing slash and with a JSON body, is answered 405
/// with a `MethodNotAllowed` problem document naming the refused method, and an Allow header naming `GET`, `POST`
/// and `DELETE` and neither refused method
/// Clause: 6.3.2, 6.4.3, 6.5.3.3, 6.5.3.4
/// Recorded: the collection serves Create Entity, Query Entities and Purge Entities only (clause 6.4.3); Replace and
/// Merge Entity are bound to `/entities/{entityId}`, so a request to the collection is a wrong verb over that resource
/// rather than an operation missing its target; the Allow header is IETF RFC 9110 clause 15.5.6
#[gtest]
#[tokio::test]
async fn o_cmn_err_9004_put_and_patch_on_the_entities_collection_get_a_405_problem_document() -> Result<()> {
    let broker = broker::front()?;
    let method_not_allowed = "https://uri.etsi.org/ngsi-ld/errors/MethodNotAllowed".parse::<ErrorType>()?;
    for method in [Method::PUT, Method::PATCH] {
        for path in ["entities", "entities/"] {
            // The body goes as raw JSON: no typed operation sends a PUT or a PATCH to the collection.
            let request = request_to(&broker, method.clone(), path)?
                .append_header(CONTENT_TYPE, HeaderValue::from_static("application/json"))
                .body(Bytes::from_static(b"{}"));
            let answer = broker.send_unchecked(request).await;
            expect_that!(
                &answer,
                problem(StatusCode::METHOD_NOT_ALLOWED, method_not_allowed.clone())
                    .with_detail_containing(method.as_str())
                    .with_header_containing(ALLOW, "GET")
                    .with_header_containing(ALLOW, "POST")
                    .with_header_containing(ALLOW, "DELETE"),
                "{method} {path}"
            );
            let allow = answer.answered_headers().map(|headers| header_values(headers, &ALLOW)).unwrap_or_default();
            expect_that!(
                allow,
                each(not(any![contains_substring("PUT"), contains_substring("PATCH")])),
                "{method} {path}"
            );
            expect_that!(instance_of(&answer), some(eq("/ngsi-ld/v1/entities")), "{method} {path}");
        }
    }
    Ok(())
}
