//! Settling a provisional tenant: establishing it once its database holds
//! information, retiring it once it has stayed empty past its time to live.
//!
//! Both run under the tenant's advisory lock, the one provisioning takes while
//! it registers the tenant, so the row a settlement reads stays as read until
//! it commits. Provisioning creates and migrates the database after releasing
//! that lock. A retirement still never drops a database a provisioning is
//! preparing: provisioning restamps the row, which keeps the tenant inside its
//! time to live, and the drop is refused while the migration is connected.

use crate::{
    error::ContextStoreError,
    tenant_database::{DatabaseDrop, drop_database_unless_in_use},
    tenant_registry::{
        PostgresTenantRegistry,
        occupancy::{Occupancy, occupancy},
        statements::{OBSERVED_AT, advisory_lock_query, establish_update, provisional_delete, settlement_query},
        status_code::{StoredStanding, standing_of},
    },
};
use andromeda_ngsi_ld_lexicon::tenant::TenantName;
use andromeda_postgresql_codes::tenant_status::TenantStatusCode;
use andromeda_postgresql_seaquery::statement::Statement;
use andromeda_tenancy::{
    database_name::TenantDatabaseName,
    provisional::{Establishment, Retirement},
};
use chrono::{DateTime, Utc};
use sqlx::{Postgres, Row, Transaction};
use std::time::Duration;

/// A named tenant's row as a settlement reads it.
struct SettlementRow {
    /// Whether the tenant exists yet.
    standing: StoredStanding,
    /// Where the tenant's data lives.
    database: TenantDatabaseName,
    /// How long ago the tenant was last provisioned, or established or
    /// administered, on the database's clock; zero for a stamp ahead of it.
    untouched_for: Duration,
}

impl PostgresTenantRegistry {
    /// Establish a provisional tenant whose database holds information.
    pub(super) async fn establish_inner(&self, name: &TenantName) -> Result<Establishment, ContextStoreError> {
        let mut tx = self.locked(name).await?;
        let establishment = match settlement_row(&mut tx, name).await? {
            None => Establishment::Empty,
            Some(SettlementRow {
                standing: StoredStanding::Established(_),
                ..
            }) => Establishment::Established,
            Some(SettlementRow {
                standing: StoredStanding::Provisional,
                database,
                ..
            }) => match occupancy(&self.direct_url, &database).await? {
                Occupancy::HoldsInformation => {
                    establish_update(name.as_str()).to_query().execute(&mut *tx).await?;
                    Establishment::Established
                }
                Occupancy::Empty => Establishment::Empty,
            },
        };
        tx.commit().await?;
        Ok(establishment)
    }

    /// Retire a provisional tenant untouched for `idle_for` whose database
    /// holds no information: drop the database, then delete the row. A
    /// provisional tenant whose database holds information is established
    /// instead, however recently it was touched.
    ///
    /// The row goes only once the database is gone, so an interrupted
    /// retirement leaves a provisional row whose database is missing, which
    /// reads as empty and is retired again.
    pub(super) async fn retire_inner(&self, name: &TenantName, idle_for: Duration) -> Result<Retirement, ContextStoreError> {
        let mut tx = self.locked(name).await?;
        let retirement = match settlement_row(&mut tx, name).await? {
            None
            | Some(SettlementRow {
                standing: StoredStanding::Established(_),
                ..
            }) => Retirement::Kept,
            Some(SettlementRow {
                standing: StoredStanding::Provisional,
                database,
                untouched_for,
            }) => match occupancy(&self.direct_url, &database).await? {
                Occupancy::HoldsInformation => {
                    establish_update(name.as_str()).to_query().execute(&mut *tx).await?;
                    Retirement::Established
                }
                Occupancy::Empty if untouched_for < idle_for => Retirement::Kept,
                Occupancy::Empty => match drop_database_unless_in_use(&self.direct_url, &database).await? {
                    DatabaseDrop::InUse => Retirement::Kept,
                    DatabaseDrop::Dropped => {
                        provisional_delete(name.as_str()).to_query().execute(&mut *tx).await?;
                        Retirement::Retired
                    }
                },
            },
        };
        tx.commit().await?;
        Ok(retirement)
    }

    /// A platform transaction holding the tenant's advisory lock.
    async fn locked(&self, name: &TenantName) -> Result<Transaction<'static, Postgres>, ContextStoreError> {
        let mut tx = self.main_pool.begin().await?;
        advisory_lock_query(name.as_str()).to_query().execute(&mut *tx).await?;
        Ok(tx)
    }
}

/// Read the named tenant's row for a settlement, if it is registered.
async fn settlement_row(tx: &mut Transaction<'static, Postgres>, name: &TenantName) -> Result<Option<SettlementRow>, ContextStoreError> {
    let Some(row) = settlement_query(name.as_str()).to_query().fetch_optional(&mut **tx).await? else {
        return Ok(None);
    };
    let status: i16 = row.try_get("status")?;
    let database_name: String = row.try_get("database_name")?;
    let modified_at: DateTime<Utc> = row.try_get("modified_at")?;
    let observed_at: DateTime<Utc> = row.try_get(OBSERVED_AT)?;
    Ok(Some(SettlementRow {
        standing: standing_of(TenantStatusCode::try_from(status).map_err(ContextStoreError::StoredTenantStatus)?),
        database: TenantDatabaseName::from(database_name),
        untouched_for: (observed_at - modified_at).to_std().unwrap_or(Duration::ZERO),
    }))
}
