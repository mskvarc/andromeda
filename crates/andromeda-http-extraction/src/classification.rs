//! Exhaustive classification shared by HTTP extractors.

use crate::{api_root::ApiRootError, request_links::ContextLinkError};
use andromeda_http_response::{classified_report::ClassifiedReport, internal_failure_summary::InternalFailureSummary, store_failure::classify_store_failure};
use andromeda_ngsi_ld_problem_details::error_type::NgsiLdErrorType;
use andromeda_tenancy::{error::TenancyError, runtime_build_failure::RuntimeBuildFailure};

/// The summary disclosed for a tenant that could not be resolved for a reason
/// the client cannot act on.
const TENANT_RESOLUTION: InternalFailureSummary = InternalFailureSummary::new("failed to resolve the tenant");

/// Classify tenant resolution independently of any policy route binding.
#[must_use]
pub fn classify_tenancy_error(error: &TenancyError) -> ClassifiedReport {
    match error {
        TenancyError::MultiTenancyDisabled => ClassifiedReport::public(NgsiLdErrorType::NoMultiTenantSupport, error.to_string()),
        TenancyError::UnknownTenant { .. } => ClassifiedReport::public(NgsiLdErrorType::NonexistentTenant, error.to_string()),
        // Backpressure, not a fault: a saturated backend pool during cold
        // resolution is shed as a 503 with Retry-After, the same as the
        // request-path stores, rather than reported as an internal 500 the
        // client cannot act on.
        TenancyError::Saturated { .. } => ClassifiedReport::store_saturated(error.to_string()),
        // A store condition met while the tenant is resolved is classified as
        // it is on every request-path store: retryable when it clears.
        TenancyError::RegistryUnavailable(cause)
        | TenancyError::ProvisioningFailed { cause, .. }
        | TenancyError::RuntimeUnavailable {
            cause: RuntimeBuildFailure::Persistence(cause),
            ..
        } => classify_store_failure(*cause, error, TENANT_RESOLUTION),
        TenancyError::RuntimeUnavailable {
            cause: RuntimeBuildFailure::JsonLdEngine | RuntimeBuildFailure::Invariant,
            ..
        } => ClassifiedReport::internal(error, TENANT_RESOLUTION),
    }
}

/// Classify a request authority no `apiRoot` can be built from at the shared
/// extraction boundary.
#[must_use]
pub fn classify_api_root_error(error: &ApiRootError) -> ClassifiedReport {
    match error {
        ApiRootError::InvalidOrigin(_) => ClassifiedReport::public(NgsiLdErrorType::InvalidRequest, error.to_string()),
    }
}

/// Classify a request's `Link` headers that name no usable context.
///
/// Clause 5.5.4 validates the context of every request, and clause 6.3.5 reads
/// it from one `Link` header and refuses one beside an `application/ld+json`
/// body: a malformed link, a second context link, a relative target that does
/// not resolve against the request URI and a context link beside an
/// `application/ld+json` body are each a `BadRequestData`. A request authority
/// that forms no origin is the same `InvalidRequest` it draws wherever the
/// broker reads it ([`classify_api_root_error`]).
#[must_use]
pub fn classify_context_link_error(error: &ContextLinkError) -> ClassifiedReport {
    match error {
        ContextLinkError::InvalidOrigin(source) => classify_api_root_error(source),
        ContextLinkError::NotVisibleAscii(_)
        | ContextLinkError::Malformed(_)
        | ContextLinkError::SeveralContexts { .. }
        | ContextLinkError::NoRequestAuthority
        | ContextLinkError::RequestUriNotIri(_)
        | ContextLinkError::Unresolvable(_)
        | ContextLinkError::ResolvedNotAbsolute(_)
        | ContextLinkError::CombinedWithLdJson => ClassifiedReport::public(NgsiLdErrorType::BadRequestData, error.to_string()),
    }
}

#[cfg(test)]
mod tests {
    use andromeda_ngsi_ld_problem_details::error_type::NgsiLdErrorType;
    use andromeda_tenancy::runtime_build_failure::RuntimeBuildFailure;

    #[test]
    fn a_saturated_tenant_resolution_is_a_503_with_retry_after() {
        use crate::classification::classify_tenancy_error;
        use andromeda_ngsi_ld_lexicon::tenant::TenantName;
        use andromeda_ngsi_ld_problem_details::problem_detail::ProblemClassification;
        use andromeda_tenancy::{error::TenancyError, saturated_wait::SaturatedWait, tenant_ref::TenantRef};
        use std::time::Duration;

        let tenant = TenantRef::Named(TenantName::try_from("acme".to_owned()).expect("valid tenant name"));
        // The detail is the error's Display, which leaves out the wait that
        // shed the request: the answer reads the same whatever the wait.
        let details: Vec<String> = [SaturatedWait::BuildPermit, SaturatedWait::MigrationPool]
            .into_iter()
            .map(|wait| {
                let error = TenancyError::Saturated {
                    tenant: tenant.clone(), // clone: each case owns its tenant reference
                    wait,
                };
                let saturated = classify_tenancy_error(&error);
                pretty_assertions::assert_eq!(saturated.classification(), ProblemClassification::ServiceUnavailable);
                pretty_assertions::assert_eq!(saturated.retry_after(), Some(Duration::from_secs(1)));
                pretty_assertions::assert_eq!(saturated.detail(), error.to_string());
                saturated.detail().to_owned()
            })
            .collect();
        pretty_assertions::assert_eq!(details[0], details[1]);

        // A genuine construction fault stays an internal 500, not backpressure.
        let fault = classify_tenancy_error(&TenancyError::RuntimeUnavailable {
            tenant,
            cause: RuntimeBuildFailure::Invariant,
        });
        pretty_assertions::assert_eq!(fault.error_type(), NgsiLdErrorType::InternalError);
    }

    /// Clause 5.5.6 with RFC 7231 6.6.4: a tenant whose stores cannot be
    /// reached while it is resolved is a retryable 503; a store fault stays
    /// an internal 500.
    #[test]
    fn an_unreachable_tenant_store_is_a_retryable_503() {
        use crate::classification::classify_tenancy_error;
        use andromeda_ngsi_ld_lexicon::tenant::TenantName;
        use andromeda_ngsi_ld_problem_details::problem_detail::ProblemClassification;
        use andromeda_persistence_failure::persistence_failure::PersistenceFailure;
        use andromeda_tenancy::{error::TenancyError, tenant_ref::TenantRef};
        use std::time::Duration;

        let name = TenantName::try_from("acme".to_owned()).expect("valid tenant name");
        for error in [
            TenancyError::RegistryUnavailable(PersistenceFailure::Unavailable),
            TenancyError::ProvisioningFailed {
                name: name.clone(), // clone: each case owns its tenant name
                cause: PersistenceFailure::Unavailable,
            },
            TenancyError::RuntimeUnavailable {
                tenant: TenantRef::Named(name),
                cause: RuntimeBuildFailure::Persistence(PersistenceFailure::Unavailable),
            },
        ] {
            let report = classify_tenancy_error(&error);
            pretty_assertions::assert_eq!(report.classification(), ProblemClassification::ServiceUnavailable, "{error}");
            pretty_assertions::assert_eq!(report.retry_after(), Some(Duration::from_secs(5)), "{error}");
        }
        let fault = classify_tenancy_error(&TenancyError::RegistryUnavailable(PersistenceFailure::Undecodable));
        pretty_assertions::assert_eq!(fault.error_type(), NgsiLdErrorType::InternalError);
        pretty_assertions::assert_eq!(fault.retry_after(), None);
    }
}
