//! The statements that write the `entities` table.
//!
//! There is one statement per kind of change, and it serves any number of
//! entities: a single-entity write is a set of one, which is why no per-row
//! variant exists. Each statement returns the rows it affected, and the caller
//! turns that into a per-entity outcome by asking what the write assumed.
//!
//! Insert and update return the surrogate key alongside the identifier, because
//! the projection of current attribute values is written against that key in
//! this same transaction and must not have to read the row back to find it.
//! Delete returns neither: the projection is removed by the single ownership
//! edge that cascades from `entities`, and clause 5.6.6 leaves the temporal
//! evolution standing, so an Entity deletion touches no other relation.
//!
//! No statement here reads. A read-modify-write takes its "before" snapshot
//! outside the transaction and states what it assumed in a precondition, which
//! the update statement enforces.
//!
//! Each statement's parallel arrays bind through `sqlx` rather than sea-query:
//! the geometry, expiry and expectation arrays carry NULLs (`Vec<Option<T>>`),
//! which `sea-query-sqlx` cannot represent, so every array is bound outside
//! sea-query in the marker order the statement builder emits. The statement
//! structure — every relation, column, cast, function and clause — is still
//! built by sea-query.

use crate::{
    entity::{
        mapper::{stored_uri, stored_written_entity},
        params::PreparedEntityColumns,
        repo::EntityRepository,
        types_present::{entity_write_failure, types_present},
        written_entity::WrittenEntity,
    },
    error::ContextStoreError,
};
use andromeda_context_commit::deleted_entity::DeletedEntity;
use andromeda_ngsi_ld_lexicon::uri::Uri;
use andromeda_postgresql_schema::current::Entities;
use andromeda_postgresql_seaquery::{
    cast::cast,
    clock::now,
    jsonb::jsonb_array_elements_text,
    pg_array::array_subquery,
    pg_type::PostgresType,
    placeholder::external_placeholder,
    postgis::validated_geom_from_ewkb,
    unnest::{UnnestColumn, apply_column_alias, unnest_call},
};
use chrono::{DateTime, Utc};
use sea_query::{CaseStatement, Cond, Expr, ExprTrait, Func, Iden, IntoIden, Keyword, OnConflict, PostgresQueryBuilder, Query, SimpleExpr, TableRef};
use sqlx::{Postgres, Row, Transaction};

/// The staging alias, `d`, each set-write reads its parallel arrays under.
///
/// Spelled nowhere else in a statement, so the single `AS "d"` its table source
/// emits is the one [`apply_column_alias`] rewrites into the column-list alias.
#[derive(Iden, Clone, Copy)]
#[iden = "d"]
struct Staged;

/// The staging columns of the entity set-writes, named as the `AS d(…)` alias
/// spells them rather than as the `entities` columns they land in.
///
/// The type and scope arrays arrive as JSONB (`types_json`, `scope_paths_json`)
/// and are expanded to `text[]`/`ltree[]` in the projection; the geometries
/// arrive as EWKB (`loc`, `obs`, `ops`) and are validated there. Not every
/// statement uses every column: the delete stages only `id` and the expectation.
#[derive(Iden, Clone, Copy)]
enum StagedColumn {
    /// Entity identifier.
    Id,
    /// Type array as JSONB.
    TypesJson,
    /// Scope-path array as JSONB, in `ltree` dot notation.
    ScopePathsJson,
    /// Payload as JSON text, cast to `jsonb` in the projection.
    Payload,
    /// Default GeoProperty geometry as EWKB.
    Loc,
    /// `observationSpace` geometry as EWKB.
    Obs,
    /// `operationSpace` geometry as EWKB.
    Ops,
    /// Expiration timestamp.
    ExpiresAt,
    /// The `modifiedAt` the caller read, or NULL for an unconditional write.
    ExpectedModifiedAt,
}

/// A staging column reference, `d.<column>`.
fn staged(column: StagedColumn) -> SimpleExpr {
    Expr::col((Staged, column))
}

/// The validated-geometry projection of a staged EWKB column:
/// `CASE WHEN d.<col> IS NOT NULL THEN ST_MakeValid(ST_GeomFromEWKB(d.<col>)) ELSE NULL END`.
///
/// The EWKB already carries its SRID, so no re-SRID is applied, and a NULL EWKB
/// projects to a NULL geometry rather than a call on a NULL argument.
fn geometry(column: StagedColumn) -> SimpleExpr {
    CaseStatement::new()
        .case(Expr::col((Staged, column)).is_not_null(), validated_geom_from_ewkb(staged(column)))
        .finally(Keyword::Null)
        .into()
}

