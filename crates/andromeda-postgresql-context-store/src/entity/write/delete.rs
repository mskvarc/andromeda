//! The batch delete: one statement that removes each staged entity whose
//! expectation still holds and hands back the system attributes it held.

use crate::{
    branch_statement::BranchStatement,
    entity::{
        census::delta::{CensusChange, CensusStatements},
        mapper::stored_uri,
        repo::EntityRepository,
        write::{
            search_image::SearchImage,
            staging::{Doomed, Srch, Staged, StagedColumn, delete_staging_columns, expectation, staged, staging_source},
        },
    },
    error::ContextStoreError,
    store_invariant::{GuardedEntityWrite, StoreInvariant},
};
use andromeda_context_commit::deleted_entity::DeletedEntity;
use andromeda_context_space::context_target::ContextTarget;
use andromeda_ngsi_ld_lexicon::identifier::Identifier;
use andromeda_postgresql_branch::table_of;
use andromeda_postgresql_schema::current::{E, Entities, EntityDocuments, EntitySearch};
use andromeda_postgresql_seaquery::unnest::apply_column_alias;
use andromeda_telemetry::statement::{StatementName, observe_statement};
use chrono::{DateTime, Utc};
use sea_query::{CommonTableExpression, Expr, ExprTrait, Iden, IntoIden, JoinType, LockType, PostgresQueryBuilder, Query, WithClause};
use sqlx::{Postgres, Row, Transaction};

/// The statement name the entity set-delete is observed under.
const ENTITY_DELETE: StatementName = StatementName("entity-delete");

/// Alias `doc` for the `entity_documents` row the delete reads its version
/// token from.
#[derive(Iden, Clone, Copy)]
#[iden = "doc"]
struct Doc;

/// The delete CTE, `gone`, whose only purpose is to run.
#[derive(Iden, Clone, Copy)]
#[iden = "gone"]
struct Gone;

/// The batch delete's text, built once for the live branch.
static ENTITY_DELETE_STATEMENT: BranchStatement = BranchStatement::new(entity_delete_sql);

