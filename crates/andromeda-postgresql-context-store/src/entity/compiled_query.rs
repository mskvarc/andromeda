//! Executing a compiled entity query: the page a window selects, its total,
//! and the identifiers alone (clauses 5.7.2 and 5.5.9).

use crate::{
    current_attribute::read::RowSelection,
    entity::{mapper::stored_uri, pushdown, repo::EntityRepository},
    error::ContextStoreError,
};
use andromeda_context_space::context_target::ContextTarget;
use andromeda_entity::{port::retrieved_entity::RetrievedEntity, query_outcome::QueryOutcome};
use andromeda_ngsi_ld_lexicon::identifier::Identifier;
use andromeda_ngsi_ld_query::{
    count_policy::CountPolicy,
    entity::compiled::CompiledEntityQuery,
    page_window::PageWindow,
    projection::returned_attributes::ReturnedAttributes,
};
use andromeda_postgresql_context_query::{
    entity::{self as entity_sql, EntityTotal},
    entity_page::TOTAL_COUNT_COLUMN,
};
use andromeda_postgresql_query::{
    dictionary::{query_names::QueryNames, resolved_names::ResolvedNames},
    projected_attribute_names::projected_attribute_names,
    projected_type_names::projected_type_names,
};
use iri_rs::IriBuf;
use sea_query::{PostgresQueryBuilder, SelectStatement};
use sea_query_sqlx::SqlxBinder;
use sqlx::{Row, postgres::PgRow};

impl EntityRepository {
    /// Execute a compiled entity query and return the matching stored states
    /// `window` selects, each with its clause 4.8 system attributes, counting
    /// every match when `count` asks for it.
    ///
    /// Two statements: the page's heads, then the `current_attributes` rows of
    /// the Entities it kept, by their keys and limited to the attributes the
    /// query's `attrs` or clause 4.21 projection can return. A projection that
    /// returns no attribute reads no row.
    ///
    /// The page is returned in the order it is presented: a backward or
    /// from-end cursor page, read descending so one index scan serves it, is
    /// reversed here.
    ///
    /// What is sent and what is handed back are decided by this store's
    /// [`StorageProfile`](andromeda_entity::storage_profile::StorageProfile).
    /// With `PostGIS` and `ltree` every predicate is sent and nothing is handed
    /// back, so the residual is always empty here -- but the decision is read
    /// off the profile rather than assumed, which is what a store without
    /// those extensions changes.
    ///
    /// # Errors
    ///
    /// Returns [`ContextStoreError`] when a name the query reads cannot be
    /// resolved through the dictionaries, when the statement cannot be rendered
    /// from the compiled query, when a statement or the separate count fails,
    /// or when the stored rows no longer read back as entity states.
    pub async fn query_compiled(
        &self,
        target: &ContextTarget,
        compiled: &CompiledEntityQuery,
        window: &PageWindow,
        count: CountPolicy,
    ) -> Result<QueryOutcome<RetrievedEntity>, ContextStoreError> {
        let planned = pushdown::plan(compiled, EntityRepository::PROFILE, window, count);
        self.ensure_collations(&planned.query.ordering).await?;
        let names = self.resolve_names(&planned.query).await?;
        let built = entity_sql::build_query(target, &planned.query, &planned.window, planned.count, &names)?;

        let (sql, values) = built.select.build_sqlx(PostgresQueryBuilder);
        let head_rows = sqlx::query_with(sqlx::AssertSqlSafe(sql.as_str()), values).fetch_all(&self.pool).await?;
        let window_count = page_total(&head_rows, matches!(built.total, EntityTotal::OnPageRows { .. }));
        let returned = ReturnedAttributes::of(&planned.query.projection, &planned.query.selection.attrs);
        let selection = RowSelection::of(&returned, &names.attributes);
        let mut rows = self.entities_of_heads(target, &head_rows, &selection).await?;
        built.read_order.present(&mut rows);
        let rows = rows;

        let total_count = match built.total {
            EntityTotal::Omitted => None,
            EntityTotal::OnPageRows { empty_page } => match (window_count, rows.is_empty()) {
                (Some(count), _) => Some(count),
                (None, true) => Some(self.count_matches(empty_page).await?),
                (None, false) => None,
            },
            EntityTotal::Counted(count) => Some(self.count_matches(count).await?),
        };

        Ok(planned.outcome(rows, total_count))
    }

