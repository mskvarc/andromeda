//! The verbatim-`instanceId` uniqueness pass that guards a bulk history insert.
//!
//! Clause 4.5.7 makes a client-supplied `instanceId` unique to one attribute
//! instance across the whole attribute, not to one `datasetId` stream of it. A
//! batch that carries such identifiers therefore, before it appends, supersedes
//! the rows it replaces across every sibling stream and re-stakes the
//! `(entity, attribute, verbatim instanceId)` claims that
//! `attribute_history_instance_claims` refuses a duplicate of. All of that is
//! its own concern, separate from spreading and inserting the history rows in
//! [`crate::attribute_history_write`].

use crate::{
    attribute_history_row::AttributeHistoryRow,
    duplicate_instance_claim::claim_write_failure,
    error::TemporalStoreError,
    stored_instance_columns::verbatim_instance_id,
};
use andromeda_context_space::context_target::ContextTarget;
use andromeda_postgresql_branch::table_of;
use andromeda_postgresql_schema::temporal::{AttributeHistory, AttributeHistoryInstanceClaims, TemporalAttributeStreams};
use andromeda_postgresql_seaquery::{
    pg_type::PostgresType,
    placeholder::external_placeholder,
    unnest::{UnnestColumn, apply_column_alias, unnest_call},
};
use sea_query::{Expr, ExprTrait, Iden, JoinType, PostgresQueryBuilder, Query};
use sqlx::{AssertSqlSafe, Postgres, Transaction};

/// The `UNNEST` staging alias, `d`.
#[derive(Iden, Clone, Copy)]
#[iden = "d"]
struct D;

/// The `temporal_attribute_streams` alias a targeted row's incoming stream is
/// resolved to its `(entity, attribute)` scope through.
#[derive(Iden, Clone, Copy)]
#[iden = "s1"]
struct S1;

/// The `temporal_attribute_streams` alias that enumerates the sibling streams of
/// that scope.
#[derive(Iden, Clone, Copy)]
#[iden = "s2"]
struct S2;

/// The `temporal_attribute_streams` alias the claim insert reads the scope from.
#[derive(Iden, Clone, Copy)]
#[iden = "s"]
struct S;

/// The two staging columns of the verbatim-dedup and claim statements.
#[derive(Iden, Clone, Copy)]
enum DedupStaged {
    /// The stream each targeted row extends.
    StreamKey,
    /// The verbatim identifier each targeted row carries.
    VerbatimInstanceId,
}

/// The `(stream, verbatim instanceId)` pair of every row in a batch that
/// carries a client-supplied instance identity, one entry per row and not
/// deduplicated.
///
/// Not deduplicated on purpose: two rows that name the same pair, or that
/// resolve to the same `(entity, attribute, verbatim instanceId)` triple
/// through sibling `datasetId` streams, must both reach
/// [`claim_verbatim_instances`] so its constraint can refuse the second one.
/// Collapsing them here would hide exactly the batch this module exists to
/// catch.
pub(crate) struct VerbatimInstanceTargets<'a> {
    /// The stream each targeted row extends.
    streams: Vec<i64>,
    /// The verbatim identifier each targeted row carries.
    ids: Vec<&'a str>,
}

/// Collect one batch's verbatim-identified rows as parallel arrays for the
/// `UNNEST`-based statements below.
pub(crate) fn verbatim_instance_targets(rows: &[AttributeHistoryRow]) -> VerbatimInstanceTargets<'_> {
    let (streams, ids) = rows
        .iter()
        .filter_map(|row| verbatim_instance_id(&row.instance).map(|identifier| (row.stream_key.get(), identifier)))
        .unzip();
    VerbatimInstanceTargets { streams, ids }
}

