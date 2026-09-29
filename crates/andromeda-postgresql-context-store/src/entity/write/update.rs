//! The batch update: one statement that overwrites each staged entity's payload
//! and rewrites its search and lifecycle rows only where they changed.

use crate::{
    entity::{
        census::delta::{CensusChange, CensusStatements},
        mapper::stored_written_entity,
        params::{PreparedEntityColumns, literal_texts},
        repo::EntityRepository,
        types_present::{entity_write_failure, types_present},
        write::{
            search_image::SearchImage,
            staging::{Docs, Ents, ExpectationStaged, Proj, Srch, Staged, StagedColumn, expectation, projected, projection_cte, update_staging_columns},
            type_membership::{Mbrs, Strays, added_memberships, dropped_memberships},
        },
        written_entity::WrittenEntity,
    },
    error::ContextStoreError,
};
use andromeda_context_space::context_target::ContextTarget;
use andromeda_ngsi_ld_lexicon::identifier::Identifier;
use andromeda_postgresql_branch::table_of;
use andromeda_postgresql_schema::current::{Entities, EntityDocuments, EntitySearch};
use andromeda_postgresql_seaquery::{
    clock::now,
    pg_operator::PgBinaryOperator,
    returning_image::{New, Old},
    transaction_id::current_transaction_id,
    unnest::apply_column_alias,
};
use andromeda_telemetry::statement::{StatementName, observe_statement};
use chrono::{DateTime, Utc};
use sea_query::{CommonTableExpression, Cond, Expr, ExprTrait, Func, IntoIden, PostgresQueryBuilder, Query, SimpleExpr, WithClause};
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
///      srch(entity_key, id, old_types, new_types, old_instance_names, …) AS
///              (UPDATE entity_search SET entity_type_name_keys = proj.entity_type_name_keys, …, written_xid = pg_current_xact_id() FROM docs, proj
///               WHERE proj.id = docs.id AND entity_search.entity_key = docs.entity_key
///                 AND (entity_search.entity_type_name_keys IS DISTINCT FROM proj.entity_type_name_keys OR … )
///               RETURNING entity_search.entity_key, docs.id, old.entity_type_name_keys, new.entity_type_name_keys, …),
///      ents AS (UPDATE entities SET expires_at = COALESCE(proj.expires_at, entities.expires_at),
///                                   min_attribute_expires_at = proj.min_attribute_expires_at
///               FROM docs, proj
///               WHERE proj.id = docs.id AND entities.entity_key = docs.entity_key
///                 AND (entities.expires_at IS DISTINCT FROM COALESCE(…) OR entities.min_attribute_expires_at IS DISTINCT FROM proj.min_attribute_expires_at)),
///      strays AS (DELETE FROM entity_type_members WHERE (entity_type_name_key, id) IN
///                 (SELECT <old type the new row lacks>, srch.id FROM srch …)),
///      mbrs AS (INSERT INTO entity_type_members (…) SELECT srch.entity_key, <new type the old row lacked>, srch.id FROM srch …),
///      census_shapes AS (SELECT <image arrays>, SUM(±1) FROM srch … GROUP BY <image arrays>),
///      census_types AS (INSERT INTO entity_type_census …), census_type_attributes AS (…), census_attributes AS (…)
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
/// derived from the payload that was just replaced. The type memberships and
/// the discovery census move with the search row: both are derived from the
/// pre- and post-image its write returns (PostgreSQL 18 `RETURNING old.*,
/// new.*`), which is the row the write actually replaced, found after any
/// concurrent writer it waited on. A value-only write rewrites no search row,
/// so it moves no membership and counts nothing.
///
/// # Errors
///
/// Returns [`ContextStoreError::Internal`] if the membership insert ever stops
/// matching its column list, which its fixed shape makes unreachable.
pub(super) fn entity_update_sql(target: &ContextTarget) -> Result<String, ContextStoreError> {
    let columns = update_staging_columns();

    let mut documents = Query::update();
    documents
        .table(table_of(target, EntityDocuments::Table))
        .value(EntityDocuments::Payload, projected(EntityDocuments::Payload))
        .value(EntityDocuments::ModifiedAt, now())
        .from(Proj)
        .from(table_of(target, Entities::Table))
        .cond_where(
            Cond::all()
                .add(Expr::col((Entities::Table, Entities::Id)).equals((Proj, Entities::Id)))
                .add(Expr::col((EntityDocuments::Table, EntityDocuments::EntityKey)).equals((Entities::Table, Entities::EntityKey)))
                .add(expectation(projected(StagedColumn::ExpectedModifiedAt), EntityDocuments::Table)),
        )
        .returning(Query::returning().exprs([
            Expr::col((EntityDocuments::Table, EntityDocuments::EntityKey)),
            Expr::col((Entities::Table, Entities::Id)),
        ]));

    let mut search = Query::update();
    search.table(table_of(target, EntitySearch::Table));
    for column in SEARCH_COLUMNS {
        search.value(column, projected(column));
    }
    // A rewritten search row is a changed type, attribute name, scope or
    // geometry, which a spatial index catching up from the store must find.
    search.value(EntitySearch::WrittenXid, current_transaction_id());
    search.from(Docs).from(Proj).cond_where(
        Cond::all()
            .add(Expr::col((Proj, Entities::Id)).equals((Docs, Entities::Id)))
            .add(Expr::col((EntitySearch::Table, EntitySearch::EntityKey)).equals((Docs, EntityDocuments::EntityKey)))
            .add(SEARCH_COLUMNS.into_iter().fold(Cond::any(), |changed, column| {
                changed.add(SimpleExpr::from(Expr::col((EntitySearch::Table, column))).distinct_from(projected(column)))
            })),
    );
    search.returning(Query::returning().exprs([
        Expr::col((EntitySearch::Table, EntitySearch::EntityKey)),
        Expr::col((Docs, Entities::Id)),
        Expr::col((Old, EntitySearch::EntityTypeNameKeys)),
        Expr::col((New, EntitySearch::EntityTypeNameKeys)),
        Expr::col((Old, EntitySearch::AttributeInstanceNameKeys)),
        Expr::col((New, EntitySearch::AttributeInstanceNameKeys)),
        Expr::col((Old, EntitySearch::AttributeInstanceKinds)),
        Expr::col((New, EntitySearch::AttributeInstanceKinds)),
    ]));

    let coalesced_expiry = || {
        SimpleExpr::from(Func::coalesce([
            projected(Entities::ExpiresAt),
            Expr::col((Entities::Table, Entities::ExpiresAt)),
        ]))
    };
    let mut lifecycle = Query::update();
    lifecycle
        .table(table_of(target, Entities::Table))
        .value(Entities::ExpiresAt, coalesced_expiry())
        .value(Entities::MinAttributeExpiresAt, projected(Entities::MinAttributeExpiresAt))
        .from(Docs)
        .from(Proj)
        .cond_where(
            Cond::all()
                .add(Expr::col((Proj, Entities::Id)).equals((Docs, Entities::Id)))
                .add(Expr::col((Entities::Table, Entities::EntityKey)).equals((Docs, EntityDocuments::EntityKey)))
                .add(
                    Cond::any()
                        .add(SimpleExpr::from(Expr::col((Entities::Table, Entities::ExpiresAt))).distinct_from(coalesced_expiry()))
                        .add(
                            SimpleExpr::from(Expr::col((Entities::Table, Entities::MinAttributeExpiresAt)))
                                .distinct_from(projected(Entities::MinAttributeExpiresAt)),
                        ),
                ),
        );

    let mut with = WithClause::new();
    with.cte(projection_cte(&columns, ExpectationStaged::Yes))
        .cte(CommonTableExpression::new().table_name(Docs).query(documents).to_owned())
        .cte(
            CommonTableExpression::new()
                .table_name(Srch)
                .columns([
                    EntitySearch::EntityKey.into_iden(),
                    Entities::Id.into_iden(),
                    SearchImage::OldTypes.into_iden(),
                    SearchImage::NewTypes.into_iden(),
                    SearchImage::OldInstanceNames.into_iden(),
                    SearchImage::NewInstanceNames.into_iden(),
                    SearchImage::OldInstanceKinds.into_iden(),
                    SearchImage::NewInstanceKinds.into_iden(),
                ])
                .query(search)
                .to_owned(),
        )
        .cte(CommonTableExpression::new().table_name(Ents).query(lifecycle).to_owned())
        .cte(CommonTableExpression::new().table_name(Strays).query(dropped_memberships(target)).to_owned())
        .cte(CommonTableExpression::new().table_name(Mbrs).query(added_memberships(target)?).to_owned());
    for census in CensusStatements::of(target, Srch, CensusChange::Replaced)?.ctes() {
        with.cte(census);
    }
    let statement = with.query(Query::select().column(EntityDocuments::EntityKey).column(Entities::Id).from(Docs).take());

    let sql = statement.to_string(PostgresQueryBuilder);
    Ok(apply_column_alias(sql, &Staged, &columns))
}

