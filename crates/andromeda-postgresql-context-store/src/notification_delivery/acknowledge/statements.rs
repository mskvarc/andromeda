//! The set-based statements recording one kind of settlement for every
//! delivery of a group on one branch.
//!
//! Each statement opens with the guarded update of the acknowledged rows, the
//! `acknowledged` common table expression, and its hold piece reads the rows
//! from it, so a row whose guard refused updates nothing and moves no hold.
//! The rows a failure or an exhaustion records each carry their own
//! diagnostic and due instant, bound as parallel arrays and unnested into a
//! source the update joins on the delivery key.
//!
//! The guard names the worker that holds the lease and requires that lease to
//! still be live, row by row. A worker whose lease expired while it was
//! delivering therefore updates nothing of that row instead of overwriting the
//! progress of the worker that reclaimed it, and the row is missing from what
//! the statement returns.

use crate::{
    error::ContextStoreError,
    notification_delivery::hold::{
        Acknowledged,
        Probed,
        Released,
        Taken,
        acknowledged_beside,
        cte,
        null,
        probed_on_exhaustion,
        released_on_delivery,
        taken_on_failure,
    },
};
use andromeda_context_commit::lease_holder::LeaseHolder;
use andromeda_context_space::context_target::ContextTarget;
use andromeda_postgresql_branch::table_of;
use andromeda_postgresql_schema::notification::{NotificationDeliveries, NotificationDeliveryHolds};
use andromeda_postgresql_seaquery::{
    pg_array::{any, text_array},
    pg_type::PostgresType,
    unnest::{UnnestAliased, UnnestColumn, unnest_call},
};
use chrono::{DateTime, Utc};
use sea_query::{Alias, CommonTableExpression, Expr, ExprTrait, Iden, JoinType, Query, SelectStatement, SimpleExpr, UpdateStatement, WithClause, WithQuery};

/// The failed attempts a group records, `failure`.
#[derive(Iden, Clone, Copy)]
#[iden = "failure"]
struct Failure;

/// The exhausted deliveries a group records, `exhaustion`.
#[derive(Iden, Clone, Copy)]
#[iden = "exhaustion"]
struct Exhaustion;

/// The failed attempts of one group on one branch, as parallel arrays.
#[derive(Debug, Default)]
pub(crate) struct FailedRows<'a> {
    /// The deliveries whose attempts failed.
    pub(crate) delivery_keys: Vec<i64>,
    /// Each one's diagnostic, already within the stored bound.
    pub(crate) failures: Vec<&'a str>,
    /// When each one is due again.
    pub(crate) next_attempts: Vec<DateTime<Utc>>,
}

/// The exhausted deliveries of one group on one branch, as parallel arrays.
#[derive(Debug, Default)]
pub(crate) struct ExhaustedRows<'a> {
    /// The deliveries exhausted.
    pub(crate) delivery_keys: Vec<i64>,
    /// Each one's diagnostic, already within the stored bound.
    pub(crate) failures: Vec<&'a str>,
}

/// Mark the deliveries sent without disturbing their claim pairs, and delete
/// the holds of their Subscriptions.
///
/// ```sql
/// WITH acknowledged AS (UPDATE notification_deliveries SET delivered_at = CURRENT_TIMESTAMP
///                       WHERE delivery_key = ANY($1) AND <lease guard> RETURNING delivery_key, subscription_key),
///      released AS (<released_on_delivery>)
/// SELECT acknowledged.delivery_key, released.subscription_key
/// FROM acknowledged LEFT JOIN released ON released.subscription_key = acknowledged.subscription_key
/// ```
///
/// The claim pair stays in place: `delivered_at` already makes a delivery
/// unclaimable, and the recorded owner is the only trace of which worker sent
/// it. Clearing the lease alone would leave `claimed_by` populated without it,
/// which the schema's claim coherence constraint rejects.
pub(crate) fn delivered_statement(target: &ContextTarget, worker_id: LeaseHolder, delivery_keys: Vec<i64>) -> WithQuery {
    let mut update = acknowledgement(target, worker_id);
    update
        .value(NotificationDeliveries::DeliveredAt, Expr::current_timestamp())
        .and_where(delivery_column(NotificationDeliveries::DeliveryKey).eq(any(Expr::val(delivery_keys))));
    with_acknowledged(
        None,
        update,
        released_on_delivery(target),
        acknowledged_beside(
            Released,
            &[NotificationDeliveryHolds::SubscriptionKey],
            JoinType::LeftJoin,
            (NotificationDeliveryHolds::SubscriptionKey, NotificationDeliveries::SubscriptionKey),
        ),
    )
}