/// Build the batch delete.
///
/// ```sql
/// WITH doomed AS (SELECT e.entity_key, e.id, e.created_at, doc.modified_at, e.expires_at
///                 FROM UNNEST(…) AS d(id, expected_modified_at)
///                 JOIN entities e ON e.id = d.id
///                 JOIN entity_documents doc ON doc.entity_key = e.entity_key
///                 WHERE d.expected_modified_at IS NULL OR doc.modified_at = d.expected_modified_at
///                 FOR UPDATE OF e, doc),
///      srch(entity_key, old_types, old_instance_names, old_instance_kinds) AS
///              (DELETE FROM entity_search WHERE entity_key IN (SELECT entity_key FROM doomed)
///               RETURNING entity_key, entity_type_name_keys, attribute_instance_name_keys, attribute_instance_kinds),
///      gone AS (DELETE FROM entities WHERE entity_key IN (SELECT entity_key FROM doomed)),
///      census_types AS (INSERT INTO entity_type_census … FROM srch …), census_type_attributes AS (…), census_attributes AS (…)
/// SELECT id, created_at, modified_at, expires_at FROM doomed
/// ```
///
/// Carries the same nullable expectation the update does, and for the same
/// reason: a delete computed from a payload the caller read is a
/// read-modify-write like any other, and one that removed a state its author
/// never saw would be a lost update. The candidates are locked before the
/// delete so a concurrent payload write cannot slip between the expectation
/// check and the removal.
///
/// The search row is deleted by name so that the rows it held come back: the
/// discovery census counts every one of their types and attribute instances
/// out, from the row the delete removed rather than from the statement's
/// snapshot of it. Everything else goes through the ownership edges that
/// cascade from `entities`: the document row, the type memberships and the
/// current attribute projection. Clause 5.6.6 leaves the temporal evolution
/// standing, so no history relation is touched. The system attributes come
/// back because the clause 5.6.10 notification carries them and cannot read
/// them once the rows are gone; the payload does not, because every caller read
/// it before the delete and decoding it again would only double its cost.
///
/// # Errors
///
/// Returns [`ContextStoreError::StatementBuild`] if a census ledger insert ever stops
/// matching its column list, which the fixed shapes make unreachable.
pub(super) fn entity_delete_sql(target: &ContextTarget) -> Result<String, ContextStoreError> {
    let columns = delete_staging_columns();

    let mut doomed = Query::select();
    doomed
        .expr(Expr::col((E, Entities::EntityKey)))
        .expr(Expr::col((E, Entities::Id)))
        .expr(Expr::col((E, Entities::CreatedAt)))
        .expr(Expr::col((Doc, EntityDocuments::ModifiedAt)))
        .expr(Expr::col((E, Entities::ExpiresAt)))
        .from_function(staging_source(&columns), Staged)
        .join_as(
            JoinType::Join,
            table_of(target, Entities::Table),
            E,
            Expr::col((E, Entities::Id)).equals((Staged, StagedColumn::Id)),
        )
        .join_as(
            JoinType::Join,
            table_of(target, EntityDocuments::Table),
            Doc,
            Expr::col((Doc, EntityDocuments::EntityKey)).equals((E, Entities::EntityKey)),
        )
        .cond_where(expectation(staged(StagedColumn::ExpectedModifiedAt), Doc))
        .lock_with_tables(LockType::Update, [E.into_iden(), Doc.into_iden()]);

    let doomed_keys = || Query::select().column(Entities::EntityKey).from(Doomed).take();
    let search = Query::delete()
        .from_table(table_of(target, EntitySearch::Table))
        .and_where(Expr::col(EntitySearch::EntityKey).in_subquery(doomed_keys()))
        .returning(Query::returning().columns([
            EntitySearch::EntityKey,
            EntitySearch::EntityTypeNameKeys,
            EntitySearch::AttributeInstanceNameKeys,
            EntitySearch::AttributeInstanceKinds,
        ]))
        .take();
    let gone = Query::delete()
        .from_table(table_of(target, Entities::Table))
        .and_where(Expr::col(Entities::EntityKey).in_subquery(doomed_keys()))
        .take();

    let mut with = WithClause::new();
    with.cte(CommonTableExpression::new().table_name(Doomed).query(doomed.take()).to_owned())
        .cte(
            CommonTableExpression::new()
                .table_name(Srch)
                .columns([
                    EntitySearch::EntityKey.into_iden(),
                    SearchImage::OldTypes.into_iden(),
                    SearchImage::OldInstanceNames.into_iden(),
                    SearchImage::OldInstanceKinds.into_iden(),
                ])
                .query(search)
                .to_owned(),
        )
        .cte(CommonTableExpression::new().table_name(Gone).query(gone).to_owned());
    for census in CensusStatements::of(target, Srch, CensusChange::Removed)?.ctes() {
        with.cte(census);
    }
    let statement = with.query(
        Query::select()
            .columns([
                Entities::Id.into_iden(),
                Entities::CreatedAt.into_iden(),
                EntityDocuments::ModifiedAt.into_iden(),
                Entities::ExpiresAt.into_iden(),
            ])
            .from(Doomed)
            .take(),
    );

    let sql = statement.to_string(PostgresQueryBuilder);
    Ok(apply_column_alias(sql, &Staged, &columns))
}

