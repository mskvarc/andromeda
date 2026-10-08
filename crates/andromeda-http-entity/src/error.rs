//! Focused classification helpers for every non-entity policy consumed by entity routes.

use andromeda_entity::error::EntityError;
use andromeda_entity_map::error::EntityMapError;
use andromeda_federation::error::FederationError;
use andromeda_http_context_registry::response_rendering::{ResponseContextError, classify_response_context_error};
use andromeda_http_extraction::classification::classify_tenancy_error;
use andromeda_http_response::{
    classified_report::ClassifiedReport,
    entity_failure::classify_entity_error,
    entity_map_failure::classify_entity_map_error,
    forwarding_failure::classify_federation_error,
    internal_failure_summary::InternalFailureSummary,
    jsonld_failure::classify_jsonld_error,
    parameter_failure::classify_parameter_error,
    problem_response::ProblemResponse,
    query_failure::classify_query_error,
};
use andromeda_jsonld::error::JsonLdError;
use andromeda_ngsi_ld_problem_details::error_type::NgsiLdErrorType;
use andromeda_ngsi_ld_query::error::QueryError;
use andromeda_ngsi_ld_render::error::RenderError;
use andromeda_ngsi_ld_request::{error::ParameterError, fragment_entity_id::FragmentEntityIdError};
use andromeda_tenancy::error::TenancyError;
use axum::http::Uri;

/// Classifies a typed failure consumed by the entity route binding.
pub trait EntityHttpClassify {
    /// Select its NGSI-LD category, binding status, and disclosure.
    fn classify_for_entity_http(&self) -> ClassifiedReport;
}

impl EntityHttpClassify for EntityError {
    fn classify_for_entity_http(&self) -> ClassifiedReport {
        classify_entity_error(self)
    }
}

impl EntityHttpClassify for EntityMapError {
    fn classify_for_entity_http(&self) -> ClassifiedReport {
        classify_entity_map_error(self)
    }
}

impl EntityHttpClassify for FederationError {
    fn classify_for_entity_http(&self) -> ClassifiedReport {
        classify_federation_error(self)
    }
}

impl EntityHttpClassify for ResponseContextError {
    fn classify_for_entity_http(&self) -> ClassifiedReport {
        classify_response_context_error(self)
    }
}

impl EntityHttpClassify for RenderError {
    fn classify_for_entity_http(&self) -> ClassifiedReport {
        ClassifiedReport::internal(self, InternalFailureSummary::RESPONSE_RENDERING)
    }
}

impl EntityHttpClassify for ParameterError {
    fn classify_for_entity_http(&self) -> ClassifiedReport {
        classify_parameter_error(self)
    }
}

impl EntityHttpClassify for FragmentEntityIdError {
    fn classify_for_entity_http(&self) -> ClassifiedReport {
        // Clause 5.4 requires a Fragment `id` to equal the target's; a Fragment
        // breaking it is not syntactically correct per clause 5.5.4.
        ClassifiedReport::public(NgsiLdErrorType::BadRequestData, self.to_string())
    }
}

impl EntityHttpClassify for JsonLdError {
    fn classify_for_entity_http(&self) -> ClassifiedReport {
        classify_jsonld_error(self)
    }
}

impl EntityHttpClassify for QueryError {
    fn classify_for_entity_http(&self) -> ClassifiedReport {
        classify_query_error(self)
    }
}

impl EntityHttpClassify for serde_json::Error {
    fn classify_for_entity_http(&self) -> ClassifiedReport {
        ClassifiedReport::internal(self, InternalFailureSummary::RESPONSE_ENCODING)
    }
}

impl EntityHttpClassify for TenancyError {
    fn classify_for_entity_http(&self) -> ClassifiedReport {
        classify_tenancy_error(self)
    }
}

/// Construct a deliberate public failure and attach the inbound request URI.
pub(crate) fn public_error(error_type: NgsiLdErrorType, detail: impl Into<String>, instance: &Uri) -> ProblemResponse {
    ClassifiedReport::public(error_type, detail).with_instance(instance.clone())
}

/// Attach the inbound request URI while converting a typed result at the entity HTTP boundary.
pub trait EntityResultExt<T> {
    /// Convert the typed error through its exhaustive route-binding classifier and attach `instance`.
    ///
    /// # Errors
    ///
    /// Returns the [`ProblemResponse`] the error classifies to whenever `self`
    /// is `Err`; an `Ok` result passes through untouched.
    fn with_entity_instance(self, instance: &Uri) -> Result<T, ProblemResponse>;
}

impl<T, E> EntityResultExt<T> for Result<T, E>
where
    E: EntityHttpClassify,
{
    fn with_entity_instance(self, instance: &Uri) -> Result<T, ProblemResponse> {
        self.map_err(|error| error.classify_for_entity_http().with_instance(instance.clone()))
    }
}
