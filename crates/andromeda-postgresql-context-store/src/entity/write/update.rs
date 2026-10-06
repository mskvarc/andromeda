//! The batch update: one statement that overwrites each staged entity's payload
//! and rewrites its search and lifecycle rows only where they changed.

use crate::{
    branch_statement::BranchStatement,
    entity::{
        census::delta::{CensusChange, CensusStatements},
        mapper::stored_written_entity,
        params::{PreparedEntityColumns, literal_texts},
        repo::EntityRepository,
        staged_geometry::{kept_column, staged_column},
        types_present::{entity_write_failure, types_present},
        write::{
            payload_staging::{DocumentWrite, Pay, apply_payload_alias, payload_cte, payload_of, staged_expectation, staged_payload},
            search_image::SearchImage,
            staging::{
                Docs,
                Ents,
                HOISTED_COLUMNS,
                HoistedColumn,
                Proj,
                ProjectedWrite,
                Srch,
                apply_staged_alias,
                expectation,
                projected,
                projection_cte,
                update_staging_columns,
            },
            type_membership::{Mbrs, Strays, added_memberships, dropped_memberships},
        },
        written_entity::WrittenEntity,
    },
    error::ContextStoreError,
    store_invariant::{GuardedEntityWrite, StoreInvariant},
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
};
use andromeda_telemetry::statement::{StatementName, observe_statement};
use chrono::{DateTime, Utc};
use sea_query::{CaseStatement, CommonTableExpression, Cond, Expr, ExprTrait, IntoIden, PostgresQueryBuilder, Query, SimpleExpr, UpdateStatement, WithClause};
use sqlx::{Postgres, Transaction};

/// The statement name the entity set-update is observed under.
const ENTITY_UPDATE: StatementName = StatementName("entity-update");

/// The batch update's text, built once for the live branch.
static ENTITY_UPDATE_STATEMENT: BranchStatement = BranchStatement::new(entity_update_sql);