/// The `text[]`/`ltree[]` projection of a staged JSONB array column:
/// `ARRAY(SELECT jsonb_array_elements_text(d.<col>)[::element_cast])`.
fn text_array_of(column: StagedColumn, element_cast: Option<PostgresType>) -> SimpleExpr {
    let element: SimpleExpr = jsonb_array_elements_text(staged(column)).into();
    let element = match element_cast {
        Some(ty) => cast(element, ty),
        None => element,
    };
    array_subquery(Query::select().expr(element).to_owned())
}

/// The precondition both the update and the delete carry:
/// `d.expected_modified_at IS NULL OR entities.modified_at = d.expected_modified_at`.
///
/// A null expectation matches whatever is there; a non-null one matches only a
/// row still standing as the caller read it, which is what turns a
/// read-modify-write into a compare-and-set rather than a lost update.
fn expectation() -> Cond {
    Cond::any()
        .add(Expr::col((Staged, StagedColumn::ExpectedModifiedAt)).is_null())
        .add(Expr::col((Entities::Table, Entities::ModifiedAt)).equals((Staged, StagedColumn::ExpectedModifiedAt)))
}

/// The eight staging columns of the insert, in bind order.
fn insert_staging_columns() -> [UnnestColumn; 8] {
    [
        UnnestColumn::new(StagedColumn::Id, PostgresType::TextArray),
        UnnestColumn::new(StagedColumn::TypesJson, PostgresType::JsonbArray),
        UnnestColumn::new(StagedColumn::ScopePathsJson, PostgresType::JsonbArray),
        UnnestColumn::new(StagedColumn::Payload, PostgresType::TextArray),
        UnnestColumn::new(StagedColumn::Loc, PostgresType::ByteaArray),
        UnnestColumn::new(StagedColumn::Obs, PostgresType::ByteaArray),
        UnnestColumn::new(StagedColumn::Ops, PostgresType::ByteaArray),
        UnnestColumn::new(StagedColumn::ExpiresAt, PostgresType::TimestamptzArray),
    ]
}

/// The nine staging columns of the update: the insert's eight plus the
/// expectation the compare-and-set reads.
fn update_staging_columns() -> [UnnestColumn; 9] {
    [
        UnnestColumn::new(StagedColumn::Id, PostgresType::TextArray),
        UnnestColumn::new(StagedColumn::TypesJson, PostgresType::JsonbArray),
        UnnestColumn::new(StagedColumn::ScopePathsJson, PostgresType::JsonbArray),
        UnnestColumn::new(StagedColumn::Payload, PostgresType::TextArray),
        UnnestColumn::new(StagedColumn::Loc, PostgresType::ByteaArray),
        UnnestColumn::new(StagedColumn::Obs, PostgresType::ByteaArray),
        UnnestColumn::new(StagedColumn::Ops, PostgresType::ByteaArray),
        UnnestColumn::new(StagedColumn::ExpiresAt, PostgresType::TimestamptzArray),
        UnnestColumn::new(StagedColumn::ExpectedModifiedAt, PostgresType::TimestamptzArray),
    ]
}

/// The two staging columns of the delete: the identifier and the expectation.
fn delete_staging_columns() -> [UnnestColumn; 2] {
    [
        UnnestColumn::new(StagedColumn::Id, PostgresType::TextArray),
        UnnestColumn::new(StagedColumn::ExpectedModifiedAt, PostgresType::TimestamptzArray),
    ]
}

/// Build the batch insert.
///
/// The conflict clause is how a duplicate create is detected: the identifier
/// simply does not come back, and the caller answers 409 off that (NGSI-LD
/// 6.4.3.1). Suppressing the conflict rather than raising a duplicate-key error
/// is what lets one statement serve a batch that is permitted to succeed
/// partially (clause 5.6.7) and the idempotent placeholder the Temporal API
/// needs, without three different statements.
///
/// # Errors
///
/// Returns [`ContextStoreError::Internal`] if the projection ever stops matching
/// the eight-wide column list, which the fixed shapes here make unreachable.
fn entity_insert_sql() -> Result<String, ContextStoreError> {
    let columns = insert_staging_columns();
    let call = unnest_call(columns.iter().enumerate().map(|(index, column)| (column, external_placeholder(index + 1))));

    let mut select = Query::select();
    select
        .expr(staged(StagedColumn::Id))
        .expr(text_array_of(StagedColumn::TypesJson, None))
        .expr(text_array_of(StagedColumn::ScopePathsJson, Some(PostgresType::Ltree)))
        .expr(cast(staged(StagedColumn::Payload), PostgresType::Jsonb))
        .expr(geometry(StagedColumn::Loc))
        .expr(geometry(StagedColumn::Obs))
        .expr(geometry(StagedColumn::Ops))
        .expr(staged(StagedColumn::ExpiresAt))
        .from_function(call, Staged);
    let select = select;

    let mut insert = Query::insert();
    insert.into_table(Entities::Table).columns([
        Entities::Id,
        Entities::Types,
        Entities::ScopePaths,
        Entities::Payload,
        Entities::Location,
        Entities::ObservationSpace,
        Entities::OperationSpace,
        Entities::ExpiresAt,
    ]);
    insert.select_from(select).map_err(|error| ContextStoreError::Internal(error.to_string()))?;
    insert
        .on_conflict(OnConflict::column(Entities::Id).do_nothing().to_owned())
        .returning(Query::returning().columns([Entities::EntityKey, Entities::Id]));

    let sql = insert.to_string(PostgresQueryBuilder);
    Ok(apply_column_alias(sql, &Staged, &columns))
}

