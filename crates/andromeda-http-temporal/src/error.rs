//! Exhaustive classification for every typed failure consumed by temporal routes.

use crate::snapshot_scope::TemporalSnapshotError;
use andromeda_entity_map::error::EntityMapError;
use andromeda_http_extraction::classification::{classify_jsonld_error, classify_parameter_error, classify_query_error, classify_tenancy_error};
use andromeda_http_response::{classified_report::ClassifiedReport, problem_response::ProblemResponse};
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
pub fn classify_temporal_error(error: &TemporalError) -> ClassifiedReport {
    match error {
        TemporalError::Validation { detail } => ClassifiedReport::public(NgsiLdErrorType::BadRequestData, detail),
        TemporalError::NotFound { detail } => ClassifiedReport::public(NgsiLdErrorType::ResourceNotFound, detail),
        TemporalError::OperationNotSupported { detail } => ClassifiedReport::public(NgsiLdErrorType::OperationNotSupported, detail),
        TemporalError::JsonLd(source) => classify_jsonld_error(source),
        TemporalError::Query(source) => classify_query_error(source),
        TemporalError::Request(source) => classify_parameter_error(source),
        TemporalError::Commit(_) | TemporalError::Persistence { .. } | TemporalError::Rendering { .. } | TemporalError::Internal { .. } => {
            ClassifiedReport::internal(error)
        }
    }
}

/// Convert one temporal-policy failure into its wire response for `instance`.
pub(crate) fn temporal_problem(error: &TemporalError, instance: &Uri) -> ProblemResponse {
    classify_temporal_error(error).with_instance(instance.clone())
}

impl TemporalHttpClassify for TemporalSnapshotError {
    fn classify_for_temporal_http(&self) -> ClassifiedReport {
        match self {
            TemporalSnapshotError::NotFound { detail } => ClassifiedReport::public(NgsiLdErrorType::ResourceNotFound, detail),
            TemporalSnapshotError::Internal { .. } => ClassifiedReport::internal(self),
        }
    }
}

impl TemporalHttpClassify for EntityMapError {
    fn classify_for_temporal_http(&self) -> ClassifiedReport {
        match self {
            EntityMapError::NotFound { identifier } => {
                ClassifiedReport::public(NgsiLdErrorType::ResourceNotFound, format!("Entity Map {identifier} not found"))
            }
            EntityMapError::EntityNotInMap {
                entity_identifier,
                map_identifier,
            } => ClassifiedReport::public(
                NgsiLdErrorType::ResourceNotFound,
                format!("Entity {entity_identifier} is not present in Entity Map {map_identifier}"),
            ),
            EntityMapError::Persistence { .. } | EntityMapError::Invariant { .. } => ClassifiedReport::internal(self),
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
        ClassifiedReport::internal(self)
    }
}

impl TemporalHttpClassify for TemporalSimplifyError {
    fn classify_for_temporal_http(&self) -> ClassifiedReport {
        ClassifiedReport::internal(self)
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
    use andromeda_context_commit::error::CommitError;
    use andromeda_jsonld::error::JsonLdError;
    use andromeda_ngsi_ld_problem_details::error_type::NgsiLdErrorType;
    use andromeda_ngsi_ld_query::error::QueryError;
    use andromeda_ngsi_ld_request::error::ParameterError;
    use andromeda_temporal::error::TemporalError;

    #[test]
    fn every_temporal_error_variant_has_one_typed_http_classification() {
        for (name, error, expected) in [
            ("Validation", TemporalError::validation("invalid window"), NgsiLdErrorType::BadRequestData),
            ("NotFound", TemporalError::not_found("missing history"), NgsiLdErrorType::ResourceNotFound),
            (
                "OperationNotSupported",
                TemporalError::operation_not_supported("aggregation refused"),
                NgsiLdErrorType::OperationNotSupported,
            ),
            (
                "JsonLd",
                TemporalError::JsonLd(JsonLdError::invalid_document("bad document")),
                NgsiLdErrorType::BadRequestData,
            ),
            ("Query", TemporalError::Query(QueryError::QEmpty), NgsiLdErrorType::InvalidRequest),
            (
                "Request",
                TemporalError::Request(ParameterError::OptionsInvalid("bad".to_string())),
                NgsiLdErrorType::InvalidRequest,
            ),
            (
                "Commit",
                TemporalError::Commit(CommitError::Persistence {
                    detail: "db-7.internal".to_string(),
                }),
                NgsiLdErrorType::InternalError,
            ),
            ("Persistence", TemporalError::persistence("db-7.internal"), NgsiLdErrorType::InternalError),
            ("Rendering", TemporalError::rendering("secret row"), NgsiLdErrorType::InternalError),
            ("Internal", TemporalError::internal("secret invariant"), NgsiLdErrorType::InternalError),
        ] {
            let report = classify_temporal_error(&error);
            pretty_assertions::assert_eq!(report.error_type(), expected, "wrong category for {name}");
            pretty_assertions::assert_eq!(report.classification().status_code(), expected.status_code(), "wrong status for {name}");
            if expected == NgsiLdErrorType::InternalError {
                pretty_assertions::assert_eq!(report.detail(), "Internal error", "internal detail leaked for {name}");
            }
        }
    }

    #[test]
    fn temporal_public_detail_is_preserved() {
        let report = classify_temporal_error(&TemporalError::validation("missing timerel"));
        pretty_assertions::assert_eq!(report.detail(), "missing timerel");
    }
}
