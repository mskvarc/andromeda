//! Bulk inserts into the rebuilt `attribute_history` hypertable.

use crate::{
    attribute_history_row::AttributeHistoryRow,
    bulk_write_mode::BulkWriteMode,
    copy::insert_history_batch_via_copy_in_tx,
    duplicate_instance_claim::claim_write_failure,
    error::TemporalStoreError,
    repo::TemporalRepository,
    stored_instance_columns::{generated_instance_uuid, verbatim_instance_id},
};
use andromeda_context_space::context_target::ContextTarget;
use andromeda_ngsi_ld_lexicon::uri::Uri;
use andromeda_postgresql_codes::{attribute_kind::AttributeKind, stream_key::StreamKey, value_kind::ValueKind};
use andromeda_postgresql_encoding::ewkb_binding::geometry_bytes;
use andromeda_postgresql_schema::temporal::{AttributeHistory, AttributeHistoryInstanceClaims, TemporalAttributeStreams};
use andromeda_postgresql_seaquery::{
    pg_type::PostgresType,
    placeholder::external_placeholder,
    postgis::validated_geom_from_ewkb,
    unnest::{UnnestColumn, apply_column_alias, unnest_call},
};
use andromeda_telemetry::statement::{StatementName, observe_statement};
use andromeda_unit_catalog::unit_code_id::UnitCodeId;
use bigdecimal::BigDecimal;
use chrono::{DateTime, Utc};
use sea_query::{CaseStatement, Expr, ExprTrait, Iden, JoinType, Keyword, OnConflict, PostgresQueryBuilder, Query, SimpleExpr};
use serde_json::Value;
use sqlx::{AssertSqlSafe, Postgres, Transaction};
use uuid::Uuid;

/// The statement name the history batch insert is observed under, in either
/// bulk-write mode.
const HISTORY_INSERT: StatementName = StatementName("attribute-history-insert");

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

/// The staging columns of an attribute-history row, named as the `AS d(…)` alias
/// spells them rather than as the `attribute_history` columns they land in.
#[derive(Iden, Clone, Copy)]
enum HistoryStaged {
    /// Write clock of the row.
    RecordedAt,
    /// Atomic write the row belongs to.
    WriteId,
    /// Stream the row extends.
    StreamKey,
    /// Position of the row within its atomic write.
    WriteOrdinal,
    /// Attribute family the row was written as.
    AttributeKind,
    /// Stored value shape of the row.
    ValueKind,
    /// Client-declared observation instant.
    ObservedAt,
    /// Instant the instance was first recorded at.
    CreatedAt,
    /// Instant the instance was last altered at.
    ModifiedAt,
    /// Deletion instant, present only on a tombstone.
    DeletedAt,
    /// Timestamp value.
    ValueDatetime,
    /// Broker-minted instance identifier.
    GeneratedInstanceUuid,
    /// Catalog identity of the instance's unit.
    UnitCodeId,
    /// Boolean value.
    ValueBool,
    /// Instance identifier stored exactly as received.
    VerbatimInstanceId,
    /// Arbitrary-precision value bound directly as `NUMERIC`.
    ValueNumber,
    /// String value.
    ValueString,
    /// Geometry value as EWKB.
    ValueGeo,
    /// Structured value.
    ValueObject,
    /// URI value.
    ValueUri,
    /// Retained expanded members.
    Metadata,
}

impl TemporalRepository {
    /// Bulk-insert attribute-history rows in the caller's transaction.
    ///
    /// # Errors
    ///
    /// Returns [`TemporalStoreError::Validation`] when a row's position within
    /// its write exceeds the stored column's width or a geometry cannot be
    /// encoded, and [`TemporalStoreError::Database`] when a statement fails.
    pub async fn insert_history_batch_in_tx(
        &self,
        tx: &mut Transaction<'_, Postgres>,
        target: &ContextTarget,
        rows: &[AttributeHistoryRow],
    ) -> Result<(), TemporalStoreError> {
        if rows.is_empty() {
            return Ok(());
        }
        let targets = verbatim_instance_targets(rows);
        run_history_dedup_deletes(tx, target, &targets).await?;
        claim_verbatim_instances(tx, target, &targets).await?;
        match self.bulk_write_mode {
            BulkWriteMode::Unnest => observe_statement(HISTORY_INSERT, insert_history_batch_unnest_in_tx(tx, target, rows)).await?,
            BulkWriteMode::CopyStaging => observe_statement(HISTORY_INSERT, insert_history_batch_via_copy_in_tx(tx, target, rows)).await?,
        }
        Ok(())
    }
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
struct VerbatimInstanceTargets<'a> {
    /// The stream each targeted row extends.
    streams: Vec<i64>,
    /// The verbatim identifier each targeted row carries.
    ids: Vec<&'a str>,
}

