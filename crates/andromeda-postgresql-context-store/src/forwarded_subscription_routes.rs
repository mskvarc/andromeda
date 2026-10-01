//! The platform route from a minted subscription identifier to its tenant
//! (clause 5.8.1.4), kept in the main database beside the tenant registry.
//!
//! A route references the tenant by its `tenants(tenant_key)` surrogate, so removing a
//! tenant removes its routes; the name is resolved against the registry when a
//! route is written and read back through it when a route is followed.

use crate::store_fault::{StoreFault, persistence_failure, undecodable};
use andromeda_ngsi_ld_lexicon::{identifier::Identifier, tenant::TenantName};
use andromeda_postgresql_schema::{subscription_forwarding::ForwardedSubscriptionRoutes, tenancy::Tenants};
use andromeda_subscription_forwarding::{error::SubscriptionForwardingError, mapping::ForwardedSubscriptionRouter};
use andromeda_tenancy::tenant_ref::TenantRef;
use futures::future::BoxFuture;
use sea_query::{
    Expr,
    ExprTrait,
    InsertStatement,
    JoinType,
    OnConflict,
    PostgresQueryBuilder,
    Query,
    QueryStatementBuilder,
    SelectStatement,
    SimpleExpr,
    error::Error as SeaQueryError,
};
use sea_query_sqlx::SqlxBinder;
use sqlx::{PgPool, Row};

/// Erase an adapter failure into the forwarding policy's persistence category.
///
/// Classify an adapter failure at the port boundary: backpressure keeps the
/// port's retryable category and every fault becomes its persistence failure.
#[track_caller]
fn persistence_error(error: impl StoreFault) -> SubscriptionForwardingError {
    SubscriptionForwardingError::from(persistence_failure(&error))
}

/// `tenants` rows that are `tenant`: the named row, or the one unnamed default.
fn is_tenant(tenant: &TenantRef) -> SimpleExpr {
    match tenant {
        TenantRef::Named(name) => Expr::col((Tenants::Table, Tenants::Name))
            .eq(name.as_str())
            .and(Expr::col((Tenants::Table, Tenants::IsUnnamedDefault)).eq(false)),
        TenantRef::UnnamedDefault => Expr::col((Tenants::Table, Tenants::IsUnnamedDefault)).eq(true),
    }
}

/// Route `minted` to `tenant`, resolving the tenant's surrogate by subquery; a
/// tenant the registry does not hold resolves to `NULL`, which the column
/// refuses.
fn register_statement(minted: &Identifier, tenant: &TenantRef) -> Result<InsertStatement, SeaQueryError> {
    let tenant_key = Query::select()
        .column((Tenants::Table, Tenants::TenantKey))
        .from(Tenants::Table)
        .and_where(is_tenant(tenant))
        .to_owned();
    let mut insert = Query::insert();
    insert
        .into_table(ForwardedSubscriptionRoutes::Table)
        .columns([ForwardedSubscriptionRoutes::MintedSubscriptionId, ForwardedSubscriptionRoutes::TenantKey])
        .values([
            Expr::val(minted.as_str()),
            SimpleExpr::SubQuery(None, Box::new(tenant_key.into_sub_query_statement())),
        ])?
        .on_conflict(
            OnConflict::column(ForwardedSubscriptionRoutes::MintedSubscriptionId)
                .update_column(ForwardedSubscriptionRoutes::TenantKey)
                .to_owned(),
        );
    Ok(insert)
}