/// Remove the rows a batch is about to supersede.
///
/// A client that supplied its own `instanceId` (clause 4.5.7) is replacing a
/// named instance, so the row carrying that verbatim identifier goes first;
/// nothing is deleted for broker-minted identifiers, which are unique by
/// construction.
///
/// The delete is scoped to every sibling stream of the incoming row's
/// `(entity, attribute)` — every `datasetId`, not only the one the incoming
/// row targets — because clause 4.5.7 makes `instanceId` unique to one
/// attribute instance, not to one dataset stream of it, and that is the same
/// scope the instance PATCH and DELETE lookups already search. A narrower,
/// single-stream scope would let a client address one row through the
/// PATCH/DELETE endpoints while a same-named row on a sibling dataset stream
/// silently kept its own separate lifetime, in violation of "one `instanceId`
/// names one row".
///
/// The claim table gets the same treatment, for the same reason: a batch that
/// legitimately replaces an already-claimed identity must first release the
/// claim it is about to re-stake, or [`claim_verbatim_instances`] would refuse
/// its own replacement as a conflict with itself.
///
/// There used to be a second pass here deleting every row on the stream that
/// shared an incoming row's `observedAt`, on the theory that a re-sent
/// observation at the same instant is an update rather than a duplicate. That
/// reasoning depended on the append being reached only once per write, which a
/// claim gate on the write record used to guarantee. Now that idempotency is
/// row-level (`uq_attribute_history_write_identity` plus the verbatim-instance pass
/// above), the append is reached again on every replay of a durable intent,
/// and an `observedAt`-keyed delete cannot distinguish "this is my own row
/// from a previous attempt" from "this is a different write's row that
/// happens to share an instant" -- a retried intent append or a crash-retried
/// side-effect replay would delete a later, unrelated write's history row.
/// The pass is removed rather than scoped, because scoping it to the current
/// write would silently drop the replace-on-resend behaviour it existed for
/// while keeping the same footgun for any two writes that legitimately share
/// an `observedAt` on one stream.
pub(crate) async fn run_history_dedup_deletes(
    tx: &mut Transaction<'_, Postgres>,
    target: &ContextTarget,
    targets: &VerbatimInstanceTargets<'_>,
) -> Result<(), TemporalStoreError> {
    if targets.ids.is_empty() {
        return Ok(());
    }
    let history_dedup_sql = history_verbatim_dedup_sql(target);
    sqlx::query(AssertSqlSafe(history_dedup_sql))
        .bind(&targets.streams)
        .bind(&targets.ids)
        .execute(&mut **tx)
        .await?;
    let claim_dedup_sql = claim_verbatim_dedup_delete_sql(target);
    sqlx::query(AssertSqlSafe(claim_dedup_sql))
        .bind(&targets.streams)
        .bind(&targets.ids)
        .execute(&mut **tx)
        .await?;
    Ok(())
}

/// Stake this batch's claim on every `(entity, attribute, verbatim
/// instanceId)` triple it carries.
///
/// This is what turns the race the review found into a refusal. Two rows in
/// one batch that resolve to the same triple -- whether they name the same
/// stream or sibling `datasetId` streams of the same attribute -- both appear
/// in this statement's `SELECT`, and `attribute_history_instance_claims`'s
/// primary key refuses the second one within the same `INSERT`, aborting the
/// whole batch's transaction. Two concurrent batches claiming the same triple
/// get the same refusal from the same constraint, whichever commits second --
/// a guarantee no check confined to one batch's own rows could give, because
/// by the time either transaction's `INSERT` runs, neither has seen the
/// other's uncommitted row.
pub(crate) async fn claim_verbatim_instances(
    tx: &mut Transaction<'_, Postgres>,
    target: &ContextTarget,
    targets: &VerbatimInstanceTargets<'_>,
) -> Result<(), TemporalStoreError> {
    if targets.ids.is_empty() {
        return Ok(());
    }
    let claim_sql = claim_verbatim_instances_sql(target)?;
    sqlx::query(AssertSqlSafe(claim_sql))
        .bind(&targets.streams)
        .bind(&targets.ids)
        .execute(&mut **tx)
        .await
        .map_err(claim_write_failure)?;
    Ok(())
}

