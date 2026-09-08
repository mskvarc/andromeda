//! The batch update: one statement that overwrites each staged entity's payload
//! and rewrites its search and lifecycle rows only where they changed.

use crate::{
    entity::{
        mapper::stored_written_entity,
        params::PreparedEntityColumns,
        repo::EntityRepository,
        types_present::{entity_write_failure, types_present},
        write::staging::{Docs, Ents, ExpectationStaged, Proj, Srch, Staged, StagedColumn, expectation, projected, projection_cte, update_staging_columns},
        written_entity::WrittenEntity,
    },
    error::ContextStoreError,
};
use andromeda_context_space::context_target::ContextTarget;
use andromeda_ngsi_ld_lexicon::uri::Uri;
use andromeda_postgresql_schema::current::{Entities, EntityDocuments, EntitySearch};
use andromeda_postgresql_seaquery::{clock::now, pg_operator::PgBinaryOperator, placeholder::external_placeholder, unnest::apply_column_alias};
use andromeda_telemetry::statement::{StatementName, observe_statement};
use chrono::{DateTime, Utc};
use sea_query::{CommonTableExpression, Cond, Expr, ExprTrait, Func, PostgresQueryBuilder, Query, SimpleExpr, WithClause};
use sqlx::{Postgres, Transaction};

/// The statement name the entity set-update is observed under.
const ENTITY_UPDATE: StatementName = StatementName("entity-update");

