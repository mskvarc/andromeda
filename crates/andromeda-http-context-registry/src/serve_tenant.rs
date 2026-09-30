//! Which tenant a Serve @context request addresses.
//!
//! Stored `@context`s are per tenant (clause 4.14: "any information related to
//! one Tenant [...] are only visible to users of the same Tenant"), and clause
//! 5.13.1 says the `Hosted` and `ImplicitlyCreated` ones "shall be served by brokers
//! on-demand". A JSON-LD processor dereferencing a context URL sends no
//! `NGSILD-Tenant` header, so the URL the broker mints for an entry of a
//! non-default tenant names the tenant in the vendor query parameter
//! `andromedaTenant` (documented in `docs/extensions/tenant-qualified-context-urls.md`).
//!
//! The header, when present, still decides, as it does for every other
//! operation; the parameter stands in for it only when it is absent, and the
//! two naming different tenants is a request that contradicts itself.

use andromeda_ngsi_ld_lexicon::tenant::TenantName;
use andromeda_tenancy::tenant_ref::TenantRef;
use thiserror::Error;

/// The `NGSILD-Tenant` header and the `andromedaTenant` parameter name
/// different tenants.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
#[error("the NGSILD-Tenant header names tenant '{header}' and the andromedaTenant parameter names tenant '{parameter}'")]
pub(crate) struct ServeTenantConflict {
    /// The tenant the header names.
    header: TenantName,
    /// The tenant the parameter names.
    parameter: TenantName,
}

/// The tenant a Serve request addresses: the header's, else the parameter's,
/// else the default tenant (clause 4.14, "any operation not explicitly
/// specifying a Tenant targets a default Tenant").
///
/// # Errors
///
/// Returns [`ServeTenantConflict`] when both name a tenant and the names differ.
pub(crate) fn serve_tenant(header: Option<TenantName>, parameter: Option<TenantName>) -> Result<TenantRef, ServeTenantConflict> {
    match (header, parameter) {
        (Some(header), Some(parameter)) if header != parameter => Err(ServeTenantConflict { header, parameter }),
        (Some(named), Some(_) | None) | (None, Some(named)) => Ok(TenantRef::from_header(Some(named))),
        (None, None) => Ok(TenantRef::from_header(None)),
    }
}

#[cfg(test)]
mod tests {
    use crate::serve_tenant::serve_tenant;
    use andromeda_ngsi_ld_lexicon::tenant::TenantName;
    use andromeda_tenancy::tenant_ref::TenantRef;

    fn tenant(name: &str) -> TenantName {
        name.parse().expect("a valid tenant name")
    }

    #[test]
    fn the_header_decides_when_present() {
        pretty_assertions::assert_eq!(serve_tenant(Some(tenant("a")), None), Ok(TenantRef::from_header(Some(tenant("a")))));
    }

    #[test]
    fn the_parameter_stands_in_for_an_absent_header() {
        pretty_assertions::assert_eq!(serve_tenant(None, Some(tenant("a"))), Ok(TenantRef::from_header(Some(tenant("a")))));
    }

    #[test]
    fn neither_addresses_the_default_tenant() {
        pretty_assertions::assert_eq!(serve_tenant(None, None), Ok(TenantRef::from_header(None)));
    }

    #[test]
    fn the_header_and_the_parameter_naming_one_tenant_agree() {
        pretty_assertions::assert_eq!(
            serve_tenant(Some(tenant("a")), Some(tenant("a"))),
            Ok(TenantRef::from_header(Some(tenant("a"))))
        );
    }

    #[test]
    fn the_header_and_the_parameter_naming_different_tenants_conflict() {
        assert!(serve_tenant(Some(tenant("a")), Some(tenant("b"))).is_err());
    }
}
