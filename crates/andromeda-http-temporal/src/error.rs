//! Exhaustive classification for every typed failure consumed by temporal routes.

use andromeda_entity_map::error::EntityMapError;
use andromeda_failure_classification::{
    classified_report::ClassifiedReport,
    forwarding_failure::classify_federation_error,
    internal_failure_summary::InternalFailureSummary,
    jsonld_failure::classify_jsonld_error,
    parameter_failure::classify_parameter_error,
    query_failure::classify_query_error,
    temporal_failure::classify_temporal_error,
};
use andromeda_federation::error::FederationError;
use andromeda_http_context_registry::response_rendering::{ResponseContextError, classify_response_context_error};
use andromeda_http_extraction::classification::classify_tenancy_error;
use andromeda_http_response::{
    entity_map_failure::classify_entity_map_error,
    link_target::NotAPostedQuery,
    posted_query_failure::classify_posted_query_error,
    problem_response::ProblemResponse,
};
use andromeda_jsonld::error::JsonLdError;
use andromeda_ngsi_ld_query::error::QueryError;
use andromeda_ngsi_ld_render::error::RenderError;
use andromeda_ngsi_ld_request::error::ParameterError;
use andromeda_posted_query::error::PostedQueryError;
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

/// Convert one temporal-policy failure into its wire response for `instance`.
pub(crate) fn temporal_problem(error: &TemporalError, instance: &Uri) -> ProblemResponse {
    ProblemResponse::classified(classify_temporal_error(error), instance.clone())
}

impl TemporalHttpClassify for EntityMapError {
    fn classify_for_temporal_http(&self) -> ClassifiedReport {
        classify_entity_map_error(self)
    }
}

impl TemporalHttpClassify for FederationError {
    fn classify_for_temporal_http(&self) -> ClassifiedReport {
        classify_federation_error(self)
    }
}

impl TemporalHttpClassify for ParameterError {
    fn classify_for_temporal_http(&self) -> ClassifiedReport {
        classify_parameter_error(self)
    }
}

impl TemporalHttpClassify for PostedQueryError {
    fn classify_for_temporal_http(&self) -> ClassifiedReport {
        classify_posted_query_error(self)
    }
}

// A POST query handler is reached only on its own route, so a request whose
// path names no POST query binding is a wiring fault of the broker.
impl TemporalHttpClassify for NotAPostedQuery {
    fn classify_for_temporal_http(&self) -> ClassifiedReport {
        ClassifiedReport::internal(self, InternalFailureSummary::new("failed to page the posted query"))
    }
}

impl TemporalHttpClassify for QueryError {
    fn classify_for_temporal_http(&self) -> ClassifiedReport {
        classify_query_error(self)
    }
}

impl TemporalHttpClassify for JsonLdError {
    fn classify_for_temporal_http(&self) -> ClassifiedReport {
        classify_jsonld_error(self)
    }
}

impl TemporalHttpClassify for ResponseContextError {
    fn classify_for_temporal_http(&self) -> ClassifiedReport {
        classify_response_context_error(self)
    }
}

impl TemporalHttpClassify for RenderError {
    fn classify_for_temporal_http(&self) -> ClassifiedReport {
        ClassifiedReport::internal(self, InternalFailureSummary::new("failed to render the temporal representation"))
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
    ///
    /// # Errors
    ///
    /// The [`ProblemResponse`] the typed failure classifies to, carrying
    /// `instance` as the problem detail's `instance` member. A successful
    /// result passes its value through untouched.
    fn with_instance(self, instance: &Uri) -> Result<T, ProblemResponse>;
}

impl<T, E> TemporalResultExt<T> for Result<T, E>
where
    E: TemporalHttpClassify,
{
    fn with_instance(self, instance: &Uri) -> Result<T, ProblemResponse> {
        self.map_err(|error| ProblemResponse::classified(error.classify_for_temporal_http(), instance.clone()))
    }
}

#[cfg(test)]
mod tests {
    use andromeda_http_response::declined_representation::single_source_declined;
    use andromeda_ngsi_ld_problem_details::problem_detail::ProblemClassification;

    /// Table 6.4.3.2-3: the 501 of the one registered source holding the data
    /// is answered as a 501 carrying the `OperationNotSupported` category.
    #[test]
    fn a_lone_source_declining_the_representation_is_a_501() {
        let response = single_source_declined(501, &"/ngsi-ld/v1/temporal/entities".parse().expect("valid URI"));
        pretty_assertions::assert_eq!(response.problem().classification(), ProblemClassification::NotImplemented);
        pretty_assertions::assert_eq!(response.problem().status_code().as_u16(), 501);
    }
}