/// Build the batch update.
///
/// ```sql
/// WITH proj AS (SELECT … FROM UNNEST($1::text[], …, $15::boolean[]) AS d(…)),
///      pay AS (SELECT p.id, p.expected_modified_at, p.payload
///              FROM UNNEST($1::text[], $16::timestamptz[], $17::text[]) AS p(id, expected_modified_at, payload)),
///      docs AS (UPDATE entity_documents SET payload = pay.payload::jsonb, modified_at = NOW()
///               FROM pay, entities
///               WHERE pay.id = entities.id
///                 AND entity_documents.entity_key = entities.entity_key
///                 AND (pay.expected_modified_at IS NULL OR entity_documents.modified_at = pay.expected_modified_at)
///               RETURNING entity_documents.entity_key, entities.id),
///      srch(entity_key, id, old_types, new_types, old_instance_names, …) AS
///              (UPDATE entity_search SET entity_type_name_keys = proj.entity_type_name_keys, …,
///                      location = CASE WHEN proj.loc_kept THEN entity_search.location ELSE proj.location END, …,
///                      written_xid = pg_current_xact_id() FROM docs, proj
///               WHERE proj.id = docs.id AND entity_search.entity_key = docs.entity_key
///                 AND (entity_search.entity_type_name_keys IS DISTINCT FROM proj.entity_type_name_keys OR …
///                      OR (NOT proj.loc_kept AND entity_search.location IS DISTINCT FROM proj.location) OR … )
///               RETURNING entity_search.entity_key, docs.id, old.entity_type_name_keys, new.entity_type_name_keys, …),
///      ents AS (UPDATE entities SET expires_at = proj.expires_at,
///                                   min_attribute_expires_at = proj.min_attribute_expires_at
///               FROM docs, proj
///               WHERE proj.id = docs.id AND entities.entity_key = docs.entity_key
///                 AND (entities.expires_at IS DISTINCT FROM proj.expires_at OR entities.min_attribute_expires_at IS DISTINCT FROM proj.min_attribute_expires_at)),
///      strays AS (DELETE FROM entity_type_members WHERE (entity_type_name_key, id) IN
///                 (SELECT <old type the new row lacks>, srch.id FROM srch …)),
///      mbrs AS (INSERT INTO entity_type_members (…) SELECT srch.entity_key, <new type the old row lacked>, srch.id FROM srch …),
///      census_shapes AS (SELECT <image arrays>, SUM(±1) FROM srch … GROUP BY <image arrays>),
///      census_types AS (INSERT INTO entity_type_census …), census_type_attributes AS (…), census_attributes AS (…)
/// SELECT entity_key, id FROM docs
/// ```
///
/// The payload write is the anchor ([`document_write`]). Its expectation
/// column is nullable so one
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
/// write leaves both rows and every search index untouched. A geometry the
/// write leaves as the stored row holds it is staged as kept, with no EWKB:
/// the statement neither parses nor validates nor compares its positions, and
/// a row rewritten for another column keeps the stored geometry. Both expiry columns
/// are replaced, because both are derived from the payload that was just
/// replaced: an entity whose new document no longer carries `expiresAt`
/// (clause 4.22) must not keep an expiry instant that would later delete it.
/// The type memberships and
/// the discovery census move with the search row: both are derived from the
/// pre- and post-image its write returns (PostgreSQL 18 `RETURNING old.*,
/// new.*`), which is the row the write actually replaced, found after any
/// concurrent writer it waited on. A value-only write rewrites no search row,
/// so it moves no membership and counts nothing.
///
/// # Errors
///
/// Returns [`ContextStoreError::StatementBuild`] if the membership insert ever stops
/// matching its column list, which its fixed shape makes unreachable.
pub(super) fn entity_update_sql(target: &ContextTarget) -> Result<String, ContextStoreError> {
    let columns = update_staging_columns();

    let mut search = Query::update();
    search.table(table_of(target, EntitySearch::Table));
    for column in SCALAR_SEARCH_COLUMNS {
        search.value(column, projected(column));
    }
    for column in HOISTED_COLUMNS {
        search.value(column.stored, hoisted_value(column));
    }
    // A rewritten search row is a changed type, attribute name, scope or
    // geometry, which a spatial index catching up from the store must find.
    search.value(EntitySearch::WrittenXid, current_transaction_id());
    let scalar_changed = SCALAR_SEARCH_COLUMNS.into_iter().fold(Cond::any(), |changed, column| {
        changed.add(SimpleExpr::from(Expr::col((EntitySearch::Table, column))).distinct_from(projected(column)))
    });
    let changed = HOISTED_COLUMNS
        .into_iter()
        .fold(scalar_changed, |changed, column| changed.add(hoisted_changed(column)));
    search.from(Docs).from(Proj).cond_where(
        Cond::all()
            .add(Expr::col((Proj, Entities::Id)).equals((Docs, Entities::Id)))
            .add(Expr::col((EntitySearch::Table, EntitySearch::EntityKey)).equals((Docs, EntityDocuments::EntityKey)))
            .add(changed),
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

    let mut lifecycle = Query::update();
    lifecycle
        .table(table_of(target, Entities::Table))
        .value(Entities::ExpiresAt, projected(Entities::ExpiresAt))
        .value(Entities::MinAttributeExpiresAt, projected(Entities::MinAttributeExpiresAt))
        .from(Docs)
        .from(Proj)
        .cond_where(
            Cond::all()
                .add(Expr::col((Proj, Entities::Id)).equals((Docs, Entities::Id)))
                .add(Expr::col((Entities::Table, Entities::EntityKey)).equals((Docs, EntityDocuments::EntityKey)))
                .add(
                    Cond::any()
                        .add(SimpleExpr::from(Expr::col((Entities::Table, Entities::ExpiresAt))).distinct_from(projected(Entities::ExpiresAt)))
                        .add(
                            SimpleExpr::from(Expr::col((Entities::Table, Entities::MinAttributeExpiresAt)))
                                .distinct_from(projected(Entities::MinAttributeExpiresAt)),
                        ),
                ),
        );

    let payload_source = DocumentWrite::Update {
        expectation: columns.len() + 1,
        payload: columns.len() + 2,
    };
    let mut with = WithClause::new();
    with.cte(projection_cte(&columns, ProjectedWrite::Update))
        .cte(payload_cte(payload_source))
        .cte(CommonTableExpression::new().table_name(Docs).query(document_write(target)).to_owned())
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
    Ok(apply_payload_alias(apply_staged_alias(sql, &columns), payload_source))
}

/// The update's anchor, `docs`: each payload whose expectation still holds,
/// overwritten, with the key and identifier of every row it landed in.
///
/// It reads the payload source alone and finds each row through the unique
/// identifier index, so its cost grows with the batch whatever row estimate
/// the plan was made for.
fn document_write(target: &ContextTarget) -> UpdateStatement {
    Query::update()
        .table(table_of(target, EntityDocuments::Table))
        .value(EntityDocuments::Payload, staged_payload())
        .value(EntityDocuments::ModifiedAt, now())
        .from(Pay)
        .from(table_of(target, Entities::Table))
        .cond_where(
            Cond::all()
                .add(payload_of(Entities::Table))
                .add(Expr::col((EntityDocuments::Table, EntityDocuments::EntityKey)).equals((Entities::Table, Entities::EntityKey)))
                .add(expectation(staged_expectation(), EntityDocuments::Table)),
        )
        .returning(Query::returning().exprs([
            Expr::col((EntityDocuments::Table, EntityDocuments::EntityKey)),
            Expr::col((Entities::Table, Entities::Id)),
        ]))
        .take()
}

/// The six scalar `entity_search` columns the update projects, each written
/// and guarded by the same list so no column can be rewritten without being
/// compared, or compared without being written. The three hoisted geometries
/// ([`HOISTED_COLUMNS`]) are written and guarded the same way, unless the row
/// keeps them.
const SCALAR_SEARCH_COLUMNS: [EntitySearch; 6] = [
    EntitySearch::EntityTypeNameKeys,
    EntitySearch::AttributeNameKeys,
    EntitySearch::AttributeInstanceNameKeys,
    EntitySearch::AttributeInstanceKinds,
    EntitySearch::ScopePaths,
    EntitySearch::LocationExpiresAt,
];

/// The value a rewritten search row takes for one hoisted geometry:
/// `CASE WHEN proj.<kept> THEN entity_search.<column> ELSE proj.<column> END`.
///
/// A kept geometry is the stored column itself, so a row rewritten for another
/// column keeps the geometry's stored value rather than a re-projection of it.
fn hoisted_value(column: HoistedColumn) -> SimpleExpr {
    CaseStatement::new()
        .case(projected(column.kept), Expr::col((EntitySearch::Table, column.stored)))
        .finally(projected(column.stored))
        .into()
}

/// Whether one hoisted geometry changes the search row:
/// `NOT proj.<kept> AND entity_search.<column> IS DISTINCT FROM proj.<column>`.
fn hoisted_changed(column: HoistedColumn) -> Cond {
    Cond::all()
        .add(projected(column.kept).not())
        .add(SimpleExpr::from(Expr::col((EntitySearch::Table, column.stored))).distinct_from(projected(column.stored)))
}

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
            return Err(StoreInvariant::ExpectationCount {
                write: GuardedEntityWrite::Update,
                expectations: expected_modified_at.len(),
                rows: entities.ids.len(),
            }
            .into());
        }
        types_present(entities)?;

        let ids = entities.ids.iter().map(Identifier::as_str).collect::<Vec<_>>();
        let geometries = [&entities.locations, &entities.obs_spaces, &entities.op_spaces];
        let [locations, observation_spaces, operation_spaces] = geometries.map(|column| staged_column(column));
        let [locations_kept, observation_spaces_kept, operation_spaces_kept] = geometries.map(|column| kept_column(column));
        let sql = ENTITY_UPDATE_STATEMENT.text(target)?;
        let rows = sqlx::query_as(sqlx::AssertSqlSafe(&*sql))
            .bind(&ids)
            .bind(literal_texts(&entities.entity_type_name_keys))
            .bind(literal_texts(&entities.attribute_name_keys))
            .bind(literal_texts(&entities.attribute_instance_name_keys))
            .bind(literal_texts(&entities.attribute_instance_kinds))
            .bind(entities.scope_paths_bind())
            .bind(&locations)
            .bind(&observation_spaces)
            .bind(&operation_spaces)
            .bind(&entities.location_expires_ats)
            .bind(&entities.expires_ats)
            .bind(&entities.min_attribute_expires_ats)
            .bind(&locations_kept)
            .bind(&observation_spaces_kept)
            .bind(&operation_spaces_kept)
            .bind(expected_modified_at)
            .bind(&entities.payload_json)
            .fetch_all(&mut **tx);
        let rows: Vec<(i64, String)> = observe_statement(ENTITY_UPDATE, rows).await.map_err(entity_write_failure)?;

        rows.iter().map(|(key, id)| stored_written_entity(*key, id)).collect()
    }
}

