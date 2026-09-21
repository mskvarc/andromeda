//! Exhaustive HTTP classification for Entity Map route failures.

use andromeda_entity::error::EntityError;
use andromeda_entity_map::error::EntityMapError;
use andromeda_federation::error::FederationError;
use andromeda_http_extraction::classification::{classify_jsonld_error, classify_parameter_error, classify_query_error};
use andromeda_http_response::{classified_report::ClassifiedReport, internal_failure_summary::InternalFailureSummary, problem_response::ProblemResponse};
use andromeda_ngsi_ld_problem_details::{error_type::NgsiLdErrorType, problem_detail::ProblemClassification};
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
        match self {
            EntityMapError::NotFound { .. } => ClassifiedReport::public(NgsiLdErrorType::ResourceNotFound, self.to_string()),
            // Clause 5.5.14 with clause 6.3.20: parameters incompatible with
            // the request that created the map are an invalid request.
            EntityMapError::RequestChanged { .. } => ClassifiedReport::public(NgsiLdErrorType::InvalidRequest, self.to_string()),
            EntityMapError::StoreSaturated => ClassifiedReport::store_saturated(self.to_string()),
            EntityMapError::Persistence { .. } | EntityMapError::Invariant { .. } => {
                ClassifiedReport::internal(self, InternalFailureSummary::new("the entity store is unavailable"))
            }
        }
    }
}

impl EntityMapHttpClassify for EntityError {
    fn classify_for_entity_map_http(&self) -> ClassifiedReport {
        match self {
            EntityError::Validation(reason) => ClassifiedReport::public(NgsiLdErrorType::BadRequestData, reason.to_string()),
            EntityError::NotFound { .. } | EntityError::TypeNotFound { .. } | EntityError::AttributeNotFound { .. } => {
                ClassifiedReport::public(NgsiLdErrorType::ResourceNotFound, self.to_string())
            }
            EntityError::MemberNotFound(reason) => ClassifiedReport::public(NgsiLdErrorType::ResourceNotFound, reason.to_string()),
            EntityError::AlreadyExists { .. } => ClassifiedReport::public(NgsiLdErrorType::AlreadyExists, self.to_string()),
            EntityError::Contention { .. } => ClassifiedReport::public(NgsiLdErrorType::Conflict, self.to_string()),
            EntityError::OperationNotSupported(reason) => ClassifiedReport::public(NgsiLdErrorType::OperationNotSupported, reason.to_string()),
            EntityError::JsonLd(source) => classify_jsonld_error(source),
            EntityError::Query(source) => classify_query_error(source),
            EntityError::Request(source) => classify_parameter_error(source),
            EntityError::Representation(source) => ClassifiedReport::public(NgsiLdErrorType::BadRequestData, source.to_string()),
            EntityError::Identifier(source) => ClassifiedReport::public(NgsiLdErrorType::BadRequestData, source.to_string()),
            EntityError::EntityTypeListIdentifier(source) => ClassifiedReport::public(NgsiLdErrorType::BadRequestData, source.to_string()),
            EntityError::AttributeListIdentifier(source) => ClassifiedReport::public(NgsiLdErrorType::BadRequestData, source.to_string()),
            EntityError::StoreSaturated => ClassifiedReport::store_saturated(self.to_string()),
            EntityError::Commit(_) => ClassifiedReport::internal(self, InternalFailureSummary::new("failed to commit the entity write")),
            EntityError::Persistence(_) => ClassifiedReport::internal(self, InternalFailureSummary::new("the entity store is unavailable")),
            EntityError::Internal(_) => ClassifiedReport::internal(self, InternalFailureSummary::new("internal entity error")),
        }
    }
}

impl EntityMapHttpClassify for ParameterError {
    fn classify_for_entity_map_http(&self) -> ClassifiedReport {
        classify_parameter_error(self)
    }
}

impl EntityMapHttpClassify for RenderError {
    fn classify_for_entity_map_http(&self) -> ClassifiedReport {
        ClassifiedReport::internal(self, InternalFailureSummary::RESPONSE_RENDERING)
    }
}