/// Collect one batch's verbatim-identified rows as parallel arrays for the
/// `UNNEST`-based statements below.
fn verbatim_instance_targets(rows: &[AttributeHistoryRow]) -> VerbatimInstanceTargets<'_> {
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
/// row-level (`uq_attribute_history_write` plus the verbatim-instance pass
/// above), the append is reached again on every replay of a durable intent,
/// and an `observedAt`-keyed delete cannot distinguish "this is my own row
/// from a previous attempt" from "this is a different write's row that
/// happens to share an instant" -- a retried intent append or a crash-retried
/// side-effect replay would delete a later, unrelated write's history row.
/// The pass is removed rather than scoped, because scoping it to the current
/// write would silently drop the replace-on-resend behaviour it existed for
/// while keeping the same footgun for any two writes that legitimately share
/// an `observedAt` on one stream.
async fn run_history_dedup_deletes(
    tx: &mut Transaction<'_, Postgres>,
    target: &ContextTarget,
    targets: &VerbatimInstanceTargets<'_>,
) -> Result<(), TemporalStoreError> {
    if targets.ids.is_empty() {
        return Ok(());
    }
    let history_dedup_sql = history_verbatim_dedup_sql();
    sqlx::query(AssertSqlSafe(history_dedup_sql))
        .bind(&targets.streams)
        .bind(&targets.ids)
        .bind(target.key().as_i64())
        .execute(&mut **tx)
        .await?;
    let claim_dedup_sql = claim_verbatim_dedup_delete_sql();
    sqlx::query(AssertSqlSafe(claim_dedup_sql))
        .bind(&targets.streams)
        .bind(&targets.ids)
        .bind(target.key().as_i64())
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
async fn claim_verbatim_instances(
    tx: &mut Transaction<'_, Postgres>,
    target: &ContextTarget,
    targets: &VerbatimInstanceTargets<'_>,
) -> Result<(), TemporalStoreError> {
    if targets.ids.is_empty() {
        return Ok(());
    }
    let claim_sql = claim_verbatim_instances_sql()?;
    sqlx::query(AssertSqlSafe(claim_sql))
        .bind(&targets.streams)
        .bind(&targets.ids)
        .bind(target.key().as_i64())
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
fn history_verbatim_dedup_sql() -> String {
    let sibling_streams = Query::select()
        .column((S2, TemporalAttributeStreams::StreamKey))
        .from_as(TemporalAttributeStreams::Table, S2)
        .and_where(Expr::col((S2, TemporalAttributeStreams::TemporalEntityKey)).equals((S1, TemporalAttributeStreams::TemporalEntityKey)))
        .and_where(Expr::col((S2, TemporalAttributeStreams::AttributeNameId)).equals((S1, TemporalAttributeStreams::AttributeNameId)))
        .and_where(Expr::col((S2, TemporalAttributeStreams::ContextSpaceKey)).eq(external_placeholder(3)))
        .to_owned();
    let matches = Query::select()
        .expr(Expr::val(1))
        .from_function(dedup_unnest(), D)
        .join_as(
            JoinType::InnerJoin,
            TemporalAttributeStreams::Table,
            S1,
            Expr::col((S1, TemporalAttributeStreams::StreamKey)).equals((D, DedupStaged::StreamKey)),
        )
        .and_where(Expr::col((S1, TemporalAttributeStreams::ContextSpaceKey)).eq(external_placeholder(3)))
        .and_where(Expr::col((AttributeHistory::Table, AttributeHistory::ContextSpaceKey)).eq(external_placeholder(3)))
        .and_where(Expr::col((AttributeHistory::Table, AttributeHistory::VerbatimInstanceId)).equals((D, DedupStaged::VerbatimInstanceId)))
        .and_where(Expr::col((AttributeHistory::Table, AttributeHistory::StreamKey)).in_subquery(sibling_streams))
        .to_owned();
    let sql = Query::delete()
        .from_table(AttributeHistory::Table)
        .and_where(Expr::exists(matches))
        .to_owned()
        .to_string(PostgresQueryBuilder);
    apply_column_alias(sql, &D, &dedup_staging_columns())
}

/// Release the claims the same batch is about to re-stake, scoped to the
/// `(entity, attribute)` the claim table is keyed by rather than to the stream.
fn claim_verbatim_dedup_delete_sql() -> String {
    let matches = Query::select()
        .expr(Expr::val(1))
        .from_function(dedup_unnest(), D)
        .join_as(
            JoinType::InnerJoin,
            TemporalAttributeStreams::Table,
            S1,
            Expr::col((S1, TemporalAttributeStreams::StreamKey)).equals((D, DedupStaged::StreamKey)),
        )
        .and_where(Expr::col((S1, TemporalAttributeStreams::ContextSpaceKey)).eq(external_placeholder(3)))
        .and_where(Expr::col((AttributeHistoryInstanceClaims::Table, AttributeHistoryInstanceClaims::ContextSpaceKey)).eq(external_placeholder(3)))
        .and_where(
            Expr::col((AttributeHistoryInstanceClaims::Table, AttributeHistoryInstanceClaims::VerbatimInstanceId)).equals((D, DedupStaged::VerbatimInstanceId)),
        )
        .and_where(
            Expr::col((AttributeHistoryInstanceClaims::Table, AttributeHistoryInstanceClaims::TemporalEntityKey))
                .equals((S1, TemporalAttributeStreams::TemporalEntityKey)),
        )
        .and_where(
            Expr::col((AttributeHistoryInstanceClaims::Table, AttributeHistoryInstanceClaims::AttributeNameId))
                .equals((S1, TemporalAttributeStreams::AttributeNameId)),
        )
        .to_owned();
    let sql = Query::delete()
        .from_table(AttributeHistoryInstanceClaims::Table)
        .and_where(Expr::exists(matches))
        .to_owned()
        .to_string(PostgresQueryBuilder);
    apply_column_alias(sql, &D, &dedup_staging_columns())
}

/// Stake this batch's claim on every `(entity, attribute, verbatim instanceId)`
/// triple it carries; the primary key refuses a duplicate, so this insert
/// carries no `ON CONFLICT`.
fn claim_verbatim_instances_sql() -> Result<String, TemporalStoreError> {
    let select = Query::select()
        .expr(external_placeholder(3))
        .expr(Expr::col((S, TemporalAttributeStreams::TemporalEntityKey)))
        .expr(Expr::col((S, TemporalAttributeStreams::AttributeNameId)))
        .expr(Expr::col((D, DedupStaged::VerbatimInstanceId)))
        .from_function(dedup_unnest(), D)
        .join_as(
            JoinType::InnerJoin,
            TemporalAttributeStreams::Table,
            S,
            Expr::col((S, TemporalAttributeStreams::StreamKey)).equals((D, DedupStaged::StreamKey)),
        )
        .and_where(Expr::col((S, TemporalAttributeStreams::ContextSpaceKey)).eq(external_placeholder(3)))
        .to_owned();
    let mut insert = Query::insert();
    insert.into_table(AttributeHistoryInstanceClaims::Table).columns([
        AttributeHistoryInstanceClaims::ContextSpaceKey,
        AttributeHistoryInstanceClaims::TemporalEntityKey,
        AttributeHistoryInstanceClaims::AttributeNameId,
        AttributeHistoryInstanceClaims::VerbatimInstanceId,
    ]);
    insert.select_from(select).map_err(|error| TemporalStoreError::Validation(error.to_string()))?;
    let sql = insert.to_string(PostgresQueryBuilder);
    Ok(apply_column_alias(sql, &D, &dedup_staging_columns()))
}

/// Column-major binds for one attribute-history batch.
struct AttributeHistoryBinds<'a> {
    /// Write clock of each row, and the hypertable's partition column.
    recorded_ats: Vec<DateTime<Utc>>,
    /// Atomic write each row belongs to.
    write_ids: Vec<Uuid>,
    /// Stream each row extends.
    stream_keys: Vec<i64>,
    /// Position of each row within its atomic write.
    write_ordinals: Vec<i32>,
    /// Attribute family each row was written as.
    attribute_kinds: Vec<i16>,
    /// Stored value shape of each row.
    value_kinds: Vec<i16>,
    /// Client-declared observation instants.
    observed_ats: Vec<Option<DateTime<Utc>>>,
    /// Instants the instances were first recorded at.
    created_ats: Vec<Option<DateTime<Utc>>>,
    /// Instants the instances were last altered at.
    modified_ats: Vec<Option<DateTime<Utc>>>,
    /// Deletion instants, present only on tombstones.
    deleted_ats: Vec<Option<DateTime<Utc>>>,
    /// Timestamp values.
    value_datetimes: Vec<Option<DateTime<Utc>>>,
    /// Broker-minted instance identifiers.
    generated_instance_uuids: Vec<Option<Uuid>>,
    /// Catalog identities of the instances' units.
    unit_code_ids: Vec<Option<i16>>,
    /// Boolean values.
    value_bools: Vec<Option<bool>>,
    /// Instance identifiers stored exactly as received.
    verbatim_instance_ids: Vec<Option<&'a str>>,
    /// Arbitrary-precision values bound directly as `numeric[]`.
    value_numbers: Vec<Option<BigDecimal>>,
    /// String values.
    value_strings: Vec<Option<&'a str>>,
    /// Geometry values as EWKB, projected server-side.
    value_geos: Vec<Option<Vec<u8>>>,
    /// Structured values.
    value_objects: Vec<Option<&'a Value>>,
    /// URI values, which is how a relationship object is stored.
    value_uris: Vec<Option<&'a str>>,
    /// Retained expanded members.
    metadatas: Vec<Option<&'a Value>>,
}

/// Spread one attribute-history batch into its column-major binds.
fn attribute_history_binds<'a>(rows: &'a [AttributeHistoryRow]) -> Result<AttributeHistoryBinds<'a>, TemporalStoreError> {
    let mut binds = AttributeHistoryBinds {
        recorded_ats: Vec::with_capacity(rows.len()),
        write_ids: Vec::with_capacity(rows.len()),
        stream_keys: Vec::with_capacity(rows.len()),
        write_ordinals: Vec::with_capacity(rows.len()),
        attribute_kinds: Vec::with_capacity(rows.len()),
        value_kinds: Vec::with_capacity(rows.len()),
        observed_ats: Vec::with_capacity(rows.len()),
        created_ats: Vec::with_capacity(rows.len()),
        modified_ats: Vec::with_capacity(rows.len()),
        deleted_ats: Vec::with_capacity(rows.len()),
        value_datetimes: Vec::with_capacity(rows.len()),
        generated_instance_uuids: Vec::with_capacity(rows.len()),
        unit_code_ids: Vec::with_capacity(rows.len()),
        value_bools: Vec::with_capacity(rows.len()),
        verbatim_instance_ids: Vec::with_capacity(rows.len()),
        value_numbers: Vec::with_capacity(rows.len()),
        value_strings: Vec::with_capacity(rows.len()),
        value_geos: Vec::with_capacity(rows.len()),
        value_objects: Vec::with_capacity(rows.len()),
        value_uris: Vec::with_capacity(rows.len()),
        metadatas: Vec::with_capacity(rows.len()),
    };
    for row in rows {
        binds.recorded_ats.push(row.recorded_at);
        binds.write_ids.push(row.write_id());
        binds.stream_keys.push(StreamKey::get(row.stream_key));
        binds.write_ordinals.push(row.write_ordinal()?);
        binds.attribute_kinds.push(AttributeKind::code(row.attribute_kind));
        binds.value_kinds.push(ValueKind::code(row.value.kind()));
        binds.observed_ats.push(row.axes.observed_at);
        binds.created_ats.push(row.axes.created_at);
        binds.modified_ats.push(row.axes.modified_at);
        binds.deleted_ats.push(row.value.deleted_at());
        binds.value_datetimes.push(row.value.date_time());
        binds.generated_instance_uuids.push(generated_instance_uuid(&row.instance));
        binds.unit_code_ids.push(row.unit_code_id.map(UnitCodeId::get));
        binds.value_bools.push(row.value.boolean());
        binds.verbatim_instance_ids.push(verbatim_instance_id(&row.instance));
        // SQLx owns the array values. Clone the decimal itself so the bind is
        // exact and never passes through text or binary64.
        binds.value_numbers.push(row.value.number().cloned());
        binds.value_strings.push(row.value.string());
        binds.value_geos.push(geometry_bytes(row.value.geometry())?);
        binds.value_objects.push(row.value.object());
        binds.value_uris.push(row.value.uri().map(Uri::as_str));
        binds.metadatas.push(row.metadata.as_ref());
    }
    Ok(binds)
}

