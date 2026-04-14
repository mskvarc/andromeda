use crate::{domain::uri::Uri, tenancy::types::TenantName};
use serde::{Deserialize, Serialize};

/// Tenant list response for `GET /ngsi-ld/v1/tenants`.
///
/// This endpoint is a broker extension and always returns JSON in this fixed
/// shape (no JSON-LD expansion/compaction variants).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TenantList {
    /// Unique identifier of this tenant list response.
    pub id: Uri,
    /// Response type discriminator (`TenantList`).
    #[serde(rename = "type")]
    pub type_: TenantListType,
    /// Named tenants known to the broker, sorted ascending.
    pub tenant_list: Vec<TenantName>,
}

/// Type discriminator for [`TenantList`].
#[derive(Debug, Clone, Serialize, Deserialize, strum::Display, strum::EnumString)]
pub enum TenantListType {
    /// `TenantList`.
    #[serde(rename = "TenantList")]
    #[strum(serialize = "TenantList")]
    TenantList,
}
