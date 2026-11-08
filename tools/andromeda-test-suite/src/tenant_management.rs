//! Reading and suspending a tenant through the broker's tenant-management extension, the Andromeda resources at
//! `/tenants/{name}` no NGSI-LD operation covers, so their requests are sent as written.
//!
//! A plain read of a tenant never touches its database, so watching a tenant's `status` never loads it: a test sees
//! a tenant go `dormant`, or stay so, without disturbing it. `options=usage` does query the tenant's database.

use crate::{
    broker::BrokerClient,
    database_name::DatabaseName,
    eventually::{Poll, TimedOut, eventually},
    raw_call::target,
    tenant_resource::{TenantResource, TenantResourceError, TenantStatus},
    wire::{WireError, request_to},
};
use bytes::Bytes;
use http::{
    HeaderValue,
    Method,
    Response,
    header::{ACCEPT, CONTENT_TYPE},
};
use ngsi_ld::{
    client::{ClientError, ReqwestError},
    model::Tenant,
};
use serde::Serialize;
use std::time::Duration;
use tokio::time::{Instant, sleep};

/// What a read of a tenant asks for beside its status.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TenantView {
    /// The resource alone: no query of the tenant's database.
    Plain,
    /// The resource with its usage (`options=usage`), read from the tenant's database.
    Usage,
}

/// Whether a tenant is to serve its data path.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Activity {
    /// In service.
    Active,
    /// Suspended.
    Suspended,
}

/// The body that sets a tenant's `isActive`.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct ActivityPatch {
    /// `isActive`.
    is_active: bool,
}