impl EntityMapHttpClassify for TemporalError {
    fn classify_for_entity_map_http(&self) -> ClassifiedReport {
        match self {
            TemporalError::Validation(reason) => ClassifiedReport::public(NgsiLdErrorType::BadRequestData, reason.to_string()),
            TemporalError::NotFound(reason) => ClassifiedReport::public(NgsiLdErrorType::ResourceNotFound, reason.to_string()),
            TemporalError::OperationNotSupported(reason) => ClassifiedReport::public(NgsiLdErrorType::OperationNotSupported, reason.to_string()),
            // The delete-by-query bound: an entity-map path never triggers it,
            // but the classification is total over the shared error type.
            TemporalError::TooManyResults { .. } => ClassifiedReport::public(NgsiLdErrorType::TooManyResults, self.to_string()),
            TemporalError::JsonLd(source) => classify_jsonld_error(source),
            TemporalError::Query(source) => classify_query_error(source),
            TemporalError::Request(source) => classify_parameter_error(source),
            TemporalError::StoreSaturated => ClassifiedReport::store_saturated(self.to_string()),
            TemporalError::Commit(_) => ClassifiedReport::internal(self, InternalFailureSummary::new("failed to persist temporal history")),
            TemporalError::Persistence(_) => ClassifiedReport::internal(self, InternalFailureSummary::new("the temporal store is unavailable")),
            TemporalError::Rendering(_) => ClassifiedReport::internal(self, InternalFailureSummary::new("failed to render the temporal representation")),
            TemporalError::Internal(_) => ClassifiedReport::internal(self, InternalFailureSummary::new("internal temporal error")),
        }
    }
}

impl EntityMapHttpClassify for FederationError {
    fn classify_for_entity_map_http(&self) -> ClassifiedReport {
        match self {
            FederationError::Conflict(reason) => ClassifiedReport::public(NgsiLdErrorType::Conflict, reason.to_string()),
            FederationError::OperationNotSupported(reason) => ClassifiedReport::public(NgsiLdErrorType::OperationNotSupported, reason.to_string()),
            FederationError::LoopDetected { .. } => ClassifiedReport::public_binding(ProblemClassification::LoopDetected, self.to_string()),
            FederationError::GatewayTimeout { .. } => ClassifiedReport::public_binding(ProblemClassification::GatewayTimeout, self.to_string()),
            FederationError::RemoteNotFound { .. } => ClassifiedReport::public_binding(ProblemClassification::DistributedResourceNotFound, self.to_string()),
            FederationError::BadGateway { .. } => ClassifiedReport::public_binding(ProblemClassification::BadGateway, self.to_string()),
            FederationError::JsonLd(source) => classify_jsonld_error(source),
            FederationError::Query(source) => classify_query_error(source),
            FederationError::Registration(_) | FederationError::Representation(_) | FederationError::Internal(_) => {
                ClassifiedReport::internal(self, InternalFailureSummary::CONTEXT_FEDERATION)
            }
        }
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

pub(crate) fn invalid_request(detail: impl Into<String>, instance: &Uri) -> ProblemResponse {
    ClassifiedReport::public(NgsiLdErrorType::InvalidRequest, detail).with_instance(instance.clone())
}

pub(crate) fn bad_request_data(detail: impl Into<String>, instance: &Uri) -> ProblemResponse {
    ClassifiedReport::public(NgsiLdErrorType::BadRequestData, detail).with_instance(instance.clone())
}

pub(crate) fn internal_problem(source: &impl Display, summary: InternalFailureSummary, instance: &Uri) -> ProblemResponse {
    ClassifiedReport::internal(source, summary).with_instance(instance.clone())
}

#[cfg(test)]
mod tests {
    use super::EntityMapHttpClassify;
    use andromeda_entity_map::error::EntityMapError;
    use andromeda_ngsi_ld_lexicon::uri::Uri;
    use andromeda_ngsi_ld_problem_details::error_type::NgsiLdErrorType;
    use std::error::Error;

    #[test]
    fn every_entity_map_variant_has_focused_disclosure() -> Result<(), Box<dyn Error>> {
        let map_identifier = "urn:ngsi-ld:EntityMap:1".parse::<Uri>()?;
        for (error, expected) in [
            (
                EntityMapError::NotFound {
                    identifier: map_identifier.clone(),
                },
                NgsiLdErrorType::ResourceNotFound,
            ),
            (EntityMapError::RequestChanged { identifier: map_identifier }, NgsiLdErrorType::InvalidRequest),
            (
                EntityMapError::Persistence {
                    detail: "database host".to_string(),
                },
                NgsiLdErrorType::InternalError,
            ),
            (EntityMapError::Invariant { detail: "broken".to_string() }, NgsiLdErrorType::InternalError),
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
}