/// The twenty-one staging columns of an attribute-history row, in bind order.
fn history_staging_columns() -> [UnnestColumn; 21] {
    [
        UnnestColumn::new(HistoryStaged::RecordedAt, PostgresType::TimestamptzArray),
        UnnestColumn::new(HistoryStaged::WriteId, PostgresType::UuidArray),
        UnnestColumn::new(HistoryStaged::StreamKey, PostgresType::BigintArray),
        UnnestColumn::new(HistoryStaged::WriteOrdinal, PostgresType::IntegerArray),
        UnnestColumn::new(HistoryStaged::AttributeKind, PostgresType::SmallintArray),
        UnnestColumn::new(HistoryStaged::ValueKind, PostgresType::SmallintArray),
        UnnestColumn::new(HistoryStaged::ObservedAt, PostgresType::TimestamptzArray),
        UnnestColumn::new(HistoryStaged::CreatedAt, PostgresType::TimestamptzArray),
        UnnestColumn::new(HistoryStaged::ModifiedAt, PostgresType::TimestamptzArray),
        UnnestColumn::new(HistoryStaged::DeletedAt, PostgresType::TimestamptzArray),
        UnnestColumn::new(HistoryStaged::ValueDatetime, PostgresType::TimestamptzArray),
        UnnestColumn::new(HistoryStaged::GeneratedInstanceUuid, PostgresType::UuidArray),
        UnnestColumn::new(HistoryStaged::UnitCodeId, PostgresType::SmallintArray),
        UnnestColumn::new(HistoryStaged::ValueBool, PostgresType::BooleanArray),
        UnnestColumn::new(HistoryStaged::VerbatimInstanceId, PostgresType::TextArray),
        UnnestColumn::new(HistoryStaged::ValueNumber, PostgresType::NumericArray),
        UnnestColumn::new(HistoryStaged::ValueString, PostgresType::TextArray),
        UnnestColumn::new(HistoryStaged::ValueGeo, PostgresType::ByteaArray),
        UnnestColumn::new(HistoryStaged::ValueObject, PostgresType::JsonbArray),
        UnnestColumn::new(HistoryStaged::ValueUri, PostgresType::TextArray),
        UnnestColumn::new(HistoryStaged::Metadata, PostgresType::JsonbArray),
    ]
}

