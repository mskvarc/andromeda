//! The queue and expiry sources of a tenant's earliest obligation: the live
//! branch's sources folded into one `SELECT LEAST(...)` of scalar subqueries,
//! and one statement per published Snapshot branch for its delivery queue.
//!
//! The live statement names no Snapshot schema, so its text is the same
//! whatever Snapshots exist and a connection plans it once; a Snapshot created
//! or reaped adds or drops only its own small statement.
//!
//! A queue is read through the index its claim uses. An expression over every
//! row would read the whole heap on every probe, and a queue heap keeps the
//! pages of the largest backlog it ever held.

use crate::{
    snapshot::{reap::orphan_branch_keys_statement, status_code::stored_status as stored_snapshot_status},
    subscription_store::status_code::status_is_not,
};
use andromeda_context_space::context_target::ContextTarget;
use andromeda_ngsi_ld_model::{snapshot::SnapshotStatus, subscription::SubscriptionStatus};
use andromeda_postgresql_branch::table_of;
use andromeda_postgresql_codes::jsonld_context_kind::JsonldContextKindCode;
use andromeda_postgresql_schema::{
    csr::Csr,
    current::Entities,
    entity_map::EntityMaps,
    jsonld::JsonldContexts,
    notification::NotificationDeliveries,
    posted_query::PostedQueries,
    queue::{PendingSideEffects, TemporalAppendIntents},
    snapshot::{SnapshotNotificationOutbox, SnapshotPreparation, Snapshots},
    subscription_forwarding::ForwardedSubscriptionClaims,
    subscriptions::Subscriptions,
};
use andromeda_postgresql_seaquery::{clock::now, queue::claimable_at};
use sea_query::{
    Alias,
    CaseStatement,
    Cond,
    Condition,
    Expr,
    ExprTrait,
    Func,
    Iden,
    IntoColumnRef,
    IntoTableRef,
    Query,
    QueryStatementBuilder,
    SelectStatement,
    SimpleExpr,
};

/// The derived table holding a queue's earliest unleased due instant.
#[derive(Iden, Clone, Copy)]
#[iden = "unleased"]
enum Unleased {
    /// Table alias: `unleased`
    Table,
    /// The earliest `next_attempt_at` of an unleased row.
    Due,
}

/// `SELECT LEAST(...)` of the earliest due instant in each queue and expiry
/// source of the live branch and the tenant, a single row whose one nullable
/// column is the soonest, or NULL when nothing is due.
pub(crate) fn least_due_select() -> SelectStatement {
    let sources = vec![
        claimable_min(
            NotificationDeliveries::Table,
            NotificationDeliveries::NextAttemptAt,
            NotificationDeliveries::LeaseUntil,
            unfinished_delivery(),
        ),
        claimable_min(
            SnapshotNotificationOutbox::Table,
            SnapshotNotificationOutbox::NextAttemptAt,
            SnapshotNotificationOutbox::LeaseUntil,
            unfinished_snapshot_notification(),
        ),
        heap_only_claimable_min(
            PendingSideEffects::Table,
            PendingSideEffects::NextAttemptAt,
            PendingSideEffects::LeaseUntil,
            Cond::all().add(unfinished_side_effect()),
        ),
        heap_only_claimable_min(
            TemporalAppendIntents::Table,
            TemporalAppendIntents::NextAttemptAt,
            TemporalAppendIntents::LeaseUntil,
            Cond::all(),
        ),
        preparing_min(),
        expiry_min(Entities::Table, Entities::ExpiresAt),
        expiry_min(Entities::Table, Entities::MinAttributeExpiresAt),
        subscription_expiry_min(),
        expiry_min(Csr::Table, Csr::ExpiresAt),
        earliest_min(Snapshots::Table, Snapshots::ExpiresAt),
        orphan_branch_due(),
        earliest_min(EntityMaps::Table, EntityMaps::ExpiresAt),
        earliest_min(PostedQueries::Table, PostedQueries::ExpiresAt),
        cached_context_expiry_min(),
        // A copy placement cut off before its source answered is placed again
        // once its claim's lease ends.
        earliest_min(ForwardedSubscriptionClaims::Table, ForwardedSubscriptionClaims::LeaseUntil),
    ];
    Query::select().expr_as(Func::least(sources), Alias::new("due")).to_owned()
}

