//! Exhaustive classification for every typed failure consumed by temporal routes.

use andromeda_entity_map::error::EntityMapError;
use andromeda_federation::error::FederationError;
use andromeda_http_extraction::classification::{classify_jsonld_error, classify_parameter_error, classify_query_error, classify_tenancy_error};
use andromeda_http_response::{classified_report::ClassifiedReport, problem_response::ProblemResponse, provision::classify_forwarding_error};
use andromeda_ngsi_ld_problem_details::{error_type::NgsiLdErrorType, problem_detail::ProblemClassification};
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

/// The response a temporal read gives when the one exclusive or redirect
/// source holding all of its data declined the representation with `status`
/// (Table 6.4.3.2-3, clause 6.3.7): the source's 501 is the operation's answer.
pub(crate) fn single_source_declined(status: u16, instance: &Uri) -> ProblemResponse {
    ClassifiedReport::public_binding(
        ProblemClassification::NotImplemented,
        format!("the registered context source holding the requested data answered HTTP {status}: it does not support the requested representation"),
    )
    .with_instance(instance.clone())
}

impl TemporalHttpClassify for EntityMapError {
    fn classify_for_temporal_http(&self) -> ClassifiedReport {
        match self {
            EntityMapError::NotFound { identifier } => {
                ClassifiedReport::public(NgsiLdErrorType::ResourceNotFound, format!("Entity Map {identifier} not found"))
            }
            // Clause 5.5.14 with clause 6.3.20: parameters incompatible with
            // the request that created the map are an invalid request.
            EntityMapError::RequestChanged { .. } => ClassifiedReport::public(NgsiLdErrorType::InvalidRequest, self.to_string()),
            EntityMapError::Persistence { .. } | EntityMapError::Invariant { .. } => ClassifiedReport::internal(self),
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
            FederationError::Conflict { .. }
            | FederationError::OperationNotSupported { .. }
            | FederationError::LoopDetected { .. }
            | FederationError::GatewayTimeout { .. }
            | FederationError::RemoteNotFound { .. }
            | FederationError::BadGateway { .. }
            | FederationError::Registration(_)
            | FederationError::Representation(_)
            | FederationError::Internal { .. } => classify_forwarding_error(self),
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
    use crate::error::{classify_temporal_error, single_source_declined};
    use andromeda_context_commit::error::CommitError;
    use andromeda_jsonld::error::JsonLdError;
    use andromeda_ngsi_ld_problem_details::{error_type::NgsiLdErrorType, problem_detail::ProblemClassification};
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

    /// Clause 5.5.6: a temporal body the commit refused is invalid input, so
    /// the client learns what to change instead of being told to retry. The
    /// policy conversion is what carries the refusal out of the commit port.
    #[test]
    fn a_commit_that_refused_the_submission_reports_bad_request_data() {
        let error = TemporalError::from(CommitError::Submission {
            detail: "'QQQ' is not a UN/CEFACT common code".to_string(),
        });
        let report = classify_temporal_error(&error);
        pretty_assertions::assert_eq!(report.error_type(), NgsiLdErrorType::BadRequestData);
        pretty_assertions::assert_eq!(report.detail(), "'QQQ' is not a UN/CEFACT common code");
    }

    /// Table 6.4.3.2-3: the 501 of the one registered source holding the data
    /// is answered as a 501 carrying the OperationNotSupported category.
    #[test]
    fn a_lone_source_declining_the_representation_is_a_501() {
        let response = single_source_declined(501, &"/ngsi-ld/v1/temporal/entities".parse().expect("valid URI"));
        pretty_assertions::assert_eq!(response.problem().classification(), ProblemClassification::NotImplemented);
        pretty_assertions::assert_eq!(response.problem().status_code().as_u16(), 501);
    }

    #[test]
    fn temporal_public_detail_is_preserved() {
        let report = classify_temporal_error(&TemporalError::validation("missing timerel"));
        pretty_assertions::assert_eq!(report.detail(), "missing timerel");
    }
}
