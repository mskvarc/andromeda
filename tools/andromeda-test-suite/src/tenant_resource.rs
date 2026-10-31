//! The Tenant resource Andromeda serves at `/tenants/{name}` (clause 5.5.10 leaves tenant management to the broker):
//! its type, its name and the identifier built on the name.

use crate::reported_instant::ReportedInstant;
use bytes::Bytes;
use http::Response;
use ngsi_ld::model::Tenant;
use serde::Deserialize;

/// The `type` a Tenant resource carries.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
pub enum TenantType {
    /// A Tenant.
    Tenant,
}

/// Whether a tenant serves its data path.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum TenantStatus {
    /// Serving, with its runtime resident.
    Active,
    /// Serving, with its runtime evicted until the next request.
    Dormant,
    /// Refusing its data path until resumed.
    Suspended,
}

/// A Tenant resource as the broker answers it; the members `options` adds are absent from a plain read.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TenantResource {
    /// `type`.
    #[serde(rename = "type")]
    pub resource_type: TenantType,
    /// `tenantName`: the name the tenant is addressed by.
    pub tenant_name: Tenant,
    /// `id`: `urn:ngsi-ld:Tenant:` followed by the name.
    pub id: String,
    /// `isActive`: whether the tenant serves its data path.
    pub is_active: Option<bool>,
    /// `status`.
    pub status: Option<TenantStatus>,
    /// `createdAt`, with `options=sysAttrs`.
    pub created_at: Option<ReportedInstant>,
    /// `modifiedAt`, with `options=sysAttrs`.
    pub modified_at: Option<ReportedInstant>,
    /// `databaseSize` in bytes, with `options=usage`.
    pub database_size: Option<u64>,
    /// `entityCount`, with `options=usage`.
    pub entity_count: Option<u64>,
    /// `snapshotCount`, with `options=usage`.
    pub snapshot_count: Option<u64>,
    /// `poolConnections`, with `options=usage`.
    pub pool_connections: Option<u64>,
}

/// Why an answer is not a Tenant resource.
#[derive(Debug, thiserror::Error)]
pub enum TenantResourceError {
    /// The body is not the JSON of a Tenant resource.
    #[error("the body is not a Tenant resource")]
    NotATenant(#[source] serde_json::Error),
}

impl TenantResource {
    /// The Tenant resource `response` carries.
    ///
    /// # Errors
    ///
    /// Returns [`TenantResourceError`] when the body is not a Tenant resource.
    pub fn read(response: &Response<Bytes>) -> Result<TenantResource, TenantResourceError> {
        serde_json::from_slice(response.body()).map_err(TenantResourceError::NotATenant)
    }

    /// The Tenant resources the collection `response` carries, in the order it lists them.
    ///
    /// # Errors
    ///
    /// Returns [`TenantResourceError`] when the body is not an array of Tenant resources.
    pub fn read_all(response: &Response<Bytes>) -> Result<Vec<TenantResource>, TenantResourceError> {
        serde_json::from_slice(response.body()).map_err(TenantResourceError::NotATenant)
    }
}

#[cfg(test)]
mod tests {
    use crate::tenant_resource::{TenantResource, TenantStatus};
    use bytes::Bytes;
    use googletest::prelude::*;
    use http::Response;

    #[gtest]
    fn a_tenant_resource_is_read_with_its_name_and_identifier() -> Result<()> {
        let resource = TenantResource::read(&Response::new(Bytes::from_static(
            br#"{"id":"urn:ngsi-ld:Tenant:acme","type":"Tenant","tenantName":"acme"}"#,
        )))?;
        expect_that!(resource.tenant_name.to_string(), eq("acme"));
        verify_that!(resource.id, eq("urn:ngsi-ld:Tenant:acme"))
    }

    #[gtest]
    fn the_members_the_options_add_are_read() -> Result<()> {
        let listing = TenantResource::read_all(&Response::new(Bytes::from_static(
            br#"[{"id":"urn:ngsi-ld:Tenant:a","type":"Tenant","tenantName":"a","isActive":true,"status":"dormant","createdAt":"2026-10-30T04:59:03.350386Z","modifiedAt":"2026-10-30T04:59:07.617482Z","poolConnections":2,"databaseSize":22345407,"entityCount":1,"snapshotCount":0}]"#,
        )))?;
        let resource = listing.first().or_fail()?;
        expect_that!(resource.status, some(eq(TenantStatus::Dormant)));
        expect_that!(resource.entity_count, some(eq(1)));
        expect_that!(resource.created_at, some(anything()));
        Ok(())
    }

    #[gtest]
    fn another_type_is_refused() {
        let refused = TenantResource::read(&Response::new(Bytes::from_static(
            br#"{"id":"urn:ngsi-ld:Tenant:acme","type":"Entity","tenantName":"acme"}"#,
        )));
        expect_that!(refused.is_err(), eq(true));
    }
}
