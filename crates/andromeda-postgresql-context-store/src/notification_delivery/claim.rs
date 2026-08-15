//! Leasing due deliveries and reading the events they answer.

use crate::{
    error::ContextStoreError,
    notification_delivery::rows::{ClaimedDelivery, ClaimedDeliveryError, RawClaimedDeliveryRow},
};
use andromeda_notification::delivery_lease::LeaseTerm;
use andromeda_postgresql_schema::{
    attribute_dictionary::AttributeNames,
    notification::{NotificationDeliveries, NotificationEvents},
    subscriptions::Subscriptions,
};
use andromeda_postgresql_seaquery::{
    aggregate::array_agg_ordered,
    interval::make_interval_secs,
    pg_array::{any, text_array},
    pg_type::PostgresType,
    unnest::{UnnestColumn, apply_with_ordinality, unnest_call},
};
use sea_query::{Alias, Expr, Func, JoinType, LockBehavior, LockType, Order, PostgresQueryBuilder, Query, SelectStatement, SimpleExpr, UpdateStatement};
use sea_query_binder::SqlxBinder;
use sqlx::postgres::PgPool;

/// Claim up to `batch_size` due deliveries for `worker_id` under `lease`.
///
/// A row is claimable only while no live lease covers it, so the claim excludes
/// deliveries another worker still owns rather than handing out a second
/// delivery of the same notification the moment the first claim commits.
///
/// The original single data-modifying CTE (`WITH claimable AS (SELECT … FOR
/// UPDATE SKIP LOCKED), claimed AS (UPDATE … RETURNING) SELECT …`) is not a form
/// sea-query can build, so the claim runs as two statements in one transaction:
/// an `UPDATE` whose `WHERE id IN (SELECT … FOR UPDATE SKIP LOCKED)` both selects
/// and claims the batch atomically, then a projection that reads the claimed
/// rows back with their events. Both run inside the same transaction, so no
/// other worker observes a claimed row before its projection is read.
///
/// The scan and the update touch `notification_deliveries` alone. The events
/// are joined onto the claimed batch afterwards, so the payload rows are read
/// once per claimed delivery instead of being locked and rewritten with it.
///
/// # Errors
///
/// Returns [`ContextStoreError::Database`] when a statement fails. A single
/// unreadable row is reported inside the returned vector rather than as a
/// failure of the claim, because one row nobody can decode must not stall every
/// other pending notification.
pub async fn claim_due(
    pool: &PgPool,
    worker_id: &str,
    lease: LeaseTerm,
    batch_size: i32,
    max_attempts: i32,
) -> Result<Vec<Result<ClaimedDelivery, ClaimedDeliveryError>>, ContextStoreError> {
    let mut tx = pool.begin().await?;

    let (sql, values) = claim_statement(worker_id, batch_size, max_attempts, f64::from(lease.seconds())).build_sqlx(PostgresQueryBuilder);
    let claimed_ids: Vec<i64> = sqlx::query_scalar_with(&sql, values).fetch_all(&mut *tx).await?;
    if claimed_ids.is_empty() {
        tx.commit().await?;
        return Ok(Vec::new());
    }

    let (sql, values) = claimed_projection_statement(&claimed_ids).build_sqlx(PostgresQueryBuilder);
    let (unnest_alias, unnest_columns, ordinality) = ordinality_alias();
    let sql = apply_with_ordinality(sql, &unnest_alias, &unnest_columns, &ordinality);
    let rows: Vec<RawClaimedDeliveryRow> = sqlx::query_as_with(&sql, values).fetch_all(&mut *tx).await?;

    tx.commit().await?;
    Ok(rows.into_iter().map(ClaimedDelivery::try_from).collect())
}

/// The `UNNEST … WITH ORDINALITY AS k(name_id, position)` names that
/// [`apply_with_ordinality`] rewrites the projection's set-returning source to.
///
/// The one row's `name_id` is the dictionary key `UNNEST` yields and `position`
/// is the ordinality that preserves the order the event recorded its attributes
/// in; both must match the names the projection's aggregate reads.
fn ordinality_alias() -> (Alias, [UnnestColumn; 1], Alias) {
    (
        Alias::new("k"),
        [UnnestColumn::new(Alias::new("name_id"), PostgresType::IntegerArray)],
        Alias::new("position"),
    )
}

