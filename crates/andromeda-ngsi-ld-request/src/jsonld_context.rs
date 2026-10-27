//! Query parameter types for JSON-LD context management endpoints.

use crate::{
    pagination::{
        page_size::{BROKER_PAGE_SIZE_CAP, PageSizeBounds},
        parameters::PageParameters,
    },
    query_parameter::{QueryParameter, QueryParameterSet},
    query_parameter_set,
};
use andromeda_ngsi_ld_model::jsonld_context::ContextKind;
use andromeda_ngsi_ld_query::defined_token::deserialize_optional_token;
use serde::Deserialize;

/// The page size of List @contexts: 100 when the request names no `limit`,
/// and at most the broker's cap, since every `@context` a request carries
/// inline or by reference can add one.
pub const CONTEXT_PAGE_SIZE: PageSizeBounds = PageSizeBounds {
    default: 100,
    cap: BROKER_PAGE_SIZE_CAP,
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
    #[serde(default, deserialize_with = "deserialize_optional_token")]
    pub kind: Option<ContextKind>,
    /// Whether the response uses the detailed representation (`details`).
    pub details: Option<bool>,
}

query_parameter_set!(ListParams { Kind, Details } + PageParameters);

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
    pub details: Option<bool>,
}

query_parameter_set!(ServeParams { Details });

/// Query parameters for `DELETE /jsonldContexts/{contextId}`.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DeleteParams {
    /// Whether the context is refetched rather than served from cache (`reload`).
    pub reload: Option<bool>,
}

query_parameter_set!(DeleteParams { Reload });

#[cfg(test)]
#[expect(
    clippy::expect_used,
    reason = "test code unwraps, expects and panics on purpose; migrating it to typed assertions is pending"
)]
mod tests {
    use crate::{
        jsonld_context::{CONTEXT_PAGE_SIZE, DeleteParams, ListParams, ServeParams},
        pagination::page_size::PageSizeExceeded,
    };
    use andromeda_ngsi_ld_model::jsonld_context::ContextKind;
    use pretty_assertions::assert_eq;

    /// List @contexts answers with 100 by default, serves 1000 and refuses
    /// more.
    #[test]
    fn context_page_size_is_one_hundred_capped_at_one_thousand() {
        assert_eq!(CONTEXT_PAGE_SIZE.limit(None), Ok(100));
        assert_eq!(CONTEXT_PAGE_SIZE.limit(Some(1000)), Ok(1000));
        assert_eq!(CONTEXT_PAGE_SIZE.limit(Some(5000)), Err(PageSizeExceeded { requested: 5000, cap: 1000 }));
    }

    #[test]
    fn serve_reads_the_detail_level() {
        let params: ServeParams = serde_urlencoded::from_str("details=true").expect("valid parameters");
        pretty_assertions::assert_eq!(params.details, Some(true));
        assert!(serde_urlencoded::from_str::<ServeParams>("details=TRUE").is_err());
        assert!(serde_urlencoded::from_str::<DeleteParams>("reload=True").is_err());
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
