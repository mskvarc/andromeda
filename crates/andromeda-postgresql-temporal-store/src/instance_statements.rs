//! The statements addressing one temporal attribute instance.
//!
//! Clause 5.6.15 addresses one instance of one temporal attribute by its
//! `instanceId`, and that identifier is stored in one of two columns: the bare
//! UUID of a broker-minted identifier, or the verbatim text of one a client
//! supplied. Every statement here is therefore written twice, once per column,
//! so each reads the partial index that covers it instead of an `OR` the
//! planner has to resolve with a scan.
//!
//! The modification and the deletion bind the attribute's streams as values
//! (`$2`) the caller resolved first, never as a subquery: on a converted chunk
//! TimescaleDB narrows the compressed batches a statement decompresses only by
//! bound values on the segmenting stream key and the identifier's bloom
//! summary, and a subquery would have it decompress every batch whose bloom
//! summary admits the identifier, across every stream of the chunk.

use andromeda_context_space::context_target::ContextTarget;
use andromeda_postgresql_branch::table_of;
use andromeda_postgresql_codes::value_kind::ValueKind;
use andromeda_postgresql_schema::{
    name_dictionary::AttributeNames,
    temporal::{AttributeHistory, AttributeHistoryInstanceClaims, TemporalAttributeStreams, TemporalEntities, TemporalStreamShapes},
};
use andromeda_postgresql_seaquery::{
    clock::now,
    pg_array::any,
    placeholder::external_placeholder,
    postgis::{bounding_box_contains, st_collect, st_envelope, validated_geom_from_ewkb},
};
use andromeda_postgresql_temporal_query::{alias::Ah, recorded_shape::recorded_shape};
use sea_query::{
    CaseStatement,
    Condition,
    Expr,
    ExprTrait,
    Iden,
    JoinType,
    Keyword,
    OnConflict,
    PostgresQueryBuilder,
    Query,
    SelectStatement,
    SimpleExpr,
    error::Error as SeaQueryError,
};

/// The `temporal_attribute_streams` alias in the stream-scope subquery.
#[derive(Iden, Clone, Copy)]
#[iden = "s"]
struct S;

/// The `temporal_entities` alias in the stream-scope subquery.
#[derive(Iden, Clone, Copy)]
#[iden = "te"]
struct Te;

/// The `attribute_names` alias in the stream-scope subquery.
#[derive(Iden, Clone, Copy)]
#[iden = "an"]
struct An;

/// Which stored column carries the addressed `instanceId`.
///
/// Clause 5.6.15's identifier lives in exactly one of two columns, and every
/// statement addresses the one that matches so it reads the partial index that
/// covers it rather than resolving an `OR` with a scan.
#[derive(Clone, Copy)]
pub(crate) enum InstanceColumn {
    /// A broker-minted `instanceId`, stored as a bare UUID.
    Generated,
    /// A client-supplied `instanceId`, stored verbatim.
    Verbatim,
}

impl InstanceColumn {
    /// The `attribute_history` column this identity is addressed through.
    const fn column(self) -> AttributeHistory {
        match self {
            InstanceColumn::Generated => AttributeHistory::GeneratedInstanceId,
            InstanceColumn::Verbatim => AttributeHistory::VerbatimInstanceId,
        }
    }
}

/// The streams of one entity's attribute, whatever `datasetId` they carry.
///
/// An `instanceId` is unique across the attribute, so the dataset dimension is
/// not part of the address clause 5.6.15 defines; every stream of the named
/// attribute is a candidate and the instance predicate selects within them. The
/// entity id and attribute name bind to the placeholders `entity` and
/// `attribute` number.
fn attribute_streams_query(target: &ContextTarget, entity: usize, attribute: usize) -> SelectStatement {
    Query::select()
        .column((S, TemporalAttributeStreams::StreamKey))
        .from(table_of(target, TemporalAttributeStreams::Table).alias(S))
        .join_as(
            JoinType::InnerJoin,
            table_of(target, TemporalEntities::Table),
            Te,
            Expr::col((Te, TemporalEntities::TemporalEntityKey)).equals((S, TemporalAttributeStreams::TemporalEntityKey)),
        )
        .join_as(
            JoinType::InnerJoin,
            AttributeNames::Table,
            An,
            Expr::col((An, AttributeNames::AttributeNameKey)).equals((S, TemporalAttributeStreams::AttributeNameKey)),
        )
        .and_where(Expr::col((Te, TemporalEntities::Id)).eq(external_placeholder(entity)))
        .and_where(Expr::col((An, AttributeNames::Name)).eq(external_placeholder(attribute)))
        .to_owned()
}