/// Build the batch update.
///
/// The expectation column is nullable so one statement carries both an
/// unconditional overwrite and a compare-and-set in the same batch: a null
/// expectation matches whatever is there, a non-null one matches only an
/// untouched row. An identifier that does not come back therefore means either
/// "gone" or "moved on", and which of those it is depends on the precondition
/// the caller attached. `expires_at` is coalesced so an update that omits it
/// keeps the stored value rather than clearing it.
fn entity_update_sql() -> String {
    let columns = update_staging_columns();
    let call = unnest_call(columns.iter().enumerate().map(|(index, column)| (column, external_placeholder(index + 1))));

    let mut update = Query::update();
    update
        .table(Entities::Table)
        .value(Entities::Payload, cast(staged(StagedColumn::Payload), PostgresType::Jsonb))
        .value(Entities::Types, text_array_of(StagedColumn::TypesJson, None))
        .value(Entities::ScopePaths, text_array_of(StagedColumn::ScopePathsJson, Some(PostgresType::Ltree)))
        .value(Entities::Location, geometry(StagedColumn::Loc))
        .value(Entities::ObservationSpace, geometry(StagedColumn::Obs))
        .value(Entities::OperationSpace, geometry(StagedColumn::Ops))
        .value(Entities::ModifiedAt, now())
        .value(
            Entities::ExpiresAt,
            SimpleExpr::from(Func::coalesce([
                staged(StagedColumn::ExpiresAt),
                Expr::col((Entities::Table, Entities::ExpiresAt)),
            ])),
        )
        .from(TableRef::FunctionCall(call, Staged.into_iden()))
        .cond_where(
            Cond::all()
                .add(Expr::col((Entities::Table, Entities::Id)).equals((Staged, StagedColumn::Id)))
                .add(expectation()),
        )
        .returning(Query::returning().columns([(Entities::Table, Entities::EntityKey), (Entities::Table, Entities::Id)]));

    let sql = update.to_string(PostgresQueryBuilder);
    apply_column_alias(sql, &Staged, &columns)
}

/// Build the batch delete.
///
/// Carries the same nullable expectation the update does, and for the same
/// reason: a delete computed from a payload the caller read is a
/// read-modify-write like any other, and one that removed a state its author
/// never saw would be a lost update. `DELETE … USING` is expressed as a
/// correlated `EXISTS`, since sea-query has no `USING` clause and the existence
/// test deletes exactly the rows the staged grain names.
///
/// The payload and system attributes come back because the clause 5.6.10
/// notification carries both and cannot read them once the row is gone.
fn entity_delete_sql() -> String {
    let columns = delete_staging_columns();
    let call = unnest_call(columns.iter().enumerate().map(|(index, column)| (column, external_placeholder(index + 1))));

    let mut staged_rows = Query::select();
    staged_rows.column((Staged, StagedColumn::Id)).from_function(call, Staged).cond_where(
        Cond::all()
            .add(Expr::col((Entities::Table, Entities::Id)).equals((Staged, StagedColumn::Id)))
            .add(expectation()),
    );
    let staged_rows = staged_rows;

    let sql = Query::delete()
        .from_table(Entities::Table)
        .and_where(Expr::exists(staged_rows))
        .returning(Query::returning().columns([Entities::Id, Entities::Payload, Entities::CreatedAt, Entities::ModifiedAt, Entities::ExpiresAt]))
        .to_owned()
        .to_string(PostgresQueryBuilder);
    apply_column_alias(sql, &Staged, &columns)
}