impl EntityRepository {
    /// Delete every entity in `ids` whose expectation still holds, returning
    /// the system attributes each one held.
    ///
    /// The search row is deleted beside the identity row so the discovery
    /// census can count out what it held; the document, membership and current
    /// attribute rows the entity owns go with it through the foreign keys that
    /// cascade from `entities`, and the entity's temporal evolution
    /// deliberately survives: clause 5.6.6 deletes the Entity, and only clause
    /// 5.6.16 deletes its history.
    ///
    /// `expected_modified_at` is positionally aligned with `ids`; a `None` entry
    /// deletes unconditionally.
    ///
    /// # Errors
    ///
    /// Returns [`ContextStoreError::Invariant`] when the expectation slice is not
    /// the same length as `ids`, a returned identifier is no longer a valid URI
    /// or the statement cannot be built, and [`ContextStoreError::Database`]
    /// when the delete fails.
    pub async fn delete_batch_in_tx(
        tx: &mut Transaction<'_, Postgres>,
        target: &ContextTarget,
        ids: &[Identifier],
        expected_modified_at: &[Option<DateTime<Utc>>],
    ) -> Result<Vec<DeletedEntity>, ContextStoreError> {
        if ids.is_empty() {
            return Ok(Vec::new());
        }
        if expected_modified_at.len() != ids.len() {
            return Err(StoreInvariant::ExpectationCount {
                write: GuardedEntityWrite::Delete,
                expectations: expected_modified_at.len(),
                rows: ids.len(),
            }
            .into());
        }

        let ids = ids.iter().map(Identifier::as_str).collect::<Vec<_>>();
        let sql = ENTITY_DELETE_STATEMENT.text(target)?;
        let rows = sqlx::query(sqlx::AssertSqlSafe(&*sql))
            .bind(ids)
            .bind(expected_modified_at)
            .fetch_all(&mut **tx);
        let rows = observe_statement(ENTITY_DELETE, rows).await?;

        rows.into_iter()
            .map(|row| {
                Ok(DeletedEntity {
                    id: stored_uri(row.try_get("id")?)?,
                    created_at: row.try_get("created_at")?,
                    modified_at: row.try_get("modified_at")?,
                    expires_at: row.try_get("expires_at")?,
                })
            })
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use crate::entity::write::{delete::entity_delete_sql, placeholders};
    use andromeda_context_space::context_target::ContextTarget;
    use std::collections::BTreeSet;

    #[test]
    fn the_delete_locks_its_candidates_and_honours_the_expectation() {
        let sql = entity_delete_sql(&ContextTarget::Live).expect("the fixed delete builds");
        pretty_assertions::assert_eq!(placeholders(&sql), (1..=2).collect::<BTreeSet<u32>>());
        assert!(
            sql.starts_with(r#"WITH "doomed" AS (SELECT "e"."entity_key", "e"."id", "e"."created_at", "doc"."modified_at", "e"."expires_at" FROM UNNEST("#),
            "{sql}"
        );
        assert!(sql.contains("AS d(id, expected_modified_at)"), "{sql}");
        assert!(
            sql.contains(r#"JOIN "entities" AS "e" ON "e"."id" = "d"."id" JOIN "entity_documents" AS "doc" ON "doc"."entity_key" = "e"."entity_key""#),
            "{sql}"
        );
        assert!(
            sql.contains(r#""d"."expected_modified_at" IS NULL OR "doc"."modified_at" = "d"."expected_modified_at""#),
            "{sql}"
        );
        assert!(sql.contains(r#"FOR UPDATE OF "e", "doc")"#), "{sql}");
    }

    #[test]
    fn the_delete_removes_identity_rows_by_key_and_returns_what_the_notification_needs() {
        let sql = entity_delete_sql(&ContextTarget::Live).expect("the fixed delete builds");
        assert!(
            sql.contains(r#""gone" AS (DELETE FROM "entities" WHERE "entity_key" IN (SELECT "entity_key" FROM "doomed"))"#),
            "{sql}"
        );
        assert!(
            sql.ends_with(r#"SELECT "id", "created_at", "modified_at", "expires_at" FROM "doomed""#),
            "{sql}"
        );
    }

    /// Clause 5.6.6 leaves the temporal evolution standing, and the document,
    /// membership and projection rows cascade from `entities`.
    #[test]
    fn the_delete_names_only_the_identity_the_search_row_and_the_census() {
        let sql = entity_delete_sql(&ContextTarget::Live).expect("the fixed delete builds");
        for relation in [
            "current_attribute_values",
            "attribute_history",
            "temporal_entities",
            "temporal_attribute_streams",
            "entity_type_members",
        ] {
            assert!(!sql.contains(relation), "entity deletion must not name {relation}: {sql}");
        }
        assert!(!sql.contains(r#"DELETE FROM "entity_documents""#), "the document row cascades: {sql}");
    }

    /// The census counts out the search rows the delete removed, read from its
    /// own `RETURNING`.
    #[test]
    fn the_delete_counts_the_removed_search_rows_out_of_the_census() {
        let sql = entity_delete_sql(&ContextTarget::Live).expect("the fixed delete builds");
        assert!(
            sql.contains(r#""srch" ("entity_key", "old_types", "old_instance_names", "old_instance_kinds") AS (DELETE FROM "entity_search" WHERE "entity_key" IN (SELECT "entity_key" FROM "doomed") RETURNING "entity_key", "entity_type_name_keys", "attribute_instance_name_keys", "attribute_instance_kinds")"#),
            "{sql}"
        );
        for ledger in ["entity_type_census", "entity_type_attribute_census", "attribute_census"] {
            assert!(sql.contains(&format!(r#"INSERT INTO "{ledger}""#)), "{ledger}: {sql}");
        }
        assert!(sql.contains("-1 AS \"sign\""), "{sql}");
    }
}
