//! The statements that stage one entity's local and answered history in
//! transaction-scoped temporary copies of the temporal relations.
//!
//! The copies carry the names of the relations they copy, so the aggregation
//! statement reads them unchanged once it is pointed at the temporary schema;
//! every statement that reads the branch therefore names the branch schema,
//! the live one included, because an unqualified name now resolves to the
//! temporary copy.

use andromeda_ngsi_ld_lexicon::{identifier::Identifier, time_prop::TimeProp};
use andromeda_ngsi_ld_query::temporal::window::TimeInterval;
use andromeda_postgresql_query::time_interval_predicate::within_interval;
use andromeda_postgresql_schema::{
    name_dictionary::AttributeNames,
    temporal::{AttributeHistory, TemporalAttributeStreams, TemporalEntities},
};
use andromeda_postgresql_seaquery::{
    branch::qualified,
    pg_operator::PgBinaryOperator,
    temporary_table::{TemporarySchema, temporary_copy_of},
};
use andromeda_postgresql_temporal_query::axis::axis_column;
use chrono::{DateTime, Utc};
use sea_query::{
    DeleteStatement,
    DynIden,
    Expr,
    ExprTrait,
    Iden,
    InsertStatement,
    IntoIden,
    JoinType,
    Query,
    SelectStatement,
    SimpleExpr,
    TableRef,
    error::Error as SeaQueryError,
};
use uuid::Uuid;

/// The schema the live branch's relations live in, which a statement has to
/// name once the temporary copies shadow the unqualified names.
#[derive(Iden, Clone, Copy)]
#[iden = "public"]
pub(crate) struct LiveSchema;

/// The local rows of the history a staging copies, alias `l`.
#[derive(Iden, Clone, Copy)]
#[iden = "l"]
struct Local;

/// The staged rows a duplicate is looked for among, alias `r`.
#[derive(Iden, Clone, Copy)]
#[iden = "r"]
struct Staged;

/// The temporal catalogue's columns, in table order.
const ENTITY_COLUMNS: [TemporalEntities; 5] = [
    TemporalEntities::TemporalEntityKey,
    TemporalEntities::CreatedAt,
    TemporalEntities::ModifiedAt,
    TemporalEntities::Id,
    TemporalEntities::EntityTypeNameKeys,
];

/// The stream catalogue's columns, in table order.
const STREAM_COLUMNS: [TemporalAttributeStreams; 4] = [
    TemporalAttributeStreams::StreamKey,
    TemporalAttributeStreams::TemporalEntityKey,
    TemporalAttributeStreams::AttributeNameKey,
    TemporalAttributeStreams::DatasetId,
];

/// The history hypertable's columns.
const HISTORY_COLUMNS: [AttributeHistory; 26] = [
    AttributeHistory::RecordedAt,
    AttributeHistory::WriteId,
    AttributeHistory::StreamKey,
    AttributeHistory::WriteOrdinal,
    AttributeHistory::AttributeKind,
    AttributeHistory::ValueKind,
    AttributeHistory::ObservedAt,
    AttributeHistory::CreatedAt,
    AttributeHistory::ModifiedAt,
    AttributeHistory::DeletedAt,
    AttributeHistory::ValueDateTime,
    AttributeHistory::GeneratedInstanceId,
    AttributeHistory::UnitCodeId,
    AttributeHistory::ValueBoolean,
    AttributeHistory::VerbatimInstanceId,
    AttributeHistory::ValueNumber,
    AttributeHistory::ValueString,
    AttributeHistory::ValueGeometry,
    AttributeHistory::ValueGeometryMembers,
    AttributeHistory::ValueObject,
    AttributeHistory::ValueUri,
    AttributeHistory::ValueTypeKey,
    AttributeHistory::ObjectTypeKeys,
    AttributeHistory::ProofSeals,
    AttributeHistory::ExpiresAt,
    AttributeHistory::SubAttributes,
];

/// Which of the entity's local history rows the staging copies.
#[derive(Debug, Clone, Copy)]
pub(crate) enum CopiedHistory {
    /// The rows positioned within the window on the Temporal Property: what an
    /// aggregation over the window reads.
    Window {
        /// The Temporal Property the window is measured on.
        timeproperty: TimeProp,
        /// The window.
        interval: TimeInterval,
    },
    /// Every row: a grouping whose keys read the value in effect reads rows
    /// before the window too.
    Whole,
}

/// The schema the branch's relations are named in: its Snapshot schema, or
/// the live branch's own.
pub(crate) fn branch_schema(snapshot: Option<DynIden>) -> DynIden {
    snapshot.unwrap_or_else(|| LiveSchema.into_iden())
}

/// The temporary schema, as an iden a statement builder qualifies with.
pub(crate) fn temporary_schema() -> DynIden {
    TemporarySchema.into_iden()
}

/// `table` in the temporary schema.
fn temporary(table: impl IntoIden) -> TableRef {
    qualified(Some(&temporary_schema()), table)
}