/// The nine `entity_search` columns the update projects, each written and
/// guarded by the same list so no column can be rewritten without being
/// compared, or compared without being written.
const SEARCH_COLUMNS: [EntitySearch; 9] = [
    EntitySearch::EntityTypeNameKeys,
    EntitySearch::AttributeNameKeys,
    EntitySearch::AttributeInstanceNameKeys,
    EntitySearch::AttributeInstanceKinds,
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

        let ids = entities.ids.iter().map(Identifier::as_str).collect::<Vec<_>>();
        let sql = entity_update_sql(target)?;
        let rows = sqlx::query_as(sqlx::AssertSqlSafe(sql.as_str()))
            .bind(&ids)
            .bind(literal_texts(&entities.entity_type_name_keys))
            .bind(literal_texts(&entities.attribute_name_keys))
            .bind(literal_texts(&entities.attribute_instance_name_keys))
            .bind(literal_texts(&entities.attribute_instance_kinds))
            .bind(&entities.scope_paths_json)
            .bind(&entities.payload_json)
            .bind(&entities.locations)
            .bind(&entities.obs_spaces)
            .bind(&entities.op_spaces)
            .bind(&entities.location_expires_ats)
            .bind(&entities.expires_ats)
            .bind(&entities.min_attribute_expires_ats)
            .bind(expected_modified_at)
            .fetch_all(&mut **tx);
        let rows: Vec<(i64, String)> = observe_statement(ENTITY_UPDATE, rows).await.map_err(entity_write_failure)?;

        rows.iter().map(|(key, id)| stored_written_entity(*key, id)).collect()
    }
}