/// The earliest claimable instant of the delivery queue of `branch`, a single
/// row whose one nullable column is NULL when the queue holds nothing
/// unfinished.
///
/// A delivery lives in the queue of the branch whose subscription matched, and
/// the delivery drain claims from each published Snapshot branch as from the
/// live one, so a Snapshot-branch retry is an obligation of the tenant like any
/// other.
pub(crate) fn branch_delivery_due_select(branch: &ContextTarget) -> SelectStatement {
    claimable_min_select(
        table_of(branch, NotificationDeliveries::Table),
        NotificationDeliveries::NextAttemptAt,
        NotificationDeliveries::LeaseUntil,
        unfinished_delivery(),
    )
}

/// A scalar subquery of the earliest claimable instant of a queue's unfinished
/// rows, for a queue whose claim index is that expression.
fn claimable_min(table: impl IntoTableRef, next_attempt: impl IntoColumnRef, lease_until: impl IntoColumnRef, unfinished: SimpleExpr) -> SimpleExpr {
    scalar(claimable_min_select(table, next_attempt, lease_until, unfinished))
}

/// The earliest claimable instant of a queue's unfinished rows,
/// `MIN(GREATEST(next_attempt_at, COALESCE(lease_until, ...)))`.
fn claimable_min_select(
    table: impl IntoTableRef,
    next_attempt: impl IntoColumnRef,
    lease_until: impl IntoColumnRef,
    unfinished: SimpleExpr,
) -> SelectStatement {
    let mut select = Query::select();
    select
        .expr(Func::min(claimable_at(Expr::col(next_attempt), Expr::col(lease_until))))
        .from(table)
        .and_where(unfinished);
    select
}

/// The same earliest claimable instant for a queue whose claim index leads
/// with `next_attempt_at` and leaves the lease out, so that a claim stays a
/// heap-only update; `unfinished` is that index's predicate.
///
/// No index holds the expression, so it is read through the claim index in two
/// parts. An unleased row is claimable at `next_attempt_at`: the first unleased
/// entry of the index. A leased row is claimable once its lease lapses, and a
/// claim takes only a row whose `next_attempt_at` has passed while every
/// release clears the lease, so every leased row lies in the index range up to
/// now. That range is read only when no unleased row is due: otherwise the
/// answer is already past, and every consumer treats a past instant as due now,
/// whichever it is. A backlog of due rows therefore costs one index entry, and
/// a queue emptied after a backlog costs its index rather than every heap page
/// the backlog grew it to.
fn heap_only_claimable_min(
    table: impl IntoTableRef + Copy,
    next_attempt: impl IntoColumnRef + Copy,
    lease_until: impl IntoColumnRef + Copy,
    unfinished: Condition,
) -> SimpleExpr {
    let mut unleased = Query::select();
    unleased
        .expr_as(Func::min(Expr::col(next_attempt)), Unleased::Due)
        .from(table)
        // clone: the same predicate also filters the leased part, so it is copied here.
        .cond_where(unfinished.clone().add(Expr::col(lease_until).is_null()));
    let mut leased = Query::select();
    leased
        .expr(Func::min(Func::greatest([Expr::col(next_attempt), Expr::col(lease_until)])))
        .from(table)
        .cond_where(
            unfinished
                .add(Expr::col(lease_until).is_not_null())
                .add(Expr::col(next_attempt).lte(Expr::current_timestamp())),
        );
    let due = || Expr::col((Unleased::Table, Unleased::Due));
    let earliest = CaseStatement::new()
        .case(due().lte(Expr::current_timestamp()), due())
        .finally(Func::least([due(), scalar(leased)]));
    let mut select = Query::select();
    select.expr(earliest).from_subquery(unleased, Unleased::Table);
    scalar(select)
}

