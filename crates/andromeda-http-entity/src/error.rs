//! Focused classification helpers for every non-entity policy consumed by entity routes.

use crate::entity_error::classify_entity_error;
use andromeda_entity::error::EntityError;
use andromeda_entity_map::error::EntityMapError;
use andromeda_federation::error::FederationError;
use andromeda_http_context_registry::response_rendering::{ResponseContextError, classify_response_context_error};
use andromeda_http_extraction::classification::{classify_jsonld_error, classify_parameter_error, classify_query_error, classify_tenancy_error};
use andromeda_http_response::{
    classified_report::ClassifiedReport,
    internal_failure_summary::InternalFailureSummary,
    problem_response::ProblemResponse,
    provision::classify_forwarding_error,
};
use andromeda_jsonld::error::JsonLdError;
use andromeda_ngsi_ld_problem_details::error_type::NgsiLdErrorType;
use andromeda_ngsi_ld_query::error::QueryError;
use andromeda_ngsi_ld_render::error::RenderError;
use andromeda_ngsi_ld_request::error::ParameterError;
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
        match self {
            EntityMapError::NotFound { .. } => ClassifiedReport::public(NgsiLdErrorType::ResourceNotFound, self.to_string()),
            // Clause 5.5.14 with clause 6.3.20: parameters incompatible with
            // the request that created the map are an invalid request.
            EntityMapError::RequestChanged { .. } => ClassifiedReport::public(NgsiLdErrorType::InvalidRequest, self.to_string()),
            EntityMapError::StoreSaturated => ClassifiedReport::store_saturated(self.to_string()),
            EntityMapError::Persistence(_) | EntityMapError::Invariant(_) => {
                ClassifiedReport::internal(self, InternalFailureSummary::new("the entity store is unavailable"))
            }
        }
    }
}

impl EntityHttpClassify for FederationError {
    fn classify_for_entity_http(&self) -> ClassifiedReport {
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
            | FederationError::ForwardedBody(_)
            | FederationError::Internal(_) => classify_forwarding_error(self),
        }
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
