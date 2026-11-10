//! Exhaustive HTTP classification for snapshot failures.

use andromeda_failure_classification::{
    classified_report::ClassifiedReport,
    entity_failure::classify_entity_error,
    forwarding_failure::classify_federation_error,
    internal_failure_summary::InternalFailureSummary,
    jsonld_failure::classify_jsonld_error,
    parameter_failure::classify_parameter_error,
    temporal_failure::classify_temporal_error,
};
use andromeda_http_response::problem_response::ProblemResponse;
use andromeda_jsonld::error::JsonLdError;
use andromeda_ngsi_ld_problem_details::{batch_operation::BatchEntityError, error_type::NgsiLdErrorType, problem_detail::ProblemClassification};
use andromeda_snapshot::{error::SnapshotError, service::purge::SnapshotPurgeFailure};
use axum::http::Uri;
use std::{fmt::Display, time::Duration};

/// Exhaustively classify one snapshot policy failure.
#[must_use]
pub fn classify_snapshot_error(error: &SnapshotError) -> ClassifiedReport {
    match error {
        SnapshotError::Validation(reason) => ClassifiedReport::public(NgsiLdErrorType::BadRequestData, reason.to_string()),
        SnapshotError::NotFound { .. } => ClassifiedReport::public(NgsiLdErrorType::ResourceNotFound, error.to_string()),
        SnapshotError::AlreadyExists { .. } => ClassifiedReport::public(NgsiLdErrorType::AlreadyExists, error.to_string()),
        SnapshotError::PreparationTimeout { .. } => {
            ClassifiedReport::public_binding(ProblemClassification::ServiceUnavailable, error.to_string()).with_retry_after(Duration::from_secs(1))
        }
        // A quarantined Snapshot is a broker-side condition the operator must
        // resolve, answered as an internal error whose detail names it.
        SnapshotError::Quarantined { .. } => ClassifiedReport::public(NgsiLdErrorType::InternalError, error.to_string()),
        SnapshotError::Entity(source) => classify_entity_error(source),
        SnapshotError::Temporal(source) => classify_temporal_error(source),
        SnapshotError::Federation(source) => classify_federation_error(source),
        SnapshotError::Request(source) => classify_parameter_error(source),
        SnapshotError::StoreSaturated | SnapshotError::Contended => ClassifiedReport::store_saturated(error.to_string()),
        SnapshotError::StoreUnavailable => ClassifiedReport::store_unavailable(error.to_string()),
        SnapshotError::Representation(_) => ClassifiedReport::internal(error, InternalFailureSummary::new("failed to render the snapshot")),
        SnapshotError::Persistence(_) => ClassifiedReport::internal(error, InternalFailureSummary::SNAPSHOT_STORE_FAULT),
        SnapshotError::Internal(_) => ClassifiedReport::internal(error, InternalFailureSummary::new("internal snapshot error")),
    }
}

/// Attach the inbound URI while classifying a snapshot result.
pub trait SnapshotResultExt<T> {
    /// Map the focused failure and attach `instance`.
    ///
    /// # Errors
    ///
    /// Returns the [`ProblemResponse`] the failure classifies to whenever `self`
    /// is `Err`; an `Ok` result passes through untouched.
    fn with_snapshot_instance(self, instance: &Uri) -> Result<T, ProblemResponse>;
}

impl<T> SnapshotResultExt<T> for Result<T, SnapshotError> {
    fn with_snapshot_instance(self, instance: &Uri) -> Result<T, ProblemResponse> {
        self.map_err(|error| ProblemResponse::classified(classify_snapshot_error(&error), instance.clone()))
    }
}

/// The admission of the user `@context` a Snapshot is created under (clause
/// 5.5.7) is refused as every other operation's is.
impl<T> SnapshotResultExt<T> for Result<T, JsonLdError> {
    fn with_snapshot_instance(self, instance: &Uri) -> Result<T, ProblemResponse> {
        self.map_err(|error| ProblemResponse::classified(classify_jsonld_error(&error), instance.clone()))
    }
}

/// The multi-status entry for one Snapshot a purge could not delete.
pub(crate) fn project_purge_failure(failure: SnapshotPurgeFailure) -> BatchEntityError {
    let report = classify_snapshot_error(&failure.error);
    BatchEntityError::new(failure.identifier.into(), report.error_type(), report.detail())
}