/// The tenant `minted` is routed to, read back through the registry.
fn tenant_of_statement(minted: &Identifier) -> SelectStatement {
    Query::select()
        .column((Tenants::Table, Tenants::Name))
        .from(ForwardedSubscriptionRoutes::Table)
        .join(
            JoinType::Join,
            Tenants::Table,
            Expr::col((Tenants::Table, Tenants::TenantKey)).equals((ForwardedSubscriptionRoutes::Table, ForwardedSubscriptionRoutes::TenantKey)),
        )
        .and_where(Expr::col((ForwardedSubscriptionRoutes::Table, ForwardedSubscriptionRoutes::MintedSubscriptionId)).eq(minted.as_str()))
        .to_owned()
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
        minted: &'life1 Identifier,
        tenant: &'life2 TenantRef,
    ) -> BoxFuture<'async_trait, Result<(), SubscriptionForwardingError>>
    where
        'life0: 'async_trait,
        'life1: 'async_trait,
        'life2: 'async_trait,
        Self: 'async_trait,
    {
        Box::pin(async move {
            let (sql, values) = register_statement(minted, tenant)
                .map_err(|error| persistence_error(error))?
                .build_sqlx(PostgresQueryBuilder);
            sqlx::query_with(sqlx::AssertSqlSafe(sql.as_str()), values)
                .execute(&self.pool)
                .await
                .map_err(|error| persistence_error(error))?;
            Ok(())
        })
    }

    fn tenant_of<'life0, 'life1, 'async_trait>(
        &'life0 self,
        minted: &'life1 Identifier,
    ) -> BoxFuture<'async_trait, Result<Option<TenantRef>, SubscriptionForwardingError>>
    where
        'life0: 'async_trait,
        'life1: 'async_trait,
        Self: 'async_trait,
    {
        Box::pin(async move {
            let (sql, values) = tenant_of_statement(minted).build_sqlx(PostgresQueryBuilder);
            let row = sqlx::query_with(sqlx::AssertSqlSafe(sql.as_str()), values)
                .fetch_optional(&self.pool)
                .await
                .map_err(|error| persistence_error(error))?;
            let Some(row) = row else {
                return Ok(None);
            };
            // The unnamed default tenant's row carries no name.
            let tenant: Option<String> = row.try_get("name").map_err(|error| persistence_error(error))?;
            let tenant = tenant
                .map(|name| name.parse::<TenantName>())
                .transpose()
                .map_err(|error| SubscriptionForwardingError::from(undecodable(&error)))?;
            Ok(Some(TenantRef::from_header(tenant)))
        })
    }

    fn unregister<'life0, 'life1, 'async_trait>(&'life0 self, minted: &'life1 Identifier) -> BoxFuture<'async_trait, Result<(), SubscriptionForwardingError>>
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
                .map_err(|error| persistence_error(error))?;
            Ok(())
        })
    }
}

#[cfg(test)]
mod tests {
    use crate::forwarded_subscription_routes::{register_statement, tenant_of_statement};
    use andromeda_ngsi_ld_lexicon::{identifier::Identifier, tenant::TenantName};
    use andromeda_tenancy::tenant_ref::TenantRef;
    use pretty_assertions::assert_eq;
    use sea_query::PostgresQueryBuilder;

    fn minted() -> Identifier {
        "urn:ngsi-ld:Subscription:forwarded-1".parse().expect("a URI")
    }

    #[test]
    fn a_route_resolves_a_named_tenant_to_its_surrogate_by_subquery() {
        let tenant = TenantRef::Named(TenantName::try_from("acme".to_owned()).expect("a tenant name"));
        let sql = register_statement(&minted(), &tenant)
            .expect("the fixed insert builds")
            .to_string(PostgresQueryBuilder);
        assert_eq!(
            sql,
            r#"INSERT INTO "forwarded_subscription_routes" ("minted_subscription_id", "tenant_key") VALUES ('urn:ngsi-ld:Subscription:forwarded-1', (SELECT "tenants"."tenant_key" FROM "tenants" WHERE "tenants"."name" = 'acme' AND "tenants"."is_unnamed_default" = FALSE)) ON CONFLICT ("minted_subscription_id") DO UPDATE SET "tenant_key" = "excluded"."tenant_key""#
        );
    }

    #[test]
    fn a_route_to_the_unnamed_default_resolves_its_marked_row() {
        let sql = register_statement(&minted(), &TenantRef::UnnamedDefault)
            .expect("the fixed insert builds")
            .to_string(PostgresQueryBuilder);
        assert!(
            sql.contains(r#"(SELECT "tenants"."tenant_key" FROM "tenants" WHERE "tenants"."is_unnamed_default" = TRUE)"#),
            "{sql}"
        );
    }

    #[test]
    fn a_followed_route_reads_the_tenant_back_through_the_registry() {
        let sql = tenant_of_statement(&minted()).to_string(PostgresQueryBuilder);
        assert_eq!(
            sql,
            r#"SELECT "tenants"."name" FROM "forwarded_subscription_routes" JOIN "tenants" ON "tenants"."tenant_key" = "forwarded_subscription_routes"."tenant_key" WHERE "forwarded_subscription_routes"."minted_subscription_id" = 'urn:ngsi-ld:Subscription:forwarded-1'"#
        );
    }
}
