use crate::api::extractor::error::ExtractorRejection;
use axum::{extract::FromRequestParts, http::request::Parts};
use serde::de::DeserializeOwned;

pub struct ValidatedQuery<T>(pub T);

impl<S, T> FromRequestParts<S> for ValidatedQuery<T>
where
    S: Send + Sync,
    T: DeserializeOwned,
{
    type Rejection = ExtractorRejection;

    async fn from_request_parts(parts: &mut Parts, _state: &S) -> Result<Self, Self::Rejection> {
        let uri = parts.uri.clone();
        let query_str = parts.uri.query().unwrap_or("");

        let params = serde_urlencoded::from_str::<T>(query_str).map_err(|error| {
            // Unknown/unsupported parameters → InvalidRequest
            // Invalid parameter values (e.g. details=other for bool) → BadRequestData
            // serde_urlencoded doesn't expose structured errors, so we use heuristic
            let error_msg = format!("Invalid query parameters: {}", error);
            let is_unknown_field = error_msg.contains("unknown field");
            if is_unknown_field {
                ExtractorRejection::invalid_request(error_msg, uri)
            } else {
                ExtractorRejection::bad_request_data(error_msg, uri)
            }
        })?;

        Ok(ValidatedQuery(params))
    }
}
