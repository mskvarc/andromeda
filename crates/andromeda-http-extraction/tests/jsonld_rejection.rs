//! The problem details the JSON-LD body extractor answers a malformed or
//! structurally invalid request body with.

use andromeda_http_extraction::jsonld::JsonLd;
use andromeda_http_response::extraction_rejection::ExtractorRejection;
use andromeda_jsonld::user_context_admission::AdmittedContext;
use andromeda_ngsi_ld_model::request_body::RequestBody;
use andromeda_ngsi_ld_problem_details::error_type::NgsiLdErrorType;
use axum::{
    body::{Body, to_bytes},
    extract::FromRequest,
    http::{
        Request,
        header::{CONTENT_LENGTH, CONTENT_TYPE},
    },
    response::IntoResponse,
};
use serde::Deserialize;
use serde_json::Value;
use std::error::Error;

#[derive(Deserialize)]
struct RequiredIdentifier {
    id: String,
}

impl RequestBody for RequiredIdentifier {}

/// Build a JSON request carrying the `Content-Length` the extractor requires
/// before it will look at a body at all: without it the extraction ends in
/// `411 Length Required` and never reaches the case under test.
fn json_request(body: &'static str) -> Result<Request<Body>, Box<dyn Error>> {
    Ok(Request::builder()
        .uri("/ngsi-ld/v1/entities")
        .header(CONTENT_TYPE, "application/json")
        .header(CONTENT_LENGTH, body.len())
        .body(Body::from(body))?)
}

async fn rejection_body(rejection: ExtractorRejection) -> Result<Value, Box<dyn Error>> {
    let response = rejection.into_response();
    let bytes = to_bytes(response.into_body(), usize::MAX).await?;
    Ok(serde_json::from_slice(&bytes)?)
}

#[tokio::test]
async fn malformed_json_is_invalid_request_with_the_exact_instance() -> Result<(), Box<dyn Error>> {
    let request = json_request("{")?;
    let Err(rejection) = JsonLd::<RequiredIdentifier>::from_request(request, &()).await else {
        return Err("malformed JSON was accepted".into());
    };
    let body = rejection_body(rejection).await?;
    pretty_assertions::assert_eq!(body["type"], NgsiLdErrorType::InvalidRequest.type_iri().as_str());
    assert!(
        body["detail"]
            .as_str()
            .is_some_and(|detail| detail.starts_with("request body is not a JSON document:")),
        "{body}"
    );
    pretty_assertions::assert_eq!(body["instance"], "/ngsi-ld/v1/entities");
    Ok(())
}

/// A body that does not decode is refused once its context was admitted, so
/// an unretrievable context answers first (clauses 5.5.4 and 5.5.6).
#[tokio::test]
async fn structurally_invalid_json_is_bad_request_data_once_its_context_is_admitted() -> Result<(), Box<dyn Error>> {
    let request = json_request("{}")?;
    let Ok(JsonLd { body, .. }) = JsonLd::<RequiredIdentifier>::from_request(request, &()).await else {
        return Err("the body was refused before its context was admitted".into());
    };
    let refusal = match body.admitted(&AdmittedContext::core_only()).await {
        Err(refusal) => refusal,
        Ok(value) => return Err(format!("body without id was accepted as {}", value.id).into()),
    };
    let body = rejection_body(ExtractorRejection::Classified(refusal)).await?;
    pretty_assertions::assert_eq!(body["status"], 400);
    pretty_assertions::assert_eq!(body["title"], "Bad request data");
    pretty_assertions::assert_eq!(body["instance"], "/ngsi-ld/v1/entities");
    Ok(())
}
