//! Exhaustive HTTP classification for Entity Map route failures.

use andromeda_entity::error::EntityError;
use andromeda_entity_map::error::EntityMapError;
use andromeda_federation::error::FederationError;
use andromeda_http_context_registry::response_rendering::{ResponseContextError, classify_response_context_error};
use andromeda_http_response::{
    classified_report::ClassifiedReport,
    entity_failure::classify_entity_error,
    entity_map_failure::classify_entity_map_error,
    forwarding_failure::classify_federation_error,
    internal_failure_summary::InternalFailureSummary,
    jsonld_failure::classify_jsonld_error,
    parameter_failure::classify_parameter_error,
    problem_response::ProblemResponse,
    temporal_failure::classify_temporal_error,
};
use andromeda_jsonld::error::JsonLdError;
use andromeda_ngsi_ld_problem_details::error_type::NgsiLdErrorType;
use andromeda_ngsi_ld_render::error::RenderError;
use andromeda_ngsi_ld_request::error::ParameterError;
use andromeda_temporal::error::TemporalError;
use axum::http::Uri;
use std::fmt::Display;

/// A typed failure consumed by Entity Map routes.
pub trait EntityMapHttpClassify {
    /// Select the NGSI-LD category and disclosure.
    fn classify_for_entity_map_http(&self) -> ClassifiedReport;
}

impl EntityMapHttpClassify for EntityMapError {
    fn classify_for_entity_map_http(&self) -> ClassifiedReport {
        classify_entity_map_error(self)
    }
}

impl EntityMapHttpClassify for EntityError {
    fn classify_for_entity_map_http(&self) -> ClassifiedReport {
        classify_entity_error(self)
    }
}

impl EntityMapHttpClassify for ParameterError {
    fn classify_for_entity_map_http(&self) -> ClassifiedReport {
        classify_parameter_error(self)
    }
}

impl EntityMapHttpClassify for JsonLdError {
    fn classify_for_entity_map_http(&self) -> ClassifiedReport {
        classify_jsonld_error(self)
    }
}

impl EntityMapHttpClassify for ResponseContextError {
    fn classify_for_entity_map_http(&self) -> ClassifiedReport {
        classify_response_context_error(self)
    }
}

impl EntityMapHttpClassify for RenderError {
    fn classify_for_entity_map_http(&self) -> ClassifiedReport {
        ClassifiedReport::internal(self, InternalFailureSummary::RESPONSE_RENDERING)
    }
}

impl EntityMapHttpClassify for TemporalError {
    fn classify_for_entity_map_http(&self) -> ClassifiedReport {
        classify_temporal_error(self)
    }
}

impl EntityMapHttpClassify for FederationError {
    fn classify_for_entity_map_http(&self) -> ClassifiedReport {
        classify_federation_error(self)
    }
}

/// Attach the inbound URI while classifying a typed Entity Map result.
pub trait EntityMapResultExt<T> {
    /// Map the focused failure and attach `instance`.
    ///
    /// # Errors
    ///
    /// Returns the [`ProblemResponse`] the failure classifies to, carrying
    /// `instance` as the problem's `instance` member. A successful result
    /// passes through unchanged.
    fn with_entity_map_instance(self, instance: &Uri) -> Result<T, ProblemResponse>;
}

impl<T, E> EntityMapResultExt<T> for Result<T, E>
where
    E: EntityMapHttpClassify,
{
    fn with_entity_map_instance(self, instance: &Uri) -> Result<T, ProblemResponse> {
        self.map_err(|error| error.classify_for_entity_map_http().with_instance(instance.clone()))
    }
}

pub(crate) fn bad_request_data(detail: impl Into<String>, instance: &Uri) -> ProblemResponse {
    ClassifiedReport::public(NgsiLdErrorType::BadRequestData, detail).with_instance(instance.clone())
}

pub(crate) fn internal_problem(source: &impl Display, summary: InternalFailureSummary, instance: &Uri) -> ProblemResponse {
    ClassifiedReport::internal(source, summary).with_instance(instance.clone())
}

#[cfg(test)]
mod tests {
    use crate::error::EntityMapHttpClassify;
    use andromeda_entity_map::{entity_map_invariant::EntityMapInvariant, error::EntityMapError};
    use andromeda_ngsi_ld_lexicon::identifier::Identifier;
    use andromeda_ngsi_ld_problem_details::error_type::NgsiLdErrorType;
    use andromeda_persistence_failure::persistence_failure::PersistenceFailure;
    use std::error::Error;

    #[test]
    fn every_entity_map_variant_has_focused_disclosure() -> Result<(), Box<dyn Error>> {
        let map_identifier = "urn:ngsi-ld:EntityMap:1".parse::<Identifier>()?;
        for (error, expected) in [
            (
                EntityMapError::NotFound {
                    identifier: map_identifier.clone(),
                },
                NgsiLdErrorType::ResourceNotFound,
            ),
            (EntityMapError::RequestChanged { identifier: map_identifier }, NgsiLdErrorType::InvalidRequest),
            (
                EntityMapError::Persistence(PersistenceFailure::StatementRejected),
                NgsiLdErrorType::InternalError,
            ),
            (
                EntityMapError::Invariant(EntityMapInvariant::LifetimeOutOfRange),
                NgsiLdErrorType::InternalError,
            ),
        ] {
            let report = error.classify_for_entity_map_http();
            pretty_assertions::assert_eq!(report.error_type(), expected);
            if expected == NgsiLdErrorType::InternalError {
                // Both internal Entity Map variants disclose the store summary and never their operator diagnostic.
                pretty_assertions::assert_eq!(report.detail(), "the entity store is unavailable");
                assert!(!report.detail().contains("database host"), "the raw cause leaked: {}", report.detail());
                assert!(!report.detail().contains("broken"), "the raw cause leaked: {}", report.detail());
            }
        }
        Ok(())
    }

    /// Clause 5.5.6 with RFC 7231 6.6.4: a store that cannot be reached is a
    /// retryable 503 with `Retry-After`, never the 500 of a fault.
    #[test]
    fn an_unreachable_store_is_a_retryable_503() {
        use andromeda_ngsi_ld_problem_details::problem_detail::ProblemClassification;
        use std::time::Duration;

        let report = EntityMapError::StoreUnavailable.classify_for_entity_map_http();
        pretty_assertions::assert_eq!(report.classification(), ProblemClassification::ServiceUnavailable);
        pretty_assertions::assert_eq!(report.retry_after(), Some(Duration::from_secs(5)));
        pretty_assertions::assert_eq!(report.detail(), "the datastore is temporarily unavailable; retry shortly");
    }
}
