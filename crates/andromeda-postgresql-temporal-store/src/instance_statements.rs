//! The statements addressing one temporal attribute instance.
//!
//! Clause 5.6.15 addresses one instance of one temporal attribute by its
//! `instanceId`, and that identifier is stored in one of two columns: the bare
//! UUID of a broker-minted identifier, or the verbatim text of one a client
//! supplied. Every statement here is therefore written twice, once per column,
//! so each reads the partial index that covers it instead of an `OR` the
//! planner has to resolve with a scan.

use andromeda_context_space::context_target::ContextTarget;
use andromeda_postgresql_branch::table_of;
use andromeda_postgresql_schema::{
    name_dictionary::AttributeNames,
    temporal::{AttributeHistory, AttributeHistoryInstanceClaims, TemporalAttributeStreams, TemporalEntities},
};
use andromeda_postgresql_seaquery::{clock::now, placeholder::external_placeholder, postgis::validated_geom_from_ewkb};
use sea_query::{CaseStatement, Expr, ExprTrait, Iden, JoinType, Keyword, PostgresQueryBuilder, Query, SelectStatement, SimpleExpr};

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

/// The `attribute_history` alias the single-instance select reads under.
#[derive(Iden, Clone, Copy)]
#[iden = "ah"]
struct Ah;

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
/// entity id (`$2`) and attribute name (`$3`) are bound by the caller's chain.
fn attribute_streams_subquery(target: &ContextTarget) -> SelectStatement {
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
        .and_where(Expr::col((Te, TemporalEntities::Id)).eq(external_placeholder(2)))
        .and_where(Expr::col((An, AttributeNames::Name)).eq(external_placeholder(3)))
        .to_owned()
}

/// The instance delete, removing the addressed row rather than tombstoning it.
pub(crate) fn delete_instance_sql(column: InstanceColumn, target: &ContextTarget) -> String {
    Query::delete()
        .from_table(table_of(target, AttributeHistory::Table))
        .and_where(Expr::col(column.column()).eq(external_placeholder(1)))
        .and_where(Expr::col(AttributeHistory::StreamKey).in_subquery(attribute_streams_subquery(target)))
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

/// The geometry column's `CASE WHEN $8 IS NOT NULL THEN ST_MakeValid(ST_GeomFromEWKB($8)) ELSE NULL END`.
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
/// `$5` binds directly as the column's arbitrary-precision type, so a number
/// with no exact double is stored exactly as a creation stores it. `modified_at`
/// advances the NGSI-LD axis while `recorded_at` is left alone.
/// `attribute_kind` moves with the value columns so a replacement that changes
/// family keeps the row's declared family in agreement with its value shape,
/// and every member the replacement carries beside its value replaces the
/// row's, so none of the replaced instance's survives.
pub(crate) fn update_instance_sql(column: InstanceColumn, target: &ContextTarget) -> String {
    Query::update()
        .table(table_of(target, AttributeHistory::Table))
        .values([
            (AttributeHistory::ValueKind, external_placeholder(4)),
            (AttributeHistory::ValueNumber, external_placeholder(5)),
            (AttributeHistory::ValueString, external_placeholder(6)),
            (AttributeHistory::ValueBoolean, external_placeholder(7)),
            (AttributeHistory::ValueGeometry, geometry_write(8)),
            (AttributeHistory::ValueObject, external_placeholder(9)),
            (AttributeHistory::ValueDateTime, external_placeholder(10)),
            (AttributeHistory::ValueUri, external_placeholder(11)),
            (AttributeHistory::ObservedAt, external_placeholder(12)),
            (AttributeHistory::ValueTypeKey, external_placeholder(13)),
            (AttributeHistory::UnitCodeId, external_placeholder(14)),
            (AttributeHistory::AttributeKind, external_placeholder(15)),
            (AttributeHistory::ValueGeometryMembers, external_placeholder(16)),
            (AttributeHistory::ObjectTypeKeys, external_placeholder(17)),
            (AttributeHistory::ProofSeals, external_placeholder(18)),
            (AttributeHistory::ExpiresAt, external_placeholder(19)),
            (AttributeHistory::SubAttributes, external_placeholder(20)),
            (AttributeHistory::ModifiedAt, now()),
        ])
        .and_where(Expr::col(column.column()).eq(external_placeholder(1)))
        .and_where(Expr::col(AttributeHistory::StreamKey).in_subquery(attribute_streams_subquery(target)))
        .to_owned()
        .to_string(PostgresQueryBuilder)
}

/// The single-instance select used for policy validation.
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
        .and_where(Expr::col((Ah, AttributeHistory::StreamKey)).in_subquery(attribute_streams_subquery(target)))
        .limit(1)
        .to_owned()
        .to_string(PostgresQueryBuilder)
}

#[cfg(test)]
mod tests {
    use crate::instance_statements::{InstanceColumn, delete_instance_sql, release_verbatim_instance_claim_sql, select_instance_sql, update_instance_sql};
    use andromeda_context_space::context_target::ContextTarget;
    use pretty_assertions::assert_eq;

    /// The six identity-column statements, rendered.
    fn all_statements() -> Vec<String> {
        [InstanceColumn::Generated, InstanceColumn::Verbatim]
            .into_iter()
            .flat_map(|column| {
                [
                    delete_instance_sql(column, &ContextTarget::Live),
                    update_instance_sql(column, &ContextTarget::Live),
                    select_instance_sql(column, &ContextTarget::Live),
                ]
            })
            .collect()
    }

    #[test]
    fn each_statement_addresses_exactly_one_instance_identity_column() {
        for column in [InstanceColumn::Generated, InstanceColumn::Verbatim] {
            for statement in [
                delete_instance_sql(column, &ContextTarget::Live),
                update_instance_sql(column, &ContextTarget::Live),
                select_instance_sql(column, &ContextTarget::Live),
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
    fn every_statement_is_confined_to_the_named_entity_and_attribute() {
        for statement in all_statements() {
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
            assert!(statement.contains(r#""value_kind" = $4"#), "{statement}");
            assert!(statement.contains(r#""attribute_kind" = $15"#), "{statement}");
            for member in [
                r#""value_type_key" = $13"#,
                r#""unit_code_id" = $14"#,
                r#""object_type_keys" = $17"#,
                r#""proof_seals" = $18"#,
                r#""expires_at" = $19"#,
                r#""sub_attributes" = $20"#,
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
            assert!(statement.contains(r#""value_number" = $5"#), "{statement}");
            assert!(!statement.contains("double precision"), "{statement}");
        }
    }
}
