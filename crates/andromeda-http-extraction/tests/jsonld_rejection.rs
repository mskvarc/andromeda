use andromeda_http_extraction::jsonld::JsonLd;
use andromeda_http_response::extraction_rejection::ExtractorRejection;
use andromeda_ngsi_ld_problem_details::error_type::NgsiLdErrorType;
use axum::{
    body::{Body, to_bytes},
    extract::FromRequest,
    http::{Request, header::CONTENT_TYPE},
    response::IntoResponse,
};
use serde::Deserialize;
use serde_json::Value;

#[derive(Deserialize)]
struct RequiredIdentifier {
    id: String,
}

async fn rejection_body(rejection: ExtractorRejection) -> Result<Value, Box<dyn std::error::Error>> {
    let response = rejection.into_response();
    let bytes = to_bytes(response.into_body(), usize::MAX).await?;
    Ok(serde_json::from_slice(&bytes)?)
}

#[tokio::test]
async fn malformed_json_is_invalid_request_with_the_exact_instance() -> Result<(), Box<dyn std::error::Error>> {
    let request = Request::builder()
        .uri("/ngsi-ld/v1/entities")
        .header(CONTENT_TYPE, "application/json")
        .body(Body::from("{"))?;
    let rejection = match JsonLd::<Value>::from_request(request, &()).await {
        Err(rejection) => rejection,
        Ok(_) => return Err("malformed JSON was accepted".into()),
    };
    assert!(matches!(rejection, ExtractorRejection::InvalidRequest { .. }));
    let body = rejection_body(rejection).await?;
    pretty_assertions::assert_eq!(body["type"], NgsiLdErrorType::InvalidRequest.type_iri().as_str());
    pretty_assertions::assert_eq!(body["instance"], "/ngsi-ld/v1/entities");
    Ok(())
}

#[tokio::test]
async fn structurally_invalid_json_is_bad_request_data_before_policy_execution() -> Result<(), Box<dyn std::error::Error>> {
    let request = Request::builder()
        .uri("/ngsi-ld/v1/entities")
        .header(CONTENT_TYPE, "application/json")
        .body(Body::from("{}"))?;
    let rejection = match JsonLd::<RequiredIdentifier>::from_request(request, &()).await {
        Err(rejection) => rejection,
        Ok(value) => return Err(format!("body without id was accepted as {}", value.value.id).into()),
    };
    let body = rejection_body(rejection).await?;
    pretty_assertions::assert_eq!(body["status"], 400);
    pretty_assertions::assert_eq!(body["title"], "Bad request data");
    pretty_assertions::assert_eq!(body["instance"], "/ngsi-ld/v1/entities");
    Ok(())
}