#[cfg(test)]
mod tests {
    use crate::entity::write::{placeholders, update::entity_update_sql};
    use andromeda_context_space::context_target::ContextTarget;
    use std::collections::BTreeSet;

    #[test]
    fn the_update_binds_fourteen_placeholders_and_names_its_staging_columns() {
        let sql = entity_update_sql(&ContextTarget::Live).expect("the fixed update builds");
        pretty_assertions::assert_eq!(placeholders(&sql), (1..=14).collect::<BTreeSet<u32>>());
        assert!(
            sql.contains(
                "AS d(id, entity_type_name_keys_literal, attribute_name_keys_literal, attribute_instance_name_keys_literal, attribute_instance_kinds_literal, scope_paths_json, payload, loc, obs, ops, loc_expires_at, expires_at, min_attribute_expires_at, expected_modified_at)"
            ),
            "{sql}"
        );
    }

    #[test]
    fn the_document_write_is_the_compare_and_set_anchor_and_names_only_heap_neutral_columns() {
        let sql = entity_update_sql(&ContextTarget::Live).expect("the fixed update builds");
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
        let sql = entity_update_sql(&ContextTarget::Live).expect("the fixed update builds");
        assert!(
            sql.contains(r#""srch" ("entity_key", "id", "old_types", "new_types", "old_instance_names", "new_instance_names", "old_instance_kinds", "new_instance_kinds") AS (UPDATE "entity_search" SET "entity_type_name_keys" = "proj"."entity_type_name_keys""#),
            "{sql}"
        );
        pretty_assertions::assert_eq!(
            sql.matches("IS DISTINCT FROM").count(),
            11,
            "nine search columns and two lifecycle columns are guarded: {sql}"
        );
        for column in [
            "entity_type_name_keys",
            "attribute_name_keys",
            "attribute_instance_name_keys",
            "attribute_instance_kinds",
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

    /// A spatial index catching up from the store reads the search rows written
    /// since its last read, so every rewrite of one must stamp the writing
    /// transaction; the stamp itself is not guarded, or it would rewrite rows
    /// no projected column changed.
    #[test]
    fn a_rewritten_search_row_is_stamped_with_the_writing_transaction() {
        let sql = entity_update_sql(&ContextTarget::Live).expect("the fixed update builds");
        assert!(sql.contains(r#""written_xid" = pg_current_xact_id() FROM "docs", "proj""#), "{sql}");
        assert!(!sql.contains(r#""written_xid" IS DISTINCT FROM"#), "{sql}");
    }

    #[test]
    fn the_lifecycle_row_coalesces_the_expiry_and_replaces_the_attribute_expiry() {
        let sql = entity_update_sql(&ContextTarget::Live).expect("the fixed update builds");
        assert!(sql.contains(r#""ents" AS (UPDATE "entities" SET "expires_at" = COALESCE("proj"."expires_at", "entities"."expires_at"), "min_attribute_expires_at" = "proj"."min_attribute_expires_at""#), "{sql}");
        assert!(
            sql.contains(r#""entities"."expires_at" IS DISTINCT FROM COALESCE("proj"."expires_at", "entities"."expires_at")"#),
            "{sql}"
        );
        assert!(!sql.contains(r#"SET "id""#), "the identity is never rewritten: {sql}");
    }

    /// Clause 4.17 selects on membership, so an update that changes an
    /// Entity's types moves its membership rows in the same statement, from
    /// the images the search write returned.
    #[test]
    fn a_type_change_moves_the_type_memberships_in_the_same_statement() {
        let sql = entity_update_sql(&ContextTarget::Live).expect("the fixed update builds");
        assert!(sql.contains(r#""strays" AS (DELETE FROM "entity_type_members""#), "{sql}");
        assert!(sql.contains(r#""mbrs" AS (INSERT INTO "entity_type_members""#), "{sql}");
        pretty_assertions::assert_eq!(
            sql.matches(r#"JOIN "entity_search""#).count(),
            0,
            "no sibling reads the search relation's snapshot: {sql}"
        );
    }

    /// The search write returns both images of every row it rewrote, and the
    /// census counts the difference of the rows whose types or instances
    /// changed.
    #[test]
    fn the_search_write_returns_both_images_and_the_census_reads_them() {
        let sql = entity_update_sql(&ContextTarget::Live).expect("the fixed update builds");
        assert!(
            sql.contains(r#"RETURNING "entity_search"."entity_key", "docs"."id", "old"."entity_type_name_keys", "new"."entity_type_name_keys", "old"."attribute_instance_name_keys", "new"."attribute_instance_name_keys", "old"."attribute_instance_kinds", "new"."attribute_instance_kinds")"#),
            "{sql}"
        );
        assert!(sql.contains(r#""census_shapes" AS (SELECT"#), "{sql}");
        for ledger in ["entity_type_census", "entity_type_attribute_census", "attribute_census"] {
            assert!(sql.contains(&format!(r#"INSERT INTO "{ledger}""#)), "{ledger}: {sql}");
        }
    }

    #[test]
    fn the_projection_validates_each_geometry_once() {
        let sql = entity_update_sql(&ContextTarget::Live).expect("the fixed update builds");
        pretty_assertions::assert_eq!(sql.matches("ST_MakeValid").count(), 3, "{sql}");
        assert!(sql.starts_with(r#"WITH "proj" AS ("#), "{sql}");
    }
}