/// Claim a batch of due deliveries for `worker_id`, returning their identifiers.
///
/// The candidate predicate is exactly the one the queue's partial claim index
/// carries: undelivered, below the attempt ceiling, due, and either unleased or
/// held under a lease that has expired. A predicate that drifted from the index
/// would leave the index unusable rather than incorrect, which is the failure
/// mode that shows up only under load. `FOR UPDATE SKIP LOCKED` in the candidate
/// subquery steps over a row another worker holds instead of waiting on it.
fn claim_statement(worker_id: &str, batch_size: i32, max_attempts: i32, lease_seconds: f64) -> UpdateStatement {
    let candidates = Query::select()
        .column(NotificationDeliveries::Id)
        .from(NotificationDeliveries::Table)
        .and_where(Expr::col(NotificationDeliveries::DeliveredAt).is_null())
        .and_where(Expr::col(NotificationDeliveries::AttemptCount).lt(max_attempts))
        .and_where(Expr::col(NotificationDeliveries::NextAttemptAt).lte(Expr::current_timestamp()))
        .and_where(
            Expr::col(NotificationDeliveries::LeaseUntil)
                .is_null()
                .or(Expr::col(NotificationDeliveries::LeaseUntil).lte(Expr::current_timestamp())),
        )
        .order_by(NotificationDeliveries::NextAttemptAt, Order::Asc)
        .limit(batch_size.max(0) as u64)
        .lock_with_behavior(LockType::Update, LockBehavior::SkipLocked)
        .to_owned();

    Query::update()
        .table(NotificationDeliveries::Table)
        .value(NotificationDeliveries::ClaimedAt, Expr::current_timestamp())
        .value(NotificationDeliveries::ClaimedBy, worker_id)
        .value(
            NotificationDeliveries::LeaseUntil,
            Expr::current_timestamp().add(make_interval_secs(Expr::val(lease_seconds).into())),
        )
        .and_where(Expr::col(NotificationDeliveries::Id).in_subquery(candidates))
        .returning_col(NotificationDeliveries::Id)
        .to_owned()
}

/// Read the claimed deliveries back with the events they answer.
///
/// Only the rows this worker just claimed are named, by identifier: they carry
/// a live lease held by this transaction, so no other worker can have altered
/// them between the claim and this read. The changed attributes are resolved
/// back through the dictionary in event-recorded order by a lateral aggregate
/// over the event's `changed_attribute_name_ids`, defaulting to an empty array
/// when the event touched none.
fn claimed_projection_statement(claimed_ids: &[i64]) -> SelectStatement {
    let (delivery, event, subscription) = (Alias::new("c"), Alias::new("e"), Alias::new("s"));
    let (unnest_alias, unnest_columns, position) = ordinality_alias();
    let dictionary = Alias::new("n");

    let ordered_names = Query::select()
        .expr(array_agg_ordered(
            Expr::col((dictionary.clone(), AttributeNames::Name)).into(),
            Expr::col((unnest_alias.clone(), position)).into(),
        ))
        .from_function(
            unnest_call([(
                &unnest_columns[0],
                Expr::col((event.clone(), NotificationEvents::ChangedAttributeNameIds)).into(),
            )]),
            unnest_alias.clone(),
        )
        .join_as(
            JoinType::Join,
            AttributeNames::Table,
            dictionary.clone(),
            Expr::col((dictionary, AttributeNames::Id)).equals((unnest_alias, Alias::new("name_id"))),
        )
        .to_owned();

    let changed_attributes = Func::coalesce([SimpleExpr::SubQuery(None, Box::new(ordered_names.into_sub_query_statement())), text_array(&[])]);

    Query::select()
        .column((delivery.clone(), NotificationDeliveries::Id))
        .expr_as(Expr::col((subscription.clone(), Subscriptions::Id)), Alias::new("subscription_id"))
        .column((delivery.clone(), NotificationDeliveries::AttemptCount))
        .column((delivery.clone(), NotificationDeliveries::NextAttemptAt))
        .expr_as(Expr::col((event.clone(), NotificationEvents::Id)), Alias::new("event_id"))
        .column((event.clone(), NotificationEvents::WriteId))
        .column((event.clone(), NotificationEvents::EventOrdinal))
        .column((event.clone(), NotificationEvents::Trigger))
        .column((event.clone(), NotificationEvents::EntityId))
        .expr_as(changed_attributes, Alias::new("changed_attributes"))
        .column((event.clone(), NotificationEvents::EntitySnapshot))
        .column((event.clone(), NotificationEvents::PreviousSnapshot))
        .column((event.clone(), NotificationEvents::CreatedAt))
        .from_as(NotificationDeliveries::Table, delivery.clone())
        .join_as(
            JoinType::InnerJoin,
            NotificationEvents::Table,
            event.clone(),
            Expr::col((event, NotificationEvents::Id)).equals((delivery.clone(), NotificationDeliveries::EventId)),
        )
        .join_as(
            JoinType::InnerJoin,
            Subscriptions::Table,
            subscription.clone(),
            Expr::col((subscription, Subscriptions::SubscriptionKey)).equals((delivery.clone(), NotificationDeliveries::SubscriptionKey)),
        )
        .and_where(Expr::col((delivery, NotificationDeliveries::Id)).eq(any(bigint_array(claimed_ids))))
        .to_owned()
}

