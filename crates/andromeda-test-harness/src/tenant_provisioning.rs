//! Bringing a leased tenant into existence on one broker and locating its store.
//!
//! A tenant exists on a broker only once an operation clause 5.5.10 lists as
//! implicitly creating it has run there, and each broker keeps the tenant in a
//! database of its own that its platform registry names. A topology reads its
//! peers under the leased tenant before it writes to all of them, so the
//! tenant is created on every broker up front, once per process.

use crate::configuration::DatabaseEndpoint;
use andromeda_ngsi_ld_lexicon::tenant::{InvalidTenantName, TenantName};
use andromeda_tenancy::database_name::TenantDatabaseName;
use andromeda_test_client::{entity::EntityRequestError, test_client::TestClient};
use andromeda_test_fixtures::context::CORE_CONTEXT;
use dashmap::DashMap;
use http::StatusCode;
use serde_json::json;
use sqlx::PgPool;
use std::sync::{Arc, LazyLock};
use thiserror::Error;
use tokio::sync::OnceCell;
use url::Url;

/// One leased tenant on one broker store, named by the store's platform
/// database.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
struct ProvisionedTenant {
    /// The platform database of the broker store.
    platform: Url,
    /// The tenant's name as the platform registry records it.
    tenant: String,
}

/// The tenant databases this process has located, each exactly once however
/// many topologies lease the tenant.
static PROVISIONED: LazyLock<DashMap<ProvisionedTenant, Arc<OnceCell<DatabaseEndpoint>>>> = LazyLock::new(DashMap::new);

/// The entity whose creation brings a leased tenant into existence; it is
/// deleted at once.
const PROBE_ENTITY_ID: &str = "urn:ngsi-ld:TenantLease:provisioning-probe";

/// Whether this process located a tenant database for the first time, in which
/// case it may still hold what an earlier run of the suite left in it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ProvisioningHistory {
    /// This process had not used the tenant database before.
    FirstUse,
    /// This process has used the tenant database before.
    Known,
}

/// The database holding `tenant` on the broker behind `client`, created the
/// first time this process names it.
///
/// `client` runs under the leased tenant; `platform` is the broker's platform
/// database, reached through `platform_endpoint`.
///
/// # Errors
///
/// Returns [`TenantProvisioningError`] when the tenant cannot be created on the
/// broker, or when the platform registry cannot be read or never names the
/// tenant's database.
pub async fn provisioned_tenant_database(
    client: &TestClient,
    platform: &PgPool,
    platform_endpoint: &DatabaseEndpoint,
    tenant: &str,
) -> Result<(DatabaseEndpoint, ProvisioningHistory), TenantProvisioningError> {
    let key = ProvisionedTenant {
        platform: platform_endpoint.url().clone(), // clone: the registry key owns the store it names
        tenant: tenant.to_owned(),
    };
    let cell = Arc::clone(PROVISIONED.entry(key).or_default().value());
    let mut history = ProvisioningHistory::Known;
    let endpoint = cell
        .get_or_try_init(|| async {
            history = ProvisioningHistory::FirstUse;
            let name: TenantName = tenant.parse().map_err(TenantProvisioningError::InvalidTenant)?;
            let database = if let Some(database) = registered_tenant_database(platform, &name).await.map_err(TenantProvisioningError::registry)? {
                database
            } else {
                create_tenant(client).await?;
                registered_tenant_database(platform, &name)
                    .await
                    .map_err(TenantProvisioningError::registry)?
                    .ok_or_else(|| TenantProvisioningError::NotRegistered { tenant: tenant.to_owned() })?
            };
            Ok::<DatabaseEndpoint, TenantProvisioningError>(platform_endpoint.with_database(database.as_str()))
        })
        .await?;
    Ok((endpoint.clone(), history)) // clone: every topology leasing the tenant owns its endpoint
}

/// The database the platform registry behind `platform` holds `tenant` in, or
/// `None` before a broker has created the tenant.
///
/// # Errors
///
/// Returns the [`sqlx::Error`] the registry read reported.
pub async fn registered_tenant_database(platform: &PgPool, tenant: &TenantName) -> Result<Option<TenantDatabaseName>, sqlx::Error> {
    let database: Option<String> = sqlx::query_scalar("SELECT database_name FROM tenants WHERE name = $1")
        .bind(tenant.as_str())
        .fetch_optional(platform)
        .await?;
    Ok(database.map(TenantDatabaseName::from))
}

/// Create the tenant on the broker through Create Entity, one of the operations
/// clause 5.5.10 lists as implicitly creating a tenant, and delete the entity
/// again.
async fn create_tenant(client: &TestClient) -> Result<(), TenantProvisioningError> {
    let probe = json!({
        "id": PROBE_ENTITY_ID,
        "type": "TenantLease",
        "@context": CORE_CONTEXT.as_str(),
    });
    let created = client.post_entity(&probe).with_ld_json().send().await?;
    if !matches!(created.status, StatusCode::CREATED | StatusCode::CONFLICT) {
        return Err(TenantProvisioningError::Rejected { status: created.status });
    }
    let deleted = client.delete_entity(PROBE_ENTITY_ID).await?;
    if deleted.status != StatusCode::NO_CONTENT {
        return Err(TenantProvisioningError::Rejected { status: deleted.status });
    }
    Ok(())
}

/// Failure while bringing a leased tenant into existence on one broker.
#[derive(Debug, Error)]
pub enum TenantProvisioningError {
    /// The request that creates the tenant could not be sent.
    #[error("the tenant-creating request failed: {0}")]
    Request(#[from] EntityRequestError),
    /// The broker refused the request that creates the tenant.
    #[error("the broker answered {status} while creating the leased tenant")]
    Rejected {
        /// The status the broker answered with.
        status: StatusCode,
    },
    /// The platform registry could not be read.
    ///
    /// Boxed for the same reason as
    /// [`crate::database_isolation::DatabaseIsolationError::Sqlx`].
    #[error("the platform tenant registry could not be read: {0}")]
    Registry(#[source] Box<sqlx::Error>),
    /// The leased tenant label is not a valid tenant name.
    #[error("the leased tenant label is not a valid tenant name: {0}")]
    InvalidTenant(#[source] InvalidTenantName),
    /// The broker created the tenant but its registry names no database for it.
    #[error("the platform registry names no database for tenant {tenant}")]
    NotRegistered {
        /// The tenant the registry was asked about.
        tenant: String,
    },
}

impl TenantProvisioningError {
    /// Record a registry read failure, moving it behind the variant's box.
    fn registry(error: sqlx::Error) -> TenantProvisioningError {
        TenantProvisioningError::Registry(Box::new(error))
    }
}
