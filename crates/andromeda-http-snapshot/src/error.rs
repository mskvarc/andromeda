//! Exhaustive HTTP classification for snapshot failures.

use andromeda_entity::error::EntityError;
use andromeda_federation::error::FederationError;
use andromeda_http_extraction::classification::{classify_jsonld_error, classify_parameter_error, classify_query_error};
use andromeda_http_response::{classified_report::ClassifiedReport, problem_response::ProblemResponse};
use andromeda_ngsi_ld_problem_details::{batch_operation::BatchEntityError, error_type::NgsiLdErrorType, problem_detail::ProblemClassification};
use andromeda_snapshot::{error::SnapshotError, service::SnapshotPurgeFailure};
use andromeda_temporal::error::TemporalError;
use axum::http::Uri;
use std::{fmt::Display, time::Duration};

/// Exhaustively classify one snapshot policy failure.
#[must_use]
pub fn classify_snapshot_error(error: &SnapshotError) -> ClassifiedReport {
    match error {
        SnapshotError::Validation { detail } => ClassifiedReport::public(NgsiLdErrorType::BadRequestData, detail),
        SnapshotError::NotFound { .. } => ClassifiedReport::public(NgsiLdErrorType::ResourceNotFound, error.to_string()),
        SnapshotError::AlreadyExists { .. } => ClassifiedReport::public(NgsiLdErrorType::AlreadyExists, error.to_string()),
        SnapshotError::PreparationTimeout { .. } => {
            ClassifiedReport::public_binding(ProblemClassification::ServiceUnavailable, error.to_string()).with_retry_after(Duration::from_secs(1))
        }
        SnapshotError::Entity(source) => classify_entity_error(source),
        SnapshotError::Temporal(source) => classify_temporal_error(source),
        SnapshotError::Federation(source) => classify_federation_error(source),
        SnapshotError::Request(source) => classify_parameter_error(source),
        SnapshotError::Representation(_) | SnapshotError::Persistence { .. } | SnapshotError::Internal { .. } => ClassifiedReport::internal(error),
    }
}

fn classify_entity_error(error: &EntityError) -> ClassifiedReport {
    match error {
        EntityError::Validation { detail } => ClassifiedReport::public(NgsiLdErrorType::BadRequestData, detail),
        EntityError::NotFound { .. } | EntityError::TypeNotFound { .. } | EntityError::AttributeNotFound { .. } | EntityError::MemberNotFound { .. } => {
            ClassifiedReport::public(NgsiLdErrorType::ResourceNotFound, error.to_string())
        }
        EntityError::AlreadyExists { .. } => ClassifiedReport::public(NgsiLdErrorType::AlreadyExists, error.to_string()),
        EntityError::Contention { .. } => ClassifiedReport::public(NgsiLdErrorType::Conflict, error.to_string()),
        EntityError::OperationNotSupported { detail } => ClassifiedReport::public(NgsiLdErrorType::OperationNotSupported, detail),
        EntityError::JsonLd(source) => classify_jsonld_error(source),
        EntityError::Query(source) => classify_query_error(source),
        EntityError::Request(source) => classify_parameter_error(source),
        EntityError::Representation(source) => ClassifiedReport::public(NgsiLdErrorType::BadRequestData, source.to_string()),
        EntityError::Identifier(source) => ClassifiedReport::public(NgsiLdErrorType::BadRequestData, source.to_string()),
        EntityError::EntityTypeListIdentifier(source) => ClassifiedReport::public(NgsiLdErrorType::BadRequestData, source.to_string()),
        EntityError::AttributeListIdentifier(source) => ClassifiedReport::public(NgsiLdErrorType::BadRequestData, source.to_string()),
        EntityError::Commit(_) | EntityError::Persistence { .. } | EntityError::Internal { .. } => ClassifiedReport::internal(error),
    }
}

