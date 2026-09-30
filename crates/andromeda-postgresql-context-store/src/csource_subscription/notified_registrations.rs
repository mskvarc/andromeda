//! The registrations each context source registration subscription was
//! notified of, kept in `subscription_notified_registrations`.
//!
//! Every decision is the statement itself: a claim is an insert that skips the
//! pairs already present and returns the ones it wrote, and a release is a
//! delete that returns the pair it removed. Brokers sharing the tenant database
//! therefore agree on which of them sends a notification without coordinating.

use crate::{
    error::ContextStoreError,
    store_fault::{persistence_failure, undecodable},
    subscription_store::stored_subscription_error::StoredSubscriptionError,
};
use andromeda_ngsi_ld_lexicon::identifier::Identifier;
use andromeda_notification::{
    error::NotificationError,
    notified_registration_store::{NotifiedRegistrationStore, Release},
};
use andromeda_postgresql_schema::subscriptions::{SubscriptionNotifiedRegistrations, Subscriptions};
use andromeda_postgresql_seaquery::{
    pg_type::PostgresType,
    unnest::{UnnestColumn, apply_column_alias, unnest_call},
};
use futures::future::BoxFuture;
use sea_query::{Alias, DeleteStatement, Expr, ExprTrait, InsertStatement, JoinType, LockType, OnConflict, PostgresQueryBuilder, Query, SelectStatement};
use sea_query_sqlx::SqlxBinder;
use sqlx::{PgExecutor, PgPool};

/// The PostgreSQL store of notified registrations.
pub struct NotifiedRegistrationLedger {
    /// The tenant's serving pool.
    pool: PgPool,
}

impl NotifiedRegistrationLedger {
    /// A ledger over the tenant database `pool` serves.
    #[must_use]
    pub fn new(pool: &PgPool) -> NotifiedRegistrationLedger {
        NotifiedRegistrationLedger {
            pool: pool.clone(), // clone: SQLx pools are shared reference-counted handles.
        }
    }
}

