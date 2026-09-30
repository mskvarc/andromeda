//! Exhaustive classification for every typed failure consumed by temporal routes.

use andromeda_entity_map::error::EntityMapError;
use andromeda_federation::error::FederationError;
use andromeda_http_extraction::classification::{classify_jsonld_error, classify_parameter_error, classify_query_error, classify_tenancy_error};
use andromeda_http_response::{
    classified_report::ClassifiedReport,
    internal_failure_summary::InternalFailureSummary,
    problem_response::ProblemResponse,
    provision::classify_forwarding_error,
};
use andromeda_ngsi_ld_problem_details::error_type::NgsiLdErrorType;
use andromeda_ngsi_ld_query::error::QueryError;
use andromeda_ngsi_ld_render::{error::RenderError, temporal::simplify::TemporalSimplifyError};
use andromeda_ngsi_ld_request::error::ParameterError;
use andromeda_temporal::error::TemporalError;
use andromeda_tenancy::error::TenancyError;
use axum::http::Uri;

/// Select an NGSI-LD problem classification for one temporal-binding failure.
pub trait TemporalHttpClassify {
    /// Select category, status, and disclosure without formatted-message inspection.
    fn classify_for_temporal_http(&self) -> ClassifiedReport;
}

impl TemporalHttpClassify for TemporalError {
    fn classify_for_temporal_http(&self) -> ClassifiedReport {
        classify_temporal_error(self)
    }
}

/// Exhaustively classify one temporal-policy failure.
#[must_use]
pub fn classify_temporal_error(error: &TemporalError) -> ClassifiedReport {
    match error {
        TemporalError::Validation(reason) => ClassifiedReport::public(NgsiLdErrorType::BadRequestData, reason.to_string()),
        TemporalError::NotFound(reason) => ClassifiedReport::public(NgsiLdErrorType::ResourceNotFound, reason.to_string()),
        TemporalError::OperationNotSupported(reason) => ClassifiedReport::public(NgsiLdErrorType::OperationNotSupported, reason.to_string()),
        // The clause 5.6.21.4 `TooManyResults` bound the standard Purge raises,
        // applied to the delete-by-query extension: a 403 naming the count and
        // the limit so the client can narrow its window or selectors.
        TemporalError::TooManyResults { .. } => ClassifiedReport::public(NgsiLdErrorType::TooManyResults, error.to_string()),
        TemporalError::JsonLd(source) => classify_jsonld_error(source),
        TemporalError::Query(source) => classify_query_error(source),
        TemporalError::Request(source) => classify_parameter_error(source),
        TemporalError::StoreSaturated => ClassifiedReport::store_saturated(error.to_string()),
        TemporalError::TransactionContended => ClassifiedReport::transaction_contended(error.to_string()),
        TemporalError::Commit(_) => ClassifiedReport::internal(error, InternalFailureSummary::new("failed to persist temporal history")),
        TemporalError::Persistence(_) => ClassifiedReport::internal(error, InternalFailureSummary::new("the temporal store is unavailable")),
        TemporalError::Rendering(_) => ClassifiedReport::internal(error, InternalFailureSummary::new("failed to render the temporal representation")),
        TemporalError::Internal(_) => ClassifiedReport::internal(error, InternalFailureSummary::new("internal temporal error")),
    }
}

/// Convert one temporal-policy failure into its wire response for `instance`.
pub(crate) fn temporal_problem(error: &TemporalError, instance: &Uri) -> ProblemResponse {
    classify_temporal_error(error).with_instance(instance.clone())
}

impl TemporalHttpClassify for EntityMapError {
    fn classify_for_temporal_http(&self) -> ClassifiedReport {
        match self {
            EntityMapError::NotFound { .. } => ClassifiedReport::public(NgsiLdErrorType::ResourceNotFound, self.to_string()),
            // Clause 5.5.14 with clause 6.3.20: parameters incompatible with
            // the request that created the map are an invalid request.
            EntityMapError::RequestChanged { .. } => ClassifiedReport::public(NgsiLdErrorType::InvalidRequest, self.to_string()),
            EntityMapError::StoreSaturated => ClassifiedReport::store_saturated(self.to_string()),
            EntityMapError::Persistence(_) | EntityMapError::Invariant(_) => {
                ClassifiedReport::internal(self, InternalFailureSummary::new("the entity store is unavailable"))
            }
        }
    }
}