/// The stream keys of one entity's attribute: the entity id binds to `$1`, the
/// attribute name to `$2`.
pub(crate) fn attribute_stream_keys_sql(target: &ContextTarget) -> String {
    attribute_streams_query(target, 1, 2).to_string(PostgresQueryBuilder)
}

/// `stream_key = ANY($2)`, the resolved streams the instance is sought in.
fn within_resolved_streams() -> SimpleExpr {
    Expr::col(AttributeHistory::StreamKey).eq(any(external_placeholder(2)))
}

/// The instance delete, removing the addressed row rather than tombstoning it.
pub(crate) fn delete_instance_sql(column: InstanceColumn, target: &ContextTarget) -> String {
    Query::delete()
        .from_table(table_of(target, AttributeHistory::Table))
        .and_where(Expr::col(column.column()).eq(external_placeholder(1)))
        .and_where(within_resolved_streams())
        .to_owned()
        .to_string(PostgresQueryBuilder)
}

/// Free the `(entity, attribute, verbatim instanceId)` claim a deleted
/// instance held, so a future write may reuse the identifier (clause 4.5.7
/// does not reserve a deleted instance's identity forever).
///
/// `DELETE … USING` is expressed as two correlated `EXISTS`, since sea-query has
/// no `USING`: each resolves one of the scope tables the join did.
pub(crate) fn release_verbatim_instance_claim_sql(target: &ContextTarget) -> String {
    let entity_scope = Query::select()
        .expr(Expr::val(1))
        .from(table_of(target, TemporalEntities::Table).alias(Te))
        .and_where(
            Expr::col((Te, TemporalEntities::TemporalEntityKey))
                .equals((AttributeHistoryInstanceClaims::Table, AttributeHistoryInstanceClaims::TemporalEntityKey)),
        )
        .and_where(Expr::col((Te, TemporalEntities::Id)).eq(external_placeholder(2)))
        .to_owned();
    let attribute_scope = Query::select()
        .expr(Expr::val(1))
        .from_as(AttributeNames::Table, An)
        .and_where(
            Expr::col((An, AttributeNames::AttributeNameKey)).equals((AttributeHistoryInstanceClaims::Table, AttributeHistoryInstanceClaims::AttributeNameKey)),
        )
        .and_where(Expr::col((An, AttributeNames::Name)).eq(external_placeholder(3)))
        .to_owned();
    Query::delete()
        .from_table(table_of(target, AttributeHistoryInstanceClaims::Table))
        .and_where(Expr::col(AttributeHistoryInstanceClaims::VerbatimInstanceId).eq(external_placeholder(1)))
        .and_where(Expr::exists(entity_scope))
        .and_where(Expr::exists(attribute_scope))
        .to_owned()
        .to_string(PostgresQueryBuilder)
}

/// The geometry column's `CASE WHEN $7 IS NOT NULL THEN ST_MakeValid(ST_GeomFromEWKB($7)) ELSE NULL END`.
fn geometry_write(marker: usize) -> SimpleExpr {
    CaseStatement::new()
        .case(
            Expr::expr(external_placeholder(marker)).is_not_null(),
            validated_geom_from_ewkb(external_placeholder(marker)),
        )
        .finally(Keyword::Null)
        .into()
}