/// A staging column reference, `d.<column>`.
fn staged(column: HistoryStaged) -> SimpleExpr {
    Expr::col((D, column))
}

/// The bulk attribute-history insert.
///
/// The value number reaches the arbitrary-precision column through a native
/// `numeric[]` bind, never widened to a double; the geometry is
/// validated from its embedded-SRID EWKB, with a NULL EWKB projecting to a NULL
/// geometry. The conflict target names the columns `uq_attribute_history_write`
/// covers, so a row replayed under its original write identity is a no-op.
fn attribute_history_insert_sql() -> Result<String, TemporalStoreError> {
    let columns = history_staging_columns();
    let call = unnest_call(columns.iter().enumerate().map(|(index, column)| (column, external_placeholder(index + 1))));

    let geometry: SimpleExpr = CaseStatement::new()
        .case(
            Expr::col((D, HistoryStaged::ValueGeo)).is_not_null(),
            validated_geom_from_ewkb(staged(HistoryStaged::ValueGeo)),
        )
        .finally(Keyword::Null)
        .into();

    let mut select = Query::select();
    select
        .expr(staged(HistoryStaged::RecordedAt))
        .expr(external_placeholder(22))
        .expr(staged(HistoryStaged::WriteId))
        .expr(staged(HistoryStaged::StreamKey))
        .expr(staged(HistoryStaged::WriteOrdinal))
        .expr(staged(HistoryStaged::AttributeKind))
        .expr(staged(HistoryStaged::ValueKind))
        .expr(staged(HistoryStaged::ObservedAt))
        .expr(staged(HistoryStaged::CreatedAt))
        .expr(staged(HistoryStaged::ModifiedAt))
        .expr(staged(HistoryStaged::DeletedAt))
        .expr(staged(HistoryStaged::ValueDatetime))
        .expr(staged(HistoryStaged::GeneratedInstanceUuid))
        .expr(staged(HistoryStaged::UnitCodeId))
        .expr(staged(HistoryStaged::ValueBool))
        .expr(staged(HistoryStaged::VerbatimInstanceId))
        .expr(staged(HistoryStaged::ValueNumber))
        .expr(staged(HistoryStaged::ValueString))
        .expr(geometry)
        .expr(staged(HistoryStaged::ValueObject))
        .expr(staged(HistoryStaged::ValueUri))
        .expr(staged(HistoryStaged::Metadata))
        .from_function(call, D);
    let select = select;

    let mut insert = Query::insert();
    insert.into_table(AttributeHistory::Table).columns([
        AttributeHistory::RecordedAt,
        AttributeHistory::ContextSpaceKey,
        AttributeHistory::WriteId,
        AttributeHistory::StreamKey,
        AttributeHistory::WriteOrdinal,
        AttributeHistory::AttributeKind,
        AttributeHistory::ValueKind,
        AttributeHistory::ObservedAt,
        AttributeHistory::CreatedAt,
        AttributeHistory::ModifiedAt,
        AttributeHistory::DeletedAt,
        AttributeHistory::ValueDatetime,
        AttributeHistory::GeneratedInstanceUuid,
        AttributeHistory::UnitCodeId,
        AttributeHistory::ValueBool,
        AttributeHistory::VerbatimInstanceId,
        AttributeHistory::ValueNumber,
        AttributeHistory::ValueString,
        AttributeHistory::ValueGeo,
        AttributeHistory::ValueObject,
        AttributeHistory::ValueUri,
        AttributeHistory::Metadata,
    ]);
    insert
        .select_from(select)
        .map_err(|error| TemporalStoreError::Validation(error.to_string()))?
        .on_conflict(
            OnConflict::columns([
                AttributeHistory::RecordedAt,
                AttributeHistory::ContextSpaceKey,
                AttributeHistory::WriteId,
                AttributeHistory::WriteOrdinal,
            ])
            .do_nothing()
            .to_owned(),
        );

    let sql = insert.to_string(PostgresQueryBuilder);
    Ok(apply_column_alias(sql, &D, &columns))
}