/// The two staging columns the verbatim-dedup and claim statements read.
fn dedup_staging_columns() -> [UnnestColumn; 2] {
    [
        UnnestColumn::new(DedupStaged::StreamKey, PostgresType::BigintArray),
        UnnestColumn::new(DedupStaged::VerbatimInstanceId, PostgresType::TextArray),
    ]
}

/// The `UNNEST($1::bigint[], $2::text[])` staging call the dedup/claim statements
/// share; the two arrays bind through the caller's chain.
fn dedup_unnest() -> sea_query::FunctionCall {
    let columns = dedup_staging_columns();
    unnest_call([(&columns[0], external_placeholder(1)), (&columns[1], external_placeholder(2))])
}

/// Remove the history rows a verbatim-identified batch supersedes, across every
/// sibling `datasetId` stream of the incoming row's `(entity, attribute)`.
///
/// `DELETE … USING … JOIN` is expressed as a correlated `EXISTS`, since
/// sea-query has no `USING`.
fn history_verbatim_dedup_sql(target: &ContextTarget) -> String {
    let sibling_streams = Query::select()
        .column((S2, TemporalAttributeStreams::StreamKey))
        .from(table_of(target, TemporalAttributeStreams::Table).alias(S2))
        .and_where(Expr::col((S2, TemporalAttributeStreams::TemporalEntityKey)).equals((S1, TemporalAttributeStreams::TemporalEntityKey)))
        .and_where(Expr::col((S2, TemporalAttributeStreams::AttributeNameKey)).equals((S1, TemporalAttributeStreams::AttributeNameKey)))
        .to_owned();
    let matches = Query::select()
        .expr(Expr::val(1))
        .from_function(dedup_unnest(), D)
        .join_as(
            JoinType::InnerJoin,
            table_of(target, TemporalAttributeStreams::Table),
            S1,
            Expr::col((S1, TemporalAttributeStreams::StreamKey)).equals((D, DedupStaged::StreamKey)),
        )
        .and_where(Expr::col((AttributeHistory::Table, AttributeHistory::VerbatimInstanceId)).equals((D, DedupStaged::VerbatimInstanceId)))
        .and_where(Expr::col((AttributeHistory::Table, AttributeHistory::StreamKey)).in_subquery(sibling_streams))
        .to_owned();
    let sql = Query::delete()
        .from_table(table_of(target, AttributeHistory::Table))
        .and_where(Expr::exists(matches))
        .to_owned()
        .to_string(PostgresQueryBuilder);
    apply_column_alias(sql, &D, &dedup_staging_columns())
}

/// Release the claims the same batch is about to re-stake, scoped to the
/// `(entity, attribute)` the claim table is keyed by rather than to the stream.
fn claim_verbatim_dedup_delete_sql(target: &ContextTarget) -> String {
    let matches = Query::select()
        .expr(Expr::val(1))
        .from_function(dedup_unnest(), D)
        .join_as(
            JoinType::InnerJoin,
            table_of(target, TemporalAttributeStreams::Table),
            S1,
            Expr::col((S1, TemporalAttributeStreams::StreamKey)).equals((D, DedupStaged::StreamKey)),
        )
        .and_where(
            Expr::col((AttributeHistoryInstanceClaims::Table, AttributeHistoryInstanceClaims::VerbatimInstanceId)).equals((D, DedupStaged::VerbatimInstanceId)),
        )
        .and_where(
            Expr::col((AttributeHistoryInstanceClaims::Table, AttributeHistoryInstanceClaims::TemporalEntityKey))
                .equals((S1, TemporalAttributeStreams::TemporalEntityKey)),
        )
        .and_where(
            Expr::col((AttributeHistoryInstanceClaims::Table, AttributeHistoryInstanceClaims::AttributeNameKey))
                .equals((S1, TemporalAttributeStreams::AttributeNameKey)),
        )
        .to_owned();
    let sql = Query::delete()
        .from_table(table_of(target, AttributeHistoryInstanceClaims::Table))
        .and_where(Expr::exists(matches))
        .to_owned()
        .to_string(PostgresQueryBuilder);
    apply_column_alias(sql, &D, &dedup_staging_columns())
}