/// Count each failed attempt, record its diagnostic, release its lease for a
/// retry at its own due instant, and take or find the hold of each
/// Subscription.
///
/// ```sql
/// WITH failure AS (SELECT f.delivery_key, f.last_error, f.next_attempt_at
///                  FROM UNNEST($1::bigint[], $2::text[], $3::timestamptz[]) AS f(delivery_key, last_error, next_attempt_at)),
///      acknowledged AS (UPDATE notification_deliveries SET attempt_count = attempt_count + 1,
///                         last_error = failure.last_error, next_attempt_at = failure.next_attempt_at,
///                         claimed_by = NULL, lease_until = NULL
///                       FROM failure WHERE notification_deliveries.delivery_key = failure.delivery_key AND <lease guard>
///                       RETURNING delivery_key, subscription_key),
///      taken AS (<taken_on_failure>)
/// SELECT acknowledged.delivery_key, taken.subscription_key, taken.delivery_key
/// FROM acknowledged INNER JOIN taken ON taken.subscription_key = acknowledged.subscription_key
/// ```
///
/// # Errors
///
/// Returns [`ContextStoreError::StatementBuild`] when the hold insert cannot
/// take its rows from the acknowledged deliveries.
pub(crate) fn failed_statement(target: &ContextTarget, worker_id: LeaseHolder, rows: FailedRows<'_>) -> Result<UnnestAliased<WithQuery>, ContextStoreError> {
    let (source, columns) = settled_rows([
        (NotificationDeliveries::DeliveryKey, PostgresType::BigintArray, Expr::val(rows.delivery_keys)),
        (NotificationDeliveries::LastError, PostgresType::TextArray, text_array(&rows.failures)),
        (
            NotificationDeliveries::NextAttemptAt,
            PostgresType::TimestamptzArray,
            Expr::val(rows.next_attempts),
        ),
    ]);
    let mut update = acknowledgement(target, worker_id);
    update
        .value(
            NotificationDeliveries::AttemptCount,
            delivery_column(NotificationDeliveries::AttemptCount).add(1),
        )
        .value(NotificationDeliveries::LastError, Expr::col((Failure, NotificationDeliveries::LastError)))
        .value(
            NotificationDeliveries::NextAttemptAt,
            Expr::col((Failure, NotificationDeliveries::NextAttemptAt)),
        )
        .value(NotificationDeliveries::ClaimedBy, null())
        .value(NotificationDeliveries::LeaseUntil, null())
        .from(Failure)
        .and_where(delivery_column(NotificationDeliveries::DeliveryKey).equals((Failure, NotificationDeliveries::DeliveryKey)));
    let statement = with_acknowledged(
        Some(cte(Failure, source)),
        update,
        taken_on_failure(target)?,
        acknowledged_beside(
            Taken,
            &[NotificationDeliveryHolds::SubscriptionKey, NotificationDeliveryHolds::DeliveryKey],
            JoinType::InnerJoin,
            (NotificationDeliveryHolds::SubscriptionKey, NotificationDeliveries::SubscriptionKey),
        ),
    );
    Ok(UnnestAliased::new(statement).with_columns(source_alias(), &columns))
}

