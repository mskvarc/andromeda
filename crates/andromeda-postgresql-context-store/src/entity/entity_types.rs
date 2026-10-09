//! Reading the types one entity carries from its search row.
//!
//! The search row names every type of the entity by its dictionary key, so the
//! types are known without reading the Entity's attributes.

use crate::{
    entity::{
        dictionary_join::{TypeName, join_type_name},
        mapper::stored_type_name,
        repo::EntityRepository,
    },
    error::ContextStoreError,
};
use andromeda_context_space::context_target::ContextTarget;
use andromeda_ngsi_ld_lexicon::{entity_type_name::EntityTypeName, identifier::Identifier};
use andromeda_postgresql_branch::table_of;
use andromeda_postgresql_schema::{
    current::{E, Entities, EntitySearch},
    name_dictionary::EntityTypeNames,
};
use andromeda_postgresql_seaquery::pg_array::any;
use sea_query::{Expr, ExprTrait, Iden, JoinType, Order, PostgresQueryBuilder, Query, SelectStatement};
use sea_query_sqlx::SqlxBinder;
use sqlx::Row;

/// Alias `s` for the entity's search row.
#[derive(Iden, Clone, Copy)]
#[iden = "s"]
pub(crate) struct Search;

/// One row per type the entity carries:
///
/// ```sql
/// SELECT n.name
/// FROM entities e JOIN entity_search s ON s.entity_key = e.entity_key
/// JOIN entity_type_names n ON n.entity_type_name_key = ANY(s.entity_type_name_keys)
/// WHERE e.id = $1 ORDER BY n.name
/// ```
///
/// An entity always carries a type, so no row means no entity. The types are
/// a set (clause 4.6.3 gives them no order); they are listed by name so every
/// read of one entity lists them alike.
pub(crate) fn entity_types_statement(target: &ContextTarget, id: &str) -> SelectStatement {
    let mut statement = Query::select();
    statement
        .column((TypeName, EntityTypeNames::Name))
        .from_as(table_of(target, Entities::Table), E)
        .join_as(
            JoinType::InnerJoin,
            table_of(target, EntitySearch::Table),
            Search,
            Expr::col((Search, EntitySearch::EntityKey)).equals((E, Entities::EntityKey)),
        );
    join_type_name(&mut statement, JoinType::InnerJoin, any(Expr::col((Search, EntitySearch::EntityTypeNameKeys))));
    statement
        .and_where(Expr::col((E, Entities::Id)).eq(id))
        .order_by((TypeName, EntityTypeNames::Name), Order::Asc);
    statement
}

impl EntityRepository {
    /// The expanded types of the entity `id` names, or `None` when there is
    /// none.
    ///
    /// # Errors
    ///
    /// Returns [`ContextStoreError`] when the read fails or a stored type name
    /// does not read back as one.
    pub async fn entity_types(&self, target: &ContextTarget, id: &Identifier) -> Result<Option<Vec<EntityTypeName>>, ContextStoreError> {
        let (sql, values) = entity_types_statement(target, id.as_str()).build_sqlx(PostgresQueryBuilder);
        let rows = sqlx::query_with(sqlx::AssertSqlSafe(sql.as_str()), values).fetch_all(&self.pool).await?;
        if rows.is_empty() {
            return Ok(None);
        }
        rows.iter()
            .map(|row| stored_type_name(row.try_get::<&str, _>(0)?))
            .collect::<Result<Vec<_>, ContextStoreError>>()
            .map(Some)
    }
}

#[cfg(test)]
mod tests {
    use crate::entity::entity_types::entity_types_statement;
    use andromeda_context_space::context_target::ContextTarget;
    use sea_query::PostgresQueryBuilder;

    #[test]
    fn the_types_are_read_from_the_search_row_by_identifier() {
        let sql = entity_types_statement(&ContextTarget::Live, "urn:ngsi-ld:T:1").to_string(PostgresQueryBuilder);
        assert!(sql.contains(r#""n"."entity_type_name_key" = ANY("s"."entity_type_name_keys")"#), "{sql}");
        assert!(sql.contains(r#""e"."id" = 'urn:ngsi-ld:T:1'"#), "{sql}");
        assert!(sql.ends_with(r#"ORDER BY "n"."name" ASC"#), "{sql}");
    }
}