/// The three `CREATE TEMPORARY TABLE` statements copying the shape of the
/// branch's temporal catalogue, stream catalogue and history.
pub(crate) fn staging_tables(branch: &DynIden) -> [String; 3] {
    [
        temporary_copy_of(&TemporalEntities::Table.into_iden(), branch, &TemporalEntities::Table.into_iden()),
        temporary_copy_of(
            &TemporalAttributeStreams::Table.into_iden(),
            branch,
            &TemporalAttributeStreams::Table.into_iden(),
        ),
        temporary_copy_of(&AttributeHistory::Table.into_iden(), branch, &AttributeHistory::Table.into_iden()),
    ]
}

/// Copy the entity's catalogue row from the branch.
///
/// # Errors
///
/// Returns [`SeaQueryError`] when the statement cannot be assembled.
pub(crate) fn copy_entity(branch: &DynIden, entity_id: &Identifier) -> Result<InsertStatement, SeaQueryError> {
    let mut select = Query::select();
    select
        .columns(ENTITY_COLUMNS)
        .from(qualified(Some(branch), TemporalEntities::Table))
        .and_where(Expr::col(TemporalEntities::Id).eq(entity_id.as_str()));
    let mut insert = Query::insert();
    insert
        .into_table(temporary(TemporalEntities::Table))
        .columns(ENTITY_COLUMNS)
        .select_from(select)?;
    Ok(insert.take())
}

/// Copy the streams of the staged entity from the branch.
///
/// # Errors
///
/// Returns [`SeaQueryError`] when the statement cannot be assembled.
pub(crate) fn copy_streams(branch: &DynIden) -> Result<InsertStatement, SeaQueryError> {
    let mut staged_entities = Query::select();
    staged_entities
        .column(TemporalEntities::TemporalEntityKey)
        .from(temporary(TemporalEntities::Table));
    let mut select = Query::select();
    select
        .columns(STREAM_COLUMNS)
        .from(qualified(Some(branch), TemporalAttributeStreams::Table))
        .and_where(Expr::col(TemporalAttributeStreams::TemporalEntityKey).in_subquery(staged_entities));
    let mut insert = Query::insert();
    insert
        .into_table(temporary(TemporalAttributeStreams::Table))
        .columns(STREAM_COLUMNS)
        .select_from(select)?;
    Ok(insert.take())
}

/// Copy the history rows of the staged streams the aggregation reads.
///
/// # Errors
///
/// Returns [`SeaQueryError`] when the statement cannot be assembled.
pub(crate) fn copy_history(branch: &DynIden, copied: CopiedHistory) -> Result<InsertStatement, SeaQueryError> {
    let mut staged_streams = Query::select();
    staged_streams
        .column(TemporalAttributeStreams::StreamKey)
        .from(temporary(TemporalAttributeStreams::Table));
    let mut select = Query::select();
    select
        .columns(HISTORY_COLUMNS)
        .from(qualified(Some(branch), AttributeHistory::Table))
        .and_where(Expr::col(AttributeHistory::StreamKey).in_subquery(staged_streams));
    match copied {
        CopiedHistory::Window { timeproperty, interval } => {
            select.cond_where(within_interval(&Expr::col(axis_column(timeproperty)), &interval));
        }
        CopiedHistory::Whole => {}
    }
    let mut insert = Query::insert();
    insert
        .into_table(temporary(AttributeHistory::Table))
        .columns(HISTORY_COLUMNS)
        .select_from(select)?;
    Ok(insert.take())
}

/// The staged catalogue key of the entity, when the branch holds it.
pub(crate) fn staged_entity_key() -> SelectStatement {
    Query::select()
        .column(TemporalEntities::TemporalEntityKey)
        .from(temporary(TemporalEntities::Table))
        .to_owned()
}

/// Stage a catalogue row for an entity the branch holds none of.
///
/// # Errors
///
/// Returns [`SeaQueryError`] when the statement cannot be assembled.
pub(crate) fn stage_entity(key: i64, entity_id: &Identifier, type_keys: Vec<i32>, now: DateTime<Utc>) -> Result<InsertStatement, SeaQueryError> {
    let mut insert = Query::insert();
    insert.into_table(temporary(TemporalEntities::Table)).columns(ENTITY_COLUMNS).values([
        key.into(),
        now.into(),
        now.into(),
        entity_id.as_str().into(),
        type_keys.into(),
    ])?;
    Ok(insert.take())
}

/// The attribute name and dataset each staged stream is the grain of.
pub(crate) fn staged_streams() -> SelectStatement {
    Query::select()
        .column((AttributeNames::Table, AttributeNames::Name))
        .column((TemporalAttributeStreams::Table, TemporalAttributeStreams::DatasetId))
        .from(temporary(TemporalAttributeStreams::Table))
        .join(
            JoinType::Join,
            AttributeNames::Table,
            Expr::col((AttributeNames::Table, AttributeNames::AttributeNameKey))
                .equals((TemporalAttributeStreams::Table, TemporalAttributeStreams::AttributeNameKey)),
        )
        .to_owned()
}