fn classify_temporal_error(error: &TemporalError) -> ClassifiedReport {
    match error {
        TemporalError::Validation { detail } => ClassifiedReport::public(NgsiLdErrorType::BadRequestData, detail),
        TemporalError::NotFound { detail } => ClassifiedReport::public(NgsiLdErrorType::ResourceNotFound, detail),
        TemporalError::OperationNotSupported { detail } => ClassifiedReport::public(NgsiLdErrorType::OperationNotSupported, detail),
        // A snapshot path never triggers the delete-by-query bound, but the
        // classification is total over the shared error type.
        TemporalError::TooManyResults { .. } => ClassifiedReport::public(NgsiLdErrorType::TooManyResults, error.to_string()),
        TemporalError::JsonLd(source) => classify_jsonld_error(source),
        TemporalError::Query(source) => classify_query_error(source),
        TemporalError::Request(source) => classify_parameter_error(source),
        TemporalError::Commit(_) | TemporalError::Persistence { .. } | TemporalError::Rendering { .. } | TemporalError::Internal { .. } => {
            ClassifiedReport::internal(error)
        }
    }
}

fn classify_federation_error(error: &FederationError) -> ClassifiedReport {
    match error {
        FederationError::Conflict { detail } => ClassifiedReport::public(NgsiLdErrorType::Conflict, detail),
        FederationError::OperationNotSupported { detail } => ClassifiedReport::public(NgsiLdErrorType::OperationNotSupported, detail),
        FederationError::LoopDetected { .. } => ClassifiedReport::public_binding(ProblemClassification::LoopDetected, error.to_string()),
        FederationError::GatewayTimeout { .. } => ClassifiedReport::public_binding(ProblemClassification::GatewayTimeout, error.to_string()),
        FederationError::RemoteNotFound { .. } => ClassifiedReport::public_binding(ProblemClassification::DistributedResourceNotFound, error.to_string()),
        FederationError::BadGateway { .. } => ClassifiedReport::public_binding(ProblemClassification::BadGateway, error.to_string()),
        FederationError::JsonLd(source) => classify_jsonld_error(source),
        FederationError::Query(source) => classify_query_error(source),
        FederationError::Registration(_) | FederationError::Representation(_) | FederationError::Internal { .. } => ClassifiedReport::internal(error),
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

pub(crate) fn internal_problem(source: &impl Display, instance: &Uri) -> ProblemResponse {
    ClassifiedReport::internal(source).with_instance(instance.clone())
}

#[cfg(test)]
mod tests {
    use super::classify_snapshot_error;
    use andromeda_ngsi_ld_lexicon::uri::Uri;
    use andromeda_ngsi_ld_problem_details::error_type::NgsiLdErrorType;
    use andromeda_snapshot::error::SnapshotError;
    use std::error::Error;

    #[test]
    fn focused_snapshot_categories_and_disclosure_are_stable() -> Result<(), Box<dyn Error>> {
        let identifier = "urn:ngsi-ld:Snapshot:1".parse::<Uri>()?;
        for (error, expected) in [
            (SnapshotError::Validation { detail: "invalid".to_string() }, NgsiLdErrorType::BadRequestData),
            (
                SnapshotError::NotFound {
                    identifier: identifier.clone(),
                },
                NgsiLdErrorType::ResourceNotFound,
            ),
            (SnapshotError::AlreadyExists { identifier }, NgsiLdErrorType::AlreadyExists),
            (
                SnapshotError::Persistence {
                    detail: "database host".to_string(),
                },
                NgsiLdErrorType::InternalError,
            ),
            (SnapshotError::Internal { detail: "broken".to_string() }, NgsiLdErrorType::InternalError),
        ] {
            let report = classify_snapshot_error(&error);
            pretty_assertions::assert_eq!(report.error_type(), expected);
            if expected == NgsiLdErrorType::InternalError {
                pretty_assertions::assert_eq!(report.detail(), "Internal error");
            }
        }
        Ok(())
    }
}