impl EntityRepository {
    /// Insert every entity in `entities` that does not already exist, returning
    /// the rows actually inserted.
    pub async fn create_batch_in_tx(tx: &mut Transaction<'_, Postgres>, entities: &PreparedEntityColumns) -> Result<Vec<WrittenEntity>, ContextStoreError> {
        if entities.ids.is_empty() {
            return Ok(Vec::new());
        }
        types_present(entities)?;

        let ids = entities.ids.iter().map(Uri::as_str).collect::<Vec<_>>();
        let sql = entity_insert_sql()?;
        let rows: Vec<(i64, String)> = sqlx::query_as(sqlx::AssertSqlSafe(sql.as_str()))
            .bind(&ids)
            .bind(&entities.types_json)
            .bind(&entities.scope_paths_json)
            .bind(&entities.payload_json)
            .bind(&entities.locations)
            .bind(&entities.obs_spaces)
            .bind(&entities.op_spaces)
            .bind(&entities.expires_ats)
            .fetch_all(&mut **tx)
            .await
            .map_err(entity_write_failure)?;

        rows.into_iter().map(|(key, id)| stored_written_entity(key, id)).collect()
    }

    /// Overwrite every entity in `entities` whose expectation still holds,
    /// returning the rows actually written.
    ///
    /// `expected_modified_at` is positionally aligned with `entities`; a `None`
    /// entry overwrites unconditionally.
    pub async fn save_batch_in_tx(
        tx: &mut Transaction<'_, Postgres>,
        entities: &PreparedEntityColumns,
        expected_modified_at: &[Option<DateTime<Utc>>],
    ) -> Result<Vec<WrittenEntity>, ContextStoreError> {
        if entities.ids.is_empty() {
            return Ok(Vec::new());
        }
        if expected_modified_at.len() != entities.ids.len() {
            return Err(ContextStoreError::Internal(format!(
                "entity update given {} expectations for {} rows",
                expected_modified_at.len(),
                entities.ids.len()
            )));
        }
        types_present(entities)?;

        let ids = entities.ids.iter().map(Uri::as_str).collect::<Vec<_>>();
        let sql = entity_update_sql();
        let rows: Vec<(i64, String)> = sqlx::query_as(sqlx::AssertSqlSafe(sql.as_str()))
            .bind(&ids)
            .bind(&entities.types_json)
            .bind(&entities.scope_paths_json)
            .bind(&entities.payload_json)
            .bind(&entities.locations)
            .bind(&entities.obs_spaces)
            .bind(&entities.op_spaces)
            .bind(&entities.expires_ats)
            .bind(expected_modified_at)
            .fetch_all(&mut **tx)
            .await
            .map_err(entity_write_failure)?;

        rows.into_iter().map(|(key, id)| stored_written_entity(key, id)).collect()
    }

