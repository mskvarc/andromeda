//! Exhaustive HTTP classification of failures owned by temporal policy,
//! shared by every binding that answers one.

use crate::{
    classified_report::ClassifiedReport,
    internal_failure_summary::InternalFailureSummary,
    jsonld_failure::classify_jsonld_error,
    parameter_failure::classify_parameter_error,
    query_failure::classify_query_error,
};
use andromeda_ngsi_ld_problem_details::error_type::NgsiLdErrorType;
use andromeda_temporal::error::TemporalError;

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
        // Clause 6.3.2 raises "Request Entity too large" (413) for an input
        // too large to process, counted here in values and positions.
        TemporalError::PayloadBudget(source) => ClassifiedReport::public(NgsiLdErrorType::RequestEntityTooLarge, source.to_string()),
        // A depth, not a size: clause 6.3.2 keeps 413 for too many bytes, and
        // clauses 5.7.1 and 5.7.2 answer a too deep query with `BadRequestData`.
        TemporalError::AttributeNesting(source) => ClassifiedReport::public(NgsiLdErrorType::BadRequestData, source.to_string()),
        TemporalError::JsonLd(source) => classify_jsonld_error(source),
        TemporalError::Query(source) => classify_query_error(source),
        TemporalError::Request(source) => classify_parameter_error(source),
        TemporalError::StoreSaturated => ClassifiedReport::store_saturated(error.to_string()),
        TemporalError::TransactionContended => ClassifiedReport::transaction_contended(error.to_string()),
        TemporalError::StoreUnavailable => ClassifiedReport::store_unavailable(error.to_string()),
        TemporalError::Commit(_) => ClassifiedReport::internal(error, InternalFailureSummary::new("failed to persist temporal history")),
        TemporalError::Persistence(_) => ClassifiedReport::internal(error, InternalFailureSummary::new("the temporal store is unavailable")),
        TemporalError::Internal(_) => ClassifiedReport::internal(error, InternalFailureSummary::new("internal temporal error")),
    }
}

#[cfg(test)]
mod tests {
    use crate::temporal_failure::classify_temporal_error;
    use andromeda_context_commit::{error::CommitError, submission_refusal::SubmissionRefusal};
    use andromeda_jsonld::error::JsonLdError;
    use andromeda_ngsi_ld_geometry::geometry_census::GeometryPositionLimit;
    use andromeda_ngsi_ld_problem_details::{error_type::NgsiLdErrorType, problem_detail::ProblemClassification};
    use andromeda_ngsi_ld_query::error::QueryError;
    use andromeda_ngsi_ld_representation::payload_budget::{GeometryOwner, PayloadBudgetExceeded};
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
    use std::{num::NonZeroUsize, time::Duration};

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
                TemporalError::JsonLd(JsonLdError::TypeNotExpanded),
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
                TemporalError::Commit(CommitError::Persistence(PersistenceFailure::StatementRejected)),
                NgsiLdErrorType::InternalError,
                Some(("failed to persist temporal history", "the store rejected a statement")),
            ),
            (
                "Persistence",
                TemporalError::Persistence(PersistenceFailure::StatementRejected),
                NgsiLdErrorType::InternalError,
                Some(("the temporal store is unavailable", "the store rejected a statement")),
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
        let error = TemporalError::from(CommitError::Submission(SubmissionRefusal::RelationshipMissingObject));
        let report = classify_temporal_error(&error);
        pretty_assertions::assert_eq!(report.error_type(), NgsiLdErrorType::BadRequestData);
        pretty_assertions::assert_eq!(report.detail(), "Relationship missing object");
    }

    #[test]
    fn temporal_public_detail_is_preserved() {
        let report = classify_temporal_error(&TemporalError::Validation(TemporalValidationReason::EntityMissingId));
        pretty_assertions::assert_eq!(report.detail(), "entity is missing an id");
    }

    /// Clause 5.5.6: a temporal write every retry lost to a concurrent
    /// transaction, a saturated pool and a store that cannot be reached are
    /// `InternalError` failures; only the 503 binding status and `Retry-After`
    /// say the request can be repeated, and how soon.
    #[test]
    fn a_load_shed_temporal_answer_is_a_503_internal_error_with_retry_after() {
        for (error, retry_after) in [
            (TemporalError::StoreSaturated, Duration::from_secs(1)),
            (TemporalError::TransactionContended, Duration::from_secs(1)),
            (TemporalError::StoreUnavailable, Duration::from_secs(5)),
        ] {
            let report = classify_temporal_error(&error);
            let classification = report.classification();
            pretty_assertions::assert_eq!(classification, ProblemClassification::ServiceUnavailable, "{error}");
            pretty_assertions::assert_eq!(
                classification.type_iri().as_str(),
                NgsiLdErrorType::InternalError.type_iri().as_str(),
                "{error}"
            );
            pretty_assertions::assert_eq!(classification.status_code(), StatusCode::SERVICE_UNAVAILABLE, "{error}");
            pretty_assertions::assert_eq!(report.retry_after(), Some(retry_after), "{error}");
        }
    }

    /// Clause 6.3.2: a temporal write past the payload budget is "Request
    /// Entity too large", naming the attribute and the budget.
    #[test]
    fn a_temporal_write_past_the_payload_budget_is_a_413() {
        let error = TemporalError::PayloadBudget(PayloadBudgetExceeded::GeometryPositions {
            owner: GeometryOwner::Attribute("location".parse().expect("an attribute name")),
            limit: GeometryPositionLimit::new(NonZeroUsize::MIN.saturating_add(99_999)),
        });

        let report = classify_temporal_error(&error);

        pretty_assertions::assert_eq!(report.error_type(), NgsiLdErrorType::RequestEntityTooLarge);
        pretty_assertions::assert_eq!(report.classification().status_code(), StatusCode::PAYLOAD_TOO_LARGE);
        assert!(
            report.detail().contains("'location'") && report.detail().contains("100000"),
            "{}",
            report.detail()
        );
    }

    /// Clauses 5.7.1 and 5.7.2 answer a too deep query with `BadRequestData`,
    /// and clause 6.3.2 keeps 413 for too many bytes: an attribute nesting
    /// past the limit is a 400 `BadRequestData` naming the attribute and the
    /// limit.
    #[test]
    fn an_attribute_nesting_past_the_limit_is_a_400_bad_request_data() {
        use andromeda_ngsi_ld_representation::attribute_nesting::{AttributeNestingExceeded, AttributeNestingLimit};

        let exceeded = AttributeNestingExceeded {
            attribute: "https://example.org/reading".parse().expect("an attribute name"),
            limit: AttributeNestingLimit::new(NonZeroUsize::MIN.saturating_add(31)),
        };
        let report = classify_temporal_error(&TemporalError::AttributeNesting(exceeded));
        pretty_assertions::assert_eq!(report.error_type(), NgsiLdErrorType::BadRequestData);
        pretty_assertions::assert_eq!(report.classification().status_code(), StatusCode::BAD_REQUEST);
        assert!(
            report.detail().contains("'https://example.org/reading'") && report.detail().contains("32 levels"),
            "{}",
            report.detail()
        );
    }
}
