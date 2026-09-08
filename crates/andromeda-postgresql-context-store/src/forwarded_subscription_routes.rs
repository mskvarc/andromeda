//! The platform route from a minted subscription identifier to its tenant
//! (clause 5.8.1.4), kept in the main database beside the tenant registry.

use andromeda_ngsi_ld_lexicon::{tenant::TenantName, uri::Uri};
use andromeda_postgresql_schema::subscription_forwarding::ForwardedSubscriptionRoutes;
use andromeda_subscription_forwarding::{error::SubscriptionForwardingError, mapping::ForwardedSubscriptionRouter};
use andromeda_tenancy::tenant_ref::TenantRef;
use futures::future::BoxFuture;
use sea_query::{Expr, ExprTrait, OnConflict, PostgresQueryBuilder, Query};
use sea_query_sqlx::SqlxBinder;
use sqlx::{PgPool, Row};
use std::fmt::Display;

/// Erase an adapter failure into the forwarding policy's persistence category.
fn persistence_error(error: impl Display) -> SubscriptionForwardingError {
    SubscriptionForwardingError::persistence(error.to_string())
}

/// Repository over the platform `forwarded_subscription_routes` table.
#[derive(Debug)]
pub struct PostgresForwardedSubscriptionRouter {
    /// The platform (main database) pool.
    pool: PgPool,
}

impl PostgresForwardedSubscriptionRouter {
    /// Create a router over the platform pool.
    #[must_use]
    pub fn new(pool: &PgPool) -> PostgresForwardedSubscriptionRouter {
        PostgresForwardedSubscriptionRouter { pool: pool.clone() } // clone: `PgPool` is an atomically reference-counted pool handle.
    }
}

impl ForwardedSubscriptionRouter for PostgresForwardedSubscriptionRouter {
    fn register<'life0, 'life1, 'life2, 'async_trait>(
        &'life0 self,
        minted: &'life1 Uri,
        tenant: &'life2 TenantRef,
    ) -> BoxFuture<'async_trait, Result<(), SubscriptionForwardingError>>
    where
        'life0: 'async_trait,
        'life1: 'async_trait,
        'life2: 'async_trait,
        Self: 'async_trait,
    {
        Box::pin(async move {
            let mut insert = Query::insert();
            insert
                .into_table(ForwardedSubscriptionRoutes::Table)
                .columns([ForwardedSubscriptionRoutes::MintedSubscriptionId, ForwardedSubscriptionRoutes::Tenant])
                .values([Expr::val(minted.as_str()), Expr::val(tenant.tenant_name().map(TenantName::as_str))])
                .map_err(persistence_error)?
                .on_conflict(
                    OnConflict::column(ForwardedSubscriptionRoutes::MintedSubscriptionId)
                        .update_column(ForwardedSubscriptionRoutes::Tenant)
                        .to_owned(),
                );
            let (sql, values) = insert.build_sqlx(PostgresQueryBuilder);
            sqlx::query_with(sqlx::AssertSqlSafe(sql.as_str()), values)
                .execute(&self.pool)
                .await
                .map_err(persistence_error)?;
            Ok(())
        })
    }

    fn tenant_of<'life0, 'life1, 'async_trait>(
        &'life0 self,
        minted: &'life1 Uri,
    ) -> BoxFuture<'async_trait, Result<Option<TenantRef>, SubscriptionForwardingError>>
    where
        'life0: 'async_trait,
        'life1: 'async_trait,
        Self: 'async_trait,
    {
        Box::pin(async move {
            let (sql, values) = Query::select()
                .column(ForwardedSubscriptionRoutes::Tenant)
                .from(ForwardedSubscriptionRoutes::Table)
                .and_where(Expr::col(ForwardedSubscriptionRoutes::MintedSubscriptionId).eq(minted.as_str()))
                .build_sqlx(PostgresQueryBuilder);
            let row = sqlx::query_with(sqlx::AssertSqlSafe(sql.as_str()), values)
                .fetch_optional(&self.pool)
                .await
                .map_err(persistence_error)?;
            let Some(row) = row else {
                return Ok(None);
            };
            let tenant: Option<String> = row.try_get("tenant").map_err(persistence_error)?;
            let tenant = tenant
                .map(|name| name.parse::<TenantName>())
                .transpose()
                .map_err(|error| SubscriptionForwardingError::persistence(format!("invalid routed tenant: {error}")))?;
            Ok(Some(TenantRef::from_header(tenant)))
        })
    }

    fn unregister<'life0, 'life1, 'async_trait>(&'life0 self, minted: &'life1 Uri) -> BoxFuture<'async_trait, Result<(), SubscriptionForwardingError>>
    where
        'life0: 'async_trait,
        'life1: 'async_trait,
        Self: 'async_trait,
    {
        Box::pin(async move {
            let (sql, values) = Query::delete()
                .from_table(ForwardedSubscriptionRoutes::Table)
                .and_where(Expr::col(ForwardedSubscriptionRoutes::MintedSubscriptionId).eq(minted.as_str()))
                .build_sqlx(PostgresQueryBuilder);
            sqlx::query_with(sqlx::AssertSqlSafe(sql.as_str()), values)
                .execute(&self.pool)
                .await
                .map_err(persistence_error)?;
            Ok(())
        })
    }
}
