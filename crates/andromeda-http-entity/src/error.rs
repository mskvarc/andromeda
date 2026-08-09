//! Focused classification helpers for every non-entity policy consumed by entity routes.

use crate::snapshot_scope::EntitySnapshotError;
use andromeda_entity::error::EntityError;
use andromeda_entity_map::error::EntityMapError;
use andromeda_federation::error::FederationError;
use andromeda_http_extraction::classification::{classify_jsonld_error, classify_parameter_error, classify_query_error};
use andromeda_http_response::{classified_report::ClassifiedReport, problem_response::ProblemResponse};
use andromeda_ngsi_ld_problem_details::{error_type::NgsiLdErrorType, problem_detail::ProblemClassification};
use andromeda_ngsi_ld_query::error::QueryError;
use andromeda_ngsi_ld_render::error::RenderError;
use andromeda_ngsi_ld_request::error::ParameterError;
use andromeda_tenancy::error::TenancyError;
use axum::http::Uri;
use std::fmt::Display;

/// Classifies a typed failure consumed by the entity route binding.
pub trait EntityHttpClassify {
    /// Select its NGSI-LD category, binding status, and disclosure.
    fn classify_for_entity_http(&self) -> ClassifiedReport;
}

impl EntityHttpClassify for EntityError {
    fn classify_for_entity_http(&self) -> ClassifiedReport {
        crate::entity_error::classify_entity_error(self)
    }
}

impl EntityHttpClassify for EntityMapError {
    fn classify_for_entity_http(&self) -> ClassifiedReport {
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

impl EntityHttpClassify for FederationError {
    fn classify_for_entity_http(&self) -> ClassifiedReport {
        match self {
            FederationError::Conflict { detail } => ClassifiedReport::public(NgsiLdErrorType::Conflict, detail),
            FederationError::Gateway { .. } => {
                let report = ClassifiedReport::internal(self);
                ClassifiedReport::public_binding(ProblemClassification::BadGateway, report.detail())
            }
            FederationError::Registration(_) | FederationError::Request(_) | FederationError::Representation(_) | FederationError::Internal { .. } => {
                ClassifiedReport::internal(self)
            }
            FederationError::JsonLd(source) => classify_jsonld_error(source),
            FederationError::Query(source) => classify_query_error(source),
        }
    }
}

impl EntityHttpClassify for EntitySnapshotError {
    fn classify_for_entity_http(&self) -> ClassifiedReport {
        match self {
            EntitySnapshotError::NotFound { detail } => ClassifiedReport::public(NgsiLdErrorType::ResourceNotFound, detail),
            EntitySnapshotError::Internal { .. } => ClassifiedReport::internal(self),
        }
    }
}

impl EntityHttpClassify for RenderError {
    fn classify_for_entity_http(&self) -> ClassifiedReport {
        ClassifiedReport::internal(self)
    }
}

impl EntityHttpClassify for ParameterError {
    fn classify_for_entity_http(&self) -> ClassifiedReport {
        classify_parameter_error(self)
    }
}

impl EntityHttpClassify for QueryError {
    fn classify_for_entity_http(&self) -> ClassifiedReport {
        classify_query_error(self)
    }
}

impl EntityHttpClassify for serde_json::Error {
    fn classify_for_entity_http(&self) -> ClassifiedReport {
        ClassifiedReport::internal(self)
    }
}

impl EntityHttpClassify for TenancyError {
    fn classify_for_entity_http(&self) -> ClassifiedReport {
        classify_tenancy_error(self)
    }
}

/// Classify tenant resolution while retaining typed tenancy semantics.
pub(crate) fn classify_tenancy_error(error: &TenancyError) -> ClassifiedReport {
    match error {
        TenancyError::MultiTenancyDisabled => ClassifiedReport::public(NgsiLdErrorType::NoMultiTenantSupport, error.to_string()),
        TenancyError::UnknownTenant { name } => ClassifiedReport::public(NgsiLdErrorType::NonexistentTenant, format!("Tenant {name} does not exist")),
        TenancyError::RegistryUnavailable { .. } | TenancyError::ProvisioningFailed { .. } | TenancyError::RuntimeUnavailable { .. } => {
            ClassifiedReport::internal(error)
        }
    }
}

/// Construct a deliberate public failure and attach the inbound request URI.
pub(crate) fn public_error(error_type: NgsiLdErrorType, detail: impl Into<String>, instance: &Uri) -> ProblemResponse {
    ClassifiedReport::public(error_type, detail).with_instance(instance.clone())
}

/// Log and sanitize an internal failure before attaching the inbound request URI.
pub(crate) fn internal_error(source: &impl Display, instance: &Uri) -> ProblemResponse {
    ClassifiedReport::internal(source).with_instance(instance.clone())
}

/// Attach the inbound request URI while converting a typed result at the entity HTTP boundary.
pub trait EntityResultExt<T> {
    /// Convert the typed error through its exhaustive route-binding classifier and attach `instance`.
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