#[cfg(test)]
mod tests {
    use crate::entity::write::{
        placeholders,
        update::{ENTITY_UPDATE_STATEMENT, entity_update_sql},
    };
    use andromeda_context_space::{context_target::ContextTarget, snapshot_key::SnapshotKey, snapshot_target::SnapshotTarget};
    use std::{borrow::Cow, collections::BTreeSet, ptr};

    /// The live branch's update text is built once and lent to every write;
    /// a Snapshot branch's is built for its own schema each time.
    #[test]
    fn the_live_update_text_is_built_once_and_a_snapshot_text_for_each_write() {
        let first = ENTITY_UPDATE_STATEMENT.text(&ContextTarget::Live).expect("the fixed update builds");
        let second = ENTITY_UPDATE_STATEMENT.text(&ContextTarget::Live).expect("the fixed update builds");
        assert!(matches!((&first, &second), (Cow::Borrowed(a), Cow::Borrowed(b)) if ptr::eq(*a, *b)));
        pretty_assertions::assert_eq!(first, entity_update_sql(&ContextTarget::Live).expect("the fixed update builds"));

        let key = SnapshotKey::try_from(5).expect("positive key");
        let snapshot = ContextTarget::Snapshot(SnapshotTarget::staging(key, "urn:ngsi-ld:Snapshot:s".parse().expect("valid snapshot uri")));
        let text = ENTITY_UPDATE_STATEMENT.text(&snapshot).expect("the fixed update builds");
        assert!(matches!(text, Cow::Owned(_)));
        assert!(text.contains(r#"UPDATE "snap_5_stg"."entity_documents""#), "{text}");
    }

    #[test]
    fn the_update_binds_seventeen_placeholders_and_names_its_staging_columns() {
        let sql = entity_update_sql(&ContextTarget::Live).expect("the fixed update builds");
        pretty_assertions::assert_eq!(placeholders(&sql), (1..=17).collect::<BTreeSet<u32>>());
        assert!(
            sql.contains(
                " AS d(id, entity_type_name_keys_literal, attribute_name_keys_literal, attribute_instance_name_keys_literal, attribute_instance_kinds_literal, scope_paths_json, loc, obs, ops, loc_expires_at, expires_at, min_attribute_expires_at, loc_kept, obs_kept, ops_kept)"
            ),
            "{sql}"
        );
        assert!(
            sql.contains("UNNEST($1::text[], $16::timestamptz[], $17::text[]) AS p(id, expected_modified_at, payload)"),
            "{sql}"
        );
        assert!(!sql.contains("ORDINALITY"), "no row is paired by its position in the batch: {sql}");
    }

    /// The stored document is read by the document write alone, so it is
    /// staged apart from the materialised projection every sibling reads, and
    /// cast to `jsonb` only where it is stored.
    #[test]
    fn the_payload_is_staged_apart_from_the_projection() {
        let sql = entity_update_sql(&ContextTarget::Live).expect("the fixed update builds");
        let projection = sql.split(r#", "pay" AS ("#).next().unwrap_or_default();
        assert!(!projection.contains("payload"), "the projection carries no payload: {projection}");
        pretty_assertions::assert_eq!(sql.matches(r#""pay"."#).count(), 4, "only the document write reads the payload source: {sql}");
    }

    /// A geometry the row keeps is the stored column itself and changes
    /// nothing: the row is neither rewritten for it nor, when rewritten for
    /// another column, given a re-projection of it.
    #[test]
    fn a_kept_geometry_keeps_the_stored_column_and_is_not_compared() {
        let sql = entity_update_sql(&ContextTarget::Live).expect("the fixed update builds");
        for (column, kept) in [("location", "loc_kept"), ("observation_space", "obs_kept"), ("operation_space", "ops_kept")] {
            assert!(
                sql.contains(&format!(
                    r#""{column}" = (CASE WHEN ("proj"."{kept}") THEN "entity_search"."{column}" ELSE "proj"."{column}" END)"#
                )),
                "{column}: {sql}"
            );
            assert!(
                sql.contains(&format!(
                    r#"((NOT "proj"."{kept}") AND ("entity_search"."{column}" IS DISTINCT FROM "proj"."{column}"))"#
                )),
                "{column}: {sql}"
            );
        }
    }

    #[test]
    fn the_document_write_is_the_compare_and_set_anchor_and_names_only_heap_neutral_columns() {
        let sql = entity_update_sql(&ContextTarget::Live).expect("the fixed update builds");
        assert!(
            sql.contains(r#""docs" AS (UPDATE "entity_documents" SET "payload" = "pay"."payload"::jsonb, "modified_at" = NOW() FROM "pay", "entities" WHERE "pay"."id" = "entities"."id" AND "entity_documents"."entity_key" = "entities"."entity_key" AND"#),
            "{sql}"
        );
        assert!(
            sql.contains(r#""pay"."expected_modified_at" IS NULL OR "entity_documents"."modified_at" = "pay"."expected_modified_at""#),
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

    /// Both expiries follow the payload just written (clause 4.22): an update
    /// whose document dropped the entity's `expiresAt` clears the column, so
    /// the expiry sweep never deletes the entity at an instant its own
    /// document no longer asks for.
    #[test]
    fn the_lifecycle_row_replaces_both_expiries_with_the_written_payloads() {
        let sql = entity_update_sql(&ContextTarget::Live).expect("the fixed update builds");
        assert!(
            sql.contains(
                r#""ents" AS (UPDATE "entities" SET "expires_at" = "proj"."expires_at", "min_attribute_expires_at" = "proj"."min_attribute_expires_at""#
            ),
            "{sql}"
        );
        assert!(sql.contains(r#""entities"."expires_at" IS DISTINCT FROM "proj"."expires_at""#), "{sql}");
        assert!(
            !sql.contains(r#"COALESCE("proj"."expires_at""#),
            "a dropped expiry must not fall back to the stored one: {sql}"
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