/// Build the batch update.
///
/// ```sql
/// WITH proj AS (SELECT … FROM UNNEST(…) AS d(…)),
///      docs AS (UPDATE entity_documents SET payload = proj.payload, modified_at = NOW()
///               FROM proj, entities
///               WHERE entities.id = proj.id AND entity_documents.entity_key = entities.entity_key
///                 AND (proj.expected_modified_at IS NULL OR entity_documents.modified_at = proj.expected_modified_at)
///               RETURNING entity_documents.entity_key, entities.id),
///      srch AS (UPDATE entity_search SET type_ids = proj.type_ids, … FROM docs, proj
///               WHERE proj.id = docs.id AND entity_search.entity_key = docs.entity_key
///                 AND (entity_search.type_ids IS DISTINCT FROM proj.type_ids OR … )),
///      ents AS (UPDATE entities SET expires_at = COALESCE(proj.expires_at, entities.expires_at),
///                                   min_attribute_expires_at = proj.min_attribute_expires_at
///               FROM docs, proj
///               WHERE proj.id = docs.id AND entities.entity_key = docs.entity_key
///                 AND (entities.expires_at IS DISTINCT FROM COALESCE(…) OR entities.min_attribute_expires_at IS DISTINCT FROM proj.min_attribute_expires_at))
/// SELECT entity_key, id FROM docs
/// ```
///
/// The payload write is the anchor. Its expectation column is nullable so one
/// statement carries both an unconditional overwrite and a compare-and-set in
/// the same batch: a null expectation matches whatever is there, a non-null
/// one matches only an untouched row. An identifier that does not come back
/// therefore means either "gone" or "moved on", and which of those it is
/// depends on the precondition the caller attached. The document statement
/// names only the payload and its version token — no column any index of
/// `entity_documents` covers — so the update stays a heap-only tuple.
///
/// The search and lifecycle rows are rewritten only for the entities whose
/// payload write landed, and only where a projected column actually differs
/// from what is stored: `IS DISTINCT FROM` on every column, so a value-only
/// write leaves both rows and every search index untouched. `expires_at` is
/// coalesced so an update that omits it keeps the stored value rather than
/// clearing it; the earliest attribute expiry is replaced, because it is
/// derived from the payload that was just replaced.
pub(super) fn entity_update_sql() -> String {
    let columns = update_staging_columns();

    let mut documents = Query::update();
    documents
        .table(EntityDocuments::Table)
        .value(EntityDocuments::Payload, projected(EntityDocuments::Payload))
        .value(EntityDocuments::ModifiedAt, now())
        .from(Proj)
        .from(Entities::Table)
        .cond_where(
            Cond::all()
                .add(Expr::col((Entities::Table, Entities::Id)).equals((Proj, Entities::Id)))
                .add(Expr::col((Entities::Table, Entities::ContextSpaceKey)).eq(external_placeholder(13)))
                .add(Expr::col((EntityDocuments::Table, EntityDocuments::ContextSpaceKey)).eq(external_placeholder(13)))
                .add(Expr::col((EntityDocuments::Table, EntityDocuments::EntityKey)).equals((Entities::Table, Entities::EntityKey)))
                .add(expectation(projected(StagedColumn::ExpectedModifiedAt), EntityDocuments::Table)),
        )
        .returning(Query::returning().exprs([
            Expr::col((EntityDocuments::Table, EntityDocuments::EntityKey)),
            Expr::col((Entities::Table, Entities::Id)),
        ]));

    let mut search = Query::update();
    search.table(EntitySearch::Table);
    for column in SEARCH_COLUMNS {
        search.value(column, projected(column));
    }
    search.from(Docs).from(Proj).cond_where(
        Cond::all()
            .add(Expr::col((Proj, Entities::Id)).equals((Docs, Entities::Id)))
            .add(Expr::col((EntitySearch::Table, EntitySearch::EntityKey)).equals((Docs, EntityDocuments::EntityKey)))
            .add(Expr::col((EntitySearch::Table, EntitySearch::ContextSpaceKey)).eq(external_placeholder(13)))
            .add(SEARCH_COLUMNS.into_iter().fold(Cond::any(), |changed, column| {
                changed.add(SimpleExpr::from(Expr::col((EntitySearch::Table, column))).distinct_from(projected(column)))
            })),
    );

    let coalesced_expiry = || {
        SimpleExpr::from(Func::coalesce([
            projected(Entities::ExpiresAt),
            Expr::col((Entities::Table, Entities::ExpiresAt)),
        ]))
    };
    let mut lifecycle = Query::update();
    lifecycle
        .table(Entities::Table)
        .value(Entities::ExpiresAt, coalesced_expiry())
        .value(Entities::MinAttributeExpiresAt, projected(Entities::MinAttributeExpiresAt))
        .from(Docs)
        .from(Proj)
        .cond_where(
            Cond::all()
                .add(Expr::col((Proj, Entities::Id)).equals((Docs, Entities::Id)))
                .add(Expr::col((Entities::Table, Entities::EntityKey)).equals((Docs, EntityDocuments::EntityKey)))
                .add(Expr::col((Entities::Table, Entities::ContextSpaceKey)).eq(external_placeholder(13)))
                .add(
                    Cond::any()
                        .add(SimpleExpr::from(Expr::col((Entities::Table, Entities::ExpiresAt))).distinct_from(coalesced_expiry()))
                        .add(
                            SimpleExpr::from(Expr::col((Entities::Table, Entities::MinAttributeExpiresAt)))
                                .distinct_from(projected(Entities::MinAttributeExpiresAt)),
                        ),
                ),
        );

    let statement = WithClause::new()
        .cte(projection_cte(&columns, ExpectationStaged::Yes))
        .cte(CommonTableExpression::new().table_name(Docs).query(documents).to_owned())
        .cte(CommonTableExpression::new().table_name(Srch).query(search).to_owned())
        .cte(CommonTableExpression::new().table_name(Ents).query(lifecycle).to_owned())
        .to_owned()
        .query(Query::select().column(EntityDocuments::EntityKey).column(Entities::Id).from(Docs).take());

    let sql = statement.to_string(PostgresQueryBuilder);
    apply_column_alias(sql, &Staged, &columns)
}

/// The seven `entity_search` columns the update projects, each written and
/// guarded by the same list so no column can be rewritten without being
/// compared, or compared without being written.
const SEARCH_COLUMNS: [EntitySearch; 7] = [
    EntitySearch::TypeIds,
    EntitySearch::AttributeNameIds,
    EntitySearch::ScopePaths,
    EntitySearch::Location,
    EntitySearch::ObservationSpace,
    EntitySearch::OperationSpace,
    EntitySearch::LocationExpiresAt,
];

