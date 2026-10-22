//! The refusal of an `EntityMap` update whose `expiresAt` cannot be applied
//! (clause 5.14.2.3).

use andromeda_http_response::request_refusal::RequestRefusal;
use andromeda_ngsi_ld_problem_details::error_type::NgsiLdErrorType;
use thiserror::Error;

/// Why an `EntityMap` update cannot set the expiry it carries.
///
/// The update's only member is the new `expiresAt`, so an update without one,
/// or with one already past, changes nothing a client could ask for; either is
/// valid JSON with invalid data, hence `BadRequestData` (clause 5.5.4).
#[derive(Debug, Error, PartialEq, Eq)]
pub(crate) enum ExpiryUpdateRefusal {
    /// The update names no `expiresAt`.
    #[error("an EntityMap update requires expiresAt")]
    Missing,
    /// The update's `expiresAt` is not in the future.
    #[error("expiresAt must be in the future")]
    NotInTheFuture,
}

impl RequestRefusal for ExpiryUpdateRefusal {
    fn error_type(&self) -> NgsiLdErrorType {
        match self {
            ExpiryUpdateRefusal::Missing | ExpiryUpdateRefusal::NotInTheFuture => NgsiLdErrorType::BadRequestData,
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::expiry_update_refusal::ExpiryUpdateRefusal;
    use andromeda_http_response::request_refusal::RequestRefusal;
    use andromeda_ngsi_ld_problem_details::error_type::NgsiLdErrorType;
    use axum::http::Uri;

    #[test]
    fn each_refusal_is_bad_request_data_with_a_lowercase_detail() {
        for (refusal, detail) in [
            (ExpiryUpdateRefusal::Missing, "an EntityMap update requires expiresAt"),
            (ExpiryUpdateRefusal::NotInTheFuture, "expiresAt must be in the future"),
        ] {
            let problem = refusal.into_problem(Uri::from_static("/ngsi-ld/v1/entityMaps/urn:ngsi-ld:EntityMap:1"));
            pretty_assertions::assert_eq!(problem.problem().error_type(), NgsiLdErrorType::BadRequestData);
            pretty_assertions::assert_eq!(problem.problem().detail(), detail);
        }
    }
}