    /// Delete every entity in `ids` whose expectation still holds, returning
    /// what each one held.
    ///
    /// Nothing else is deleted here. The current attribute values the entity
    /// owns go with it through the one foreign key that cascades from
    /// `entities`, and the entity's temporal evolution deliberately survives:
    /// clause 5.6.6 deletes the Entity, and only clause 5.6.16 deletes its
    /// history.
    ///
    /// `expected_modified_at` is positionally aligned with `ids`; a `None` entry
    /// deletes unconditionally.
    pub async fn delete_batch_in_tx(
        tx: &mut Transaction<'_, Postgres>,
        ids: &[Uri],
        expected_modified_at: &[Option<DateTime<Utc>>],
    ) -> Result<Vec<DeletedEntity>, ContextStoreError> {
        if ids.is_empty() {
            return Ok(Vec::new());
        }
        if expected_modified_at.len() != ids.len() {
            return Err(ContextStoreError::Internal(format!(
                "entity delete given {} expectations for {} rows",
                expected_modified_at.len(),
                ids.len()
            )));
        }

        let ids = ids.iter().map(Uri::as_str).collect::<Vec<_>>();
        let sql = entity_delete_sql();
        let rows = sqlx::query(sqlx::AssertSqlSafe(sql.as_str()))
            .bind(ids)
            .bind(expected_modified_at)
            .fetch_all(&mut **tx)
            .await?;

        rows.into_iter()
            .map(|row| {
                Ok(DeletedEntity {
                    id: stored_uri(row.get("id"))?,
                    payload: row.get("payload"),
                    created_at: row.get("created_at"),
                    modified_at: row.get("modified_at"),
                    expires_at: row.get("expires_at"),
                })
            })
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use crate::entity::write::{entity_delete_sql, entity_insert_sql, entity_update_sql};
    use std::collections::BTreeSet;

    /// Every `$n` placeholder in `sql`, as numbers.
    fn placeholders(sql: &str) -> BTreeSet<u32> {
        sql.match_indices('$')
            .filter_map(|(at, _)| {
                let digits: String = sql[at + 1..].chars().take_while(char::is_ascii_digit).collect();
                digits.parse().ok()
            })
            .collect()
    }

    /// The rendered insert, unwrapped for assertion.
    fn insert() -> String {
        entity_insert_sql().expect("the fixed eight-wide insert builds")
    }

    #[test]
    fn the_insert_binds_contiguous_placeholders_and_names_its_staging_columns() {
        let sql = insert();
        pretty_assertions::assert_eq!(placeholders(&sql), (1..=8).collect::<BTreeSet<u32>>());
        assert!(
            sql.contains("AS d(id, types_json, scope_paths_json, payload, loc, obs, ops, expires_at)"),
            "{sql}"
        );
    }

    #[test]
    fn the_insert_expands_the_type_and_scope_arrays_and_casts_the_scope_to_ltree() {
        let sql = insert();
        assert!(sql.contains(r#"ARRAY(SELECT jsonb_array_elements_text("d"."types_json"))"#), "{sql}");
        assert!(
            sql.contains(r#"ARRAY(SELECT jsonb_array_elements_text("d"."scope_paths_json")::ltree)"#),
            "{sql}"
        );
    }

    #[test]
    fn the_insert_validates_every_geometry_from_its_embedded_srid() {
        let sql = insert();
        for column in ["loc", "obs", "ops"] {
            assert!(sql.contains(&format!(r#"ST_MakeValid(ST_GeomFromEWKB("d"."{column}"))"#)), "{sql}");
        }
        assert!(!sql.contains("ST_SetSRID"), "{sql}");
    }

    #[test]
    fn the_insert_keeps_its_conflict_clause_and_returns_the_surrogate_key() {
        let sql = insert();
        assert!(sql.contains(r#"ON CONFLICT ("id") DO NOTHING"#), "{sql}");
        assert!(sql.contains(r#"RETURNING "entity_key", "id""#), "{sql}");
    }

    #[test]
    fn the_update_honours_a_null_expectation_and_binds_nine_placeholders() {
        let sql = entity_update_sql();
        pretty_assertions::assert_eq!(placeholders(&sql), (1..=9).collect::<BTreeSet<u32>>());
        assert!(
            sql.contains(r#""d"."expected_modified_at" IS NULL OR "entities"."modified_at" = "d"."expected_modified_at""#),
            "{sql}"
        );
    }

    #[test]
    fn the_update_stamps_the_transaction_clock_and_coalesces_the_expiry() {
        let sql = entity_update_sql();
        assert!(sql.contains(r#""modified_at" = NOW()"#), "{sql}");
        assert!(sql.contains(r#"COALESCE("d"."expires_at", "entities"."expires_at")"#), "{sql}");
    }

    #[test]
    fn the_update_reads_its_staging_rows_through_a_function_source() {
        let sql = entity_update_sql();
        assert!(sql.contains("FROM UNNEST("), "{sql}");
        assert!(
            sql.contains("AS d(id, types_json, scope_paths_json, payload, loc, obs, ops, expires_at, expected_modified_at)"),
            "{sql}"
        );
        assert!(sql.contains(r#"RETURNING "entities"."entity_key", "entities"."id""#), "{sql}");
    }

    #[test]
    fn the_delete_is_a_correlated_existence_test_that_honours_the_expectation() {
        let sql = entity_delete_sql();
        pretty_assertions::assert_eq!(placeholders(&sql), (1..=2).collect::<BTreeSet<u32>>());
        assert!(sql.contains(r#"DELETE FROM "entities" WHERE EXISTS"#), "{sql}");
        assert!(sql.contains(r#""entities"."id" = "d"."id""#), "{sql}");
        assert!(
            sql.contains(r#""d"."expected_modified_at" IS NULL OR "entities"."modified_at" = "d"."expected_modified_at""#),
            "{sql}"
        );
    }

    #[test]
    fn the_delete_returns_what_the_notification_needs_and_touches_no_other_relation() {
        let sql = entity_delete_sql();
        for column in ["id", "payload", "created_at", "modified_at", "expires_at"] {
            assert!(sql.contains(&format!(r#""{column}""#)), "the delete must return {column}: {sql}");
        }
        for relation in [
            "current_attribute_values",
            "attribute_history",
            "temporal_entities",
            "temporal_attribute_streams",
        ] {
            assert!(!sql.contains(relation), "entity deletion must not name {relation}: {sql}");
        }
    }
}