/// A scalar subquery of when the earliest still-preparing snapshot next needs a
/// preparation tick: its lease expiry, or now when it holds no lease.
fn preparing_min() -> SimpleExpr {
    let mut select = Query::select();
    select
        .expr(Func::min(Func::coalesce([Expr::col(SnapshotPreparation::LeaseUntil), now()])))
        .from(SnapshotPreparation::Table)
        .and_where(Expr::col(SnapshotPreparation::Status).eq(stored_snapshot_status(SnapshotStatus::Preparing).code()));
    scalar(select)
}

/// A scalar subquery of the earliest non-null `expires_at` in a table.
fn expiry_min(table: impl IntoTableRef, column: impl IntoColumnRef + Copy) -> SimpleExpr {
    let mut select = Query::select();
    select.expr(Func::min(Expr::col(column))).from(table).and_where(Expr::col(column).is_not_null());
    scalar(select)
}

/// A scalar subquery that is now while a branch schema outlives its
/// Snapshot's control record, so the expiry sweep that drops it is due. A
/// Snapshot deleted while PostgreSQL maintenance held its branch lock leaves
/// such a schema, and no control record is left to carry an expiry.
fn orphan_branch_due() -> SimpleExpr {
    let mut select = Query::select();
    select.expr(now()).and_where(Expr::exists(orphan_branch_keys_statement()));
    scalar(select)
}

/// A scalar subquery of the earliest `expires_at` in a table whose column is
/// always present, so no null filter is needed.
fn earliest_min(table: impl IntoTableRef, column: impl IntoColumnRef + Copy) -> SimpleExpr {
    let mut select = Query::select();
    select.expr(Func::min(Expr::col(column))).from(table);
    scalar(select)
}

/// A scalar subquery of the earliest expiration time of a `"Cached"` `@context`
/// (clause 5.13.1): the instant the periodic invalidation next has a row to
/// reclaim. Hosted contexts never expire and are left out.
fn cached_context_expiry_min() -> SimpleExpr {
    let mut select = Query::select();
    select
        .expr(Func::min(Expr::col(JsonldContexts::ExpiresAt)))
        .from(JsonldContexts::Table)
        .and_where(Expr::col(JsonldContexts::Kind).eq(JsonldContextKindCode::Cached.code()))
        .and_where(Expr::col(JsonldContexts::ExpiresAt).is_not_null());
    scalar(select)
}

/// A scalar subquery of the earliest subscription expiry, ignoring subscriptions
/// already expired.
fn subscription_expiry_min() -> SimpleExpr {
    let mut select = Query::select();
    select
        .expr(Func::min(Expr::col(Subscriptions::ExpiresAt)))
        .from(Subscriptions::Table)
        .and_where(Expr::col(Subscriptions::ExpiresAt).is_not_null())
        .and_where(status_is_not(SubscriptionStatus::Expired));
    scalar(select)
}

/// A durable delivery a claim may reach: neither delivered nor exhausted, nor
/// set aside behind its Subscription's probe. The probe is the obligation of a
/// held Subscription; what it holds back is due only once it is released.
/// This is the predicate of the claim index, which the read goes through.
fn unfinished_delivery() -> SimpleExpr {
    Expr::col(NotificationDeliveries::DeliveredAt)
        .is_null()
        .and(Expr::col(NotificationDeliveries::ExhaustedAt).is_null())
        .and(Expr::col(NotificationDeliveries::HeldAt).is_null())
}

/// An unfinished snapshot status notification: neither delivered nor exhausted.
fn unfinished_snapshot_notification() -> SimpleExpr {
    Expr::col(SnapshotNotificationOutbox::DeliveredAt)
        .is_null()
        .and(Expr::col(SnapshotNotificationOutbox::ExhaustedAt).is_null())
}

