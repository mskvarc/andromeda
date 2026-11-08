//! The change revisions of context-source subscriptions, kept in
//! `subscriptions.context_source_revision` and
//! `subscriptions.evaluated_context_source_revision`.
//!
//! The claim is the statement itself: an update that moves the evaluated
//! revision up to the latest one only while it lags behind, returning the
//! revision it claimed. Brokers sharing the tenant database therefore agree on
//! which of them notifies a write without coordinating. Context-source
//! subscriptions live on the live branch only, so every statement addresses it.

use crate::{
    error::ContextStoreError,
    store_fault::{persistence_failure, undecodable},
    subscription_store::stored_kind::{StoredKind, of_kind},
};
use andromeda_ngsi_ld_lexicon::identifier::Identifier;
use andromeda_ngsi_ld_model::csource_change_revision::CsourceChangeRevision;
use andromeda_notification::{
    csource_change_order::{CsourceChangeOrder, LatestWriteClaim},
    error::NotificationError,
};
use andromeda_postgresql_schema::subscriptions::Subscriptions;
use andromeda_postgresql_seaquery::statement::Statement;
use futures::future::BoxFuture;
use sea_query::{Cond, Expr, ExprTrait, Query, SelectStatement, UpdateStatement};
use sqlx::PgPool;
use std::collections::HashSet;

/// The PostgreSQL store of context-source subscription revisions.
pub struct CsourceRevisionLedger {
    /// The tenant's serving pool.
    pool: PgPool,
}

impl CsourceRevisionLedger {
    /// A ledger over the tenant database `pool` serves.
    #[must_use]
    pub fn new(pool: &PgPool) -> CsourceRevisionLedger {
        CsourceRevisionLedger { pool: pool.clone() }
    }
}

impl CsourceChangeOrder for CsourceRevisionLedger {
    fn written_after(&self, revision: CsourceChangeRevision) -> BoxFuture<'_, Result<HashSet<Identifier>, NotificationError>> {
        Box::pin(async move {
            let subscriptions: Vec<(String,)> = written_after_statement(revision)
                .to_query_as()
                .fetch_all(&self.pool)
                .await
                .map_err(|error| persistence(&ContextStoreError::from(error)))?;
            subscriptions
                .into_iter()
                .map(|(subscription,)| {
                    subscription
                        .parse::<Identifier>()
                        .map_err(|error| NotificationError::Persistence(undecodable(&error)))
                })
                .collect()
        })
    }

    fn claim_latest_write<'a>(&'a self, subscription: &'a Identifier) -> BoxFuture<'a, Result<LatestWriteClaim, NotificationError>> {
        Box::pin(async move {
            let claimed: Option<(i64,)> = claim_statement(subscription)
                .to_query_as()
                .fetch_optional(&self.pool)
                .await
                .map_err(|error| persistence(&ContextStoreError::from(error)))?;
            Ok(claimed.map_or(LatestWriteClaim::Settled, |(revision,)| {
                LatestWriteClaim::Claimed(CsourceChangeRevision::from(revision))
            }))
        })
    }

    fn latest_write<'a>(&'a self, subscription: &'a Identifier) -> BoxFuture<'a, Result<Option<CsourceChangeRevision>, NotificationError>> {
        Box::pin(async move {
            let latest: Option<(i64,)> = latest_write_statement(subscription)
                .to_query_as()
                .fetch_optional(&self.pool)
                .await
                .map_err(|error| persistence(&ContextStoreError::from(error)))?;
            Ok(latest.map(|(revision,)| CsourceChangeRevision::from(revision)))
        })
    }
}

/// A store failure as the notification policy reports it.
#[track_caller]
fn persistence(error: &ContextStoreError) -> NotificationError {
    NotificationError::Persistence(persistence_failure(error))
}

/// The identifiers of the context-source subscriptions last written after
/// `revision`.
fn written_after_statement(revision: CsourceChangeRevision) -> SelectStatement {
    Query::select()
        .column(Subscriptions::Id)
        .from(Subscriptions::Table)
        .and_where(of_kind(StoredKind::Csource))
        .and_where(Expr::col(Subscriptions::ContextSourceRevision).gt(i64::from(revision)))
        .to_owned()
}

/// Mark `subscription`'s latest revision evaluated while it is not yet,
/// returning the revision claimed.
fn claim_statement(subscription: &Identifier) -> UpdateStatement {
    Query::update()
        .table(Subscriptions::Table)
        .value(Subscriptions::EvaluatedContextSourceRevision, Expr::col(Subscriptions::ContextSourceRevision))
        .and_where(Expr::col(Subscriptions::Id).eq(subscription.as_str()))
        .and_where(of_kind(StoredKind::Csource))
        .cond_where(
            Cond::any()
                .add(Expr::col(Subscriptions::EvaluatedContextSourceRevision).is_null())
                .add(Expr::col(Subscriptions::EvaluatedContextSourceRevision).lt(Expr::col(Subscriptions::ContextSourceRevision))),
        )
        .returning_col(Subscriptions::ContextSourceRevision)
        .to_owned()
}

/// The revision `subscription`'s latest write drew.
fn latest_write_statement(subscription: &Identifier) -> SelectStatement {
    Query::select()
        .column(Subscriptions::ContextSourceRevision)
        .from(Subscriptions::Table)
        .and_where(Expr::col(Subscriptions::Id).eq(subscription.as_str()))
        .and_where(of_kind(StoredKind::Csource))
        .to_owned()
}

#[cfg(test)]
mod tests {
    use crate::csource_subscription::revision_ledger::{claim_statement, latest_write_statement, written_after_statement};
    use andromeda_ngsi_ld_lexicon::identifier::Identifier;
    use andromeda_ngsi_ld_model::csource_change_revision::CsourceChangeRevision;
    use pretty_assertions::assert_eq;
    use sea_query::PostgresQueryBuilder;

    fn uri(raw: &str) -> Identifier {
        raw.parse().expect("valid URI")
    }

    #[test]
    fn the_subscriptions_written_after_a_revision_are_the_context_source_ones_with_a_later_revision() {
        let (sql, _) = written_after_statement(CsourceChangeRevision::from(7)).build(PostgresQueryBuilder);
        assert_eq!(
            sql,
            r#"SELECT "id" FROM "subscriptions" WHERE "subscriptions"."kind" = $1 AND "context_source_revision" > $2"#
        );
    }

    #[test]
    fn a_claim_moves_the_evaluated_revision_up_only_while_it_lags_and_returns_the_revision_claimed() {
        let (sql, _) = claim_statement(&uri("urn:ngsi-ld:Subscription:s")).build(PostgresQueryBuilder);
        assert_eq!(
            sql,
            r#"UPDATE "subscriptions" SET "evaluated_context_source_revision" = "context_source_revision" WHERE "id" = $1 AND "subscriptions"."kind" = $2 AND ("evaluated_context_source_revision" IS NULL OR "evaluated_context_source_revision" < "context_source_revision") RETURNING "context_source_revision""#
        );
    }

    #[test]
    fn the_latest_write_is_read_from_the_context_source_subscription_alone() {
        let (sql, _) = latest_write_statement(&uri("urn:ngsi-ld:Subscription:s")).build(PostgresQueryBuilder);
        assert_eq!(
            sql,
            r#"SELECT "context_source_revision" FROM "subscriptions" WHERE "id" = $1 AND "subscriptions"."kind" = $2"#
        );
    }
}
