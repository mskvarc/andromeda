//! Query parameter types for the tenant-management reads.

use serde::Deserialize;

/// Query parameters for `GET /ngsi-ld/v1/tenants` (list tenants).
///
/// `deny_unknown_fields` is the point of the type: a parameter the endpoint does
/// not define is rejected (clause 6.3.20) rather than ignored.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TenantListParams {
    /// The page size (clause 5.5.9 `limit`).
    pub limit: Option<u32>,
    /// The opaque, keyset-backed pagination pointer (clause 5.5.9.1).
    ///
    /// A broker-emitted token carried inside a `prev`/`next`/`last`/`first`
    /// `Link`; the client only passes it back by following the link.
    pub cursor: Option<String>,
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