/// Count each final attempt, stamp the exhaustion instant, record its
/// diagnostic, release its claim for good, and lock the holds the deliveries
/// were the probes of.
///
/// ```sql
/// WITH exhaustion AS (SELECT f.delivery_key, f.last_error FROM UNNEST($1::bigint[], $2::text[]) AS f(delivery_key, last_error)),
///      acknowledged AS (UPDATE notification_deliveries SET attempt_count = attempt_count + 1,
///                         exhausted_at = CURRENT_TIMESTAMP, last_error = exhaustion.last_error,
///                         claimed_by = NULL, lease_until = NULL
///                       FROM exhaustion WHERE notification_deliveries.delivery_key = exhaustion.delivery_key AND <lease guard>
///                       RETURNING delivery_key, subscription_key),
///      probed AS (<probed_on_exhaustion>)
/// SELECT acknowledged.delivery_key, probed.subscription_key
/// FROM acknowledged LEFT JOIN probed ON probed.delivery_key = acknowledged.delivery_key
/// ```
///
/// Exhaustion is a decision the worker takes, on a failure no retry can fix
/// or on the attempt that reached the configured ceiling, and it is recorded
/// as an instant of its own so the claim predicate and the dead-letter reap
/// need not know the ceiling. The claim pair is cleared in the same statement
/// because `chk_notification_deliveries_exhausted_state` forbids an exhausted
/// row that still holds a claim.
pub(crate) fn exhausted_statement(target: &ContextTarget, worker_id: LeaseHolder, rows: ExhaustedRows<'_>) -> UnnestAliased<WithQuery> {
    let (source, columns) = settled_rows([
        (NotificationDeliveries::DeliveryKey, PostgresType::BigintArray, Expr::val(rows.delivery_keys)),
        (NotificationDeliveries::LastError, PostgresType::TextArray, text_array(&rows.failures)),
    ]);
    let mut update = acknowledgement(target, worker_id);
    update
        .value(
            NotificationDeliveries::AttemptCount,
            delivery_column(NotificationDeliveries::AttemptCount).add(1),
        )
        .value(NotificationDeliveries::ExhaustedAt, Expr::current_timestamp())
        .value(NotificationDeliveries::LastError, Expr::col((Exhaustion, NotificationDeliveries::LastError)))
        .value(NotificationDeliveries::ClaimedBy, null())
        .value(NotificationDeliveries::LeaseUntil, null())
        .from(Exhaustion)
        .and_where(delivery_column(NotificationDeliveries::DeliveryKey).equals((Exhaustion, NotificationDeliveries::DeliveryKey)));
    let statement = with_acknowledged(
        Some(cte(Exhaustion, source)),
        update,
        probed_on_exhaustion(target),
        acknowledged_beside(
            Probed,
            &[NotificationDeliveryHolds::SubscriptionKey],
            JoinType::LeftJoin,
            (NotificationDeliveryHolds::DeliveryKey, NotificationDeliveries::DeliveryKey),
        ),
    );
    UnnestAliased::new(statement).with_columns(source_alias(), &columns)
}

/// The guarded update of the acknowledged rows, returning each one's key and
/// its Subscription's for the hold: only rows the worker still holds under a
/// lease that has not yet expired.
fn acknowledgement(target: &ContextTarget, worker_id: LeaseHolder) -> UpdateStatement {
    Query::update()
        .table(table_of(target, NotificationDeliveries::Table))
        .and_where(delivery_column(NotificationDeliveries::ClaimedBy).eq(worker_id.as_uuid()))
        .and_where(delivery_column(NotificationDeliveries::LeaseUntil).gt(Expr::current_timestamp()))
        .returning(Query::returning().columns([
            (NotificationDeliveries::Table, NotificationDeliveries::DeliveryKey),
            (NotificationDeliveries::Table, NotificationDeliveries::SubscriptionKey),
        ]))
        .to_owned()
}

/// `notification_deliveries.<column>`: the updated table's own column, named
/// apart from the settled rows' column of the same name.
fn delivery_column(column: NotificationDeliveries) -> Expr {
    Expr::col((NotificationDeliveries::Table, column))
}

/// The alias the settled rows are unnested under, `f`.
fn source_alias() -> Alias {
    Alias::new("f")
}

/// `SELECT f.<column>, … FROM UNNEST(<array>, …) AS f(<column>, …)`: one row
/// per settled delivery, each column's array bound as one parameter of its
/// array type, and the columns the alias names.
fn settled_rows<const N: usize>(columns: [(NotificationDeliveries, PostgresType, SimpleExpr); N]) -> (SelectStatement, [UnnestColumn; N]) {
    let unnested = columns.each_ref().map(|(name, array_type, _)| UnnestColumn::new(*name, *array_type));
    let mut select = Query::select();
    for (name, _, _) in &columns {
        select.column((source_alias(), *name));
    }
    select.from_function(unnest_call(unnested.iter().zip(columns.into_iter().map(|(_, _, array)| array))), source_alias());
    (select, unnested)
}

/// `WITH [<source>,] acknowledged AS (<update>), <hold> <select>`.
fn with_acknowledged(source: Option<CommonTableExpression>, update: UpdateStatement, hold: CommonTableExpression, select: SelectStatement) -> WithQuery {
    let mut with = WithClause::new();
    if let Some(source) = source {
        with.cte(source);
    }
    with.cte(cte(Acknowledged, update)).cte(hold).to_owned().query(select)
}

#[cfg(test)]
#[expect(
    clippy::expect_used,
    reason = "test code unwraps, expects and panics on purpose; migrating it to typed assertions is pending"
)]
mod tests {
    use crate::{
        notification_delivery::acknowledge::statements::{ExhaustedRows, FailedRows, delivered_statement, exhausted_statement, failed_statement},
        statement_text::text_of,
    };
    use andromeda_context_commit::lease_holder::LeaseHolder;
    use andromeda_context_space::{context_target::ContextTarget, snapshot_key::SnapshotKey, snapshot_target::SnapshotTarget};
    use chrono::Utc;
    use uuid::Uuid;

