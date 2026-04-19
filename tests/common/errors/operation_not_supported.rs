//! `OperationNotSupported` (422) Problem-Details tests (NGSI-LD 5.5.2).
//!
//! The broker emits this error via `ErrorKind::OperationNotSupported`; the
//! only current production path is `src/http/handler/i18n.rs:113`, which
//! rejects every non-`application/json` Accept on `/i18n/entities/*`.

#![allow(clippy::expect_used, clippy::unwrap_used)]

use andromeda_test_common::prelude::*;
use pretty_assertions::assert_eq;
use reqwest::{Method, StatusCode};
use serde_json::json;

#[tokio::test]
async fn operation_not_supported_i18n_retrieve_rejects_ld_json_accept() {
    let scope = TestScope::new("common_errors");
    let client = NgsiLdClient::new(&scope).await;
    let id = scope.id("Offer:ons-retrieve");

    let create = client
        .post_entity(&json!({
            "id": id,
            "type": "Offer",
            "date": {"type": "Property", "value": "2020-09-28T17:13:39Z"},
            "@context": [CORE_CONTEXT]
        }))
        .send()
        .await
        .expect("create entity failed");
    assert_eq!(create.status, StatusCode::CREATED);

    let path = format!("/ngsi-ld/v1/i18n/entities/{}?locale=en-US", urlencoding::encode(&id));
    let response = client
        .raw(Method::GET, &path)
        .header("Accept", "application/ld+json")
        .send()
        .await
        .expect("localized GET failed");
    assert_eq!(response.status, StatusCode::UNPROCESSABLE_ENTITY);
    assert_problem_details(&response.body, error::OPERATION_NOT_SUPPORTED);

    scope.cleanup().await.expect("cleanup failed");
}
