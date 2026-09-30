//! Exhaustive HTTP classification for snapshot failures.

use andromeda_entity::error::EntityError;
use andromeda_federation::error::FederationError;
use andromeda_http_extraction::classification::{classify_jsonld_error, classify_parameter_error, classify_query_error};
use andromeda_http_response::{classified_report::ClassifiedReport, internal_failure_summary::InternalFailureSummary, problem_response::ProblemResponse};
use andromeda_ngsi_ld_problem_details::{batch_operation::BatchEntityError, error_type::NgsiLdErrorType, problem_detail::ProblemClassification};
use andromeda_snapshot::{error::SnapshotError, service::purge::SnapshotPurgeFailure};
use andromeda_temporal::error::TemporalError;
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
        SnapshotError::Representation(_) => ClassifiedReport::internal(error, InternalFailureSummary::new("failed to render the snapshot")),
        SnapshotError::Persistence(_) => ClassifiedReport::internal(error, InternalFailureSummary::new("the snapshot store is unavailable")),
        SnapshotError::Internal(_) => ClassifiedReport::internal(error, InternalFailureSummary::new("internal snapshot error")),
    }
}

fn classify_entity_error(error: &EntityError) -> ClassifiedReport {
    match error {
        EntityError::Validation(reason) => ClassifiedReport::public(NgsiLdErrorType::BadRequestData, reason.to_string()),
        EntityError::NotFound { .. } | EntityError::TypeNotFound { .. } | EntityError::AttributeNotFound { .. } => {
            ClassifiedReport::public(NgsiLdErrorType::ResourceNotFound, error.to_string())
        }
        EntityError::MemberNotFound(reason) => ClassifiedReport::public(NgsiLdErrorType::ResourceNotFound, reason.to_string()),
        EntityError::AlreadyExists { .. } => ClassifiedReport::public(NgsiLdErrorType::AlreadyExists, error.to_string()),
        EntityError::Contention { .. } => ClassifiedReport::public(NgsiLdErrorType::Conflict, error.to_string()),
        EntityError::OperationNotSupported(reason) => ClassifiedReport::public(NgsiLdErrorType::OperationNotSupported, reason.to_string()),
        EntityError::JsonLd(source) => classify_jsonld_error(source),
        EntityError::Query(source) => classify_query_error(source),
        EntityError::Request(source) => classify_parameter_error(source),
        EntityError::Representation(source) => ClassifiedReport::public(NgsiLdErrorType::BadRequestData, source.to_string()),
        EntityError::Identifier(source) => ClassifiedReport::public(NgsiLdErrorType::BadRequestData, source.to_string()),
        EntityError::EntityTypeListIdentifier(source) => ClassifiedReport::public(NgsiLdErrorType::BadRequestData, source.to_string()),
        EntityError::AttributeListIdentifier(source) => ClassifiedReport::public(NgsiLdErrorType::BadRequestData, source.to_string()),
        EntityError::StoreSaturated => ClassifiedReport::store_saturated(error.to_string()),
        EntityError::TransactionContended => ClassifiedReport::transaction_contended(error.to_string()),
        EntityError::Commit(_) => ClassifiedReport::internal(error, InternalFailureSummary::new("failed to commit the entity write")),
        EntityError::Persistence(_) => ClassifiedReport::internal(error, InternalFailureSummary::new("the entity store is unavailable")),
        EntityError::Internal(_) => ClassifiedReport::internal(error, InternalFailureSummary::new("internal entity error")),
    }
}

fn classify_temporal_error(error: &TemporalError) -> ClassifiedReport {
    match error {
        TemporalError::Validation(reason) => ClassifiedReport::public(NgsiLdErrorType::BadRequestData, reason.to_string()),
        TemporalError::NotFound(reason) => ClassifiedReport::public(NgsiLdErrorType::ResourceNotFound, reason.to_string()),
        TemporalError::OperationNotSupported(reason) => ClassifiedReport::public(NgsiLdErrorType::OperationNotSupported, reason.to_string()),
        // A snapshot path never triggers the delete-by-query bound, but the
        // classification is total over the shared error type.
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

fn classify_federation_error(error: &FederationError) -> ClassifiedReport {
    match error {
        FederationError::Conflict(reason) => ClassifiedReport::public(NgsiLdErrorType::Conflict, reason.to_string()),
        FederationError::OperationNotSupported(reason) => ClassifiedReport::public(NgsiLdErrorType::OperationNotSupported, reason.to_string()),
        FederationError::LoopDetected { .. } => ClassifiedReport::public_binding(ProblemClassification::LoopDetected, error.to_string()),
        FederationError::GatewayTimeout { .. } => ClassifiedReport::public_binding(ProblemClassification::GatewayTimeout, error.to_string()),
        FederationError::RemoteNotFound { .. } => ClassifiedReport::public_binding(ProblemClassification::DistributedResourceNotFound, error.to_string()),
        FederationError::BadGateway { .. } => ClassifiedReport::public_binding(ProblemClassification::BadGateway, error.to_string()),
        FederationError::JsonLd(source) => classify_jsonld_error(source),
        FederationError::Query(source) => classify_query_error(source),
        FederationError::Registration(_) | FederationError::Representation(_) | FederationError::Internal(_) => {
            ClassifiedReport::internal(error, InternalFailureSummary::CONTEXT_FEDERATION)
        }
        FederationError::ForwardedBody(_) => ClassifiedReport::internal(error, InternalFailureSummary::RESPONSE_ENCODING),
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
        self.map_err(|error| classify_snapshot_error(&error).with_instance(instance.clone()))
    }
}

pub(crate) fn project_purge_failure(failure: SnapshotPurgeFailure) -> BatchEntityError {
    let report = classify_snapshot_error(&failure.error);
    BatchEntityError::new(failure.identifier, report.error_type(), report.detail())
}

pub(crate) fn internal_problem(source: &impl Display, summary: InternalFailureSummary, instance: &Uri) -> ProblemResponse {
    ClassifiedReport::internal(source, summary).with_instance(instance.clone())
}

#[cfg(test)]
mod tests {
    use super::classify_snapshot_error;
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
            (SnapshotError::Persistence(PersistenceFailure::Unavailable), NgsiLdErrorType::InternalError),
            (
                SnapshotError::Internal(SnapshotInvariant::LeaseDeadlineOverflow),
                NgsiLdErrorType::InternalError,
            ),
        ] {
            let report = classify_snapshot_error(&error);
            pretty_assertions::assert_eq!(report.error_type(), expected);
            match &error {
                SnapshotError::Persistence(failure) => {
                    pretty_assertions::assert_eq!(report.detail(), "the snapshot store is unavailable");
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
}
