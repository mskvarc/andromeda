//! One live broker of a [`MultiBrokerTopology`](crate::multi_broker::MultiBrokerTopology).

use andromeda_test_client::{broker_endpoint::BrokerEndpoint, tenant_name::TenantName, test_client::TestClient};
use http::HeaderValue;
use sqlx::PgPool;
use url::Url;

/// A live broker handle owned by a
/// [`MultiBrokerTopology`](crate::multi_broker::MultiBrokerTopology).
///
/// Every client it hands out by default runs under the tenant the topology
/// leased, and [`BrokerHandle::pool`] reaches that tenant's database; the
/// default tenant is reached only on request.
pub struct BrokerHandle<R> {
    pub(super) role: R,
    pub(super) identity: R,
    pub(super) client: TestClient,
    pub(super) default_client: TestClient,
    pub(super) endpoint: BrokerEndpoint,
    pub(super) peer: Url,
    pub(super) alias: String,
    pub(super) tenant_pool: PgPool,
    pub(super) platform_pool: PgPool,
    pub(super) context: Option<HeaderValue>,
}

impl<R> BrokerHandle<R>
where
    R: Copy,
{
    /// Borrow the broker's HTTP client, running under the topology's leased
    /// tenant.
    pub const fn client(&self) -> &TestClient {
        &self.client
    }

    /// Borrow a client for this broker running under the default tenant, for
    /// the cases whose point is the default tenant itself (clause 6.3.4).
    pub const fn default_tenant_client(&self) -> &TestClient {
        &self.default_client
    }

    /// Return this broker's role.
    pub const fn role(&self) -> R {
        self.role
    }

    /// Return the role whose durable store this broker shares.
    pub const fn identity(&self) -> R {
        self.identity
    }

    /// Borrow the endpoint to place in a Context Source Registration.
    pub fn peer_url(&self) -> &str {
        self.peer.as_str()
    }

    /// Borrow the broker's source alias.
    pub fn alias(&self) -> &str {
        &self.alias
    }

    /// Borrow the pool of the leased tenant's database on this broker, for
    /// isolation assertions over what the topology's clients wrote.
    pub const fn pool(&self) -> &PgPool {
        &self.tenant_pool
    }

    /// Build a client for this broker under another tenant.
    pub fn tenant_client(&self, tenant: TenantName) -> TestClient {
        let client = TestClient::new(self.endpoint.clone(), Some(tenant));
        match self.context.clone() {
            Some(link) => client.with_default_context(link),
            None => client,
        }
    }
}
