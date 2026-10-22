//! The refusal of a request that is malformed before any operation runs.
//!
//! Clause 5.5.4 separates a request that is syntactically wrong (`InvalidRequest`)
//! from data that is valid JSON but breaks the API data types (`BadRequestData`).
//! A refusal type states that split once, in [`RequestRefusal::error_type`], and
//! the response, the status and the detail follow from it.

use crate::{classified_report::ClassifiedReport, extraction_rejection::ExtractorRejection, problem_response::ProblemResponse};
use andromeda_ngsi_ld_problem_details::error_type::NgsiLdErrorType;
use axum::http::Uri;
use std::error::Error;

/// A typed reason a request is refused with a clause 5.5.4 problem.
///
/// The detail of the problem is the refusal's own message, so a message starts
/// lowercase and names the source it carries.
pub trait RequestRefusal: Error + Sized {
    /// The clause 5.5.2 category the refusal is reported as.
    fn error_type(&self) -> NgsiLdErrorType;

    /// The problem response answering the request at `instance`.
    #[must_use]
    fn into_problem(self, instance: Uri) -> ProblemResponse {
        ClassifiedReport::public(self.error_type(), self.to_string()).with_instance(instance)
    }

    /// The extraction rejection answering the request at `instance`.
    #[must_use]
    fn into_rejection(self, instance: Uri) -> ExtractorRejection {
        ExtractorRejection::Classified(self.into_problem(instance))
    }
}

#[cfg(test)]
mod tests {
    use crate::{extraction_rejection::ExtractorRejection, request_refusal::RequestRefusal};
    use andromeda_ngsi_ld_problem_details::{error_type::NgsiLdErrorType, problem_detail::ProblemDetail};
    use axum::http::{StatusCode, Uri};
    use thiserror::Error;

    /// A refusal of each category.
    #[derive(Debug, Error)]
    enum Sample {
        /// Reported as `InvalidRequest`.
        #[error("the request is malformed")]
        Malformed,
        /// Reported as `BadRequestData`.
        #[error("the data is wrong")]
        Wrong,
    }

    impl RequestRefusal for Sample {
        fn error_type(&self) -> NgsiLdErrorType {
            match self {
                Sample::Malformed => NgsiLdErrorType::InvalidRequest,
                Sample::Wrong => NgsiLdErrorType::BadRequestData,
            }
        }
    }

    #[test]
    fn a_refusal_becomes_a_problem_with_its_category_detail_and_instance() {
        for (refusal, expected, detail) in [
            (Sample::Malformed, NgsiLdErrorType::InvalidRequest, "the request is malformed"),
            (Sample::Wrong, NgsiLdErrorType::BadRequestData, "the data is wrong"),
        ] {
            let problem = refusal.into_problem(Uri::from_static("/ngsi-ld/v1/entities"));
            pretty_assertions::assert_eq!(problem.problem().error_type(), expected);
            pretty_assertions::assert_eq!(problem.problem().status_code(), StatusCode::BAD_REQUEST);
            pretty_assertions::assert_eq!(problem.problem().detail(), detail);
            pretty_assertions::assert_eq!(problem.problem().instance().map(ToString::to_string).as_deref(), Some("/ngsi-ld/v1/entities"));
        }
    }

    #[test]
    fn a_refusal_becomes_a_classified_rejection() {
        let rejection = Sample::Wrong.into_rejection(Uri::from_static("/ngsi-ld/v1/entities"));
        pretty_assertions::assert_eq!(rejection.problem().map(ProblemDetail::error_type), Some(NgsiLdErrorType::BadRequestData));
        assert!(matches!(rejection, ExtractorRejection::Classified(_)));
    }
}
