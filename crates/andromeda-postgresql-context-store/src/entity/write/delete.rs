//! The batch delete: one statement that removes each staged entity whose
//! expectation still holds and hands back what it held.

use crate::{
    entity::{
        mapper::stored_uri,
        repo::EntityRepository,
        write::staging::{Doomed, Staged, StagedColumn, delete_staging_columns, expectation, staged, staging_source},
    },
    error::ContextStoreError,
};
use andromeda_context_commit::deleted_entity::DeletedEntity;
use andromeda_context_space::context_target::ContextTarget;
use andromeda_ngsi_ld_lexicon::uri::Uri;
use andromeda_postgresql_schema::current::{E, Entities, EntityDocuments};
use andromeda_postgresql_seaquery::{placeholder::external_placeholder, unnest::apply_column_alias};
use andromeda_telemetry::statement::{StatementName, observe_statement};
use chrono::{DateTime, Utc};
use sea_query::{CommonTableExpression, Expr, ExprTrait, Iden, IntoIden, JoinType, LockType, PostgresQueryBuilder, Query, WithClause};
use sqlx::{Postgres, Row, Transaction};

/// The statement name the entity set-delete is observed under.
const ENTITY_DELETE: StatementName = StatementName("entity-delete");

/// Alias `doc` for the `entity_documents` row the delete reads its payload and
/// version token from.
#[derive(Iden, Clone, Copy)]
#[iden = "doc"]
struct Doc;

/// The delete CTE, `gone`, whose only purpose is to run.
#[derive(Iden, Clone, Copy)]
#[iden = "gone"]
struct Gone;

/// Build the batch delete.
///
/// ```sql
/// WITH doomed AS (SELECT e.entity_key, e.id, doc.payload, e.created_at, doc.modified_at, e.expires_at
///                 FROM UNNEST(…) AS d(id, expected_modified_at)
///                 JOIN entities e ON e.id = d.id
///                 JOIN entity_documents doc ON doc.entity_key = e.entity_key
///                 WHERE d.expected_modified_at IS NULL OR doc.modified_at = d.expected_modified_at
///                 FOR UPDATE OF e, doc),
///      gone AS (DELETE FROM entities WHERE entity_key IN (SELECT entity_key FROM doomed))
/// SELECT id, payload, created_at, modified_at, expires_at FROM doomed
/// ```
///
/// Carries the same nullable expectation the update does, and for the same
/// reason: a delete computed from a payload the caller read is a
/// read-modify-write like any other, and one that removed a state its author
/// never saw would be a lost update. The candidates are locked before the
/// delete so a concurrent payload write cannot slip between the expectation
/// check and the removal.
///
/// Only `entities` is named in the delete: the document and search rows go
/// through the ownership edges that cascade from it, and so does the current
/// attribute projection. Clause 5.6.6 leaves the temporal evolution standing,
/// so no history relation is touched. The payload and system attributes come
/// back because the clause 5.6.10 notification carries both and cannot read
/// them once the rows are gone.
pub(super) fn entity_delete_sql() -> String {
    let columns = delete_staging_columns();

    let mut doomed = Query::select();
    doomed
        .expr(Expr::col((E, Entities::EntityKey)))
        .expr(Expr::col((E, Entities::Id)))
        .expr(Expr::col((Doc, EntityDocuments::Payload)))
        .expr(Expr::col((E, Entities::CreatedAt)))
        .expr(Expr::col((Doc, EntityDocuments::ModifiedAt)))
        .expr(Expr::col((E, Entities::ExpiresAt)))
        .from_function(staging_source(&columns), Staged)
        .join_as(
            JoinType::Join,
            Entities::Table,
            E,
            Expr::col((E, Entities::Id)).equals((Staged, StagedColumn::Id)),
        )
        .join_as(
            JoinType::Join,
            EntityDocuments::Table,
            Doc,
            Expr::col((Doc, EntityDocuments::EntityKey)).equals((E, Entities::EntityKey)),
        )
        .and_where(Expr::col((E, Entities::ContextSpaceKey)).eq(external_placeholder(3)))
        .and_where(Expr::col((Doc, EntityDocuments::ContextSpaceKey)).eq(external_placeholder(3)))
        .cond_where(expectation(staged(StagedColumn::ExpectedModifiedAt), Doc))
        .lock_with_tables(LockType::Update, [E.into_iden(), Doc.into_iden()]);

    let gone = Query::delete()
        .from_table(Entities::Table)
        .and_where(Expr::col(Entities::EntityKey).in_subquery(Query::select().column(Entities::EntityKey).from(Doomed).take()))
        .take();

    let statement = WithClause::new()
        .cte(CommonTableExpression::new().table_name(Doomed).query(doomed.take()).to_owned())
        .cte(CommonTableExpression::new().table_name(Gone).query(gone).to_owned())
        .to_owned()
        .query(
            Query::select()
                .columns([
                    Entities::Id.into_iden(),
                    EntityDocuments::Payload.into_iden(),
                    Entities::CreatedAt.into_iden(),
                    EntityDocuments::ModifiedAt.into_iden(),
                    Entities::ExpiresAt.into_iden(),
                ])
                .from(Doomed)
                .take(),
        );

    let sql = statement.to_string(PostgresQueryBuilder);
    apply_column_alias(sql, &Staged, &columns)
}

