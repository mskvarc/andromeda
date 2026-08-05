//! The tenant listing the broker exposes at `GET /ngsi-ld/v1/tenants` (ETSI GS CIM 009 clause 6.3.14).

use andromeda_ngsi_ld_lexicon::{constant_string, tenant::TenantName, uri::Uri};
use serde::{Deserialize, Serialize};

constant_string!(TenantListType, "TenantList");

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

#[cfg(test)]
mod tests {
    use crate::tenant_list::{TenantList, TenantListType};
    use andromeda_ngsi_ld_lexicon::{tenant::TenantName, uri::Uri};

    #[test]
    fn a_tenant_list_renders_its_fixed_type_discriminator_and_its_names() {
        let list = TenantList {
            id: "urn:ngsi-ld:TenantList:1".parse::<Uri>().expect("identifier"),
            type_: TenantListType,
            tenant_list: vec!["alpha".parse::<TenantName>().expect("name"), "beta".parse::<TenantName>().expect("name")],
        };
        let json = serde_json::to_value(&list).expect("serialises");
        pretty_assertions::assert_eq!(
            json,
            serde_json::json!({"id": "urn:ngsi-ld:TenantList:1", "type": "TenantList", "tenantList": ["alpha", "beta"]})
        );
    }

    #[test]
    fn a_tenant_list_rejects_a_foreign_type_discriminator() {
        let payload = serde_json::json!({"id": "urn:ngsi-ld:TenantList:1", "type": "EntityList", "tenantList": []});
        assert!(serde_json::from_value::<TenantList>(payload).is_err());
    }
}
