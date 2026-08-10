//! Exhaustive HTTP classification for Entity Map route failures.

use andromeda_entity::error::EntityError;
use andromeda_entity_map::error::EntityMapError;
use andromeda_http_extraction::classification::{classify_jsonld_error, classify_parameter_error, classify_query_error};
use andromeda_http_response::{classified_report::ClassifiedReport, problem_response::ProblemResponse};
use andromeda_ngsi_ld_problem_details::error_type::NgsiLdErrorType;
use andromeda_ngsi_ld_render::error::RenderError;
use andromeda_ngsi_ld_request::error::ParameterError;
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
            EntityMapError::NotFound { .. } | EntityMapError::EntityNotInMap { .. } => {
                ClassifiedReport::public(NgsiLdErrorType::ResourceNotFound, self.to_string())
            }
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

    #[test]
    fn every_entity_map_variant_has_focused_disclosure() -> Result<(), Box<dyn std::error::Error>> {
        let map_identifier = "urn:ngsi-ld:EntityMap:1".parse::<Uri>()?;
        let entity_identifier = "urn:ngsi-ld:Entity:1".parse::<Uri>()?;
        for (error, expected) in [
            (
                EntityMapError::NotFound {
                    identifier: map_identifier.clone(),
                },
                NgsiLdErrorType::ResourceNotFound,
            ),
            (
                EntityMapError::EntityNotInMap {
                    map_identifier,
                    entity_identifier,
                },
                NgsiLdErrorType::ResourceNotFound,
            ),
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