impl TemporalHttpClassify for FederationError {
    fn classify_for_temporal_http(&self) -> ClassifiedReport {
        match self {
            FederationError::JsonLd(source) => classify_jsonld_error(source),
            FederationError::Query(source) => classify_query_error(source),
            // Clause 6.3.17: the failures of a forwarded request carry their
            // own status and the most informative detail available.
            FederationError::Conflict(_)
            | FederationError::OperationNotSupported(_)
            | FederationError::LoopDetected { .. }
            | FederationError::GatewayTimeout { .. }
            | FederationError::RemoteNotFound { .. }
            | FederationError::BadGateway { .. }
            | FederationError::Registration(_)
            | FederationError::Representation(_)
            | FederationError::ForwardedBody(_)
            | FederationError::Internal(_) => classify_forwarding_error(self),
        }
    }
}

impl TemporalHttpClassify for ParameterError {
    fn classify_for_temporal_http(&self) -> ClassifiedReport {
        classify_parameter_error(self)
    }
}

impl TemporalHttpClassify for QueryError {
    fn classify_for_temporal_http(&self) -> ClassifiedReport {
        classify_query_error(self)
    }
}

impl TemporalHttpClassify for RenderError {
    fn classify_for_temporal_http(&self) -> ClassifiedReport {
        ClassifiedReport::internal(self, InternalFailureSummary::new("failed to render the temporal representation"))
    }
}

impl TemporalHttpClassify for TemporalSimplifyError {
    fn classify_for_temporal_http(&self) -> ClassifiedReport {
        ClassifiedReport::internal(self, InternalFailureSummary::new("failed to render the temporal representation"))
    }
}

impl TemporalHttpClassify for TenancyError {
    fn classify_for_temporal_http(&self) -> ClassifiedReport {
        classify_tenancy_error(self)
    }
}

/// Attach the inbound request URI while classifying a typed temporal-binding result.
pub trait TemporalResultExt<T> {
    /// Convert the typed error through the focused classifier and attach `instance`.
    ///
    /// # Errors
    ///
    /// The [`ProblemResponse`] the typed failure classifies to, carrying
    /// `instance` as the problem detail's `instance` member. A successful
    /// result passes its value through untouched.
    fn with_instance(self, instance: &Uri) -> Result<T, ProblemResponse>;
}

impl<T, E> TemporalResultExt<T> for Result<T, E>
where
    E: TemporalHttpClassify,
{
    fn with_instance(self, instance: &Uri) -> Result<T, ProblemResponse> {
        self.map_err(|error| error.classify_for_temporal_http().with_instance(instance.clone()))
    }
}

#[cfg(test)]
mod tests {
    use crate::error::classify_temporal_error;
    use andromeda_context_commit::{error::CommitError, submission_refusal::SubmissionRefusal};
    use andromeda_http_response::declined_representation::single_source_declined;
    use andromeda_jsonld::error::JsonLdError;
    use andromeda_ngsi_ld_problem_details::{error_type::NgsiLdErrorType, problem_detail::ProblemClassification};
    use andromeda_ngsi_ld_query::error::QueryError;
    use andromeda_ngsi_ld_render::temporal::values::TemporalValuesError;
    use andromeda_ngsi_ld_request::{error::ParameterError, unsupported_representation::UnsupportedRepresentation};
    use andromeda_persistence_failure::persistence_failure::PersistenceFailure;
    use andromeda_temporal::{
        error::TemporalError,
        temporal_invariant::TemporalInvariant,
        temporal_not_found_reason::TemporalNotFoundReason,
        temporal_operation_unsupported::TemporalOperationUnsupported,
        temporal_validation_reason::TemporalValidationReason,
    };
    use axum::http::StatusCode;
    use std::time::Duration;