/// Stake this batch's claim on every `(entity, attribute, verbatim instanceId)`
/// triple it carries; the primary key refuses a duplicate, so this insert
/// carries no `ON CONFLICT`.
fn claim_verbatim_instances_sql(target: &ContextTarget) -> Result<String, TemporalStoreError> {
    let select = Query::select()
        .expr(Expr::col((S, TemporalAttributeStreams::TemporalEntityKey)))
        .expr(Expr::col((S, TemporalAttributeStreams::AttributeNameKey)))
        .expr(Expr::col((D, DedupStaged::VerbatimInstanceId)))
        .from_function(dedup_unnest(), D)
        .join_as(
            JoinType::InnerJoin,
            table_of(target, TemporalAttributeStreams::Table),
            S,
            Expr::col((S, TemporalAttributeStreams::StreamKey)).equals((D, DedupStaged::StreamKey)),
        )
        .to_owned();
    let mut insert = Query::insert();
    insert.into_table(table_of(target, AttributeHistoryInstanceClaims::Table)).columns([
        AttributeHistoryInstanceClaims::TemporalEntityKey,
        AttributeHistoryInstanceClaims::AttributeNameKey,
        AttributeHistoryInstanceClaims::VerbatimInstanceId,
    ]);
    insert.select_from(select).map_err(TemporalStoreError::StatementBuild)?;
    let sql = insert.to_string(PostgresQueryBuilder);
    Ok(apply_column_alias(sql, &D, &dedup_staging_columns()))
}

#[cfg(test)]
mod tests {
    use crate::verbatim_instance_claim::{claim_verbatim_dedup_delete_sql, claim_verbatim_instances_sql, history_verbatim_dedup_sql};
    use andromeda_context_space::context_target::ContextTarget;

    #[test]
    fn deduplication_addresses_one_identity_column_and_never_the_generated_one() {
        let sql = history_verbatim_dedup_sql(&ContextTarget::Live);
        assert!(
            sql.contains(r#""attribute_history"."verbatim_instance_id" = "d"."verbatim_instance_id""#),
            "{sql}"
        );
        assert!(!sql.contains("generated_instance_id"), "{sql}");
    }

    #[test]
    fn the_claim_dedup_delete_is_scoped_to_the_entity_and_attribute_not_the_stream() {
        let sql = claim_verbatim_dedup_delete_sql(&ContextTarget::Live);
        assert!(sql.starts_with(r#"DELETE FROM "attribute_history_instance_claims""#), "{sql}");
        assert!(
            sql.contains(r#""attribute_history_instance_claims"."temporal_entity_key" = "s1"."temporal_entity_key""#),
            "{sql}"
        );
        assert!(
            sql.contains(r#""attribute_history_instance_claims"."attribute_name_key" = "s1"."attribute_name_key""#),
            "{sql}"
        );
    }

    #[test]
    fn the_claim_insert_lets_a_duplicate_triple_fail_the_statement() {
        let sql = claim_verbatim_instances_sql(&ContextTarget::Live).expect("the fixed claim insert builds");
        assert!(sql.starts_with(r#"INSERT INTO "attribute_history_instance_claims""#), "{sql}");
        assert!(
            sql.contains(r#"("temporal_entity_key", "attribute_name_key", "verbatim_instance_id")"#),
            "{sql}"
        );
        assert!(!sql.contains("ON CONFLICT"), "{sql}");
    }

    #[test]
    fn deduplication_is_widened_to_every_sibling_stream_of_the_entity_and_attribute() {
        let sql = history_verbatim_dedup_sql(&ContextTarget::Live);
        assert!(sql.contains(r#""s2"."temporal_entity_key" = "s1"."temporal_entity_key""#), "{sql}");
        assert!(sql.contains(r#""s2"."attribute_name_key" = "s1"."attribute_name_key""#), "{sql}");
    }

    #[test]
    fn no_delete_pass_is_keyed_on_observed_at_alone() {
        assert!(!history_verbatim_dedup_sql(&ContextTarget::Live).contains("observed_at"));
    }
}