/// Stage one stream for a grain the branch holds none of.
///
/// # Errors
///
/// Returns [`SeaQueryError`] when the statement cannot be assembled.
pub(crate) fn stage_stream(
    stream_key: i64,
    entity_key: i64,
    attribute_name_key: i32,
    dataset_id: Option<&Identifier>,
) -> Result<InsertStatement, SeaQueryError> {
    let mut insert = Query::insert();
    insert.into_table(temporary(TemporalAttributeStreams::Table)).columns(STREAM_COLUMNS).values([
        stream_key.into(),
        entity_key.into(),
        attribute_name_key.into(),
        dataset_id.map(|dataset| dataset.as_str().to_owned()).into(),
    ])?;
    Ok(insert.take())
}

/// Remove every row staged under `staged_write` whose instance a local row of
/// the same stream already carries: the same `instanceId` names the same
/// instance (clause 4.5.7), which is counted once.
pub(crate) fn drop_restated_instances(staged_write: Uuid) -> DeleteStatement {
    let local = |column: AttributeHistory| SimpleExpr::from(Expr::col((Local, column)));
    let staged = |column: AttributeHistory| SimpleExpr::from(Expr::col((Staged, column)));
    let mut restated = Query::select();
    restated
        .expr(Expr::val(1))
        .from(temporary(AttributeHistory::Table).alias(Local))
        .and_where(local(AttributeHistory::WriteId).ne(staged_write))
        .and_where(local(AttributeHistory::StreamKey).eq(staged(AttributeHistory::StreamKey)))
        .and_where(local(AttributeHistory::GeneratedInstanceId).not_distinct_from(staged(AttributeHistory::GeneratedInstanceId)))
        .and_where(local(AttributeHistory::VerbatimInstanceId).not_distinct_from(staged(AttributeHistory::VerbatimInstanceId)));
    Query::delete()
        .from_table(temporary(AttributeHistory::Table).alias(Staged))
        .and_where(Expr::col((Staged, AttributeHistory::WriteId)).eq(staged_write))
        .and_where(Expr::exists(restated))
        .to_owned()
}

#[cfg(test)]
mod tests {
    use crate::merged_aggregation::staging_statements::{CopiedHistory, branch_schema, copy_history, staging_tables};
    use andromeda_ngsi_ld_lexicon::time_prop::TimeProp;
    use andromeda_ngsi_ld_query::temporal::window::TimeInterval;
    use sea_query::{Alias, IntoIden, PostgresQueryBuilder};

    /// The copies are temporary and dropped with the transaction, and every
    /// branch relation is named schema-qualified, the live one included,
    /// because the copies shadow the unqualified names.
    #[test]
    fn the_staging_tables_are_temporary_and_the_branch_is_qualified() {
        let live = branch_schema(None);
        pretty_assertions::assert_eq!(
            staging_tables(&live)[0],
            r#"CREATE TEMPORARY TABLE "temporal_entities" (LIKE "public"."temporal_entities" INCLUDING DEFAULTS INCLUDING INDEXES) ON COMMIT DROP"#
        );
        let copy = copy_history(&live, CopiedHistory::Whole)
            .expect("the copy builds")
            .to_string(PostgresQueryBuilder);
        assert!(copy.starts_with(r#"INSERT INTO "pg_temp"."attribute_history""#), "{copy}");
        assert!(copy.contains(r#"FROM "public"."attribute_history""#), "{copy}");
        assert!(copy.contains(r#"FROM "pg_temp"."temporal_attribute_streams""#), "{copy}");

        let snapshot = branch_schema(Some(Alias::new("snap_7").into_iden()));
        assert!(staging_tables(&snapshot)[2].contains(r#"(LIKE "snap_7"."attribute_history""#));
    }

    /// A copied local row keeps every column the branch's row holds, the typed
    /// members beside its value included.
    #[test]
    fn the_staged_answer_carries_the_typed_metadata_columns() {
        let copy = copy_history(&branch_schema(None), CopiedHistory::Whole)
            .expect("the copy builds")
            .to_string(PostgresQueryBuilder);
        for column in ["value_type_key", "object_type_keys", "proof_seals", "expires_at", "sub_attributes"] {
            assert!(copy.contains(&format!(r#""{column}""#)), "{column} missing: {copy}");
        }
        assert!(!copy.contains("metadata"), "{copy}");
    }

    /// An aggregation reads the window alone, so only its rows are copied.
    #[test]
    fn a_windowed_copy_reads_the_window_on_the_temporal_property() {
        let window = CopiedHistory::Window {
            timeproperty: TimeProp::Modified,
            interval: TimeInterval::Before("2030-01-01T00:00:00Z".parse().expect("an instant")),
        };
        let copy = copy_history(&branch_schema(None), window)
            .expect("the copy builds")
            .to_string(PostgresQueryBuilder);
        assert!(copy.contains(r#""modified_at" <"#), "{copy}");
    }
}
