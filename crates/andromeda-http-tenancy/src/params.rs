//! Query parameter types for the tenant-management reads.

use andromeda_ngsi_ld_request::{
    pagination::{
        page_size::{BROKER_PAGE_SIZE_CAP, PageSizeBounds},
        parameters::PageParameters,
    },
    query_parameter::{QueryParameter, QueryParameterSet},
    query_parameter_set,
};
use serde::Deserialize;

/// The page size of the tenant collection: 100 when the request names no
/// `limit`, and at most the broker's cap, as for every other paged collection.
pub const TENANT_PAGE_SIZE: PageSizeBounds = PageSizeBounds {
    default: 100,
    cap: BROKER_PAGE_SIZE_CAP,
};

/// Query parameters for `GET /ngsi-ld/v1/tenants` (list tenants).
///
/// `deny_unknown_fields` is the point of the type: a parameter the endpoint does
/// not define is rejected (clause 6.3.20) rather than ignored.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TenantListParams {
    /// The page asked for (clauses 5.5.9 and 6.3.10 `limit`, `offset` and
    /// `cursor`), decided by the broker's pagination policy.
    #[serde(flatten)]
    pub page: PageParameters,
    /// The `options` keywords (`sysAttrs`, `usage`).
    pub options: Option<String>,
}

query_parameter_set!(TenantListParams { Options } + PageParameters);

/// Query parameters for `GET /ngsi-ld/v1/tenants/{tenantName}` (one tenant).
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TenantIntrospectionParams {
    /// The `options` keywords (`sysAttrs`, `usage`).
    pub options: Option<String>,
}

query_parameter_set!(TenantIntrospectionParams { Options });

#[cfg(test)]
mod tests {
    use crate::params::TENANT_PAGE_SIZE;
    use andromeda_ngsi_ld_request::pagination::page_size::PageSizeExceeded;
    use pretty_assertions::assert_eq;

    /// The tenant collection answers with 100 by default, serves 1000 and
    /// refuses more.
    #[test]
    fn tenant_page_size_is_one_hundred_capped_at_one_thousand() {
        assert_eq!(TENANT_PAGE_SIZE.limit(None), Ok(100));
        assert_eq!(TENANT_PAGE_SIZE.limit(Some(1000)), Ok(1000));
        assert_eq!(TENANT_PAGE_SIZE.limit(Some(1001)), Err(PageSizeExceeded { requested: 1001, cap: 1000 }));
    }
}