/// The instance modification (clause 5.6.14 replacement).
///
/// `$4` binds directly as the column's arbitrary-precision type, so a number
/// with no exact double is stored exactly as a creation stores it. `modified_at`
/// advances the NGSI-LD axis while `recorded_at` is left alone.
/// `attribute_kind` moves with the value columns so a replacement that changes
/// family keeps the row's declared family in agreement with its value shape,
/// and every member the replacement carries beside its value replaces the
/// row's, so none of the replaced instance's survives. It returns the stream
/// of the row it replaced, which [`grow_stream_extent_sql`] grows.
pub(crate) fn update_instance_sql(column: InstanceColumn, target: &ContextTarget) -> String {
    Query::update()
        .table(table_of(target, AttributeHistory::Table))
        .values([
            (AttributeHistory::ValueKind, external_placeholder(3)),
            (AttributeHistory::ValueNumber, external_placeholder(4)),
            (AttributeHistory::ValueString, external_placeholder(5)),
            (AttributeHistory::ValueBoolean, external_placeholder(6)),
            (AttributeHistory::ValueGeometry, geometry_write(7)),
            (AttributeHistory::ValueObject, external_placeholder(8)),
            (AttributeHistory::ValueDateTime, external_placeholder(9)),
            (AttributeHistory::ValueUri, external_placeholder(10)),
            (AttributeHistory::ObservedAt, external_placeholder(11)),
            (AttributeHistory::ValueTypeKey, external_placeholder(12)),
            (AttributeHistory::UnitCodeId, external_placeholder(13)),
            (AttributeHistory::AttributeKind, external_placeholder(14)),
            (AttributeHistory::ValueGeometryMembers, external_placeholder(15)),
            (AttributeHistory::ObjectTypeKeys, external_placeholder(16)),
            (AttributeHistory::ProofSeals, external_placeholder(17)),
            (AttributeHistory::ExpiresAt, external_placeholder(18)),
            (AttributeHistory::SubAttributes, external_placeholder(19)),
            (AttributeHistory::ModifiedAt, now()),
        ])
        .and_where(Expr::col(column.column()).eq(external_placeholder(1)))
        .and_where(within_resolved_streams())
        .returning_col(AttributeHistory::StreamKey)
        .to_owned()
        .to_string(PostgresQueryBuilder)
}

/// Grow the extent of the streams `$1` names to cover the replacing
/// geometry, the EWKB `$2`, made valid as the modification stores it.
///
/// An extent already covering it is left alone, and none shrinks: the
/// geometry replaced stays inside an extent too large, which still holds
/// every geometry of its stream.
pub(crate) fn grow_stream_extent_sql(target: &ContextTarget) -> String {
    let extent = || Expr::col(TemporalAttributeStreams::ValueExtent);
    let geometry = || validated_geom_from_ewkb(external_placeholder(2));
    Query::update()
        .table(table_of(target, TemporalAttributeStreams::Table))
        .value(
            TemporalAttributeStreams::ValueExtent,
            st_envelope(st_collect(extent(), st_envelope(geometry()))),
        )
        .and_where(Expr::col(TemporalAttributeStreams::StreamKey).eq(any(external_placeholder(1))))
        .cond_where(Condition::any().add(extent().is_null()).add(bounding_box_contains(extent(), geometry()).not()))
        .to_owned()
        .to_string(PostgresQueryBuilder)
}

/// Add the combination of family and value shape the modified instance `$1`
/// carries to the shapes of its stream, among the streams `$2` the
/// modification returned; a combination the stream holds is left alone.
///
/// # Errors
///
/// Returns [`SeaQueryError`] if the projection ever stops matching the column
/// list, which the fixed shape here makes unreachable.
pub(crate) fn record_instance_shape_sql(column: InstanceColumn, target: &ContextTarget) -> Result<String, SeaQueryError> {
    let modified = Query::select()
        .distinct()
        .column((Ah, AttributeHistory::StreamKey))
        .column((S, TemporalAttributeStreams::AttributeNameKey))
        .column((Ah, AttributeHistory::AttributeKind))
        .expr(recorded_shape())
        .from(table_of(target, AttributeHistory::Table).alias(Ah))
        .join_as(
            JoinType::InnerJoin,
            table_of(target, TemporalAttributeStreams::Table),
            S,
            Expr::col((S, TemporalAttributeStreams::StreamKey)).equals((Ah, AttributeHistory::StreamKey)),
        )
        .and_where(Expr::col((Ah, column.column())).eq(external_placeholder(1)))
        .and_where(Expr::col((Ah, AttributeHistory::StreamKey)).eq(any(external_placeholder(2))))
        .and_where(Expr::col((Ah, AttributeHistory::ValueKind)).ne(ValueKind::Tombstone.code()))
        .to_owned();
    let mut statement = Query::insert();
    statement
        .into_table(table_of(target, TemporalStreamShapes::Table))
        .columns([
            TemporalStreamShapes::StreamKey,
            TemporalStreamShapes::AttributeNameKey,
            TemporalStreamShapes::AttributeKind,
            TemporalStreamShapes::RecordedShape,
        ])
        .on_conflict(
            OnConflict::columns([
                TemporalStreamShapes::StreamKey,
                TemporalStreamShapes::AttributeKind,
                TemporalStreamShapes::RecordedShape,
            ])
            .do_nothing()
            .to_owned(),
        );
    Ok(statement.select_from(modified)?.to_string(PostgresQueryBuilder))
}