/// An unfinished side effect: not yet exhausted.
fn unfinished_side_effect() -> SimpleExpr {
    Expr::col(PendingSideEffects::ExhaustedAt).is_null()
}

/// Wrap a single-row select as a scalar subquery expression.
fn scalar(select: SelectStatement) -> SimpleExpr {
    SimpleExpr::SubQuery(None, Box::new(select.into_sub_query_statement()))
}

#[cfg(test)]
mod tests {
    use crate::pending_work::due_sources::{branch_delivery_due_select, least_due_select};
    use andromeda_context_space::{context_target::ContextTarget, snapshot_key::SnapshotKey, snapshot_target::SnapshotTarget};
    use andromeda_postgresql_codes::jsonld_context_kind::JsonldContextKindCode;
    use sea_query::PostgresQueryBuilder;

    fn least_sql() -> String {
        least_due_select().to_string(PostgresQueryBuilder)
    }

    /// A delivery queued on a published Snapshot branch is an obligation of the
    /// tenant: the drain claims it like a live one, so its retry must wake a
    /// dormant tenant and keep a resident one's drain from resting past it.
    #[test]
    fn the_probe_reads_the_delivery_queue_of_a_published_snapshot_branch() {
        let snapshot = ContextTarget::Snapshot(SnapshotTarget::published(
            SnapshotKey::try_from(7_i64).expect("a positive key"),
            "urn:ngsi-ld:Snapshot:probe".parse().expect("a valid URI"),
        ));
        let sql = branch_delivery_due_select(&snapshot).to_string(PostgresQueryBuilder);
        assert!(sql.contains("FROM \"snap_7\".\"notification_deliveries\""), "{sql}");
        assert!(sql.contains("GREATEST(\"next_attempt_at\", COALESCE(\"lease_until\""), "{sql}");
    }

    /// The live statement is planned once per connection only if its text
    /// does not change as Snapshots are created and reaped.
    #[test]
    fn the_live_statement_names_no_snapshot_schema() {
        assert!(!least_sql().contains("\"snap_"), "{}", least_sql());
    }

    #[test]
    fn the_probe_reads_the_durable_delivery_queue() {
        let sql = least_sql();
        assert!(sql.contains("FROM \"notification_deliveries\""), "{sql}");
        assert!(sql.contains("GREATEST(\"next_attempt_at\", COALESCE(\"lease_until\""), "{sql}");
    }

    #[test]
    fn the_probe_reads_the_snapshot_status_outbox() {
        assert!(least_sql().contains("FROM \"snapshot_notification_outbox\""), "{}", least_sql());
    }

    #[test]
    fn the_probe_reads_the_pending_side_effects_queue() {
        assert!(least_sql().contains("FROM \"pending_side_effects\""), "{}", least_sql());
    }

    #[test]
    fn the_probe_reads_the_temporal_append_intents_queue() {
        assert!(least_sql().contains("FROM \"temporal_append_intents\""), "{}", least_sql());
    }

    #[test]
    fn the_probe_reads_the_preparing_snapshots() {
        let sql = least_sql();
        assert!(sql.contains("FROM \"snapshot_preparation\""), "{sql}");
        assert!(sql.contains("\"status\" = 1"), "only preparing snapshots, by code: {sql}");
    }

    #[test]
    fn the_probe_reads_entity_and_attribute_expiry() {
        let sql = least_sql();
        assert!(sql.contains("FROM \"entities\""), "{sql}");
        assert!(sql.contains("\"expires_at\""), "{sql}");
        assert!(sql.contains("\"min_attribute_expires_at\""), "{sql}");
    }

    #[test]
    fn the_probe_reads_subscription_expiry() {
        assert!(least_sql().contains("FROM \"subscriptions\""), "{}", least_sql());
    }

    #[test]
    fn the_probe_reads_context_source_registration_expiry() {
        assert!(least_sql().contains("FROM \"context_source_registrations\""), "{}", least_sql());
    }