impl EntityRepository {
    /// Delete every entity in `ids` whose expectation still holds, returning
    /// what each one held.
    ///
    /// Nothing else is deleted here. The document, search and current attribute
    /// rows the entity owns go with it through the foreign keys that cascade
    /// from `entities`, and the entity's temporal evolution deliberately
    /// survives: clause 5.6.6 deletes the Entity, and only clause 5.6.16
    /// deletes its history.
    ///
    /// `expected_modified_at` is positionally aligned with `ids`; a `None` entry
    /// deletes unconditionally.
    ///
    /// # Errors
    ///
    /// Returns [`ContextStoreError::Internal`] when the expectation slice is not
    /// the same length as `ids` or a returned identifier is no longer a valid
    /// URI, and [`ContextStoreError::Database`] when the delete fails.
    pub async fn delete_batch_in_tx(
        tx: &mut Transaction<'_, Postgres>,
        target: &ContextTarget,
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
            .bind(target.key().as_i64())
            .fetch_all(&mut **tx);
        let rows = observe_statement(ENTITY_DELETE, rows).await?;

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
    use crate::entity::write::{delete::entity_delete_sql, placeholders};
    use std::collections::BTreeSet;

    #[test]
    fn the_delete_locks_its_candidates_and_honours_the_expectation() {
        let sql = entity_delete_sql();
        pretty_assertions::assert_eq!(placeholders(&sql), (1..=3).collect::<BTreeSet<u32>>());
        assert!(
            sql.starts_with(
                r#"WITH "doomed" AS (SELECT "e"."entity_key", "e"."id", "doc"."payload", "e"."created_at", "doc"."modified_at", "e"."expires_at" FROM UNNEST("#
            ),
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
        let sql = entity_delete_sql();
        assert!(
            sql.contains(r#""gone" AS (DELETE FROM "entities" WHERE "entity_key" IN (SELECT "entity_key" FROM "doomed"))"#),
            "{sql}"
        );
        assert!(
            sql.ends_with(r#"SELECT "id", "payload", "created_at", "modified_at", "expires_at" FROM "doomed""#),
            "{sql}"
        );
    }

    #[test]
    fn the_delete_touches_no_other_relation() {
        let sql = entity_delete_sql();
        for relation in [
            "current_attribute_values",
            "attribute_history",
            "temporal_entities",
            "temporal_attribute_streams",
            "entity_search",
        ] {
            assert!(!sql.contains(relation), "entity deletion must not name {relation}: {sql}");
        }
        assert!(!sql.contains(r#"DELETE FROM "entity_documents""#), "the document row cascades: {sql}");
    }
}