/// The single-instance select used for policy validation.
///
/// A read narrows the compressed batches through the join itself, so it keeps
/// the stream scope as a subquery bound by the entity id (`$2`) and attribute
/// name (`$3`).
pub(crate) fn select_instance_sql(column: InstanceColumn, target: &ContextTarget) -> String {
    Query::select()
        .column((Ah, AttributeHistory::RecordedAt))
        .column((Ah, AttributeHistory::WriteId))
        .column((Ah, AttributeHistory::WriteOrdinal))
        .column((Ah, AttributeHistory::StreamKey))
        .column((Ah, AttributeHistory::AttributeKind))
        .column((Ah, AttributeHistory::ValueKind))
        .column((Ah, AttributeHistory::ObservedAt))
        .column((Ah, AttributeHistory::DeletedAt))
        .from(table_of(target, AttributeHistory::Table).alias(Ah))
        .and_where(Expr::col((Ah, column.column())).eq(external_placeholder(1)))
        .and_where(Expr::col((Ah, AttributeHistory::StreamKey)).in_subquery(attribute_streams_query(target, 2, 3)))
        .limit(1)
        .to_owned()
        .to_string(PostgresQueryBuilder)
}

#[cfg(test)]
mod tests {
    use crate::instance_statements::{
        InstanceColumn,
        attribute_stream_keys_sql,
        delete_instance_sql,
        grow_stream_extent_sql,
        record_instance_shape_sql,
        release_verbatim_instance_claim_sql,
        select_instance_sql,
        update_instance_sql,
    };
    use andromeda_context_space::context_target::ContextTarget;
    use pretty_assertions::assert_eq;