    #[test]
    fn the_probe_reads_snapshot_entity_map_and_posted_query_expiry() {
        let sql = least_sql();
        assert!(sql.contains("FROM \"snapshots\""), "{sql}");
        assert!(sql.contains("FROM \"entity_maps\""), "{sql}");
        assert!(sql.contains("FROM \"posted_queries\""), "{sql}");
    }

    #[test]
    fn the_probe_reports_a_branch_schema_without_a_control_record_as_due_now() {
        let sql = least_sql();
        assert!(
            sql.contains(r#"(SELECT NOW() WHERE EXISTS(SELECT DISTINCT substring("nspname", '^snap_([1-9][0-9]*)(_stg)?$')::bigint"#),
            "{sql}"
        );
        assert!(sql.contains(r#"NOT EXISTS(SELECT 1 FROM "snapshots" WHERE"#), "{sql}");
    }

    /// Clause 5.13.1 invalidation is due when the earliest `"Cached"` entry
    /// lapses; a hosted entry never lapses.
    #[test]
    fn the_probe_reads_the_earliest_cached_context_expiry() {
        let sql = least_sql();
        assert!(
            sql.contains(&format!(
                "(SELECT MIN(\"expires_at\") FROM \"jsonld_contexts\" WHERE \"kind\" = {} AND \"expires_at\" IS NOT NULL)",
                JsonldContextKindCode::Cached.code()
            )),
            "{sql}"
        );
    }

    /// An abandoned copy placement is placed again when its claim's lease
    /// ends, read through the index on the claims' lease ends.
    #[test]
    fn the_probe_reads_the_earliest_lease_end_of_an_unsettled_copy_claim() {
        let sql = least_sql();
        assert!(sql.contains(r#"(SELECT MIN("lease_until") FROM "forwarded_subscription_claims")"#), "{sql}");
    }

    #[test]
    fn the_probe_folds_every_source_into_one_least() {
        assert!(least_sql().contains("SELECT LEAST("), "{}", least_sql());
    }

    /// A subscription already expired owes no expiry sweep, so it never keeps
    /// the tenant due.
    #[test]
    fn the_subscription_expiry_source_ignores_expired_subscriptions() {
        assert!(least_sql().contains("\"subscriptions\".\"status\" <> 3"), "{}", least_sql());
    }

    /// The two queues whose claim index leaves the lease out are read through
    /// that index; only the expression-indexed queues read the claimable
    /// instant as one expression.
    #[test]
    fn a_queue_whose_claim_index_leaves_the_lease_out_is_read_through_it() {
        let sql = least_sql();
        assert!(
            sql.contains(r#"FROM (SELECT MIN("next_attempt_at") AS "due" FROM "temporal_append_intents" WHERE "lease_until" IS NULL) AS "unleased""#),
            "{sql}"
        );
        assert!(
            sql.contains(r#"FROM (SELECT MIN("next_attempt_at") AS "due" FROM "pending_side_effects" WHERE "exhausted_at" IS NULL AND "lease_until" IS NULL) AS "unleased""#),
            "{sql}"
        );
        assert_eq!(sql.matches("'-infinity'::timestamptz").count(), 2, "{sql}");
    }

    /// Every leased row was due when it was claimed, so the leased part reads
    /// the index range up to now, and only once no unleased row is due.
    #[test]
    fn the_leased_rows_are_read_only_when_no_unleased_row_is_due() {
        let sql = least_sql();
        assert!(
            sql.contains(concat!(
                r#"CASE WHEN ("unleased"."due" <= CURRENT_TIMESTAMP) THEN "unleased"."due" ELSE LEAST("unleased"."due", "#,
                r#"(SELECT MIN(GREATEST("next_attempt_at", "lease_until")) FROM "temporal_append_intents" "#,
                r#"WHERE "lease_until" IS NOT NULL AND "next_attempt_at" <= CURRENT_TIMESTAMP)) END"#
            )),
            "{sql}"
        );
    }
}