    /// Run a `COUNT` statement and read back its one value.
    ///
    /// # Errors
    ///
    /// Returns [`ContextStoreError`] when the statement fails.
    async fn count_matches(&self, count: SelectStatement) -> Result<i64, ContextStoreError> {
        let (count_sql, count_values) = count.build_sqlx(PostgresQueryBuilder);
        Ok(sqlx::query_scalar_with(sqlx::AssertSqlSafe(count_sql.as_str()), count_values)
            .fetch_one(&self.pool)
            .await?)
    }

    /// Execute a compiled query and return only the identifiers of the
    /// matching entities `window` selects, in the order they are presented.
    ///
    /// Planned the same way [`EntityRepository::query_compiled`] is; the
    /// statement never counts, so the outcome carries no total either way.
    ///
    /// # Errors
    ///
    /// Returns [`ContextStoreError`] when a name the query reads cannot be
    /// resolved through the dictionaries, when the statement cannot be rendered
    /// from the compiled query, when the select fails, or when a returned
    /// identifier is no longer a valid URI.
    pub async fn query_ids(
        &self,
        target: &ContextTarget,
        compiled: &CompiledEntityQuery,
        window: &PageWindow,
    ) -> Result<QueryOutcome<Identifier>, ContextStoreError> {
        let planned = pushdown::plan(compiled, EntityRepository::PROFILE, window, CountPolicy::Omit);
        self.ensure_collations(&planned.query.ordering).await?;
        let names = self.resolve_names(&planned.query).await?;
        let statement = entity_sql::build_ids_query(target, &planned.query, &planned.window, &names)?;

        let (sql, values) = statement.select.build_sqlx(PostgresQueryBuilder);
        let rows = sqlx::query_with(sqlx::AssertSqlSafe(sql.as_str()), values).fetch_all(&self.pool).await?;
        let mut ids: Vec<Identifier> = rows
            .iter()
            .map(|row| -> Result<Identifier, ContextStoreError> { stored_uri(row.try_get("id")?) })
            .collect::<Result<_, _>>()?;
        statement.read_order.present(&mut ids);
        let ids = ids;
        Ok(planned.outcome(ids, None))
    }

    /// Resolve the attribute and entity type IRIs a compiled query reads by
    /// dictionary key.
    ///
    /// A clause 5.5.9 `attrs` term is answered by the search row's interned
    /// attribute names, so every term resolves, and so is every name a clause
    /// 4.21 projection keeps an entity alive by or returns, which the rows of
    /// the page are read by. A clause 4.9 linked-entity
    /// traversal contributes more than one name per predicate: the Relationship
    /// each hop descends through is probed by dictionary key just as the
    /// terminal attribute is, and both come back from the same collection walk.
    /// A geo-query and a distance ordering each contribute their target
    /// `GeoProperty`, which is read from the same projection (clauses 4.10 and
    /// 4.23). The type selection of every selector
    /// (clause 4.17) and the type guard of every traversal hop are tested
    /// against the interned `entity_type_name_keys` column, so they resolve through the type
    /// dictionary.
    ///
    /// The lookups need no barrier in front of them. Dictionary rows and the
    /// rows keyed on them are written in the entity's own transaction, so a
    /// name that resolves to nothing is a name nothing has ever written, not
    /// one written moments ago.
    async fn resolve_names(&self, compiled: &CompiledEntityQuery) -> Result<QueryNames, ContextStoreError> {
        let returned = match ReturnedAttributes::of(&compiled.projection, &compiled.selection.attrs) {
            ReturnedAttributes::Every => Vec::new(),
            ReturnedAttributes::Named(iris) | ReturnedAttributes::AllBut(iris) => iris,
        };
        let members = compiled
            .selection
            .attrs
            .iter()
            .chain(returned)
            .map(IriBuf::as_str)
            .chain(compiled.selection.projected_members.names());
        let attribute_names = projected_attribute_names(
            members,
            compiled.selection.q_expr.as_ref(),
            compiled.selection.geo.as_ref(),
            &compiled.ordering.terms,
        );
        let attributes = if attribute_names.is_empty() {
            ResolvedNames::default()
        } else {
            self.attribute_names.lookup(&attribute_names).await?
        };
        let type_names = projected_type_names(&compiled.selection);
        let types = if type_names.is_empty() {
            ResolvedNames::default()
        } else {
            self.entity_type_names.lookup(&type_names).await?
        };
        Ok(QueryNames { attributes, types })
    }
}

/// The piggybacked total of a page, read off its first row when the page
/// carries one (`COUNT(*) OVER ()` is constant across the set).
fn page_total(rows: &[PgRow], read_total_count: bool) -> Option<i64> {
    match (read_total_count, rows.first()) {
        (true, Some(first)) => first.try_get::<i64, _>(TOTAL_COUNT_COLUMN).ok(),
        (true, None) | (false, _) => None,
    }
}
