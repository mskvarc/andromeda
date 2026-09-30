//! Query parameter types for JSON-LD context management endpoints.

use crate::case_insensitive_bool::deserialize_option_bool_case_insensitive;
use andromeda_ngsi_ld_lexicon::tenant::TenantName;
use andromeda_ngsi_ld_model::jsonld_context::ContextKind;
use serde::Deserialize;

/// Query parameters for `GET /jsonldContexts` (list contexts).
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ListParams {
    /// The kind of context to list (`kind`).
    pub kind: Option<ContextKind>,
    /// Whether the response uses the detailed representation (`details`).
    #[serde(default, deserialize_with = "deserialize_option_bool_case_insensitive")]
    pub details: Option<bool>,
}

/// Query parameters for `GET /jsonldContexts/{contextId}` (Serve @context,
/// clause 6.30.3.1).
///
/// `andromedaTenant` is the vendor parameter a URL minted for a non-default
/// tenant's entry carries (`docs/extensions/tenant-qualified-context-urls.md`).
/// Every other operation's parameter set denies unknown fields, so the
/// parameter anywhere but here is the clause 6.3.20 `InvalidRequest` an
/// undefined parameter is.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ServeParams {
    /// Whether the response uses the detailed representation (`details`).
    #[serde(default, deserialize_with = "deserialize_option_bool_case_insensitive")]
    pub details: Option<bool>,
    /// The tenant a URL minted for a non-default tenant's entry names.
    #[serde(rename = "andromedaTenant")]
    pub andromeda_tenant: Option<TenantName>,
}

/// Query parameters for `DELETE /jsonldContexts/{contextId}`.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DeleteParams {
    /// Whether the context is refetched rather than served from cache (`reload`).
    #[serde(default, deserialize_with = "deserialize_option_bool_case_insensitive")]
    pub reload: Option<bool>,
}

#[cfg(test)]
mod tests {
    use crate::jsonld_context::{DeleteParams, ListParams, ServeParams};
    use andromeda_ngsi_ld_lexicon::tenant::TenantName;

    fn tenant(name: &str) -> TenantName {
        name.parse().expect("a valid tenant name")
    }

    #[test]
    fn serve_reads_the_tenant_under_its_vendor_name_and_validates_it() {
        let params: ServeParams = serde_urlencoded::from_str("andromedaTenant=acme&details=TRUE").expect("valid parameters");
        pretty_assertions::assert_eq!(params.andromeda_tenant, Some(tenant("acme")));
        pretty_assertions::assert_eq!(params.details, Some(true));

        assert!(serde_urlencoded::from_str::<ServeParams>("andromedaTenant=not%20a%20tenant").is_err());
        assert!(serde_urlencoded::from_str::<ServeParams>("tenant=acme").is_err());
    }

    #[test]
    fn serve_without_parameters_names_no_tenant_and_no_detail_level() {
        let params: ServeParams = serde_urlencoded::from_str("").expect("valid parameters");
        pretty_assertions::assert_eq!(params.andromeda_tenant, None);
        pretty_assertions::assert_eq!(params.details, None);
    }

    /// Clause 6.3.20: the vendor tenant parameter belongs to Serve alone, so
    /// listing and deleting refuse it like any undefined parameter.
    #[test]
    fn list_and_delete_refuse_the_vendor_tenant_parameter() {
        assert!(serde_urlencoded::from_str::<ListParams>("andromedaTenant=acme").is_err());
        assert!(serde_urlencoded::from_str::<DeleteParams>("andromedaTenant=acme").is_err());
    }
}