/// A bound `bigint[]` value from a batch of claimed identifiers.
fn bigint_array(ids: &[i64]) -> SimpleExpr {
    Expr::val(ids.to_vec()).into()
}

#[cfg(test)]
mod tests {
    use crate::notification_delivery::claim::{claim_statement, claimed_projection_statement, ordinality_alias};
    use andromeda_postgresql_seaquery::unnest::apply_with_ordinality;
    use sea_query::PostgresQueryBuilder;

    fn projection_sql() -> String {
        let sql = claimed_projection_statement(&[1, 2]).to_string(PostgresQueryBuilder);
        let (alias, columns, ordinality) = ordinality_alias();
        apply_with_ordinality(sql, &alias, &columns, &ordinality)
    }

    #[test]
    fn the_claim_scans_and_updates_deliveries_alone() {
        let sql = claim_statement("w", 10, 5, 30.0).to_string(PostgresQueryBuilder);
        assert!(sql.contains(r#"UPDATE "notification_deliveries""#), "{sql}");
        assert!(!sql.contains("notification_events"), "the claim must not touch the events table: {sql}");
    }

    #[test]
    fn the_candidate_predicate_matches_the_partial_claim_index() {
        let sql = claim_statement("w", 10, 5, 30.0).to_string(PostgresQueryBuilder);
        assert!(sql.contains(r#""delivered_at" IS NULL"#), "{sql}");
        assert!(sql.contains(r#""attempt_count" < 5"#), "{sql}");
        assert!(sql.contains(r#""next_attempt_at" <= CURRENT_TIMESTAMP"#), "{sql}");
        assert!(sql.contains(r#""lease_until" IS NULL OR "lease_until" <= CURRENT_TIMESTAMP"#), "{sql}");
    }

    #[test]
    fn a_row_another_worker_holds_is_skipped_rather_than_waited_on() {
        let sql = claim_statement("w", 10, 5, 30.0).to_string(PostgresQueryBuilder);
        assert!(sql.contains("FOR UPDATE SKIP LOCKED"), "{sql}");
    }

    #[test]
    fn the_claim_takes_a_lease_alongside_the_claim_triple() {
        let sql = claim_statement("w", 10, 5, 30.0).to_string(PostgresQueryBuilder);
        assert!(sql.contains(r#""claimed_at" = CURRENT_TIMESTAMP"#), "{sql}");
        assert!(sql.contains(r#""claimed_by" = 'w'"#), "{sql}");
        assert!(sql.contains("make_interval(secs =>"), "{sql}");
    }

    #[test]
    fn the_changed_attributes_keep_the_order_the_event_recorded_them_in() {
        let sql = projection_sql();
        assert!(sql.contains(r#"array_agg("n"."name" ORDER BY "k"."position")"#), "{sql}");
        assert!(sql.contains("WITH ORDINALITY AS k(name_id, position)"), "{sql}");
    }

    #[test]
    fn the_projection_reads_only_the_rows_this_worker_claimed() {
        let sql = projection_sql();
        assert!(sql.contains(r#""c"."id" = ANY("#), "{sql}");
        assert!(sql.contains("COALESCE"), "{sql}");
    }

    #[test]
    fn the_projection_names_each_column_the_row_mapper_reads() {
        let sql = projection_sql();
        assert!(sql.contains(r#"AS "subscription_id""#), "{sql}");
        assert!(sql.contains(r#"AS "event_id""#), "{sql}");
        assert!(sql.contains(r#"AS "changed_attributes""#), "{sql}");
    }
}