/// A 500 answer at `instance` that logs `source` and discloses only `summary`.
pub(crate) fn internal_problem(source: &impl Display, summary: InternalFailureSummary, instance: &Uri) -> ProblemResponse {
    ProblemResponse::classified(ClassifiedReport::internal(source, summary), instance.clone())
}

#[cfg(test)]
mod tests {
    use crate::error::classify_snapshot_error;
    use andromeda_ngsi_ld_lexicon::identifier::Identifier;
    use andromeda_ngsi_ld_problem_details::error_type::NgsiLdErrorType;
    use andromeda_persistence_failure::persistence_failure::PersistenceFailure;
    use andromeda_snapshot::{error::SnapshotError, snapshot_invariant::SnapshotInvariant, snapshot_validation_reason::SnapshotValidationReason};
    use std::error::Error;

    #[test]
    fn focused_snapshot_categories_and_disclosure_are_stable() -> Result<(), Box<dyn Error>> {
        let identifier = "urn:ngsi-ld:Snapshot:1".parse::<Identifier>()?;
        for (error, expected) in [
            (
                SnapshotError::Validation(SnapshotValidationReason::IdImmutable),
                NgsiLdErrorType::BadRequestData,
            ),
            (
                SnapshotError::NotFound {
                    identifier: identifier.clone(),
                },
                NgsiLdErrorType::ResourceNotFound,
            ),
            (SnapshotError::AlreadyExists { identifier }, NgsiLdErrorType::AlreadyExists),
            (
                SnapshotError::Persistence(PersistenceFailure::StatementRejected),
                NgsiLdErrorType::InternalError,
            ),
            (
                SnapshotError::Internal(SnapshotInvariant::LeaseDeadlineOverflow),
                NgsiLdErrorType::InternalError,
            ),
        ] {
            let report = classify_snapshot_error(&error);
            pretty_assertions::assert_eq!(report.error_type(), expected);
            match &error {
                SnapshotError::Persistence(failure) => {
                    pretty_assertions::assert_eq!(report.detail(), "the snapshot store could not complete the operation");
                    assert!(!report.detail().contains(&failure.to_string()), "the raw cause leaked: {}", report.detail());
                }
                SnapshotError::Internal(invariant) => {
                    pretty_assertions::assert_eq!(report.detail(), "internal snapshot error");
                    assert!(!report.detail().contains(&invariant.to_string()), "the raw cause leaked: {}", report.detail());
                }
                SnapshotError::Validation(_)
                | SnapshotError::NotFound { .. }
                | SnapshotError::AlreadyExists { .. }
                | SnapshotError::PreparationTimeout { .. }
                | SnapshotError::Quarantined { .. }
                | SnapshotError::Entity(_)
                | SnapshotError::Temporal(_)
                | SnapshotError::Federation(_)
                | SnapshotError::Request(_)
                | SnapshotError::Representation(_)
                | SnapshotError::StoreSaturated
                | SnapshotError::StoreUnavailable
                | SnapshotError::Contended => {}
            }
        }
        Ok(())
    }

    /// A quarantined Snapshot answers 500 `InternalError`, and unlike an
    /// internal fault its detail names the Snapshot the operator must repair.
    #[test]
    fn a_quarantined_snapshot_is_an_internal_error_that_names_the_snapshot() -> Result<(), Box<dyn Error>> {
        let identifier = "urn:ngsi-ld:Snapshot:quarantined".parse::<Identifier>()?;
        let report = classify_snapshot_error(&SnapshotError::Quarantined { identifier });
        pretty_assertions::assert_eq!(report.error_type(), NgsiLdErrorType::InternalError);
        assert!(report.detail().contains("urn:ngsi-ld:Snapshot:quarantined"), "{}", report.detail());
        Ok(())
    }

    /// Clause 5.5.6 with RFC 7231 6.6.4: a store that cannot be reached is a
    /// retryable 503 with `Retry-After`, never the 500 of a fault.
    #[test]
    fn an_unreachable_store_is_a_retryable_503() {
        use andromeda_ngsi_ld_problem_details::problem_detail::ProblemClassification;
        use std::time::Duration;

        let report = classify_snapshot_error(&SnapshotError::StoreUnavailable);
        pretty_assertions::assert_eq!(report.classification(), ProblemClassification::ServiceUnavailable);
        pretty_assertions::assert_eq!(report.retry_after(), Some(Duration::from_secs(5)));
        pretty_assertions::assert_eq!(report.detail(), "the datastore is temporarily unavailable; retry shortly");
    }
}
