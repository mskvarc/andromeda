//! Exhaustive HTTP classification for Entity Map route failures.

use andromeda_entity::error::EntityError;
use andromeda_entity_map::error::EntityMapError;
use andromeda_federation::error::FederationError;
use andromeda_http_extraction::classification::{classify_jsonld_error, classify_parameter_error, classify_query_error};
use andromeda_http_response::{classified_report::ClassifiedReport, problem_response::ProblemResponse};
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
            EntityMapError::Persistence { .. } | EntityMapError::Invariant { .. } => ClassifiedReport::internal(self),
        }
    }
}

impl EntityMapHttpClassify for EntityError {
    fn classify_for_entity_map_http(&self) -> ClassifiedReport {
        match self {
            EntityError::Validation { detail } => ClassifiedReport::public(NgsiLdErrorType::BadRequestData, detail),
            EntityError::NotFound { .. } | EntityError::TypeNotFound { .. } | EntityError::AttributeNotFound { .. } | EntityError::MemberNotFound { .. } => {
                ClassifiedReport::public(NgsiLdErrorType::ResourceNotFound, self.to_string())
            }
            EntityError::AlreadyExists { .. } => ClassifiedReport::public(NgsiLdErrorType::AlreadyExists, self.to_string()),
            EntityError::Contention { .. } => ClassifiedReport::public(NgsiLdErrorType::Conflict, self.to_string()),
            EntityError::OperationNotSupported { detail } => ClassifiedReport::public(NgsiLdErrorType::OperationNotSupported, detail),
            EntityError::JsonLd(source) => classify_jsonld_error(source),
            EntityError::Query(source) => classify_query_error(source),
            EntityError::Request(source) => classify_parameter_error(source),
            EntityError::Representation(source) => ClassifiedReport::public(NgsiLdErrorType::BadRequestData, source.to_string()),
            EntityError::Identifier(source) => ClassifiedReport::public(NgsiLdErrorType::BadRequestData, source.to_string()),
            EntityError::EntityTypeListIdentifier(source) => ClassifiedReport::public(NgsiLdErrorType::BadRequestData, source.to_string()),
            EntityError::AttributeListIdentifier(source) => ClassifiedReport::public(NgsiLdErrorType::BadRequestData, source.to_string()),
            EntityError::Commit(_) | EntityError::Persistence { .. } | EntityError::Internal { .. } => ClassifiedReport::internal(self),
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
        ClassifiedReport::internal(self)
    }
}

impl EntityMapHttpClassify for TemporalError {
    fn classify_for_entity_map_http(&self) -> ClassifiedReport {
        match self {
            TemporalError::Validation { detail } => ClassifiedReport::public(NgsiLdErrorType::BadRequestData, detail),
            TemporalError::NotFound { detail } => ClassifiedReport::public(NgsiLdErrorType::ResourceNotFound, detail),
            TemporalError::OperationNotSupported { detail } => ClassifiedReport::public(NgsiLdErrorType::OperationNotSupported, detail),
            TemporalError::JsonLd(source) => classify_jsonld_error(source),
            TemporalError::Query(source) => classify_query_error(source),
            TemporalError::Request(source) => classify_parameter_error(source),
            TemporalError::Commit(_) | TemporalError::Persistence { .. } | TemporalError::Rendering { .. } | TemporalError::Internal { .. } => {
                ClassifiedReport::internal(self)
            }
        }
    }
}

impl EntityMapHttpClassify for FederationError {
    fn classify_for_entity_map_http(&self) -> ClassifiedReport {
        match self {
            FederationError::Conflict { detail } => ClassifiedReport::public(NgsiLdErrorType::Conflict, detail),
            FederationError::OperationNotSupported { detail } => ClassifiedReport::public(NgsiLdErrorType::OperationNotSupported, detail),
            FederationError::LoopDetected { .. } => ClassifiedReport::public_binding(ProblemClassification::LoopDetected, self.to_string()),
            FederationError::GatewayTimeout { .. } => ClassifiedReport::public_binding(ProblemClassification::GatewayTimeout, self.to_string()),
            FederationError::RemoteNotFound { .. } => ClassifiedReport::public_binding(ProblemClassification::DistributedResourceNotFound, self.to_string()),
            FederationError::BadGateway { .. } => ClassifiedReport::public_binding(ProblemClassification::BadGateway, self.to_string()),
            FederationError::JsonLd(source) => classify_jsonld_error(source),
            FederationError::Query(source) => classify_query_error(source),
            FederationError::Registration(_) | FederationError::Representation(_) | FederationError::Internal { .. } => ClassifiedReport::internal(self),
        }
    }
}

/// Attach the inbound URI while classifying a typed Entity Map result.
pub trait EntityMapResultExt<T> {
    /// Map the focused failure and attach `instance`.
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

pub(crate) fn internal_problem(source: &impl Display, instance: &Uri) -> ProblemResponse {
    ClassifiedReport::internal(source).with_instance(instance.clone())
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
                pretty_assertions::assert_eq!(report.detail(), "Internal error");
            }
        }
        Ok(())
    }
}