impl EntityRepository {
    /// Overwrite every entity in `entities` whose expectation still holds,
    /// returning the rows actually written.
    ///
    /// `expected_modified_at` is positionally aligned with `entities`; a `None`
    /// entry overwrites unconditionally.
    ///
    /// # Errors
    ///
    /// Returns [`ContextStoreError`] when the expectation slice is not the same
    /// length as `entities`, when an entity carries no type, when the update
    /// statement cannot be built, when the statement fails, or when a returned
    /// surrogate key or identifier is not a valid written entity.
    pub async fn save_batch_in_tx(
        tx: &mut Transaction<'_, Postgres>,
        target: &ContextTarget,
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
        let rows = sqlx::query_as(sqlx::AssertSqlSafe(sql.as_str()))
            .bind(&ids)
            .bind(&entities.type_ids_json)
            .bind(&entities.attribute_name_ids_json)
            .bind(&entities.scope_paths_json)
            .bind(&entities.payload_json)
            .bind(&entities.locations)
            .bind(&entities.obs_spaces)
            .bind(&entities.op_spaces)
            .bind(&entities.location_expires_ats)
            .bind(&entities.expires_ats)
            .bind(&entities.min_attribute_expires_ats)
            .bind(expected_modified_at)
            .bind(target.key().as_i64())
            .fetch_all(&mut **tx);
        let rows: Vec<(i64, String)> = observe_statement(ENTITY_UPDATE, rows).await.map_err(entity_write_failure)?;

        rows.iter().map(|(key, id)| stored_written_entity(*key, id)).collect()
    }
}

#[cfg(test)]
mod tests {
    use crate::entity::write::{placeholders, update::entity_update_sql};
    use std::collections::BTreeSet;

    #[test]
    fn the_update_binds_twelve_placeholders_and_names_its_staging_columns() {
        let sql = entity_update_sql();
        pretty_assertions::assert_eq!(placeholders(&sql), (1..=13).collect::<BTreeSet<u32>>());
        assert!(
            sql.contains(
                "AS d(id, type_ids_json, attribute_name_ids_json, scope_paths_json, payload, loc, obs, ops, loc_expires_at, expires_at, min_attribute_expires_at, expected_modified_at)"
            ),
            "{sql}"
        );
    }

    #[test]
    fn the_document_write_is_the_compare_and_set_anchor_and_names_only_heap_neutral_columns() {
        let sql = entity_update_sql();
        assert!(
            sql.contains(r#""docs" AS (UPDATE "entity_documents" SET "payload" = "proj"."payload", "modified_at" = NOW() FROM "proj", "entities" WHERE"#),
            "{sql}"
        );
        assert!(
            sql.contains(r#""proj"."expected_modified_at" IS NULL OR "entity_documents"."modified_at" = "proj"."expected_modified_at""#),
            "{sql}"
        );
        assert!(sql.contains(r#"RETURNING "entity_documents"."entity_key", "entities"."id")"#), "{sql}");
        assert!(sql.ends_with(r#"SELECT "entity_key", "id" FROM "docs""#), "{sql}");
    }

    #[test]
    fn the_search_row_is_rewritten_only_where_a_projected_column_differs() {
        let sql = entity_update_sql();
        assert!(sql.contains(r#""srch" AS (UPDATE "entity_search" SET "type_ids" = "proj"."type_ids""#), "{sql}");
        pretty_assertions::assert_eq!(
            sql.matches("IS DISTINCT FROM").count(),
            9,
            "seven search columns and two lifecycle columns are guarded: {sql}"
        );
        for column in [
            "type_ids",
            "attribute_name_ids",
            "scope_paths",
            "location",
            "observation_space",
            "operation_space",
            "location_expires_at",
        ] {
            assert!(
                sql.contains(&format!(r#""entity_search"."{column}" IS DISTINCT FROM "proj"."{column}""#)),
                "{column} must be guarded: {sql}"
            );
        }
        assert!(sql.contains(r#""entity_search"."entity_key" = "docs"."entity_key""#), "{sql}");
    }

    #[test]
    fn the_lifecycle_row_coalesces_the_expiry_and_replaces_the_attribute_expiry() {
        let sql = entity_update_sql();
        assert!(sql.contains(r#""ents" AS (UPDATE "entities" SET "expires_at" = COALESCE("proj"."expires_at", "entities"."expires_at"), "min_attribute_expires_at" = "proj"."min_attribute_expires_at""#), "{sql}");
        assert!(
            sql.contains(r#""entities"."expires_at" IS DISTINCT FROM COALESCE("proj"."expires_at", "entities"."expires_at")"#),
            "{sql}"
        );
        assert!(!sql.contains(r#"SET "id""#), "the identity is never rewritten: {sql}");
    }

    #[test]
    fn the_projection_validates_each_geometry_once() {
        let sql = entity_update_sql();
        pretty_assertions::assert_eq!(sql.matches("ST_MakeValid").count(), 3, "{sql}");
        assert!(sql.starts_with(r#"WITH "proj" AS ("#), "{sql}");
    }
}