impl NotifiedRegistrationStore for NotifiedRegistrationLedger {
    fn claim<'a>(&'a self, subscription: &'a Identifier, registrations: &'a [Identifier]) -> BoxFuture<'a, Result<Vec<Identifier>, NotificationError>> {
        Box::pin(async move { claim(&self.pool, subscription, registrations).await.map_err(|error| persistence(&error)) })
    }

    fn release<'a>(&'a self, subscription: &'a Identifier, registration: &'a Identifier) -> BoxFuture<'a, Result<Release, NotificationError>> {
        Box::pin(async move {
            let (sql, values) = release_statement(subscription, registration).build_sqlx(PostgresQueryBuilder);
            let removed: Option<(String,)> = sqlx::query_as_with(sqlx::AssertSqlSafe(sql.as_str()), values)
                .fetch_optional(&self.pool)
                .await
                .map_err(|error| persistence(&ContextStoreError::from(error)))?;
            Ok(removed.map_or(Release::Absent, |_| Release::Released))
        })
    }

    fn notified_subscriptions<'a>(&'a self, registration: &'a Identifier) -> BoxFuture<'a, Result<Vec<Identifier>, NotificationError>> {
        Box::pin(async move {
            let (sql, values) = notified_subscriptions_statement(registration).build_sqlx(PostgresQueryBuilder);
            let subscriptions: Vec<(String,)> = sqlx::query_as_with(sqlx::AssertSqlSafe(sql.as_str()), values)
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

    fn registrations_notified_to<'a>(&'a self, subscription: &'a Identifier) -> BoxFuture<'a, Result<Vec<Identifier>, NotificationError>> {
        Box::pin(async move {
            let (sql, values) = notified_registrations_statement(subscription).build_sqlx(PostgresQueryBuilder);
            let registrations: Vec<(String,)> = sqlx::query_as_with(sqlx::AssertSqlSafe(sql.as_str()), values)
                .fetch_all(&self.pool)
                .await
                .map_err(|error| persistence(&ContextStoreError::from(error)))?;
            registrations
                .into_iter()
                .map(|(registration,)| {
                    registration
                        .parse::<Identifier>()
                        .map_err(|error| NotificationError::Persistence(undecodable(&error)))
                })
                .collect()
        })
    }
}

/// A store failure as the notification policy reports it.
#[track_caller]
fn persistence(error: &ContextStoreError) -> NotificationError {
    NotificationError::Persistence(persistence_failure(error))
}

/// Record `registrations` as notified to `subscription`, returning those
/// recorded by this statement.
async fn claim<'e>(executor: impl PgExecutor<'e>, subscription: &Identifier, registrations: &[Identifier]) -> Result<Vec<Identifier>, ContextStoreError> {
    if registrations.is_empty() {
        return Ok(Vec::new());
    }
    let (sql, values) = claim_statement(subscription, registrations)?.build_sqlx(PostgresQueryBuilder);
    let (alias, columns) = registration_source();
    let sql = apply_column_alias(sql, &alias, &columns);
    let recorded: Vec<(String,)> = sqlx::query_as_with(sqlx::AssertSqlSafe(sql.as_str()), values).fetch_all(executor).await?;
    recorded
        .into_iter()
        .map(|(registration,)| {
            registration
                .parse::<Identifier>()
                .map_err(|error| StoredSubscriptionError::NotifiedRegistration(error).into())
        })
        .collect()
}

/// The `UNNEST(…) AS r(registration_id)` source a claim reads its
/// registrations from.
fn registration_source() -> (Alias, [UnnestColumn; 1]) {
    (
        Alias::new("r"),
        [UnnestColumn::new(SubscriptionNotifiedRegistrations::RegistrationId, PostgresType::TextArray)],
    )
}

/// Insert every `(subscription, registration)` pair not yet present, returning
/// the registration of each pair inserted.
///
/// The join against `subscriptions` resolves the surrogate key and drops every
/// pair of a subscription that no longer exists. `FOR KEY SHARE OF s` holds off
/// a deletion in flight until this insert commits, so the foreign key never
/// refuses the insert; a deletion that commits first leaves nothing to join.
fn claim_statement(subscription: &Identifier, registrations: &[Identifier]) -> Result<InsertStatement, ContextStoreError> {
    let (source_alias, columns) = registration_source();
    let subscription_alias = Alias::new("s");
    // Sorted, so two claims of overlapping registrations for one subscription
    // take their unique-index row locks in the same order and never deadlock.
    let mut identifiers: Vec<String> = registrations.iter().map(|registration| registration.as_str().to_owned()).collect();
    identifiers.sort_unstable();
    let identifiers = identifiers;
    let source = Query::select()
        .column((subscription_alias.clone(), Subscriptions::SubscriptionKey))
        .column((source_alias.clone(), SubscriptionNotifiedRegistrations::RegistrationId))
        .from_function(unnest_call([(&columns[0], Expr::val(identifiers))]), source_alias)
        .join_as(
            JoinType::InnerJoin,
            Subscriptions::Table,
            // clone: sea-query takes each alias occurrence by value, and the
            // locking clause names the same alias again
            subscription_alias.clone(),
            Expr::col((subscription_alias.clone(), Subscriptions::Id)).eq(subscription.as_str()),
        )
        .lock_with_tables(LockType::KeyShare, [subscription_alias])
        .to_owned();
    let mut statement = Query::insert();
    statement
        .into_table(SubscriptionNotifiedRegistrations::Table)
        .columns([
            SubscriptionNotifiedRegistrations::SubscriptionKey,
            SubscriptionNotifiedRegistrations::RegistrationId,
        ])
        .select_from(source)
        .map_err(ContextStoreError::StatementBuild)?
        .on_conflict(
            OnConflict::columns([
                SubscriptionNotifiedRegistrations::SubscriptionKey,
                SubscriptionNotifiedRegistrations::RegistrationId,
            ])
            .do_nothing()
            .to_owned(),
        )
        .returning_col(SubscriptionNotifiedRegistrations::RegistrationId);
    Ok(statement.clone())
}

/// The surrogate key of `subscription`, as a subquery.
fn subscription_key(subscription: &Identifier) -> SelectStatement {
    Query::select()
        .column(Subscriptions::SubscriptionKey)
        .from(Subscriptions::Table)
        .and_where(Expr::col(Subscriptions::Id).eq(subscription.as_str()))
        .to_owned()
}

/// Delete the `(subscription, registration)` pair, returning it when it was
/// present.
fn release_statement(subscription: &Identifier, registration: &Identifier) -> DeleteStatement {
    Query::delete()
        .from_table(SubscriptionNotifiedRegistrations::Table)
        .and_where(Expr::col(SubscriptionNotifiedRegistrations::SubscriptionKey).in_subquery(subscription_key(subscription)))
        .and_where(Expr::col(SubscriptionNotifiedRegistrations::RegistrationId).eq(registration.as_str()))
        .returning_col(SubscriptionNotifiedRegistrations::RegistrationId)
        .to_owned()
}

/// The identifiers of the subscriptions `registration` was notified to.
fn notified_subscriptions_statement(registration: &Identifier) -> SelectStatement {
    let subscription = Alias::new("s");
    let notified = Alias::new("n");
    Query::select()
        .column((subscription.clone(), Subscriptions::Id))
        .from_as(SubscriptionNotifiedRegistrations::Table, notified.clone())
        .join_as(
            JoinType::InnerJoin,
            Subscriptions::Table,
            subscription.clone(),
            Expr::col((subscription, Subscriptions::SubscriptionKey)).equals((notified.clone(), SubscriptionNotifiedRegistrations::SubscriptionKey)),
        )
        .and_where(Expr::col((notified, SubscriptionNotifiedRegistrations::RegistrationId)).eq(registration.as_str()))
        .to_owned()
}

/// The identifiers of the registrations notified to `subscription`.
fn notified_registrations_statement(subscription: &Identifier) -> SelectStatement {
    let subscription_alias = Alias::new("s");
    let notified = Alias::new("n");
    Query::select()
        .column((notified.clone(), SubscriptionNotifiedRegistrations::RegistrationId))
        .from_as(SubscriptionNotifiedRegistrations::Table, notified.clone())
        .join_as(
            JoinType::InnerJoin,
            Subscriptions::Table,
            subscription_alias.clone(),
            Expr::col((subscription_alias.clone(), Subscriptions::SubscriptionKey)).equals((notified, SubscriptionNotifiedRegistrations::SubscriptionKey)),
        )
        .and_where(Expr::col((subscription_alias, Subscriptions::Id)).eq(subscription.as_str()))
        .to_owned()
}

#[cfg(test)]
mod tests {
    use crate::csource_subscription::notified_registrations::{
        claim_statement,
        notified_registrations_statement,
        notified_subscriptions_statement,
        registration_source,
        release_statement,
    };
    use andromeda_ngsi_ld_lexicon::identifier::Identifier;
    use andromeda_postgresql_seaquery::unnest::apply_column_alias;
    use pretty_assertions::assert_eq;
    use sea_query::PostgresQueryBuilder;

    fn uri(raw: &str) -> Identifier {
        raw.parse().expect("valid URI")
    }

    #[test]
    fn a_claim_inserts_the_unrecorded_pairs_of_a_live_subscription_and_returns_them() {
        let statement = claim_statement(
            &uri("urn:ngsi-ld:Subscription:s"),
            &[uri("urn:ngsi-ld:ContextSourceRegistration:1"), uri("urn:ngsi-ld:ContextSourceRegistration:2")],
        )
        .expect("the claim builds");
        let (sql, _) = statement.build(PostgresQueryBuilder);
        let (alias, columns) = registration_source();
        let sql = apply_column_alias(sql, &alias, &columns);
        assert_eq!(
            sql,
            r#"INSERT INTO "subscription_notified_registrations" ("subscription_key", "registration_id") SELECT "s"."subscription_key", "r"."registration_id" FROM UNNEST($1::text[]) AS r(registration_id) INNER JOIN "subscriptions" AS "s" ON "s"."id" = $2 FOR KEY SHARE OF "s" ON CONFLICT ("subscription_key", "registration_id") DO NOTHING RETURNING "registration_id""#
        );
    }

    #[test]
    fn a_release_deletes_one_pair_and_returns_it() {
        let (sql, _) = release_statement(&uri("urn:ngsi-ld:Subscription:s"), &uri("urn:ngsi-ld:ContextSourceRegistration:1")).build(PostgresQueryBuilder);
        assert_eq!(
            sql,
            r#"DELETE FROM "subscription_notified_registrations" WHERE "subscription_key" IN (SELECT "subscription_key" FROM "subscriptions" WHERE "id" = $1) AND "registration_id" = $2 RETURNING "registration_id""#
        );
    }

    #[test]
    fn the_subscriptions_notified_of_a_registration_are_read_by_identifier() {
        let (sql, _) = notified_subscriptions_statement(&uri("urn:ngsi-ld:ContextSourceRegistration:1")).build(PostgresQueryBuilder);
        assert_eq!(
            sql,
            r#"SELECT "s"."id" FROM "subscription_notified_registrations" AS "n" INNER JOIN "subscriptions" AS "s" ON "s"."subscription_key" = "n"."subscription_key" WHERE "n"."registration_id" = $1"#
        );
    }

    #[test]
    fn the_registrations_notified_to_a_subscription_are_read_by_its_identifier() {
        let (sql, _) = notified_registrations_statement(&uri("urn:ngsi-ld:Subscription:s")).build(PostgresQueryBuilder);
        assert_eq!(
            sql,
            r#"SELECT "n"."registration_id" FROM "subscription_notified_registrations" AS "n" INNER JOIN "subscriptions" AS "s" ON "s"."subscription_key" = "n"."subscription_key" WHERE "s"."id" = $1"#
        );
    }
}