    #[test]
    fn an_instance_modification_grows_its_stream_by_the_replacing_geometry() {
        for column in [InstanceColumn::Generated, InstanceColumn::Verbatim] {
            let modification = update_instance_sql(column, &ContextTarget::Live);
            assert!(modification.ends_with(r#"RETURNING "stream_key""#), "{modification}");
        }
        let growth = grow_stream_extent_sql(&ContextTarget::Live);
        assert_eq!(
            growth,
            concat!(
                r#"UPDATE "temporal_attribute_streams" SET "value_extent" = ST_Envelope(ST_Collect("value_extent", ST_Envelope(ST_MakeValid(ST_GeomFromEWKB($2)))))"#,
                r#" WHERE "stream_key" = ANY($1) AND ("value_extent" IS NULL OR (NOT ("value_extent" ~ ST_MakeValid(ST_GeomFromEWKB($2)))))"#
            )
        );
    }

    /// Clause 5.6.14 replaces the instance whole, so the modified row may
    /// carry a family and shape its stream did not hold: the combination is
    /// read off the stored row among the streams the modification returned.
    #[test]
    fn a_modified_instance_adds_its_combination_to_its_stream() {
        for column in [InstanceColumn::Generated, InstanceColumn::Verbatim] {
            let statement = record_instance_shape_sql(column, &ContextTarget::Live).expect("the fixed statement builds");
            assert!(
                statement.starts_with(r#"INSERT INTO "temporal_stream_shapes" ("stream_key", "attribute_name_key", "attribute_kind", "recorded_shape") SELECT DISTINCT "ah"."stream_key", "s"."attribute_name_key", "ah"."attribute_kind", (CASE"#),
                "{statement}"
            );
            assert!(statement.contains(r#""ah"."stream_key" = ANY($2) AND "ah"."value_kind" <> 0"#), "{statement}");
            assert!(
                statement.ends_with(r#"ON CONFLICT ("stream_key", "attribute_kind", "recorded_shape") DO NOTHING"#),
                "{statement}"
            );
        }
    }

    #[test]
    fn each_statement_addresses_exactly_one_instance_identity_column() {
        for column in [InstanceColumn::Generated, InstanceColumn::Verbatim] {
            for statement in [
                delete_instance_sql(column, &ContextTarget::Live),
                update_instance_sql(column, &ContextTarget::Live),
                select_instance_sql(column, &ContextTarget::Live),
                record_instance_shape_sql(column, &ContextTarget::Live).expect("the fixed statement builds"),
            ] {
                match column {
                    InstanceColumn::Generated => {
                        assert!(statement.contains(r#""generated_instance_id" = ($1)"#), "{statement}");
                        assert!(!statement.contains("verbatim_instance_id"), "{statement}");
                    }
                    InstanceColumn::Verbatim => {
                        assert!(statement.contains(r#""verbatim_instance_id" = ($1)"#), "{statement}");
                        assert!(!statement.contains("generated_instance_id"), "{statement}");
                    }
                }
            }
        }
    }

    #[test]
    fn the_stream_keys_are_resolved_from_the_named_entity_and_attribute() {
        let statement = attribute_stream_keys_sql(&ContextTarget::Live);
        assert!(
            statement.starts_with(r#"SELECT "s"."stream_key" FROM "temporal_attribute_streams" AS "s""#),
            "{statement}"
        );
        assert!(statement.contains(r#""te"."id" = ($1)"#), "{statement}");
        assert!(statement.contains(r#""an"."name" = ($2)"#), "{statement}");
    }

    #[test]
    fn a_modification_or_deletion_is_confined_to_the_resolved_streams_as_bound_values() {
        for column in [InstanceColumn::Generated, InstanceColumn::Verbatim] {
            for statement in [
                delete_instance_sql(column, &ContextTarget::Live),
                update_instance_sql(column, &ContextTarget::Live),
            ] {
                assert!(statement.contains(r#""stream_key" = ANY($2)"#), "{statement}");
                assert!(!statement.contains("temporal_attribute_streams"), "{statement}");
            }
        }
    }

    #[test]
    fn the_policy_read_is_confined_to_the_named_entity_and_attribute() {
        for column in [InstanceColumn::Generated, InstanceColumn::Verbatim] {
            let statement = select_instance_sql(column, &ContextTarget::Live);
            assert!(statement.contains(r#"FROM "temporal_attribute_streams" AS "s""#), "{statement}");
            assert!(statement.contains(r#""te"."id" = ($2)"#), "{statement}");
            assert!(statement.contains(r#""an"."name" = ($3)"#), "{statement}");
        }
    }

    #[test]
    fn a_modification_advances_the_modification_axis_and_leaves_the_write_clock_alone() {
        for column in [InstanceColumn::Generated, InstanceColumn::Verbatim] {
            let statement = update_instance_sql(column, &ContextTarget::Live);
            assert!(statement.contains(r#""modified_at" = NOW()"#), "{statement}");
            assert!(!statement.contains(r#""recorded_at" ="#), "{statement}");
            assert!(statement.contains(r#""value_kind" = $3"#), "{statement}");
            assert!(statement.contains(r#""attribute_kind" = $14"#), "{statement}");
            for member in [
                r#""value_type_key" = $12"#,
                r#""unit_code_id" = $13"#,
                r#""object_type_keys" = $16"#,
                r#""proof_seals" = $17"#,
                r#""expires_at" = $18"#,
                r#""sub_attributes" = $19"#,
            ] {
                assert!(statement.contains(member), "{member} missing: {statement}");
            }
        }
    }

    #[test]
    fn a_deletion_removes_the_row_rather_than_recording_a_tombstone() {
        for column in [InstanceColumn::Generated, InstanceColumn::Verbatim] {
            let statement = delete_instance_sql(column, &ContextTarget::Live);
            assert!(statement.starts_with(r#"DELETE FROM "attribute_history""#), "{statement}");
            assert!(!statement.contains("deleted_at"), "{statement}");
        }
    }

    #[test]
    fn a_verbatim_deletion_releases_the_instance_claim_it_held() {
        let statement = release_verbatim_instance_claim_sql(&ContextTarget::Live);
        assert!(statement.starts_with(r#"DELETE FROM "attribute_history_instance_claims""#), "{statement}");
        assert!(statement.contains(r#""verbatim_instance_id" = ($1)"#), "{statement}");
        assert!(statement.contains(r#""te"."id" = ($2)"#), "{statement}");
        assert!(statement.contains(r#""an"."name" = ($3)"#), "{statement}");
        assert_eq!(statement.matches("EXISTS").count(), 2, "{statement}");
    }

    #[test]
    fn the_update_statement_binds_numeric_without_a_double_hop() {
        for column in [InstanceColumn::Generated, InstanceColumn::Verbatim] {
            let statement = update_instance_sql(column, &ContextTarget::Live);
            assert!(statement.contains(r#""value_number" = $4"#), "{statement}");
            assert!(!statement.contains("double precision"), "{statement}");
        }
    }
}
