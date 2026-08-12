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
use andromeda_ngsi_ld_lexicon::uri::Uri;
use andromeda_postgresql_codes::{attribute_kind::AttributeKind, stream_id::StreamId, value_kind::ValueKind};
use andromeda_postgresql_encoding::ewkb_binding::geometry_bytes;
use andromeda_unit_catalog::unit_code_id::UnitCodeId;
use chrono::{DateTime, Utc};
use serde_json::Value;
use sqlx::{Postgres, Transaction};
use uuid::Uuid;

impl TemporalRepository {
    /// Bulk-insert attribute-history rows in the caller's transaction.
    ///
    /// # Errors
    ///
    /// Returns [`TemporalStoreError::Validation`] when a row's position within
    /// its write exceeds the stored column's width or a geometry cannot be
    /// encoded, and [`TemporalStoreError::Database`] when a statement fails.
    pub async fn insert_history_batch_in_tx(&self, tx: &mut Transaction<'_, Postgres>, rows: &[AttributeHistoryRow]) -> Result<(), TemporalStoreError> {
        if rows.is_empty() {
            return Ok(());
        }
        let targets = verbatim_instance_targets(rows);
        run_history_dedup_deletes(tx, &targets).await?;
        claim_verbatim_instances(tx, &targets).await?;
        match self.bulk_write_mode {
            BulkWriteMode::Unnest => insert_history_batch_unnest_in_tx(tx, rows).await?,
            BulkWriteMode::CopyStaging => insert_history_batch_via_copy_in_tx(tx, rows).await?,
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
    streams: Vec<Uuid>,
    /// The verbatim identifier each targeted row carries.
    ids: Vec<&'a str>,
}

/// Collect one batch's verbatim-identified rows as parallel arrays for the
/// `UNNEST`-based statements below.
fn verbatim_instance_targets(rows: &[AttributeHistoryRow]) -> VerbatimInstanceTargets<'_> {
    let (streams, ids) = rows
        .iter()
        .filter_map(|row| verbatim_instance_id(&row.instance).map(|identifier| (row.stream_id.get(), identifier)))
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
async fn run_history_dedup_deletes(tx: &mut Transaction<'_, Postgres>, targets: &VerbatimInstanceTargets<'_>) -> Result<(), TemporalStoreError> {
    if targets.ids.is_empty() {
        return Ok(());
    }
    sqlx::query(HISTORY_VERBATIM_DEDUP_SQL)
        .bind(&targets.streams)
        .bind(&targets.ids)
        .execute(&mut **tx)
        .await?;
    sqlx::query(CLAIM_VERBATIM_DEDUP_DELETE_SQL)
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
async fn claim_verbatim_instances(tx: &mut Transaction<'_, Postgres>, targets: &VerbatimInstanceTargets<'_>) -> Result<(), TemporalStoreError> {
    if targets.ids.is_empty() {
        return Ok(());
    }
    sqlx::query(CLAIM_VERBATIM_INSTANCES_SQL)
        .bind(&targets.streams)
        .bind(&targets.ids)
        .execute(&mut **tx)
        .await
        .map_err(claim_write_failure)?;
    Ok(())
}

const HISTORY_VERBATIM_DEDUP_SQL: &str = "DELETE FROM attribute_history ah \
     USING UNNEST($1::uuid[], $2::text[]) AS d(stream_id, verbatim_instance_id) \
     JOIN temporal_attribute_streams s1 ON s1.id = d.stream_id \
     WHERE ah.verbatim_instance_id = d.verbatim_instance_id \
       AND ah.stream_id IN ( \
           SELECT s2.id FROM temporal_attribute_streams s2 \
           WHERE s2.temporal_entity_key = s1.temporal_entity_key AND s2.attribute_name_id = s1.attribute_name_id \
       )";

const CLAIM_VERBATIM_DEDUP_DELETE_SQL: &str = "DELETE FROM attribute_history_instance_claims c \
     USING UNNEST($1::uuid[], $2::text[]) AS d(stream_id, verbatim_instance_id) \
     JOIN temporal_attribute_streams s1 ON s1.id = d.stream_id \
     WHERE c.verbatim_instance_id = d.verbatim_instance_id \
       AND c.temporal_entity_key = s1.temporal_entity_key \
       AND c.attribute_name_id = s1.attribute_name_id";

const CLAIM_VERBATIM_INSTANCES_SQL: &str = "INSERT INTO attribute_history_instance_claims \
     (temporal_entity_key, attribute_name_id, verbatim_instance_id) \
     SELECT s.temporal_entity_key, s.attribute_name_id, d.verbatim_instance_id \
     FROM UNNEST($1::uuid[], $2::text[]) AS d(stream_id, verbatim_instance_id) \
     JOIN temporal_attribute_streams s ON s.id = d.stream_id";

/// Column-major binds for one attribute-history batch.
struct AttributeHistoryBinds<'a> {
    /// Write clock of each row, and the hypertable's partition column.
    recorded_ats: Vec<DateTime<Utc>>,
    /// Atomic write each row belongs to.
    write_ids: Vec<Uuid>,
    /// Stream each row extends.
    stream_ids: Vec<Uuid>,
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
    /// Arbitrary-precision values as the decimal literals `NUMERIC` parses.
    value_numbers: Vec<Option<String>>,
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
        stream_ids: Vec::with_capacity(rows.len()),
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
        binds.stream_ids.push(StreamId::get(row.stream_id));
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
        binds.value_numbers.push(row.value.numeric_literal());
        binds.value_strings.push(row.value.string());
        binds.value_geos.push(geometry_bytes(row.value.geometry())?);
        binds.value_objects.push(row.value.object());
        binds.value_uris.push(row.value.uri().map(Uri::as_str));
        binds.metadatas.push(row.metadata.as_ref());
    }
    Ok(binds)
}

const ATTRIBUTE_HISTORY_INSERT_SQL: &str = r#"
INSERT INTO attribute_history (
    recorded_at, write_id, stream_id, write_ordinal, attribute_kind, value_kind,
    observed_at, created_at, modified_at, deleted_at, value_datetime,
    generated_instance_uuid, unit_code_id, value_bool, verbatim_instance_id,
    value_number, value_string, value_geo, value_object, value_uri, metadata
)
SELECT d.recorded_at, d.write_id, d.stream_id, d.write_ordinal, d.attribute_kind, d.value_kind,
       d.observed_at, d.created_at, d.modified_at, d.deleted_at, d.value_datetime,
       d.generated_instance_uuid, d.unit_code_id, d.value_bool, d.verbatim_instance_id,
       d.value_number::numeric, d.value_string,
       CASE WHEN d.value_geo IS NOT NULL THEN ST_MakeValid(ST_GeomFromEWKB(d.value_geo)) ELSE NULL END,
       d.value_object, d.value_uri, d.metadata
FROM UNNEST(
    $1::timestamptz[], $2::uuid[], $3::uuid[], $4::integer[], $5::smallint[], $6::smallint[],
    $7::timestamptz[], $8::timestamptz[], $9::timestamptz[], $10::timestamptz[], $11::timestamptz[],
    $12::uuid[], $13::smallint[], $14::boolean[], $15::text[],
    $16::text[], $17::text[], $18::bytea[], $19::jsonb[], $20::text[], $21::jsonb[]
) AS d(recorded_at, write_id, stream_id, write_ordinal, attribute_kind, value_kind,
       observed_at, created_at, modified_at, deleted_at, value_datetime,
       generated_instance_uuid, unit_code_id, value_bool, verbatim_instance_id,
       value_number, value_string, value_geo, value_object, value_uri, metadata)
ON CONFLICT ON CONSTRAINT uq_attribute_history_write DO NOTHING
"#;

/// Insert one batch by binding parallel arrays and expanding them with `UNNEST`.
async fn insert_history_batch_unnest_in_tx(tx: &mut Transaction<'_, Postgres>, rows: &[AttributeHistoryRow]) -> Result<(), TemporalStoreError> {
    let binds = attribute_history_binds(rows)?;
    sqlx::query(ATTRIBUTE_HISTORY_INSERT_SQL)
        .bind(&binds.recorded_ats)
        .bind(&binds.write_ids)
        .bind(&binds.stream_ids)
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
        .execute(&mut **tx)
        .await?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use crate::attribute_history_write::{
        ATTRIBUTE_HISTORY_INSERT_SQL,
        CLAIM_VERBATIM_DEDUP_DELETE_SQL,
        CLAIM_VERBATIM_INSTANCES_SQL,
        HISTORY_VERBATIM_DEDUP_SQL,
    };

    #[test]
    fn deduplication_addresses_one_identity_column_and_never_the_generated_one() {
        assert!(HISTORY_VERBATIM_DEDUP_SQL.contains("ah.verbatim_instance_id = d.verbatim_instance_id"));
        assert!(!HISTORY_VERBATIM_DEDUP_SQL.contains("generated_instance_uuid"));
    }

    /// The claim table is keyed by entity and attribute, not by stream, so its
    /// dedup delete must resolve the incoming stream to that scope through the
    /// same catalogue join the history dedup delete uses -- not by stream
    /// identity, which would only free a claim on the exact stream being
    /// rewritten and leave a sibling-stream claim stuck.
    #[test]
    fn the_claim_dedup_delete_is_scoped_to_the_entity_and_attribute_not_the_stream() {
        assert!(CLAIM_VERBATIM_DEDUP_DELETE_SQL.contains("DELETE FROM attribute_history_instance_claims"));
        assert!(CLAIM_VERBATIM_DEDUP_DELETE_SQL.contains("c.temporal_entity_key = s1.temporal_entity_key"));
        assert!(CLAIM_VERBATIM_DEDUP_DELETE_SQL.contains("c.attribute_name_id = s1.attribute_name_id"));
        assert!(!CLAIM_VERBATIM_DEDUP_DELETE_SQL.contains("c.stream_id"));
    }

    /// The claim insert carries no `ON CONFLICT`: a duplicate triple, whether
    /// from two rows in the same batch or from a concurrent batch, must reach
    /// `attribute_history_instance_claims`'s primary key and fail the
    /// statement, not be silently absorbed.
    #[test]
    fn the_claim_insert_lets_a_duplicate_triple_fail_the_statement() {
        assert!(CLAIM_VERBATIM_INSTANCES_SQL.contains("INSERT INTO attribute_history_instance_claims"));
        assert!(CLAIM_VERBATIM_INSTANCES_SQL.contains("temporal_entity_key, attribute_name_id, verbatim_instance_id"));
        assert!(!CLAIM_VERBATIM_INSTANCES_SQL.contains("ON CONFLICT"));
    }

    /// Clause 4.5.7 makes `instanceId` unique to one attribute instance, not
    /// to one dataset stream of it, so the delete must reach every sibling
    /// stream of the incoming row's `(entity, attribute)` rather than only
    /// the stream it targets — the same scope the instance PATCH and DELETE
    /// lookups already search (`temporal_attribute_streams` joined on
    /// `temporal_entity_key` and `attribute_name_id`, not on `stream_id`
    /// alone).
    #[test]
    fn deduplication_is_widened_to_every_sibling_stream_of_the_entity_and_attribute() {
        assert!(HISTORY_VERBATIM_DEDUP_SQL.contains("s2.temporal_entity_key = s1.temporal_entity_key"));
        assert!(HISTORY_VERBATIM_DEDUP_SQL.contains("s2.attribute_name_id = s1.attribute_name_id"));
        assert!(!HISTORY_VERBATIM_DEDUP_SQL.contains("ah.stream_id = d.stream_id"));
    }

    /// A row-level delete keyed only on `observedAt` cannot tell a replay of
    /// its own write apart from a different write that legitimately shares an
    /// instant on the same stream, so it must not exist alongside row-level
    /// `ON CONFLICT ... DO NOTHING` idempotency: only the verbatim-instance
    /// pass, which is keyed on a client-stable identity, is left running
    /// before the insert.
    #[test]
    fn no_delete_pass_is_keyed_on_observed_at_alone() {
        assert!(!HISTORY_VERBATIM_DEDUP_SQL.contains("observed_at"));
    }

    #[test]
    fn the_insert_carries_the_partition_column_and_the_whole_write_identity() {
        assert!(ATTRIBUTE_HISTORY_INSERT_SQL.contains("recorded_at, write_id, stream_id, write_ordinal"));
    }

    /// A commit replayed under its original write identity re-inserts every
    /// row it already wrote; the row-level conflict clause is what makes that
    /// second insert a no-op instead of a duplicate or an error.
    #[test]
    fn a_row_replayed_under_its_original_write_identity_is_not_duplicated() {
        assert!(ATTRIBUTE_HISTORY_INSERT_SQL.contains("ON CONFLICT ON CONSTRAINT uq_attribute_history_write DO NOTHING"));
    }

    #[test]
    fn the_numeric_value_reaches_storage_as_a_decimal_literal_rather_than_a_double() {
        assert!(ATTRIBUTE_HISTORY_INSERT_SQL.contains("d.value_number::numeric"));
        assert!(!ATTRIBUTE_HISTORY_INSERT_SQL.contains("double precision"));
    }
}