    fn worker() -> LeaseHolder {
        LeaseHolder::from(Uuid::nil())
    }

    fn delivered(target: &ContextTarget) -> String {
        text_of(&delivered_statement(target, worker(), vec![1, 2]))
    }

    fn failed(target: &ContextTarget) -> String {
        let rows = FailedRows {
            delivery_keys: vec![1, 2],
            failures: vec!["boom", "bang"],
            next_attempts: vec![Utc::now(), Utc::now()],
        };
        text_of(&failed_statement(target, worker(), rows).expect("a buildable statement"))
    }

    fn exhausted(target: &ContextTarget) -> String {
        let rows = ExhaustedRows {
            delivery_keys: vec![1, 2],
            failures: vec!["boom", "bang"],
        };
        text_of(&exhausted_statement(target, worker(), rows))
    }

    fn snapshot() -> ContextTarget {
        let key = SnapshotKey::try_from(5).expect("positive key");
        ContextTarget::Snapshot(SnapshotTarget::published(key, "urn:ngsi-ld:Snapshot:s".parse().expect("valid snapshot uri")))
    }

    /// The text of the guarded update, the `acknowledged` common table
    /// expression.
    fn acknowledgement_of(sql: &str) -> &str {
        let start = sql.find(r#""acknowledged" AS (UPDATE"#).expect("an acknowledged update");
        let rest = sql.get(start..).unwrap_or_default();
        rest.split(" RETURNING ").next().unwrap_or_default()
    }

    #[test]
    fn every_row_of_a_group_is_guarded_by_the_live_lease_it_was_taken_under() {
        for sql in [delivered(&ContextTarget::Live), failed(&ContextTarget::Live), exhausted(&ContextTarget::Live)] {
            let guard = acknowledgement_of(&sql);
            assert!(guard.contains(r#""notification_deliveries"."claimed_by" = $"#), "{sql}");
            assert!(guard.contains(r#""notification_deliveries"."lease_until" > CURRENT_TIMESTAMP"#), "{sql}");
        }
    }

    /// The group's keys travel as one array, so the statement text is the
    /// same however many deliveries the group holds.
    #[test]
    fn a_group_of_deliveries_is_marked_delivered_by_one_statement_over_its_keys() {
        let sql = delivered(&ContextTarget::Live);
        let guard = acknowledgement_of(&sql);
        assert!(guard.contains(r#""notification_deliveries"."delivery_key" = ANY($"#), "{sql}");
        assert!(guard.contains(r#""delivered_at" = CURRENT_TIMESTAMP"#), "{sql}");
        assert!(!guard.contains(r#""claimed_by" = NULL"#), "the delivered rows keep their claim pairs: {sql}");
        pretty_assertions::assert_eq!(sql, text_of(&delivered_statement(&ContextTarget::Live, worker(), (1..=500).collect())));
    }

    /// A row whose guard refused is missing from `acknowledged`, so the
    /// statement returns no row for it, and each row returned carries the
    /// hold of its own Subscription.
    #[test]
    fn every_statement_returns_the_rows_it_recorded_beside_their_own_holds() {
        let delivered = delivered(&ContextTarget::Live);
        assert!(
            delivered.ends_with(concat!(
                r#"SELECT "acknowledged"."delivery_key", "released"."subscription_key" FROM "acknowledged" "#,
                r#"LEFT JOIN "released" ON "released"."subscription_key" = "acknowledged"."subscription_key""#
            )),
            "{delivered}"
        );
        let failed = failed(&ContextTarget::Live);
        assert!(
            failed.ends_with(concat!(
                r#"SELECT "acknowledged"."delivery_key", "taken"."subscription_key", "taken"."delivery_key" FROM "acknowledged" "#,
                r#"INNER JOIN "taken" ON "taken"."subscription_key" = "acknowledged"."subscription_key""#
            )),
            "{failed}"
        );
        let exhausted = exhausted(&ContextTarget::Live);
        assert!(
            exhausted.ends_with(concat!(
                r#"SELECT "acknowledged"."delivery_key", "probed"."subscription_key" FROM "acknowledged" "#,
                r#"LEFT JOIN "probed" ON "probed"."delivery_key" = "acknowledged"."delivery_key""#
            )),
            "{exhausted}"
        );
    }

    #[test]
    fn a_group_of_deliveries_releases_the_holds_of_their_subscriptions() {
        let sql = delivered(&ContextTarget::Live);
        assert!(
            sql.contains(concat!(
                r#""released" AS (DELETE FROM "notification_delivery_holds" WHERE "subscription_key" IN "#,
                r#"(SELECT "subscription_key" FROM "acknowledged") RETURNING "subscription_key")"#
            )),
            "{sql}"
        );
    }

    /// Each failed row takes its own diagnostic and due instant from the
    /// unnested arrays, joined on its key.
    #[test]
    fn a_group_of_failures_records_each_row_from_its_own_element_of_the_arrays() {
        let sql = failed(&ContextTarget::Live);
        assert!(
            sql.starts_with(concat!(
                r#"WITH "failure" AS (SELECT "f"."delivery_key", "f"."last_error", "f"."next_attempt_at" "#,
                r#"FROM UNNEST($1::bigint[], $2::text[], $3::timestamptz[]) AS f(delivery_key, last_error, next_attempt_at))"#
            )),
            "{sql}"
        );
        let guard = acknowledgement_of(&sql);
        assert!(guard.contains(r#""attempt_count" = "notification_deliveries"."attempt_count" + $"#), "{sql}");
        assert!(guard.contains(r#""last_error" = "failure"."last_error""#), "{sql}");
        assert!(guard.contains(r#""next_attempt_at" = "failure"."next_attempt_at""#), "{sql}");
        assert!(guard.contains(r#""claimed_by" = NULL, "lease_until" = NULL FROM "failure""#), "{sql}");
        assert!(
            guard.contains(r#""notification_deliveries"."delivery_key" = "failure"."delivery_key""#),
            "{sql}"
        );
    }

    /// Two failures of one Subscription in a group take one hold: the one
    /// queued first is the probe, and the other is set aside behind it.
    #[test]
    fn a_group_of_failures_takes_one_hold_per_subscription_or_finds_it() {
        let sql = failed(&ContextTarget::Live);
        assert!(
            sql.contains(concat!(
                r#""taken" AS (INSERT INTO "notification_delivery_holds" ("subscription_key", "delivery_key") "#,
                r#"SELECT DISTINCT ON ("subscription_key") "subscription_key", "delivery_key" FROM "acknowledged" "#,
                r#"ORDER BY "subscription_key" ASC, "delivery_key" ASC ON CONFLICT ("subscription_key") "#,
                r#"DO UPDATE SET "delivery_key" = "notification_delivery_holds"."delivery_key" "#,
                r#"RETURNING "subscription_key", "delivery_key")"#
            )),
            "{sql}"
        );
    }

    #[test]
    fn a_group_of_exhaustions_stamps_each_row_and_releases_its_claim() {
        let sql = exhausted(&ContextTarget::Live);
        assert!(
            sql.starts_with(
                r#"WITH "exhaustion" AS (SELECT "f"."delivery_key", "f"."last_error" FROM UNNEST($1::bigint[], $2::text[]) AS f(delivery_key, last_error))"#
            ),
            "{sql}"
        );
        let guard = acknowledgement_of(&sql);
        assert!(guard.contains(r#""exhausted_at" = CURRENT_TIMESTAMP"#), "{sql}");
        assert!(guard.contains(r#""attempt_count" = "notification_deliveries"."attempt_count" + $"#), "{sql}");
        assert!(guard.contains(r#""last_error" = "exhaustion"."last_error""#), "{sql}");
        assert!(guard.contains(r#""claimed_by" = NULL, "lease_until" = NULL FROM "exhaustion""#), "{sql}");
    }

    /// Only a probe's exhaustion finds a hold, which it locks before the
    /// hand-over reads the deliveries set aside.
    #[test]
    fn a_group_of_exhaustions_locks_the_holds_its_probes_held() {
        let sql = exhausted(&ContextTarget::Live);
        assert!(
            sql.contains(concat!(
                r#""probed" AS (SELECT "subscription_key", "delivery_key" FROM "notification_delivery_holds" WHERE "delivery_key" IN "#,
                r#"(SELECT "delivery_key" FROM "acknowledged") FOR UPDATE)"#
            )),
            "{sql}"
        );
    }

    #[test]
    fn a_snapshot_branch_group_records_and_moves_the_holds_of_its_own_branch() {
        for sql in [delivered(&snapshot()), failed(&snapshot()), exhausted(&snapshot())] {
            assert!(sql.contains(r#"UPDATE "snap_5"."notification_deliveries" SET"#), "{sql}");
            assert!(sql.contains(r#""snap_5"."notification_delivery_holds""#), "{sql}");
            assert!(!sql.contains(r#"FROM "notification_delivery_holds""#), "{sql}");
            assert!(!sql.contains(r#"INTO "notification_delivery_holds""#), "{sql}");
        }
    }
}