/// Insert one batch by binding parallel arrays and expanding them with `UNNEST`.
async fn insert_history_batch_unnest_in_tx(
    tx: &mut Transaction<'_, Postgres>,
    target: &ContextTarget,
    rows: &[AttributeHistoryRow],
) -> Result<(), TemporalStoreError> {
    let binds = attribute_history_binds(rows)?;
    let sql = attribute_history_insert_sql()?;
    sqlx::query(AssertSqlSafe(sql))
        .bind(&binds.recorded_ats)
        .bind(&binds.write_ids)
        .bind(&binds.stream_keys)
        .bind(&binds.write_ordinals)
        .bind(&binds.attribute_kinds)
        .bind(&binds.value_kinds)
        .bind(&binds.observed_ats)
        .bind(&binds.created_ats)
        .bind(&binds.modified_ats)
        .bind(&binds.deleted_ats)
        .bind(&binds.value_datetimes)
        .bind(&binds.generated_instance_uuids)
        .bind(&binds.unit_code_ids)
        .bind(&binds.value_bools)
        .bind(&binds.verbatim_instance_ids)
        .bind(&binds.value_numbers)
        .bind(&binds.value_strings)
        .bind(&binds.value_geos)
        .bind(&binds.value_objects)
        .bind(&binds.value_uris)
        .bind(&binds.metadatas)
        .bind(target.key().as_i64())
        .execute(&mut **tx)
        .await?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use crate::attribute_history_write::{
        attribute_history_insert_sql,
        claim_verbatim_dedup_delete_sql,
        claim_verbatim_instances_sql,
        history_verbatim_dedup_sql,
    };

    #[test]
    fn deduplication_addresses_one_identity_column_and_never_the_generated_one() {
        let sql = history_verbatim_dedup_sql();
        assert!(
            sql.contains(r#""attribute_history"."verbatim_instance_id" = "d"."verbatim_instance_id""#),
            "{sql}"
        );
        assert!(!sql.contains("generated_instance_uuid"), "{sql}");
    }

    #[test]
    fn the_claim_dedup_delete_is_scoped_to_the_entity_and_attribute_not_the_stream() {
        let sql = claim_verbatim_dedup_delete_sql();
        assert!(sql.starts_with(r#"DELETE FROM "attribute_history_instance_claims""#), "{sql}");
        assert!(
            sql.contains(r#""attribute_history_instance_claims"."temporal_entity_key" = "s1"."temporal_entity_key""#),
            "{sql}"
        );
        assert!(
            sql.contains(r#""attribute_history_instance_claims"."attribute_name_id" = "s1"."attribute_name_id""#),
            "{sql}"
        );
    }

    #[test]
    fn the_claim_insert_lets_a_duplicate_triple_fail_the_statement() {
        let sql = claim_verbatim_instances_sql().expect("the fixed claim insert builds");
        assert!(sql.starts_with(r#"INSERT INTO "attribute_history_instance_claims""#), "{sql}");
        assert!(sql.contains(r#"("temporal_entity_key", "attribute_name_id", "verbatim_instance_id")"#), "{sql}");
        assert!(!sql.contains("ON CONFLICT"), "{sql}");
    }

    #[test]
    fn deduplication_is_widened_to_every_sibling_stream_of_the_entity_and_attribute() {
        let sql = history_verbatim_dedup_sql();
        assert!(sql.contains(r#""s2"."temporal_entity_key" = "s1"."temporal_entity_key""#), "{sql}");
        assert!(sql.contains(r#""s2"."attribute_name_id" = "s1"."attribute_name_id""#), "{sql}");
    }

    #[test]
    fn no_delete_pass_is_keyed_on_observed_at_alone() {
        assert!(!history_verbatim_dedup_sql().contains("observed_at"));
    }

    #[test]
    fn the_insert_carries_the_partition_column_and_the_whole_write_identity() {
        let sql = attribute_history_insert_sql().expect("the fixed attribute-history insert builds");
        assert!(sql.contains(r#"("recorded_at", "write_id", "stream_key", "write_ordinal""#), "{sql}");
    }

    #[test]
    fn a_row_replayed_under_its_original_write_identity_is_not_duplicated() {
        let sql = attribute_history_insert_sql().expect("the fixed attribute-history insert builds");
        assert!(sql.contains(r#"ON CONFLICT ("recorded_at", "write_id", "write_ordinal") DO NOTHING"#), "{sql}");
    }

    #[test]
    fn the_numeric_value_reaches_storage_through_a_numeric_array_bind() {
        let sql = attribute_history_insert_sql().expect("the fixed attribute-history insert builds");
        assert!(sql.contains(r#"$16::numeric[]"#), "{sql}");
        assert!(!sql.contains(r#""d"."value_number"::numeric"#), "{sql}");
        assert!(!sql.contains("double precision"), "{sql}");
    }
}
