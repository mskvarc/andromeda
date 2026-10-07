//! Query parameter types for JSON-LD context management endpoints.

use crate::{
    case_insensitive_bool::deserialize_option_bool_case_insensitive,
    pagination::{
        page_size::{PageSizeBounds, PageSizeCap},
        parameters::PageParameters,
    },
};
use andromeda_ngsi_ld_model::jsonld_context::ContextKind;
use serde::Deserialize;

/// The page size of List @contexts: 100 when the request names no `limit`,
/// and whatever it names otherwise, as for the broker's other administrative
/// collections.
pub const CONTEXT_PAGE_SIZE: PageSizeBounds = PageSizeBounds {
    default: 100,
    cap: PageSizeCap::Uncapped,
};

/// Query parameters for `GET /jsonldContexts` (list contexts).
///
/// Clause 5.13.3.4 lists every stored `@context`, so the collection is paged
/// like every other one, by the broker's pagination policy (clause 4.12).
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ListParams {
    /// The page asked for (clauses 5.5.9 and 6.3.10 `limit`, `offset` and
    /// `cursor`).
    #[serde(flatten)]
    pub page: PageParameters,
    /// The kind of context to list (`kind`).
    pub kind: Option<ContextKind>,
    /// Whether the response uses the detailed representation (`details`).
    #[serde(default, deserialize_with = "deserialize_option_bool_case_insensitive")]
    pub details: Option<bool>,
}

/// Query parameters for `GET /jsonldContexts/{contextId}` (Serve @context,
/// clause 6.30.3.1).
///
/// The tenant is named by the `NGSILD-Tenant` header alone (clauses 4.14 and
/// 6.3.14), so any parameter beside `details` is the clause 6.3.20
/// `InvalidRequest` an undefined parameter is.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ServeParams {
    /// Whether the response uses the detailed representation (`details`).
    #[serde(default, deserialize_with = "deserialize_option_bool_case_insensitive")]
    pub details: Option<bool>,
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
    use andromeda_ngsi_ld_model::jsonld_context::ContextKind;
    use pretty_assertions::assert_eq;

    #[test]
    fn serve_reads_the_detail_level() {
        let params: ServeParams = serde_urlencoded::from_str("details=TRUE").expect("valid parameters");
        pretty_assertions::assert_eq!(params.details, Some(true));
    }

    #[test]
    fn serve_without_parameters_names_no_detail_level() {
        let params: ServeParams = serde_urlencoded::from_str("").expect("valid parameters");
        pretty_assertions::assert_eq!(params.details, None);
    }

    /// Clauses 4.14, 6.3.14 and 6.3.20: the tenant is named by the
    /// `NGSILD-Tenant` header alone, so every operation refuses a
    /// vendor tenant parameter like any undefined parameter.
    #[test]
    fn every_operation_refuses_the_former_tenant_parameter() {
        assert!(serde_urlencoded::from_str::<ServeParams>("andromedaTenant=acme").is_err());
        assert!(serde_urlencoded::from_str::<ServeParams>("details=true&andromedaTenant=acme").is_err());
        assert!(serde_urlencoded::from_str::<ListParams>("andromedaTenant=acme").is_err());
        assert!(serde_urlencoded::from_str::<DeleteParams>("andromedaTenant=acme").is_err());
    }

    /// The listing reads its filter, its detail level and the page together.
    #[test]
    fn list_reads_the_filter_the_detail_level_and_the_page() {
        let params: ListParams = serde_urlencoded::from_str("kind=Hosted&details=true&limit=3&offset=6").expect("valid parameters");
        assert_eq!(params.kind, Some(ContextKind::Hosted));
        assert_eq!(params.details, Some(true));
        assert_eq!(params.page.limit, Some(3));
        assert_eq!(params.page.offset, Some(6));
    }

    /// A parameter the listing does not define stays unknown beside the page.
    #[test]
    fn list_refuses_an_undefined_parameter_beside_the_page() {
        let message = serde_urlencoded::from_str::<ListParams>("limit=3&page=2").expect_err("rejected").to_string();
        assert!(message.contains("unknown field"), "{message}");
    }
}
