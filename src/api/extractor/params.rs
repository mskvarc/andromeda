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
            // serde_urlencoded only provides string error messages (no structured error kinds).
            // Serde's "unknown field" message is stable across versions, so this heuristic
            // is reliable for distinguishing unsupported parameters (→ InvalidRequest)
            // from invalid parameter values (→ BadRequestData).
            let error_str = error.to_string();
            let error_msg = format!("Invalid query parameters: {}", error_str);
            if error_str.contains("unknown field") {
                ExtractorRejection::invalid_request(error_msg, uri)
            } else {
                ExtractorRejection::bad_request_data(error_msg, uri)
            }
        })?;

        Ok(ValidatedQuery(params))
    }
}
