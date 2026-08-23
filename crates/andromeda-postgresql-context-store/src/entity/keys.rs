//! Resolving the surrogate keys of entities a commit touched without writing
//! their rows.

use crate::{
    entity::{mapper::stored_written_entity, repo::EntityRepository, written_entity::WrittenEntity},
    error::ContextStoreError,
};
use andromeda_context_space::context_target::ContextTarget;
use andromeda_ngsi_ld_lexicon::uri::Uri;
use andromeda_postgresql_schema::current::Entities;
use andromeda_postgresql_seaquery::pg_array::{any, text_array};
use sea_query::{Expr, ExprTrait, PostgresQueryBuilder, Query, SelectStatement};
use sea_query_sqlx::SqlxBinder;
use sqlx::{Postgres, Transaction};

impl EntityRepository {
    /// Resolve the surrogate key of every existing entity in `ids`, inside the
    /// caller's transaction.
    ///
    /// A commit whose writes carry no current-state mutation — a replayed
    /// batch of observations, for instance — still projects those observations
    /// against the entity's surrogate key. No fused write statement ran for
    /// such an entity, so nothing handed the key back; this is where the
    /// commit fetches it. Identifiers with no row are simply absent from the
    /// result.
    pub async fn keys_by_ids_in_tx(tx: &mut Transaction<'_, Postgres>, target: &ContextTarget, ids: &[Uri]) -> Result<Vec<WrittenEntity>, ContextStoreError> {
        if ids.is_empty() {
            return Ok(Vec::new());
        }
        let id_strs: Vec<&str> = ids.iter().map(Uri::as_str).collect();
        let (sql, values) = keys_by_ids_query(target, &id_strs).build_sqlx(PostgresQueryBuilder);
        let rows: Vec<(i64, String)> = sqlx::query_as_with(sqlx::AssertSqlSafe(sql.as_str()), values).fetch_all(&mut **tx).await?;
        rows.into_iter().map(|(key, id)| stored_written_entity(key, id)).collect()
    }
}

/// The surrogate key and identifier of every entity in `ids`.
fn keys_by_ids_query(target: &ContextTarget, ids: &[&str]) -> SelectStatement {
    Query::select()
        .columns([Entities::EntityKey, Entities::Id])
        .from(Entities::Table)
        .and_where(Expr::col(Entities::ContextSpaceKey).eq(target.key().as_i64()))
        .and_where(Expr::col(Entities::Id).eq(any(text_array(ids))))
        .to_owned()
}

#[cfg(test)]
mod tests {
    use crate::entity::keys::keys_by_ids_query;
    use andromeda_context_space::{
        context_space::{ContextSpace, ContextSpaceKind},
        context_space_key::ContextSpaceKey,
        context_target::ContextTarget,
    };
    use sea_query::PostgresQueryBuilder;

    fn target() -> ContextTarget {
        let key = ContextSpaceKey::try_from(1).expect("positive context-space key");
        ContextTarget::live(ContextSpace::new(key, ContextSpaceKind::Live)).expect("live context target")
    }

    #[test]
    fn surrogate_keys_resolve_through_one_array_membership_test() {
        let (sql, values) = keys_by_ids_query(&target(), &["urn:a", "urn:b"]).build(PostgresQueryBuilder);
        assert_eq!(
            sql,
            "SELECT \"entity_key\", \"id\" FROM \"entities\" WHERE \"context_space_key\" = $1 AND \"id\" = ANY($2)"
        );
        assert_eq!(values.0.len(), 2, "the branch and whole batch array are bound: {sql}");
    }
}