    #[test]
    fn every_temporal_error_variant_has_one_typed_http_classification() {
        // Every internal variant discloses its own operation summary and never the operator diagnostic it carries.
        for (name, error, expected, internal_disclosure) in [
            (
                "Validation",
                TemporalError::Validation(TemporalValidationReason::EntityMissingId),
                NgsiLdErrorType::BadRequestData,
                None,
            ),
            (
                "NotFound",
                TemporalError::NotFound(TemporalNotFoundReason::Entity {
                    id: "urn:ngsi-ld:Entity:missing".parse().expect("a valid identifier"),
                }),
                NgsiLdErrorType::ResourceNotFound,
                None,
            ),
            (
                "OperationNotSupported",
                TemporalError::OperationNotSupported(TemporalOperationUnsupported::RecordingSinkNoAggregation),
                NgsiLdErrorType::OperationNotSupported,
                None,
            ),
            (
                "TooManyResults",
                TemporalError::too_many_results(500, 100),
                NgsiLdErrorType::TooManyResults,
                None,
            ),
            (
                "JsonLd",
                TemporalError::JsonLd(JsonLdError::DocumentShape),
                NgsiLdErrorType::BadRequestData,
                None,
            ),
            ("Query", TemporalError::Query(QueryError::QEmpty), NgsiLdErrorType::InvalidRequest, None),
            (
                "Request",
                TemporalError::Request(ParameterError::OptionsInvalid(UnsupportedRepresentation::UnknownOptionsToken)),
                NgsiLdErrorType::InvalidRequest,
                None,
            ),
            (
                "Commit",
                TemporalError::Commit(CommitError::Persistence(PersistenceFailure::Unavailable)),
                NgsiLdErrorType::InternalError,
                Some(("failed to persist temporal history", "the store is unreachable")),
            ),
            (
                "Persistence",
                TemporalError::Persistence(PersistenceFailure::Unavailable),
                NgsiLdErrorType::InternalError,
                Some(("the temporal store is unavailable", "the store is unreachable")),
            ),
            (
                "Rendering",
                TemporalError::Rendering(TemporalValuesError::InvalidDatasetId),
                NgsiLdErrorType::InternalError,
                Some(("failed to render the temporal representation", "invalid datasetId")),
            ),
            (
                "Internal",
                TemporalError::from(TemporalInvariant::RetrievalWithoutRoot),
                NgsiLdErrorType::InternalError,
                Some(("internal temporal error", "without its root")),
            ),
        ] {
            let report = classify_temporal_error(&error);
            pretty_assertions::assert_eq!(report.error_type(), expected, "wrong category for {name}");
            pretty_assertions::assert_eq!(report.classification().status_code(), expected.status_code(), "wrong status for {name}");
            if let Some((expected_detail, secret)) = internal_disclosure {
                pretty_assertions::assert_eq!(report.detail(), expected_detail, "wrong internal detail for {name}");
                assert!(!report.detail().contains(secret), "internal detail leaked for {name}: {}", report.detail());
            }
        }
    }

    /// Clause 5.5.6: a temporal body the commit refused is invalid input, so
    /// the client learns what to change instead of being told to retry. The
    /// policy conversion is what carries the refusal out of the commit port.
    #[test]
    fn a_commit_that_refused_the_submission_reports_bad_request_data() {
        let error = TemporalError::from(CommitError::Submission(SubmissionRefusal::UnitCodeUnregistered));
        let report = classify_temporal_error(&error);
        pretty_assertions::assert_eq!(report.error_type(), NgsiLdErrorType::BadRequestData);
        pretty_assertions::assert_eq!(report.detail(), "'unitCode' is not a UN/CEFACT common code the unit catalog registers");
    }

    /// Table 6.4.3.2-3: the 501 of the one registered source holding the data
    /// is answered as a 501 carrying the `OperationNotSupported` category.
    #[test]
    fn a_lone_source_declining_the_representation_is_a_501() {
        let response = single_source_declined(501, &"/ngsi-ld/v1/temporal/entities".parse().expect("valid URI"));
        pretty_assertions::assert_eq!(response.problem().classification(), ProblemClassification::NotImplemented);
        pretty_assertions::assert_eq!(response.problem().status_code().as_u16(), 501);
    }

    #[test]
    fn temporal_public_detail_is_preserved() {
        let report = classify_temporal_error(&TemporalError::Validation(TemporalValidationReason::EntityMissingId));
        pretty_assertions::assert_eq!(report.detail(), "entity is missing an id");
    }

    /// Clause 5.5.6: a temporal write every retry lost to a concurrent
    /// transaction, and a saturated pool, are `InternalError` failures; only the
    /// 503 binding status and `Retry-After` say the request can be repeated.
    #[test]
    fn a_load_shed_temporal_answer_is_a_503_internal_error_with_retry_after() {
        for error in [TemporalError::StoreSaturated, TemporalError::TransactionContended] {
            let report = classify_temporal_error(&error);
            let classification = report.classification();
            pretty_assertions::assert_eq!(classification, ProblemClassification::ServiceUnavailable, "{error}");
            pretty_assertions::assert_eq!(
                classification.type_iri().as_str(),
                NgsiLdErrorType::InternalError.type_iri().as_str(),
                "{error}"
            );
            pretty_assertions::assert_eq!(classification.status_code(), StatusCode::SERVICE_UNAVAILABLE, "{error}");
            pretty_assertions::assert_eq!(report.retry_after(), Some(Duration::from_secs(1)), "{error}");
        }
    }
}
