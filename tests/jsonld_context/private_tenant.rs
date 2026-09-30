//! A tenant of its own for every test.
//!
//! Stored `@context`s are per tenant (clause 4.14), and several cases assert
//! over a whole `/jsonldContexts` collection, so each test runs in a private
//! tenant whose collection starts empty: nothing another test writes is ever
//! visible to it, whatever runs in parallel.

use crate::{broker_endpoints::broker_url, unique_ids::unique_id};
use andromeda_test_harness::tenant_scoped_environment::TenantScopedEnvironment;
use reqwest::{
    Client,
    StatusCode,
    header::{HeaderMap, HeaderValue},
};
use serde_json::json;
use sqlx::PgPool;

/// A reqwest client that carries `NGSILD-Tenant` on every request.
pub fn tenant_client(tenant: &str) -> Client {
    let mut headers = HeaderMap::new();
    headers.insert("ngsild-tenant", HeaderValue::from_str(tenant).expect("tenant name is a valid header value"));
    Client::builder().default_headers(headers).build().expect("tenant client builds")
}

/// Bring the tenant `client` addresses into existence with a throwaway
/// registration (created, then deleted), so a read-first test finds an empty
/// but existing tenant rather than `NonexistentTenant` (clause 5.5.10). The
/// registration names no `@context`, so the tenant's `/jsonldContexts` stays
/// empty.
pub async fn provision_tenant(client: &Client) {
    let registration_id = unique_id("urn:ngsi-ld:ContextSourceRegistration:jsonld-provision");
    let body = json!({
        "id": registration_id,
        "type": "ContextSourceRegistration",
        "endpoint": "http://127.0.0.1:1",
        "information": [{ "entities": [{ "type": "ProvisioningSentinel" }] }]
    });
    let created = client
        .post(broker_url("csourceRegistrations"))
        .header("Content-Type", "application/json")
        .json(&body)
        .send()
        .await
        .expect("provisioning registration request failed");
    assert_eq!(
        created.status(),
        StatusCode::CREATED,
        "provisioning registration must be created to bring up the tenant"
    );
    let _ = client.delete(broker_url(&format!("csourceRegistrations/{registration_id}"))).send().await;
}

/// A provisioned private tenant and a client bound to it.
pub struct PrivateTenant {
    environment: TenantScopedEnvironment,
    client: Client,
}

impl PrivateTenant {
    /// Allocate a private tenant and bring it into existence.
    pub async fn provisioned() -> PrivateTenant {
        let environment = TenantScopedEnvironment::from_suite("jsonld_context").await.expect("private tenant environment");
        let client = tenant_client(environment.tenant());
        provision_tenant(&client).await;
        PrivateTenant { environment, client }
    }

    /// The client every request of the test goes through.
    pub const fn client(&self) -> &Client {
        &self.client
    }

    /// The tenant's name, as `NGSILD-Tenant` and `andromedaTenant` carry it.
    pub fn name(&self) -> &str {
        self.environment.tenant()
    }

    /// A pool on the tenant's own database, for the cases that make the
    /// broker's storage fail on purpose.
    pub async fn database(&self) -> PgPool {
        self.environment.tenant_database().await.expect("the tenant's database")
    }

    /// Drop the tenant.
    pub async fn finish(self) {
        self.environment.finish().await.expect("cleanup failed");
    }
}