/// Why a tenant could not be read or changed.
#[derive(Debug, thiserror::Error)]
pub enum TenantManagementError {
    /// The request target could not be written.
    #[error(transparent)]
    Wire(#[from] WireError),
    /// The broker could not be reached, or answered with an error.
    #[error("the tenant-management request failed")]
    Client(#[source] Box<ClientError<ReqwestError>>),
    /// The answer is not a Tenant resource.
    #[error(transparent)]
    Resource(#[from] TenantResourceError),
    /// The body setting `isActive` could not be written.
    #[error("the tenant patch could not be written")]
    Patch(#[source] serde_json::Error),
    /// A read reported no status.
    #[error("the resource of tenant {0} reports no status")]
    NoStatus(Tenant),
    /// A read with its usage named no database.
    #[error("the usage of tenant {0} names no database")]
    NoDatabase(Tenant),
    /// The tenant did not reach the status awaited.
    #[error("the tenant did not reach the status awaited")]
    Status(#[source] TimedOut<TenantStatus>),
}

/// The Tenant resource of `tenant` on the broker `client` speaks to.
///
/// # Errors
///
/// Returns [`TenantManagementError`] when the read is refused or does not answer a Tenant resource.
pub async fn read_tenant(client: &BrokerClient, tenant: &Tenant, view: TenantView) -> Result<TenantResource, TenantManagementError> {
    let parameters: &[(&str, &str)] = match view {
        TenantView::Plain => &[],
        TenantView::Usage => &[("options", "usage")],
    };
    let request =
        request_to(client, Method::GET, &target(&format!("tenants/{tenant}"), parameters))?.append_header(ACCEPT, HeaderValue::from_static("application/json"));
    let answer = client
        .send_unchecked(request)
        .await
        .map_err(|error| TenantManagementError::Client(Box::new(error)))?;
    Ok(TenantResource::read(&answer)?)
}

/// The database holding `tenant`, as its usage names it.
///
/// # Errors
///
/// Returns [`TenantManagementError`] when the usage cannot be read or names no database.
pub async fn database_of(client: &BrokerClient, tenant: &Tenant) -> Result<DatabaseName, TenantManagementError> {
    read_tenant(client, tenant, TenantView::Usage)
        .await?
        .database_name
        .ok_or_else(|| TenantManagementError::NoDatabase(tenant.clone())) // clone: the error must own the tenant it reports
}

/// The databases holding `tenants`, in order.
///
/// # Errors
///
/// Returns [`TenantManagementError`] when a usage cannot be read or names no database.
pub async fn databases_of(client: &BrokerClient, tenants: impl IntoIterator<Item = &Tenant>) -> Result<Vec<DatabaseName>, TenantManagementError> {
    let mut databases = Vec::new();
    for tenant in tenants {
        databases.push(database_of(client, tenant).await?);
    }
    Ok(databases)
}

/// Sets `tenant`'s `isActive` to `activity`, answering the broker's answer whatever its status.
///
/// # Errors
///
/// Returns [`TenantManagementError`] when the request cannot be written.
pub async fn set_activity(
    client: &BrokerClient,
    tenant: &Tenant,
    activity: Activity,
) -> Result<Result<Response<Bytes>, ClientError<ReqwestError>>, TenantManagementError> {
    let body = serde_json::to_vec(&ActivityPatch {
        is_active: activity == Activity::Active,
    })
    .map_err(TenantManagementError::Patch)?;
    let request = request_to(client, Method::PATCH, &target(&format!("tenants/{tenant}"), &[]))?
        .append_header(CONTENT_TYPE, HeaderValue::from_static("application/json"))
        .body(Bytes::from(body));
    Ok(client.send_unchecked(request).await)
}

/// Waits until `tenant` reads `status`, polling its plain resource for at most `bound`.
///
/// # Errors
///
/// Returns [`TenantManagementError::Status`] with the last status read when the tenant does not reach `status`, and
/// the read's failure when a read fails.
pub async fn await_status(client: &BrokerClient, tenant: &Tenant, status: TenantStatus, bound: Duration) -> Result<(), TenantManagementError> {
    eventually(bound, || async {
        match read_status(client, tenant).await {
            Ok(read) if read == status => Poll::Ready(Ok(())),
            Ok(read) => Poll::Pending(read),
            Err(error) => Poll::Ready(Err(error)),
        }
    })
    .await
    .map_err(TenantManagementError::Status)?
}

/// Every status `tenant` reads over `window`, read every `interval`, in order: a test requiring a tenant to stay in
/// one status checks each.
///
/// # Errors
///
/// Returns the failure of the first read that fails.
pub async fn statuses_over(client: &BrokerClient, tenant: &Tenant, window: Duration, interval: Duration) -> Result<Vec<TenantStatus>, TenantManagementError> {
    let deadline = Instant::now() + window;
    let mut seen = Vec::new();
    while Instant::now() < deadline {
        seen.push(read_status(client, tenant).await?);
        sleep(interval).await;
    }
    Ok(seen)
}

/// The status a plain read of `tenant` reports.
async fn read_status(client: &BrokerClient, tenant: &Tenant) -> Result<TenantStatus, TenantManagementError> {
    read_tenant(client, tenant, TenantView::Plain)
        .await?
        .status
        .ok_or_else(|| TenantManagementError::NoStatus(tenant.clone())) // clone: the error must own the tenant it reports
}

#[cfg(test)]
mod tests {
    use crate::{
        broker::client_at,
        tenant_management::{Activity, TenantView, read_tenant, set_activity},
        tenant_resource::TenantStatus,
    };
    use googletest::prelude::*;
    use http::StatusCode;
    use ngsi_ld::model::Tenant;
    use url::Url;
    use wiremock::{
        Mock,
        MockServer,
        ResponseTemplate,
        matchers::{body_string, method, path, query_param},
    };

    #[gtest]
    #[tokio::test]
    async fn a_usage_read_asks_for_the_usage_and_reads_the_database() -> Result<()> {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/ngsi-ld/v1/tenants/acme"))
            .and(query_param("options", "usage"))
            .respond_with(ResponseTemplate::new(200).set_body_string(
                r#"{"id":"urn:ngsi-ld:Tenant:acme","type":"Tenant","tenantName":"acme","status":"active","databaseName":"andromeda_tenant_acme_1"}"#,
            ))
            .mount(&server)
            .await;
        let client = client_at(Url::parse(&format!("{}/ngsi-ld/v1/", server.uri()))?)?;
        let resource = read_tenant(&client, &"acme".parse::<Tenant>()?, TenantView::Usage).await?;
        expect_that!(resource.status, some(eq(TenantStatus::Active)));
        expect_that!(resource.database_name.map(|name| name.to_string()), some(eq("andromeda_tenant_acme_1")));
        Ok(())
    }

    #[gtest]
    #[tokio::test]
    async fn a_suspension_patches_is_active_false() -> Result<()> {
        let server = MockServer::start().await;
        Mock::given(method("PATCH"))
            .and(path("/ngsi-ld/v1/tenants/acme"))
            .and(body_string(r#"{"isActive":false}"#))
            .respond_with(ResponseTemplate::new(204))
            .mount(&server)
            .await;
        let client = client_at(Url::parse(&format!("{}/ngsi-ld/v1/", server.uri()))?)?;
        let answer = set_activity(&client, &"acme".parse::<Tenant>()?, Activity::Suspended).await?;
        verify_that!(answer.map(|response| response.status()).ok(), some(eq(StatusCode::NO_CONTENT)))
    }
}
