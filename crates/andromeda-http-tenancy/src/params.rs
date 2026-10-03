//! Query parameter types for the tenant-management reads.

use andromeda_ngsi_ld_request::pagination::{
    page_size::{PageSizeBounds, PageSizeCap},
    parameters::PageParameters,
};
use serde::Deserialize;

/// The page size of the tenant collection: 100 when the request names no
/// `limit`, and whatever it names otherwise. A broker's tenants are few enough
/// to be read whole on request.
pub const TENANT_PAGE_SIZE: PageSizeBounds = PageSizeBounds {
    default: 100,
    cap: PageSizeCap::Uncapped,
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

/// Query parameters for `GET /ngsi-ld/v1/tenants/{tenantName}` (one tenant).
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TenantIntrospectionParams {
    /// The `options` keywords (`sysAttrs`, `usage`).
    pub options: Option<String>,
}
